//! `cargo xtask init`: the guest tree a first run needs, on whatever host is here.
//!
//! - **The pinned Alpine minirootfs**, hash-checked by [`crate::artifacts`], unpacked where `tormoni`
//!   looks for a root when no flag names one.
//! - **The static agent beside it**, so `up`, `exec` and `shell` answer and not only `run`.
//! - **A fixture, not the image.** `apk.static` is a Linux ELF, so nothing here installs the
//!   runtimes, locks a closure, or claims the reproducibility `build-rootfs` does; the tree carries
//!   this user's ownership rather than `0:0`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use tormoni_channel::GUEST_AGENT_PATH;

use crate::artifacts::fetch_one;
use crate::guest_bins::build_guest_agent;
use crate::rootfs::{GuestArch, alpine_artifact};
use crate::run_tool;

/// Where a guest tree goes when nothing names one, under the per-user data directory: `tormoni`'s own
/// last fallback, which `the_default_guest_root_is_the_one_the_cli_looks_in` holds this to.
const GUEST_ROOT_UNDER_DATA: &str = "tormoni/rootfs";

/// The resolver a guest reads once `--net tsi` is granted; with no file at all a name never
/// resolves, and the minirootfs ships none.
const RESOLV_CONF: &str = "nameserver 1.1.1.1\nnameserver 9.9.9.9\n";

/// What a directory must already hold to be treated as a guest tree this may replace.
const TREE_MARKERS: [&str; 2] = ["bin", "usr"];

/// Puts a bootable guest tree at `root`, fetching the base and building the agent for `arch`.
pub(crate) fn init(root: Option<PathBuf>, arch: Option<String>, force: bool) -> Result<()> {
    let arch = arch
        .as_deref()
        .map_or_else(GuestArch::host, GuestArch::parse)?;
    let root = resolve_root(root)?;
    clear_destination(&root, force)?;
    write_tree(&root, arch)?;
    report(&root, arch);
    Ok(())
}

/// Writes the guest tree at `root` for `arch`: the base, the agent, the results mount point and
/// the resolver. `dist` archives one of these; `init` puts one where `tormoni` looks.
pub(crate) fn write_tree(root: &Path, arch: GuestArch) -> Result<()> {
    let base = alpine_artifact(arch);
    fetch_one(&base)?;
    std::fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
    run_tool(
        "tar",
        &[
            OsStr::new("-xzf"),
            base.dest.as_os_str(),
            OsStr::new("-C"),
            root.as_os_str(),
        ],
    )?;

    let agent = build_guest_agent(arch)?;
    install(&agent, &root.join(GUEST_AGENT_PATH.trim_start_matches('/')))?;
    // The root is read-only unless asked otherwise, so a mount point has to be in the tree
    // already: the guest cannot make one for the results directory every run gets.
    std::fs::create_dir_all(root.join("results")).context("create the /results mount point")?;
    std::fs::write(root.join("etc/resolv.conf"), RESOLV_CONF).context("write /etc/resolv.conf")
}

/// The tree's destination: the flag, else `$TORMONI_GUEST_ROOT`, else the per-user data directory, the
/// order `tormoni` itself resolves a root in.
fn resolve_root(flag: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(root) = flag.or_else(|| std::env::var_os("TORMONI_GUEST_ROOT").map(PathBuf::from)) {
        return Ok(root);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    data.map(|d| d.join(GUEST_ROOT_UNDER_DATA))
        .context("no HOME and no XDG_DATA_HOME: pass --root or set TORMONI_GUEST_ROOT")
}

/// Empties `root` for a fresh tree, refusing a directory that holds something other than one, so
/// `--force` cannot be pointed at a home directory and delete it.
fn clear_destination(root: &Path, force: bool) -> Result<()> {
    let Ok(mut entries) = std::fs::read_dir(root) else {
        return Ok(());
    };
    if entries.next().is_none() {
        return Ok(());
    }
    if !force {
        bail!(
            "{} already holds a tree — `--force` replaces it, or pass `--root` for somewhere else",
            root.display()
        );
    }
    if !TREE_MARKERS.iter().all(|m| root.join(m).is_dir()) {
        bail!(
            "{} holds something that is not a guest tree (no {}), so --force will not empty it; \
             remove it yourself if that is what you meant",
            root.display(),
            TREE_MARKERS.join(" and no ")
        );
    }
    std::fs::remove_dir_all(root).with_context(|| format!("replace {}", root.display()))
}

/// Copies `bin` to `dest` at 0755, making the directory above it first.
fn install(bin: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    std::fs::copy(bin, dest)
        .with_context(|| format!("copy {} to {}", bin.display(), dest.display()))?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("chmod 0755 {}", dest.display()))
}

/// Prints what the tree carries and what it does not, since what it lacks is the next question.
fn report(root: &Path, arch: GuestArch) {
    println!("\n✓ guest tree at {} ({})", root.display(), arch.name());
    println!("    {GUEST_AGENT_PATH}   the exec channel `up`, `exec` and `shell` speak");
    println!("    /results                     where a run's own output lands");
    println!("    /etc/resolv.conf             reachable only where `--net tsi` is granted");
    println!("\n  next: tormoni run -- /bin/busybox uname -a");
    println!(
        "  no runtimes here (python3, nodejs): `apk` is a Linux binary, so those come from \
         `cargo xtask build-rootfs` on Linux."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tormoni_test_support::ScratchDir;

    /// The default this writes to is the default `tormoni` reads from. Two crates, one path, and no
    /// constant either can share.
    ///
    /// It reads `crates/cli/src/lib.rs`, where `resolve_root` moved when the SDKs began calling it:
    /// a binding that built its own default would look somewhere `xtask init` never writes. This
    /// test caught that move, which is the whole reason it greps a file rather than trusting one.
    #[test]
    fn the_default_guest_root_is_the_one_the_cli_looks_in() {
        let cli = std::fs::read_to_string(crate::workspace_root().join("crates/cli/src/lib.rs"))
            .expect("read the CLI's root resolution");
        assert!(
            cli.contains(&format!("join(\"{GUEST_ROOT_UNDER_DATA}\")")),
            "crates/cli/src/lib.rs no longer joins {GUEST_ROOT_UNDER_DATA:?} onto the data \
             directory, so `xtask init` would write a tree `tormoni` does not look for"
        );
    }

    /// A tree already there is kept unless `--force` says otherwise.
    #[test]
    fn an_existing_tree_is_kept_unless_forced() {
        let scratch = ScratchDir::created("init-existing");
        for marker in TREE_MARKERS {
            std::fs::create_dir_all(scratch.path().join(marker)).unwrap();
        }
        let err = clear_destination(scratch.path(), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--force"), "{err}");
        assert!(scratch.path().join("bin").is_dir(), "kept");

        clear_destination(scratch.path(), true).unwrap();
        assert!(!scratch.path().exists(), "--force empties it");
    }

    /// `--force` on a directory that is not a guest tree deletes nothing: the flag is for
    /// replacing a tree, and the path it is pointed at may be anything.
    #[test]
    fn force_refuses_a_directory_that_is_not_a_guest_tree() {
        let scratch = ScratchDir::created("init-not-a-tree");
        std::fs::write(scratch.path().join("thesis.txt"), "years of work").unwrap();
        let err = clear_destination(scratch.path(), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a guest tree"), "{err}");
        assert!(scratch.path().join("thesis.txt").is_file(), "kept");
    }

    /// An empty directory is a destination, not something to refuse.
    #[test]
    fn an_empty_destination_is_ready_as_it_is() {
        let scratch = ScratchDir::created("init-empty");
        clear_destination(scratch.path(), false).unwrap();
        assert!(scratch.path().is_dir());
    }
}
