//! `install.sh`, held in the gate: the script `curl -fsSL https://raw.githubusercontent.com/kendricklawton/tormoni/main/install.sh | sh`
//! runs, which the release job uploads beside the artifacts.
//!
//! - **`sh -n` everywhere, `shellcheck` where it is.** The gate needs no tool it cannot name a
//!   package for; a host without shellcheck says so rather than passing quietly.
//! - **A dry run is the test.** `TORMONI_INSTALL_DRY_RUN=1` makes the script print every command
//!   that would change the machine and run none, downloads included, so the tests below drive
//!   it with a fake `uname` on `PATH` and read the plan back. What they cannot reach is said in
//!   `docs/running.md`: the terminal prompt, sudo, and the real install on each host.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The script, at the root of the tree.
pub(crate) const SCRIPT: &str = "install.sh";

/// Parses the script, and lints it where shellcheck is installed.
pub(crate) fn check(root: &Path) -> Result<()> {
    let script = root.join(SCRIPT);
    let parsed = Command::new("sh")
        .arg("-n")
        .arg(&script)
        .status()
        .context("running sh -n")?;
    if !parsed.success() {
        bail!("{SCRIPT} does not parse: see `sh -n` above");
    }
    if !crate::in_path("shellcheck") {
        println!(
            "· {SCRIPT}: shellcheck not installed, syntax checked with sh -n only \
             (brew install shellcheck / pacman -S shellcheck)"
        );
        return Ok(());
    }
    let linted = Command::new("shellcheck")
        .arg("--shell=sh")
        .arg(&script)
        .status()
        .context("running shellcheck")?;
    if !linted.success() {
        bail!("shellcheck found problems in {SCRIPT}: see above");
    }
    println!("· {SCRIPT}: sh -n and shellcheck clean");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tormoni_test_support::ScratchDir;

    /// One dry run of the script on a pretend host: what it printed, and how it exited.
    struct DryRun {
        status: std::process::ExitStatus,
        output: String,
    }

    impl DryRun {
        /// Runs the script as `os` on `arch` with `home` as the home directory and `env` on top.
        fn on(os: &str, arch: &str, home: &Path, env: &[(&str, &str)]) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let bin = home.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let uname = bin.join("uname");
            std::fs::write(
                &uname,
                "#!/bin/sh\ncase \"$1\" in -m) echo \"$FAKE_ARCH\" ;; *) echo \"$FAKE_OS\" ;; esac\n",
            )
            .unwrap();
            std::fs::set_permissions(&uname, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut cmd = Command::new("sh");
            cmd.arg(crate::workspace_root().join(SCRIPT))
                .env_clear()
                .env(
                    "PATH",
                    format!("{}:/usr/local/bin:/usr/bin:/bin", bin.display()),
                )
                .env("HOME", home)
                .env("FAKE_OS", os)
                .env("FAKE_ARCH", arch)
                .env("TORMONI_INSTALL_DRY_RUN", "1");
            for (key, value) in env {
                cmd.env(key, value);
            }
            let out = cmd.output().expect("sh runs the script");
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            Self {
                status: out.status,
                output,
            }
        }

        fn line_with(&self, needle: &str) -> bool {
            self.output.lines().any(|l| l.contains(needle))
        }
    }

    /// A Mac on Apple silicon gets the zip from the latest release, the app in /Applications,
    /// the symlink on PATH pointing into the bundle's Resources, and the guest tree under home.
    #[test]
    fn a_mac_on_apple_silicon_gets_the_app_and_a_symlink() {
        let home = ScratchDir::created("install-mac");
        let run = DryRun::on("Darwin", "arm64", home.path(), &[]);
        assert!(run.status.success(), "{}", run.output);
        assert!(
            run.line_with("releases/latest/download/Tormoni-macos-aarch64.zip"),
            "{}",
            run.output
        );
        assert!(
            run.line_with("+ mv ") && run.line_with("/Applications/Tormoni.app"),
            "{}",
            run.output
        );
        assert!(
            run.line_with(
                "+ ln -sf /Applications/Tormoni.app/Contents/Resources/tormoni /usr/local/bin/tormoni"
            ),
            "{}",
            run.output
        );
        let root = home.path().join(".local/share/tormoni/rootfs");
        assert!(
            run.line_with("+ tar -xzf") && run.line_with(&root.display().to_string()),
            "{}",
            run.output
        );
        assert!(run.line_with("+ open -a Tormoni"), "{}", run.output);
        assert!(!root.exists(), "a dry run unpacked a tree");
    }

    /// Linux on x86_64 takes the tarball into the first prefix whose bin is on PATH.
    #[test]
    fn linux_on_x86_64_installs_under_the_first_prefix_on_path() {
        let home = ScratchDir::created("install-linux");
        let run = DryRun::on("Linux", "x86_64", home.path(), &[]);
        assert!(run.status.success(), "{}", run.output);
        assert!(run.line_with("tormoni-linux-x86_64.tgz"), "{}", run.output);
        assert!(
            run.line_with("tar -xzf") && run.line_with("-C /usr/local"),
            "{}",
            run.output
        );
        assert!(
            !run.line_with("+ open -a"),
            "no app to open on Linux: {}",
            run.output
        );
    }

    /// The other two hosts are refused by name, so the message says what would have worked.
    #[test]
    fn the_other_two_hosts_are_refused_by_name() {
        for (os, arch) in [("Darwin", "x86_64"), ("Linux", "aarch64")] {
            let home = ScratchDir::created("install-refused");
            let run = DryRun::on(os, arch, home.path(), &[]);
            assert!(
                !run.status.success(),
                "{os}/{arch} was not refused: {}",
                run.output
            );
            assert!(
                run.line_with("macOS on ARM64") && run.line_with("Linux on x86_64"),
                "{}",
                run.output
            );
        }
    }

    /// A version pins the download to that tag's assets, with or without the `v`.
    #[test]
    fn a_version_pins_the_download_to_its_tag() {
        for version in ["0.0.5", "v0.0.5"] {
            let home = ScratchDir::created("install-version");
            let run = DryRun::on(
                "Darwin",
                "arm64",
                home.path(),
                &[("TORMONI_VERSION", version)],
            );
            assert!(run.status.success(), "{}", run.output);
            assert!(run.line_with("releases/download/v0.0.5/"), "{}", run.output);
        }
    }

    /// A guest tree the installer did not write is kept, since it may be one `cargo xtask init`
    /// or a person built; `TORMONI_REPLACE_ROOTFS=1` is the way to say otherwise.
    #[test]
    fn a_tree_the_installer_did_not_write_is_kept() {
        let home = ScratchDir::created("install-kept");
        let root = home.path().join(".local/share/tormoni/rootfs");
        for marker in ["bin", "usr"] {
            std::fs::create_dir_all(root.join(marker)).unwrap();
        }
        let kept = DryRun::on("Darwin", "arm64", home.path(), &[]);
        assert!(kept.status.success(), "{}", kept.output);
        assert!(kept.line_with("keeping it"), "{}", kept.output);
        assert!(!kept.line_with("+ rm -rf"), "{}", kept.output);

        let replaced = DryRun::on(
            "Darwin",
            "arm64",
            home.path(),
            &[("TORMONI_REPLACE_ROOTFS", "1")],
        );
        assert!(
            replaced.line_with(&format!("+ rm -rf {}", root.display())),
            "{}",
            replaced.output
        );
        assert!(root.join("bin").is_dir(), "a dry run removed the tree");
    }
}
