//! `boxdesk pull`: an OCI image, fetched by name and flattened into a guest root.
//!
//! **An image here is a directory, because that is what libkrun boots.** There is no disk image
//! and no kernel to unpack: the VM is given a tree over virtiofs, and a flattened OCI image is a
//! tree. That is the whole of why adopting OCI is cheap here and why `ROADMAP.md` recommends it —
//! every image anyone has already published is reachable, and nothing new has to be run.
//!
//! - **Through `curl`, `tar` and `shasum`, not four new crates.** `xtask/src/artifacts.rs` already
//!   fetches and verifies pinned inputs this way, and the install script is a shell script. An
//!   HTTP client, a TLS stack, a gzip decoder and a tar reader would be a large new dependency
//!   surface for `cargo deny` to carry, to do what three tools on every supported host already do.
//! - **Every blob is verified against its digest before it is opened.** A layer that does not hash
//!   to the digest the manifest named is refused and removed, so nothing untrusted reaches `tar`.
//! - **Layers are flattened in order, with whiteouts applied first.** A `.wh.<name>` entry deletes
//!   what a lower layer put there and `.wh..wh..opq` empties the directory it sits in; both are
//!   applied to the tree *before* the layer is extracted over it, and the markers themselves never
//!   survive into the root.
//! - **Nothing is kept that is not the tree.** Blobs are fetched to a scratch directory, verified,
//!   extracted and removed. A store that grows without bound is a bug report, and layer sharing
//!   across images is roadmap phase 17 rather than a thing to half-build here.
//! - **A pulled root carries no boxdesk guest agent.** `run` works from one; `shell` and `up` dial
//!   an agent on a vsock that a stock image has never heard of. [`Pulled::has_agent`] is what the
//!   verb reports, so nobody finds out by watching a boot hang.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The registry a bare name means, as every other tool means it.
const DEFAULT_REGISTRY: &str = "docker.io";
/// Where `docker.io` is actually served from. The name in a reference is not the host.
const DOCKER_ENDPOINT: &str = "registry-1.docker.io";
/// The namespace a single-word name sits in on Docker Hub: `alpine` is `library/alpine`.
const DOCKER_LIBRARY: &str = "library";
/// The tag a reference with none means.
const DEFAULT_TAG: &str = "latest";

/// The media types a manifest request will accept, newest first. Both the OCI spellings and
/// Docker's own, because a registry serves whichever its image was pushed with.
const MANIFEST_TYPES: &str = "application/vnd.oci.image.index.v1+json,\
     application/vnd.docker.distribution.manifest.list.v2+json,\
     application/vnd.oci.image.manifest.v1+json,\
     application/vnd.docker.distribution.manifest.v2+json";

/// How long any one request may take. A registry that has stopped answering should fail the pull
/// rather than hold the terminal.
const TIMEOUT: &str = "120";

/// An image reference: where it lives, what it is called, and which tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    /// The registry's name as written, which is `docker.io` when none was.
    pub(crate) registry: String,
    /// The repository path, with Docker Hub's `library/` filled in for a bare name.
    pub(crate) repository: String,
    /// The tag, or `latest`.
    pub(crate) tag: String,
}

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.registry, self.repository, self.tag)
    }
}

impl Reference {
    /// Parses `alpine`, `alpine:3.20`, `library/alpine:3.20` or `ghcr.io/org/img:tag`.
    ///
    /// **What makes the first element a registry is a dot or a colon in it**, or its being
    /// `localhost`. That is the rule every other client uses, and it is the reason `org/img` is a
    /// Docker Hub repository while `ghcr.io/img` is not.
    ///
    /// # Errors
    ///
    /// The reference is empty, or names a tag or repository that is not one.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("an image reference is needed, as NAME[:TAG]".to_string());
        }
        if text.contains('@') {
            return Err(format!(
                "{text:?} names a digest; this build pulls by tag (roadmap phase 13)"
            ));
        }
        let (head, rest) = match text.split_once('/') {
            Some((head, rest)) if is_registry(head) => (head.to_string(), rest.to_string()),
            _ => (DEFAULT_REGISTRY.to_string(), text.to_string()),
        };
        let (repository, tag) = match rest.rsplit_once(':') {
            // A colon after the last slash is a tag; one before it is a port in a registry that
            // was not split off, which is not a shape this reaches.
            Some((repo, tag)) if !tag.contains('/') => (repo.to_string(), tag.to_string()),
            _ => (rest.clone(), DEFAULT_TAG.to_string()),
        };
        if repository.is_empty() || tag.is_empty() {
            return Err(format!("{text:?} is not NAME[:TAG]"));
        }
        let repository = if head == DEFAULT_REGISTRY && !repository.contains('/') {
            format!("{DOCKER_LIBRARY}/{repository}")
        } else {
            repository
        };
        Ok(Self {
            registry: head,
            repository,
            tag,
        })
    }

    /// The host this registry is actually served from. Docker Hub is the one whose name is not it.
    fn endpoint(&self) -> &str {
        if self.registry == DEFAULT_REGISTRY {
            DOCKER_ENDPOINT
        } else {
            &self.registry
        }
    }

    /// The `/v2` base every request hangs off.
    fn base(&self) -> String {
        format!("https://{}/v2/{}", self.endpoint(), self.repository)
    }
}

/// Whether the first element of a reference names a registry rather than a namespace.
fn is_registry(head: &str) -> bool {
    head == "localhost" || head.contains('.') || head.contains(':')
}

/// What a pull produced.
#[derive(Debug, Clone)]
pub(crate) struct Pulled {
    /// The manifest's digest, which is the image's identity and the directory its root sits in.
    pub(crate) digest: String,
    /// Where the flattened tree is.
    pub(crate) root: PathBuf,
    /// Whether the tree carries a boxdesk guest agent, which `shell` and `up` need and a stock
    /// image has never had.
    pub(crate) has_agent: bool,
    /// How many layers were flattened, which is the one number that says how much work it was.
    pub(crate) layers: usize,
}

/// Where the images are, refused as a `String` because everything in this module reports that way.
fn images_dir() -> Result<PathBuf, String> {
    boxdesk_record::images_dir().map_err(|e| e.to_string())
}

/// The OCI platform this host's guests are, as `(os, architecture)`.
///
/// **The guest is always Linux**, whatever the host is: a boxdesk VM boots a Linux guest on macOS
/// exactly as it does on Linux, so the only thing the host decides is the architecture.
pub(crate) fn platform() -> (&'static str, &'static str) {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    };
    ("linux", arch)
}

/// Fetches `reference` and flattens it into a guest root under the images directory.
///
/// # Errors
///
/// The registry refuses, a digest does not match, a tool is missing, or the tree cannot be written.
pub(crate) fn pull(reference: &Reference, out: &mut impl io::Write) -> Result<Pulled, String> {
    let dir = images_dir()?;
    make_dir(&dir)?;
    let scratch = Scratch::inside(&dir)?;

    let token = token_for(reference)?;
    writeln!(out, "resolving {reference}").map_err(|e| e.to_string())?;
    let (digest, manifest) = manifest_of(reference, token.as_deref(), &scratch)?;

    let root = dir.join("roots").join(safe_digest(&digest));
    if root.is_dir() {
        writeln!(out, "already here: {digest}").map_err(|e| e.to_string())?;
        let pulled = Pulled {
            digest: digest.clone(),
            has_agent: carries_agent(&root),
            layers: manifest.layers.len(),
            root,
        };
        index_put(&dir, reference, &digest)?;
        return Ok(pulled);
    }

    // Into a scratch tree and renamed into place at the end: a root that is half a pull is one a
    // later run would boot as if it were whole.
    let building = scratch.path().join("rootfs");
    make_dir(&building)?;
    for (n, layer) in manifest.layers.iter().enumerate() {
        writeln!(
            out,
            "layer {}/{} {}",
            n + 1,
            manifest.layers.len(),
            short(&layer.digest)
        )
        .map_err(|e| e.to_string())?;
        let blob = scratch.path().join(safe_digest(&layer.digest));
        fetch(
            &format!("{}/blobs/{}", reference.base(), layer.digest),
            token.as_deref(),
            &[],
            &blob,
        )?;
        verify(&blob, &layer.digest)?;
        apply_layer(&blob, &building)?;
        let _ = std::fs::remove_file(&blob);
    }

    prepare_root(&building)?;

    make_dir(&dir.join("roots"))?;
    std::fs::rename(&building, &root).map_err(|e| format!("putting the root in place: {e}"))?;
    index_put(&dir, reference, &digest)?;
    Ok(Pulled {
        digest,
        has_agent: carries_agent(&root),
        layers: manifest.layers.len(),
        root,
    })
}

/// The root `--image REF` names, refusing one that has never been pulled rather than falling back
/// to a tree the caller did not ask for.
///
/// # Errors
///
/// The reference is not one, or nothing of that name has been pulled.
pub(crate) fn asked_for(reference: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(text) = reference else {
        return Ok(None);
    };
    let reference = Reference::parse(text)?;
    root_of(&reference)?
        .map(Some)
        .ok_or_else(|| format!("no image {reference} here (pull it with `boxdesk pull {text}`)"))
}

/// The root a reference resolves to, or `None` if it has never been pulled.
fn root_of(reference: &Reference) -> Result<Option<PathBuf>, String> {
    let dir = images_dir()?;
    let Some(digest) = index_get(&dir, reference)? else {
        return Ok(None);
    };
    let root = dir.join("roots").join(safe_digest(&digest));
    Ok(root.is_dir().then_some(root))
}

/// One line of the index: a reference, the manifest it resolved to, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) reference: String,
    pub(crate) digest: String,
    pub(crate) pulled_ms: u64,
}

/// Every image this machine has pulled, by reference.
pub(crate) fn list() -> Result<Vec<Entry>, String> {
    let dir = images_dir()?;
    let mut out = index_read(&dir)?;
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok(out)
}

/// Drops a reference from the index, and the root with it when nothing else names it.
///
/// # Errors
///
/// The index cannot be written, or the tree cannot be removed.
pub(crate) fn remove(reference: &Reference) -> Result<bool, String> {
    let dir = images_dir()?;
    let mut entries = index_read(&dir)?;
    let named = reference.to_string();
    let Some(at) = entries.iter().position(|e| e.reference == named) else {
        return Ok(false);
    };
    let digest = entries.remove(at).digest;
    // Only when no other tag points at it: two references to one image is the ordinary case, and
    // removing one must not take the tree out from under the other.
    if !entries.iter().any(|e| e.digest == digest) {
        let root = dir.join("roots").join(safe_digest(&digest));
        if root.is_dir() {
            std::fs::remove_dir_all(&root)
                .map_err(|e| format!("removing {}: {e}", root.display()))?;
        }
    }
    index_write(&dir, &entries)?;
    Ok(true)
}

/// `boxdesk pull REF`: fetch an image and flatten it into a guest root.
#[derive(clap::Args)]
pub(crate) struct PullArgs {
    /// The image, as `NAME`, `NAME:TAG` or `REGISTRY/NAME:TAG`.
    #[arg(value_name = "REF")]
    reference: String,
}

/// `boxdesk registry`: where images come from, and who this machine is when it asks.
#[derive(clap::Args)]
pub(crate) struct RegistryArgs {
    #[command(subcommand)]
    cmd: RegistryCmd,
}

#[derive(clap::Subcommand)]
enum RegistryCmd {
    /// Add a registry: a host, and optionally a project and the username to sign in as.
    Add(RegistryAddArgs),
    /// List the registries this machine knows.
    Ls(RegistryLsArgs),
    /// Remove a registry. The images already pulled from it are untouched.
    Rm(RegistryRmArgs),
}

#[derive(clap::Args)]
pub(crate) struct RegistryAddArgs {
    /// The short handle this is listed and reached by.
    #[arg(value_name = "NAME")]
    name: String,
    /// The registry host, as it appears in an image reference: `ghcr.io`, `docker.io`.
    #[arg(value_name = "HOST")]
    url: String,
    /// The namespace or organisation images sit under there.
    #[arg(long, value_name = "PROJECT")]
    project: Option<String>,
    /// Who this machine signs in as. The password is never stored: it is read from
    /// `$BOXDESK_REGISTRY_PASSWORD` at the moment a pull needs it.
    #[arg(long, value_name = "USERNAME")]
    username: Option<String>,
    /// Replace a registry of this name if there is one.
    #[arg(long)]
    force: bool,
}

#[derive(clap::Args)]
pub(crate) struct RegistryLsArgs {
    /// Print the registries as one JSON array.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct RegistryRmArgs {
    /// The registry's name.
    #[arg(value_name = "NAME")]
    name: String,
}

pub(crate) fn run_registry(args: &RegistryArgs) -> std::process::ExitCode {
    let store = match boxdesk_record::RegistryStore::open() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("boxdesk registry: {e}");
            return std::process::ExitCode::from(crate::EXIT_OPERATIONAL);
        }
    };
    let (verb, done) = match &args.cmd {
        RegistryCmd::Add(a) => ("add", add_registry(&store, a)),
        RegistryCmd::Ls(a) => ("ls", list_registries(&store, a, &mut std::io::stdout())),
        RegistryCmd::Rm(a) => ("rm", forget_registry(&store, a)),
    };
    match done {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("boxdesk registry {verb}: {msg}");
            std::process::ExitCode::from(crate::EXIT_OPERATIONAL)
        }
    }
}

fn add_registry(
    store: &boxdesk_record::RegistryStore,
    args: &RegistryAddArgs,
) -> Result<(), String> {
    if !boxdesk_record::valid_id(&args.name) {
        return Err(format!(
            "{:?} is not a usable registry name: letters, digits, `-` and `_`",
            args.name
        ));
    }
    if store.holds(&args.name) && !args.force {
        return Err(format!(
            "a registry named {:?} is already here (use --force to replace it)",
            args.name
        ));
    }
    let registry = boxdesk_record::Registry::new(&args.name, args.url.trim()).signed_in_as(
        args.project.as_deref().unwrap_or_default(),
        args.username.as_deref().unwrap_or_default(),
    );
    store.save(&registry).map_err(|e| e.to_string())?;
    println!("{}", registry.name);
    Ok(())
}

fn list_registries(
    store: &boxdesk_record::RegistryStore,
    args: &RegistryLsArgs,
    out: &mut impl io::Write,
) -> Result<(), String> {
    let registries = store.list();
    if args.json {
        let array: Vec<serde_json::Value> = registries
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "url": r.url,
                    "project": r.project,
                    "username": r.username,
                    "added_ms": r.added_ms,
                })
            })
            .collect();
        writeln!(out, "{}", serde_json::Value::Array(array)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if registries.is_empty() {
        writeln!(
            out,
            "no registries (add one with `boxdesk registry add`; public images pull without one)"
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let widest = registries.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for registry in &registries {
        let who = if registry.username.is_empty() {
            "anonymous".to_string()
        } else {
            registry.username.clone()
        };
        let project = if registry.project.is_empty() {
            String::new()
        } else {
            format!("/{}", registry.project)
        };
        writeln!(
            out,
            "{:widest$}  {}{project}  {who}",
            registry.name, registry.url
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn forget_registry(
    store: &boxdesk_record::RegistryStore,
    args: &RegistryRmArgs,
) -> Result<(), String> {
    store.remove(&args.name).map_err(|e| e.to_string())?;
    println!("{}", args.name);
    Ok(())
}

/// `boxdesk image`: what has been pulled onto this machine.
#[derive(clap::Args)]
pub(crate) struct ImageArgs {
    #[command(subcommand)]
    cmd: ImageCmd,
}

#[derive(clap::Subcommand)]
enum ImageCmd {
    /// List the images pulled onto this machine.
    Ls(ImageLsArgs),
    /// Remove an image: the reference, and the tree when nothing else names it.
    Rm(ImageRmArgs),
}

#[derive(clap::Args)]
pub(crate) struct ImageLsArgs {
    /// Print the images as one JSON array.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct ImageRmArgs {
    /// The image, as `NAME`, `NAME:TAG` or `REGISTRY/NAME:TAG`.
    #[arg(value_name = "REF")]
    reference: String,
}

pub(crate) fn run_pull(args: &PullArgs) -> std::process::ExitCode {
    match fetch_and_report(args, &mut std::io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("boxdesk pull: {msg}");
            std::process::ExitCode::from(crate::EXIT_OPERATIONAL)
        }
    }
}

fn fetch_and_report(args: &PullArgs, out: &mut impl io::Write) -> Result<(), String> {
    let reference = Reference::parse(&args.reference)?;
    let pulled = pull(&reference, out)?;
    writeln!(out, "{} layers -> {}", pulled.layers, pulled.root.display())
        .map_err(|e| e.to_string())?;
    writeln!(out, "{}", pulled.digest).map_err(|e| e.to_string())?;
    // Said once, here, rather than found out by watching `shell` hang on a vsock nobody is
    // listening on: a stock image has never carried this project's guest agent.
    if !pulled.has_agent {
        writeln!(
            out,
            "note: no guest agent in this image, so `boxdesk run --image {}` works and `shell` \
             and `up` do not",
            args.reference
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn run_image(args: &ImageArgs) -> std::process::ExitCode {
    let (verb, done) = match &args.cmd {
        ImageCmd::Ls(a) => ("ls", list_images(a, &mut std::io::stdout())),
        ImageCmd::Rm(a) => ("rm", forget_image(a)),
    };
    match done {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("boxdesk image {verb}: {msg}");
            std::process::ExitCode::from(crate::EXIT_OPERATIONAL)
        }
    }
}

fn list_images(args: &ImageLsArgs, out: &mut impl io::Write) -> Result<(), String> {
    let entries = list()?;
    if args.json {
        let array: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "reference": e.reference,
                    "digest": e.digest,
                    "pulled_ms": e.pulled_ms,
                })
            })
            .collect();
        writeln!(out, "{}", serde_json::Value::Array(array)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if entries.is_empty() {
        writeln!(out, "no images (fetch one with `boxdesk pull`)").map_err(|e| e.to_string())?;
        return Ok(());
    }
    let widest = entries.iter().map(|e| e.reference.len()).max().unwrap_or(0);
    for entry in &entries {
        writeln!(
            out,
            "{:widest$}  {}  {}",
            entry.reference,
            short(&entry.digest),
            boxdesk_record::format_time(entry.pulled_ms)
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn forget_image(args: &ImageRmArgs) -> Result<(), String> {
    let reference = Reference::parse(&args.reference)?;
    if remove(&reference)? {
        println!("{reference}");
        Ok(())
    } else {
        Err(format!(
            "no image named {reference} (`boxdesk image ls` lists them)"
        ))
    }
}

// -- the registry conversation ------------------------------------------------------------------

/// A manifest, reduced to the part a pull needs.
struct Manifest {
    layers: Vec<Layer>,
}

struct Layer {
    digest: String,
}

/// Resolves `reference` to one platform's manifest, following an index when the registry serves
/// one.
fn manifest_of(
    reference: &Reference,
    token: Option<&str>,
    scratch: &Scratch,
) -> Result<(String, Manifest), String> {
    let accept = [("Accept", MANIFEST_TYPES)];
    let body = scratch.path().join("manifest.json");
    let url = format!("{}/manifests/{}", reference.base(), reference.tag);
    fetch(&url, token, &accept, &body)?;
    let text = std::fs::read_to_string(&body).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("the registry's answer is not JSON: {e}"))?;

    // An index lists one manifest per platform; a manifest lists layers. Which one arrived is told
    // by whether there is a `manifests` array, not by the media type, because both spellings of
    // each are in the wild.
    if let Some(manifests) = value.get("manifests").and_then(|m| m.as_array()) {
        let (os, arch) = platform();
        let picked = manifests.iter().find(|m| {
            let p = m.get("platform");
            let field = |k: &str| p.and_then(|p| p.get(k)).and_then(|v| v.as_str());
            field("os") == Some(os) && field("architecture") == Some(arch)
        });
        let Some(picked) = picked else {
            return Err(format!(
                "{reference} has no {os}/{arch} image (this host's guests are {os}/{arch})"
            ));
        };
        let digest = picked
            .get("digest")
            .and_then(|d| d.as_str())
            .ok_or("a manifest in the index carries no digest")?
            .to_string();
        let url = format!("{}/manifests/{digest}", reference.base());
        fetch(&url, token, &accept, &body)?;
        let text = std::fs::read_to_string(&body).map_err(|e| e.to_string())?;
        verify_text(&text, &digest)?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("the manifest is not JSON: {e}"))?;
        return Ok((digest, layers_of(&value)?));
    }

    // A manifest served directly under a tag: its digest is the hash of the bytes that arrived.
    let digest = format!("sha256:{}", sha256_of_text(&text)?);
    Ok((digest, layers_of(&value)?))
}

/// The layer list of a manifest, in the order they are applied.
fn layers_of(value: &serde_json::Value) -> Result<Manifest, String> {
    let Some(layers) = value.get("layers").and_then(|l| l.as_array()) else {
        return Err("the manifest lists no layers".to_string());
    };
    let mut out = Vec::new();
    for layer in layers {
        let digest = layer
            .get("digest")
            .and_then(|d| d.as_str())
            .ok_or("a layer carries no digest")?;
        let media = layer
            .get("mediaType")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        // Foreign layers are hosted elsewhere and are a Windows-image thing; a tree built without
        // one would be missing files and look like it worked.
        if media.contains("foreign") || media.contains("nondistributable") {
            return Err(format!("{media} is a layer this build cannot fetch"));
        }
        out.push(Layer {
            digest: digest.to_string(),
        });
    }
    Ok(Manifest { layers: out })
}

/// A bearer token for `reference`, when the registry asks for one.
///
/// **Asked for, not assumed.** An unauthenticated manifest request is made first; a `401` carries
/// a `WWW-Authenticate` header naming the token service, its realm and the scope, and that is what
/// is answered. It is the flow every registry implements, which is why it works against Docker
/// Hub, ghcr.io and quay.io without any of them being special-cased.
fn token_for(reference: &Reference) -> Result<Option<String>, String> {
    let dir = images_dir()?;
    let scratch = Scratch::inside(&dir)?;
    let headers = scratch.path().join("headers");
    let body = scratch.path().join("probe");
    let url = format!("{}/manifests/{}", reference.base(), reference.tag);
    let code = curl(&[
        "-sS",
        "-L",
        "--max-time",
        TIMEOUT,
        "-o",
        &body.display().to_string(),
        "-D",
        &headers.display().to_string(),
        "-w",
        "%{http_code}",
        "-H",
        &format!("Accept: {MANIFEST_TYPES}"),
        &url,
    ])?;
    if code != "401" {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&headers).map_err(|e| e.to_string())?;
    let challenge = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("www-authenticate:"))
        .ok_or_else(|| format!("{reference}: the registry refused and offered no way in"))?;
    let (realm, service, scope) = challenge_parts(challenge)
        .ok_or_else(|| format!("{reference}: the registry's challenge names no realm"))?;

    let mut url = format!("{realm}?scope={scope}");
    if !service.is_empty() {
        url.push_str(&format!("&service={service}"));
    }
    let token_body = scratch.path().join("token.json");
    let mut args: Vec<String> = vec![
        "-sS".into(),
        "-f".into(),
        "-L".into(),
        "--max-time".into(),
        TIMEOUT.into(),
        "-o".into(),
        token_body.display().to_string(),
    ];
    // **Who this machine is comes from the registry store; the password never does.** A
    // `boxdesk registry add` record for this host names the username, and the secret is read from
    // the environment at the moment it is used, so no file this tool writes ever holds one.
    // `$BOXDESK_REGISTRY_USER` overrides the stored name for a one-off.
    if let (Some(user), Ok(password)) = (
        signed_in_as(&reference.registry),
        std::env::var("BOXDESK_REGISTRY_PASSWORD"),
    ) {
        args.push("--user".into());
        args.push(format!("{user}:{password}"));
    }
    args.push(url);
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    curl(&borrowed)?;
    let text = std::fs::read_to_string(&token_body).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("the token service answered oddly: {e}"))?;
    // `token` is the registry spec's name for it and `access_token` is OAuth2's; registries serve
    // one or the other and some serve both.
    let token = value
        .get("token")
        .or_else(|| value.get("access_token"))
        .and_then(|t| t.as_str())
        .ok_or("the token service returned no token")?;
    Ok(Some(token.to_string()))
}

/// Who this machine signs in to `host` as: the environment's override, else the username on the
/// registry record for that host, else nobody.
fn signed_in_as(host: &str) -> Option<String> {
    if let Ok(user) = std::env::var("BOXDESK_REGISTRY_USER")
        && !user.is_empty()
    {
        return Some(user);
    }
    let store = boxdesk_record::RegistryStore::open().ok()?;
    let registry = store.serving(host)?;
    (!registry.username.is_empty()).then_some(registry.username)
}

/// `realm`, `service` and `scope` out of a `WWW-Authenticate: Bearer ...` line.
fn challenge_parts(line: &str) -> Option<(String, String, String)> {
    let after = line.split_once("Bearer ").map(|(_, rest)| rest)?;
    let field = |key: &str| -> String {
        after
            .split(',')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
            .map(|(_, v)| v.trim().trim_matches('"').trim_end().to_string())
            .unwrap_or_default()
    };
    let realm = field("realm");
    if realm.is_empty() {
        return None;
    }
    Some((realm, field("service"), field("scope")))
}

// -- fetching and verifying ---------------------------------------------------------------------

/// `curl` with `args`, answering with whatever it wrote to stdout.
fn curl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("curl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("running curl (a pull fetches over HTTPS with it): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// GETs `url` to `dest`, with a bearer token and any extra headers.
fn fetch(
    url: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    dest: &Path,
) -> Result<(), String> {
    let mut args: Vec<String> = vec![
        "-sS".into(),
        "-f".into(),
        "-L".into(),
        "--max-time".into(),
        TIMEOUT.into(),
        "-o".into(),
        dest.display().to_string(),
    ];
    if let Some(token) = token {
        args.push("-H".into());
        args.push(format!("Authorization: Bearer {token}"));
    }
    for (key, value) in headers {
        args.push("-H".into());
        args.push(format!("{key}: {value}"));
    }
    args.push(url.to_string());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    curl(&borrowed).map(|_| ())
}

/// The hashers a host may have, tried in order, as `xtask` tries them: `sha256sum` from coreutils,
/// then `shasum -a 256`, which every macOS carries.
const HASHERS: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];

/// Refuses `blob` unless it hashes to `digest`.
///
/// **Before `tar` ever opens it.** Everything downstream of this treats the bytes as trustworthy,
/// so this is the line where a substituted layer stops.
fn verify(blob: &Path, digest: &str) -> Result<(), String> {
    let Some(want) = digest.strip_prefix("sha256:") else {
        return Err(format!("{digest} is not a digest this build can check"));
    };
    let got = sha256_of(blob)?;
    if !got.eq_ignore_ascii_case(want) {
        let _ = std::fs::remove_file(blob);
        return Err(format!(
            "a layer does not match its digest: wanted {want}, got {got} (removed)"
        ));
    }
    Ok(())
}

/// The same, of text already in hand.
fn verify_text(text: &str, digest: &str) -> Result<(), String> {
    let Some(want) = digest.strip_prefix("sha256:") else {
        return Err(format!("{digest} is not a digest this build can check"));
    };
    let got = sha256_of_text(text)?;
    if !got.eq_ignore_ascii_case(want) {
        return Err(format!(
            "the manifest does not match its digest: wanted {want}, got {got}"
        ));
    }
    Ok(())
}

/// The sha256 of a file, from the first hasher this host has.
fn sha256_of(path: &Path) -> Result<String, String> {
    let mut missing = None;
    for (program, args) in HASHERS {
        let out = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .output();
        match out {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                missing = Some(program);
                continue;
            }
            Err(e) => return Err(format!("running {program}: {e}")),
            Ok(out) if !out.status.success() => {
                return Err(format!(
                    "{program}: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let hex = text.split_whitespace().next().unwrap_or_default();
                return Ok(hex.to_string());
            }
        }
    }
    Err(format!(
        "no sha256 tool: tried {}",
        missing.unwrap_or("sha256sum")
    ))
}

/// The sha256 of text, through the same tools, via a scratch file.
fn sha256_of_text(text: &str) -> Result<String, String> {
    let dir = images_dir()?;
    let scratch = Scratch::inside(&dir)?;
    let path = scratch.path().join("bytes");
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    sha256_of(&path)
}

// -- flattening ---------------------------------------------------------------------------------

/// The prefix an OCI whiteout entry wears.
const WHITEOUT: &str = ".wh.";
/// The one that empties the directory it sits in rather than naming a single file.
const OPAQUE: &str = ".wh..wh..opq";

/// Applies one layer over `root`: whiteouts first, then the layer's own files.
///
/// **The order is the whole of the correctness here.** A whiteout deletes what a *lower* layer
/// put there, so it has to be applied to the tree as it stands before this layer's files land on
/// it; doing it the other way round would delete files this layer had just written.
fn apply_layer(blob: &Path, root: &Path) -> Result<(), String> {
    let listing = tar(&["-tzf", &blob.display().to_string()])?;
    for line in listing.lines() {
        let entry = line.trim_end_matches('/');
        let Some((parent, name)) = split_entry(entry) else {
            continue;
        };
        if name == OPAQUE {
            // Everything already under this directory goes; the layer then refills it.
            let target = root.join(parent);
            if target.is_dir() {
                for child in std::fs::read_dir(&target).map_err(|e| e.to_string())? {
                    let child = child.map_err(|e| e.to_string())?.path();
                    remove_any(&child)?;
                }
            }
        } else if let Some(hidden) = name.strip_prefix(WHITEOUT) {
            remove_any(&root.join(parent).join(hidden))?;
        }
    }
    // `tar` refuses absolute paths and `..` components by default on both the GNU and the bsdtar
    // every supported host ships, which is what keeps a hostile layer inside the tree it is
    // building. The digest check above is the other half: these bytes are the ones the registry
    // named, whatever they contain.
    tar(&[
        "-xzf",
        &blob.display().to_string(),
        "-C",
        &root.display().to_string(),
    ])?;
    // The markers are instructions, not files. A root that kept them would show `.wh.` entries a
    // guest could see.
    sweep_markers(root)?;
    Ok(())
}

/// An entry's directory and its last element.
fn split_entry(entry: &str) -> Option<(&str, &str)> {
    match entry.rsplit_once('/') {
        Some((parent, name)) => Some((parent, name)),
        None => Some(("", entry)),
    }
}

/// Removes a file, a directory or a symlink, and says nothing about one that was not there.
fn remove_any(path: &Path) -> Result<(), String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    let done = if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    done.map_err(|e| format!("removing {}: {e}", path.display()))
}

/// Takes every whiteout marker back out of the tree once its instruction has been carried out.
fn sweep_markers(root: &Path) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(WHITEOUT) {
                remove_any(&path)?;
                continue;
            }
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                stack.push(path);
            }
        }
    }
    Ok(())
}

/// `tar` with `args`, answering with its stdout.
fn tar(args: &[&str]) -> Result<String, String> {
    let out = Command::new("tar")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("running tar (a layer is a gzipped tar): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Makes the mount points boxdesk mounts, which a stock image has never had.
///
/// **Every run mounts `/results` by default, and no image ships that directory.** The root is
/// read-only, so the guest cannot make one either: without this, the first thing anybody pulls
/// refuses to boot with a message about a mount point. The tree being prepared is this machine's
/// flattened copy and not the image, so making the directories this runtime mounts at is the
/// flattener's job rather than a thing to ask of every image on the internet.
fn prepare_root(root: &Path) -> Result<(), String> {
    make_dir(&root.join(boxdesk_record::RESULTS_GUEST_PATH.trim_start_matches('/')))
}

/// Whether a tree carries the guest agent `shell` and `up` dial.
fn carries_agent(root: &Path) -> bool {
    let path = boxdesk_channel::GUEST_AGENT_PATH.trim_start_matches('/');
    root.join(path).exists()
}

// -- the index ----------------------------------------------------------------------------------

/// Every reference this machine has pulled, read off the index file.
fn index_read(dir: &Path) -> Result<Vec<Entry>, String> {
    let path = dir.join("index");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("image") {
            continue;
        }
        let (Some(reference), Some(digest)) = (words.next(), words.next()) else {
            continue;
        };
        out.push(Entry {
            reference: reference.to_string(),
            digest: digest.to_string(),
            pulled_ms: words.next().and_then(|w| w.parse().ok()).unwrap_or(0),
        });
    }
    Ok(out)
}

/// Rewrites the index whole, atomically: a reader sees the old file or the new, never a torn one.
fn index_write(dir: &Path, entries: &[Entry]) -> Result<(), String> {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&format!(
            "image {} {} {}\n",
            entry.reference, entry.digest, entry.pulled_ms
        ));
    }
    let path = dir.join("index");
    let tmp = dir.join(format!("index.{}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map_err(|e| format!("writing the image index: {e}"))
}

/// Points `reference` at `digest`, replacing whatever it pointed at.
fn index_put(dir: &Path, reference: &Reference, digest: &str) -> Result<(), String> {
    let named = reference.to_string();
    let mut entries = index_read(dir)?;
    entries.retain(|e| e.reference != named);
    entries.push(Entry {
        reference: named,
        digest: digest.to_string(),
        pulled_ms: boxdesk_record::now_ms(),
    });
    index_write(dir, &entries)
}

/// What `reference` points at, if anything.
fn index_get(dir: &Path, reference: &Reference) -> Result<Option<String>, String> {
    let named = reference.to_string();
    Ok(index_read(dir)?
        .into_iter()
        .find(|e| e.reference == named)
        .map(|e| e.digest))
}

// -- odds and ends ------------------------------------------------------------------------------

/// A digest as one directory name: `sha256:abc` is `sha256-abc`, because the colon is not a
/// character to hang a path on and the algorithm is worth keeping.
fn safe_digest(digest: &str) -> String {
    digest.replace([':', '/'], "-")
}

/// A digest, shortened for a line a person reads.
fn short(digest: &str) -> &str {
    let hex = digest.split_once(':').map_or(digest, |(_, hex)| hex);
    &hex[..hex.len().min(12)]
}

/// Creates a directory `0700`, as every other store this project keeps is created.
fn make_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|e| format!("creating {}: {e}", path.display()))
}

/// A working directory removed when it goes out of scope, whether the pull finished or failed.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn inside(dir: &Path) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".pull-{}-{n}", std::process::id()));
        make_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference grammar every other client uses, including the two rules that are not
    /// obvious: a bare name is under `library/` on Docker Hub, and what makes the first element a
    /// registry is a dot, a colon or `localhost` — which is why `org/img` is a Hub repository and
    /// `ghcr.io/img` is not.
    #[test]
    fn a_reference_reads_the_way_every_other_client_reads_one() {
        let cases = [
            ("alpine", "docker.io", "library/alpine", "latest"),
            ("alpine:3.20", "docker.io", "library/alpine", "3.20"),
            ("org/img", "docker.io", "org/img", "latest"),
            ("org/img:v2", "docker.io", "org/img", "v2"),
            ("ghcr.io/org/img:v2", "ghcr.io", "org/img", "v2"),
            ("localhost:5000/img", "localhost:5000", "img", "latest"),
            (
                "registry.example.com/a/b/c:tag",
                "registry.example.com",
                "a/b/c",
                "tag",
            ),
        ];
        for (text, registry, repository, tag) in cases {
            let parsed = Reference::parse(text);
            assert!(parsed.is_ok(), "{text}: {parsed:?}");
            let got = parsed.unwrap_or_else(|_| Reference {
                registry: String::new(),
                repository: String::new(),
                tag: String::new(),
            });
            assert_eq!(got.registry, registry, "{text}: registry");
            assert_eq!(got.repository, repository, "{text}: repository");
            assert_eq!(got.tag, tag, "{text}: tag");
        }
    }

    /// Docker Hub is the one registry whose name is not the host it is served from, and a
    /// reference that forgot that would ask `docker.io` for a `/v2` it does not serve.
    #[test]
    fn docker_hub_is_asked_at_the_host_it_is_actually_served_from() {
        let hub = Reference::parse("alpine").expect("a bare name");
        assert_eq!(hub.endpoint(), DOCKER_ENDPOINT);
        assert_eq!(hub.base(), "https://registry-1.docker.io/v2/library/alpine");

        let other = Reference::parse("ghcr.io/org/img").expect("a named registry");
        assert_eq!(
            other.endpoint(),
            "ghcr.io",
            "every other one is its own host"
        );
    }

    /// A reference this build cannot serve is refused with the reason, rather than pulled as
    /// something else.
    #[test]
    fn a_reference_that_is_not_one_is_refused_by_name() {
        assert!(Reference::parse("").is_err(), "nothing is not a reference");
        let err = Reference::parse("alpine@sha256:abc").expect_err("a digest reference");
        assert!(err.contains("digest"), "said {err}");
    }

    /// The registry's challenge is parsed by field, because registries order and space the fields
    /// differently and a positional read would work against one and not the next.
    #[test]
    fn a_bearer_challenge_is_read_by_field_not_by_position() {
        let hub = "www-authenticate: Bearer realm=\"https://auth.docker.io/token\",\
                   service=\"registry.docker.io\",scope=\"repository:library/alpine:pull\"";
        let (realm, service, scope) = challenge_parts(hub).expect("a Docker Hub challenge");
        assert_eq!(realm, "https://auth.docker.io/token");
        assert_eq!(service, "registry.docker.io");
        assert_eq!(scope, "repository:library/alpine:pull");

        // ghcr.io puts the fields in another order and offers no scope until it is asked.
        let ghcr = "Www-Authenticate: Bearer service=\"ghcr.io\",\
                    realm=\"https://ghcr.io/token\"";
        let (realm, service, scope) = challenge_parts(ghcr).expect("a ghcr challenge");
        assert_eq!(realm, "https://ghcr.io/token");
        assert_eq!(service, "ghcr.io");
        assert!(scope.is_empty(), "a challenge may name none");

        assert!(
            challenge_parts("www-authenticate: Basic realm=\"x\"").is_none(),
            "only a Bearer challenge is one this answers"
        );
    }

    /// The guest is Linux whatever the host is, so the only thing the host decides is which
    /// architecture's manifest is picked out of an index.
    #[test]
    fn the_platform_asked_for_is_linux_on_this_hosts_architecture() {
        let (os, arch) = platform();
        assert_eq!(os, "linux", "a boxdesk guest is Linux on every host");
        assert!(
            matches!(arch, "arm64" | "amd64"),
            "{arch} is not an architecture the OCI names"
        );
    }

    /// A digest becomes one directory name without losing which algorithm it is.
    #[test]
    fn a_digest_becomes_one_directory_name() {
        assert_eq!(safe_digest("sha256:abc123"), "sha256-abc123");
        assert!(
            !safe_digest("sha256:abc").contains(['/', ':']),
            "a path element cannot carry either"
        );
        assert_eq!(short("sha256:0123456789abcdef"), "0123456789ab");
    }

    /// A whiteout entry names the file it deletes relative to its own directory, and the opaque
    /// marker names the directory it empties.
    #[test]
    fn a_whiteout_entry_splits_into_its_directory_and_its_target() {
        assert_eq!(
            split_entry("usr/lib/.wh.old.so"),
            Some(("usr/lib", ".wh.old.so"))
        );
        assert_eq!(split_entry(".wh.top"), Some(("", ".wh.top")));
        assert_eq!(
            split_entry("var/log/.wh..wh..opq"),
            Some(("var/log", OPAQUE))
        );
        assert_eq!(
            split_entry("usr/lib/.wh.old.so").and_then(|(_, name)| name.strip_prefix(WHITEOUT)),
            Some("old.so"),
            "the marker names the file it deletes"
        );
    }

    /// **The first thing anybody pulls has to boot.** A stock image ships no `/results`, the root
    /// is read-only so the guest cannot make one, and a run mounts one by default — so a flatten
    /// that skipped this would refuse to boot the very first image a newcomer fetched.
    #[test]
    fn a_flattened_root_gets_the_mount_points_this_runtime_mounts() {
        let dir = boxdesk_test_support::ScratchDir::created("image-prepare");
        let root = dir.path().join("rootfs");
        std::fs::create_dir_all(root.join("bin")).expect("a tree");
        prepare_root(&root).expect("prepared");

        let results = root.join(boxdesk_record::RESULTS_GUEST_PATH.trim_start_matches('/'));
        assert!(
            results.is_dir(),
            "{} is where a run mounts its results",
            boxdesk_record::RESULTS_GUEST_PATH
        );
        // Twice, because a re-pull of an image already here runs over a tree that has it.
        prepare_root(&root).expect("preparing an already-prepared tree is not an error");
    }

    /// Layers are flattened in the order the manifest lists them, and a layer this build cannot
    /// fetch is refused rather than skipped: a tree missing one would look like it worked.
    #[test]
    fn a_foreign_layer_is_refused_rather_than_skipped() {
        let ordinary = serde_json::json!({
            "layers": [
                {"digest": "sha256:a", "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip"},
                {"digest": "sha256:b", "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip"},
            ]
        });
        let manifest = layers_of(&ordinary).expect("two ordinary layers");
        assert_eq!(
            manifest
                .layers
                .iter()
                .map(|l| l.digest.as_str())
                .collect::<Vec<_>>(),
            vec!["sha256:a", "sha256:b"],
            "the manifest's order is the order they are applied"
        );

        let foreign = serde_json::json!({
            "layers": [{
                "digest": "sha256:a",
                "mediaType": "application/vnd.docker.image.rootfs.foreign.diff.tar.gzip",
            }]
        });
        assert!(
            layers_of(&foreign).is_err(),
            "a foreign layer is not fetchable"
        );
        assert!(
            layers_of(&serde_json::json!({})).is_err(),
            "a manifest with no layers is not one"
        );
    }
}
