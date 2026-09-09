//! Obtaining a pinned upstream input: download or restore from the vendor mirror, sha256-verify,
//! and cache under `artifacts/`. The sha256 is the contract; the URL is replaceable.
//!
//! The machinery `rootfs.rs` uses for the Alpine base and the static `apk`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::vendor_dir;

/// A pinned boot artifact: a stable URL, its expected sha256 (the real contract, the URL is
/// replaceable), and where it lands under `artifacts/`.
pub(crate) struct Artifact {
    pub(crate) url: String,
    pub(crate) sha256: &'static str,
    pub(crate) dest: PathBuf,
}

/// Obtains one artifact into place, from the `TORMONI_VENDOR_DIR` mirror when set and its pinned URL
/// otherwise. The sha256 is the contract either way, and every build path comes through here.
pub(crate) fn fetch_one(a: &Artifact) -> Result<()> {
    match vendor_dir() {
        Some(v) => restore_from_vendor(a, &v),
        None => download_one(a),
    }
}

/// The final path component of an artifact's `dest`, as a display string, the name it carries both
/// under `artifacts/` and in the vendor mirror.
fn artifact_name(a: &Artifact) -> String {
    a.dest.file_name().map_or_else(
        || a.dest.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Restores one artifact from the local vendor mirror, sha-verified and offline. A missing file
/// is an error naming `cargo xtask vendor`, never a silent fallback to the network.
fn restore_from_vendor(a: &Artifact, vendor: &Path) -> Result<()> {
    let name = artifact_name(a);
    if a.dest.is_file() && sha256_of(&a.dest)? == a.sha256 {
        println!("✓ {name} already present (sha256 ok)");
        return Ok(());
    }
    let src = vendor.join(&name);
    if !src.is_file() {
        bail!(
            "vendored input {name} not found in {} — run `cargo xtask vendor` to populate the \
             mirror (or unset TORMONI_VENDOR_DIR to fetch from upstream)",
            vendor.display()
        );
    }
    let got = sha256_of(&src)?;
    if got != a.sha256 {
        bail!(
            "vendored {name} sha256 mismatch: expected {}, got {got}",
            a.sha256
        );
    }
    if let Some(parent) = a.dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::copy(&src, &a.dest).with_context(|| format!("copy vendored {name} into place"))?;
    println!("✓ {name} restored from vendor (sha256 ok)");
    Ok(())
}

/// Downloads one artifact into place unless it is already there with the right hash, through a
/// `.part` renamed only after the hash verifies. The raw upstream fetch, which `vendor` calls
/// directly to populate the mirror.
pub(crate) fn download_one(a: &Artifact) -> Result<()> {
    let name = a
        .dest
        .file_name()
        .map_or_else(|| a.dest.clone(), PathBuf::from);
    if a.dest.is_file() && sha256_of(&a.dest)? == a.sha256 {
        println!("✓ {} already present (sha256 ok)", name.display());
        return Ok(());
    }
    if let Some(parent) = a.dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    println!("↓ {} <- {}", name.display(), a.url);
    // Per-pid temp name so two concurrent `xtask` fetches into the same dir can't interleave writes
    // to one `.part` (each verifies its own, then renames onto the shared final path atomically).
    let part = a
        .dest
        .with_extension(format!("part.{}", std::process::id()));
    if let Err(e) = curl_download(&a.url, &part) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    // Clean up the `.part` on *any* verify failure, including a `sha256sum` that can't run, so a
    // failed check never leaves a temp file behind.
    let got = match sha256_of(&part) {
        Ok(got) => got,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
    };
    if got != a.sha256 {
        let _ = std::fs::remove_file(&part);
        bail!(
            "sha256 mismatch for {}: expected {}, got {} (removed)",
            name.display(),
            a.sha256,
            got
        );
    }
    std::fs::rename(&part, &a.dest)
        .with_context(|| format!("move {} into place", part.display()))?;
    println!("✓ {} verified", name.display());
    Ok(())
}

/// Fetches a pinned release zip and unpacks it under `artifacts/<stem>`, returning that directory.
///
/// `download_one`, not [`fetch_one`]: the two font cuts are dev steps whose output is committed, so
/// they are not on the offline build path, and `vendor` has never mirrored their archives. Going
/// through the mirror would fail them with a message naming a command that cannot help.
pub(crate) fn unpack_release(url: &str, sha256: &'static str, stem: &str) -> Result<PathBuf> {
    let zip = crate::artifacts_dir().join(format!("{stem}.zip"));
    download_one(&Artifact {
        url: url.to_string(),
        sha256,
        dest: zip.clone(),
    })?;
    let unpacked = crate::artifacts_dir().join(stem);
    let _ = std::fs::remove_dir_all(&unpacked);
    let out = Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&zip)
        .arg("-d")
        .arg(&unpacked)
        .output()
        .context("running unzip (each pinned font release ships a zip)")?;
    if !out.status.success() {
        bail!(
            "unzip failed on {}: {}",
            zip.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(unpacked)
}

/// `curl -fSL` a URL to `dest` (fail on HTTP error, follow redirects).
fn curl_download(url: &str, dest: &Path) -> Result<()> {
    crate::run_tool(
        "curl",
        &[
            std::ffi::OsStr::new("-fSL"),
            std::ffi::OsStr::new("-o"),
            dest.as_os_str(),
            std::ffi::OsStr::new(url),
        ],
    )
}

/// The hashers a host may have, tried in order: `sha256sum` (coreutils), then `shasum -a 256`
/// (Perl's, on every macOS). No hashing crate on the dev-tooling path.
const HASHERS: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];

/// The sha256 of a file, from the first hasher this host has.
pub(crate) fn sha256_of(path: &Path) -> Result<String> {
    sha256_with(&HASHERS, path)
}

/// [`sha256_of`] over `hashers`: a hasher that is not installed is skipped for the next, and any
/// other failure is the answer.
fn sha256_with(hashers: &[(&str, &[&str])], path: &Path) -> Result<String> {
    for (program, args) in hashers {
        let out = match Command::new(program).args(*args).arg(path).output() {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("running {program}")),
        };
        if !out.status.success() {
            bail!("{program} failed for {}", path.display());
        }
        let text = String::from_utf8(out.stdout).context("hasher output not UTF-8")?;
        let hash = text
            .split_whitespace()
            .next()
            .with_context(|| format!("empty {program} output"))?;
        return Ok(hash.to_string());
    }
    bail!(
        "no sha256 tool here: none of {} is installed",
        hashers
            .iter()
            .map(|(p, _)| *p)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hasher that is not installed is passed over for the next; the digest of an empty file
    /// is the one every sha256 tool answers.
    #[test]
    fn a_missing_hasher_is_skipped_for_the_next() {
        let scratch = tormoni_test_support::ScratchDir::created("sha256");
        let empty = scratch.path().join("empty");
        std::fs::write(&empty, b"").unwrap();
        let digest = sha256_with(
            &[("no-such-hasher-here", &[]), ("shasum", &["-a", "256"])],
            &empty,
        )
        .expect("the second hasher answers");
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let why = sha256_with(&[("no-such-hasher-here", &[])], &empty).unwrap_err();
        assert!(why.to_string().contains("no sha256 tool"), "{why}");
    }
}
