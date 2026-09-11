//! `cargo xtask dist`: this host's release, packaged with its guest tree, as `install.sh`
//! fetches it.
//!
//! - **One artifact per host.** macOS ARM64 packs `Boxdesk.app`, with `boxdesk` and the guest
//!   tree under `Contents/Resources`, as a zip; Linux x86_64 packs `bin/`, the desktop entry, the
//!   icon and the tree as a tarball. [`artifact_name`] is the contract `install.sh` downloads.
//! - **The guest tree is the one `init` writes**, built here for the host's guest and archived,
//!   so an install is whole once libkrun is present.
//! - **`SHA256SUMS` beside it**, in `sha256sum` text form, which the installer verifies.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::artifacts::sha256_of;
use crate::bundle::{self, Payload};
use crate::rootfs::GuestArch;
use crate::{app_icon, artifacts_dir, cargo, dist_dir, init, run_tool, target_dir};

/// Builds, packages and checksums this host's release under `dist/`.
pub(crate) fn dist() -> Result<()> {
    let name = artifact_name(std::env::consts::OS, std::env::consts::ARCH)?;
    cargo(&[
        "build",
        "--release",
        "--locked",
        "-p",
        "boxdesk-app",
        "-p",
        "boxdesk",
    ])?;
    let rootfs = guest_tree_archive()?;
    let out = dist_dir();
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let artifact = out.join(&name);
    if cfg!(target_os = "macos") {
        let app = bundle::assemble(true, &[(&rootfs, "rootfs.tar.gz")])?;
        let _ = std::fs::remove_file(&artifact);
        run_tool(
            "ditto",
            &[
                "-c".as_ref(),
                "-k".as_ref(),
                // No resource forks or extended attributes: `unzip` writes those as `._` files inside
                // the bundle, which the seal does not cover.
                "--norsrc".as_ref(),
                "--keepParent".as_ref(),
                app.as_os_str(),
                artifact.as_os_str(),
            ],
        )?;
    } else {
        stage_linux(&rootfs, &artifact)?;
    }
    let digest = sha256_of(&artifact)?;
    std::fs::write(out.join("SHA256SUMS"), sums(&[(&name, &digest)]))
        .context("writing SHA256SUMS")?;
    println!("dist: {} and SHA256SUMS", artifact.display());
    Ok(())
}

/// The asset an `install.sh` on `os` and `arch` downloads, or why there is none.
pub(crate) fn artifact_name(os: &str, arch: &str) -> Result<String> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("Boxdesk-macos-aarch64.zip".to_string()),
        ("linux", "x86_64") => Ok("boxdesk-linux-x86_64.tgz".to_string()),
        _ => bail!(
            "no release is built for {os} on {arch}: the two are macOS on aarch64 and Linux on \
             x86_64"
        ),
    }
}

/// `SHA256SUMS` in `sha256sum` text form: the hex, two spaces, the bare file name.
pub(crate) fn sums(entries: &[(&str, &str)]) -> String {
    entries
        .iter()
        .map(|(name, hex)| format!("{hex}  {name}\n"))
        .collect()
}

/// The guest tree `init` writes, for this host's guest, archived under `artifacts/`.
fn guest_tree_archive() -> Result<PathBuf> {
    let arch = GuestArch::host()?;
    let tree = artifacts_dir().join(format!("dist-rootfs-{}", arch.name()));
    if tree.exists() {
        std::fs::remove_dir_all(&tree).with_context(|| format!("clearing {}", tree.display()))?;
    }
    std::fs::create_dir_all(&tree).with_context(|| format!("creating {}", tree.display()))?;
    init::write_tree(&tree, arch)?;
    let archive = artifacts_dir().join("rootfs.tar.gz");
    run_tool(
        "tar",
        &[
            "-czf".as_ref(),
            archive.as_os_str(),
            "-C".as_ref(),
            tree.as_os_str(),
            ".".as_ref(),
        ],
    )?;
    Ok(archive)
}

/// Stages the Linux layout under one top-level directory and tars it owned by root, since a
/// root `tar -x` keeps whatever the archive says.
fn stage_linux(rootfs: &Path, artifact: &Path) -> Result<()> {
    let top = artifact
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".tgz"))
        .context("a .tgz artifact name")?;
    let stage = dist_dir().join("stage");
    let root = stage.join(top);
    if root.exists() {
        std::fs::remove_dir_all(&root).with_context(|| format!("clearing {}", root.display()))?;
    }
    let built = target_dir().join("release");
    for (rel, payload) in bundle::linux_layout() {
        let dest = root.join(&rel);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        match payload {
            Payload::Binary(name) => {
                std::fs::copy(built.join(name), &dest)
                    .with_context(|| format!("staging {name}"))?;
                executable(&dest)?;
            }
            Payload::Desktop => {
                std::fs::write(&dest, bundle::desktop_entry())
                    .context("staging the desktop entry")?;
            }
            Payload::Icon => {
                std::fs::copy(app_icon::png(), &dest).context("staging the icon")?;
            }
            Payload::Rootfs => {
                std::fs::copy(rootfs, &dest).context("staging the guest tree")?;
            }
        }
    }
    archive_linux(&root, artifact)
}

/// Tars the staged tree at `root` into `artifact`, its top-level directories at the archive's
/// own root.
///
/// **No wrapper directory.** `install.sh` untars this straight into `/usr/local`, so a member
/// named `<name>/bin/boxdesk` would install to `/usr/local/<name>/bin/boxdesk` and nothing would
/// be on `PATH`. Owned by 0:0, since a root `tar -x` keeps whatever the archive says.
fn archive_linux(root: &Path, artifact: &Path) -> Result<()> {
    let mut args: Vec<&OsStr> = vec![
        "--owner=0".as_ref(),
        "--group=0".as_ref(),
        "--numeric-owner".as_ref(),
        "-czf".as_ref(),
        artifact.as_os_str(),
        "-C".as_ref(),
        root.as_os_str(),
    ];
    let tops = linux_top_levels();
    args.extend(tops.iter().map(|t| OsStr::new(t.as_str())));
    run_tool("tar", &args)
}

/// The directories the Linux layout puts at the archive's root, in order, each once.
fn linux_top_levels() -> Vec<String> {
    let mut tops: Vec<String> = Vec::new();
    for (rel, _) in bundle::linux_layout() {
        if let Some(top) = rel.split('/').next()
            && !tops.iter().any(|t| t == top)
        {
            tops.push(top.to_string());
        }
    }
    tops
}

/// Marks a staged binary 0755, which a copy does not carry over.
fn executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("chmod 0755 {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two asset names are what `install.sh` downloads, spelled exactly; any other host is
    /// refused naming the two.
    #[test]
    fn the_asset_names_are_the_installers_contract() {
        assert_eq!(
            artifact_name("macos", "aarch64").unwrap(),
            "Boxdesk-macos-aarch64.zip"
        );
        assert_eq!(
            artifact_name("linux", "x86_64").unwrap(),
            "boxdesk-linux-x86_64.tgz"
        );
        let why = artifact_name("linux", "aarch64").unwrap_err().to_string();
        assert!(
            why.contains("macOS on aarch64") && why.contains("Linux on x86_64"),
            "{why}"
        );
    }

    /// The archive carries `bin/` and `share/` at its own root, because `install.sh` untars it
    /// into `/usr/local` without stripping anything: a wrapper directory would put every file
    /// one level too deep, leaving nothing on `PATH` and no guest tree where the CLI looks.
    ///
    /// The staged tree holds one file per layout entry; on a case-insensitive host the two
    /// binary names fold into one, which costs a member and not the property under test.
    #[test]
    fn the_linux_archive_has_no_wrapper_directory() {
        assert_eq!(linux_top_levels(), ["bin", "share"]);
        let scratch = boxdesk_test_support::ScratchDir::created("dist-linux");
        let root = scratch.path().join("stage");
        for (rel, _) in bundle::linux_layout() {
            let dest = root.join(&rel);
            std::fs::create_dir_all(dest.parent().expect("a parent")).expect("staged");
            std::fs::write(&dest, b"x").expect("staged");
        }
        let artifact = scratch.path().join("boxdesk-linux-x86_64.tgz");
        archive_linux(&root, &artifact).expect("archived");

        let out = std::process::Command::new("tar")
            .arg("-tzf")
            .arg(&artifact)
            .output()
            .expect("tar lists the archive");
        let members: Vec<&str> = std::str::from_utf8(&out.stdout)
            .expect("utf-8")
            .lines()
            .filter(|m| !m.ends_with('/'))
            .collect();
        for member in &members {
            assert!(
                member.starts_with("bin/") || member.starts_with("share/"),
                "{member} is not at the archive's root, so it would install one level too deep"
            );
        }
        assert!(members.contains(&"bin/boxdesk"), "{members:?}");
        assert!(
            members.iter().any(|m| m.starts_with("share/")),
            "{members:?}"
        );
    }

    /// `sha256sum -c` reads the file back: hex, two spaces, a bare name, one per line.
    #[test]
    fn the_sums_are_in_sha256sum_text_form() {
        assert_eq!(
            sums(&[("a.zip", "0123"), ("b.tgz", "4567")]),
            "0123  a.zip\n4567  b.tgz\n"
        );
    }
}
