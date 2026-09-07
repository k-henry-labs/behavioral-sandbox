//! The `.app` the window runs as, so macOS names it what a person calls it.
//!
//! - **The menu bar reads a bundle, not a binary.** A bare executable is named in the menu bar by
//!   its file name, which is why a `target/debug/bsx-app` window says `bsx-app`. `CFBundleName` is
//!   what replaces it, and only a bundle has one.
//! - **The pair travels together.** `bsx` is copied in beside `bsx-app`, because that is the
//!   second place `bsx_path` looks and the only one a double-clicked app can rely on: Finder
//!   starts it with a login `PATH` that no `cargo` layout is on.
//! - **The copy is signed, not the original.** `codesign` writes a new inode, so the `bsx` inside
//!   the bundle is entitled after it is copied; entitling the one in `target/` would not travel.
//! - **Assembled, never edited in place.** The bundle is removed and rebuilt, so a stale binary
//!   from an older build cannot survive inside it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{artifacts_dir, sign, target_dir};

/// What the menu bar, the Dock and the About window call it: the product's name, not its
/// initials, which stay on the bundle's file name and the CLI.
const APP_NAME: &str = "Behavioral Sandbox";

/// The bundle's own directory name.
const BUNDLE: &str = "BSX.app";

/// The binary the bundle runs, and the one copied in beside it.
const EXECUTABLE: &str = "bsx-app";
const CLI: &str = "bsx";

/// Assembles `artifacts/BSX.app` from the built binaries, or explains why there is nothing to do.
pub(crate) fn bundle_app(release: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        // Said rather than passed over: a step that silently does nothing reads as a step that
        // worked, and a `.app` is a macOS shape no other platform reads.
        println!("bundle: nothing to bundle on this host (a .app is macOS's)");
        return Ok(());
    }

    let built = target_dir().join(if release { "release" } else { "debug" });
    for binary in [EXECUTABLE, CLI] {
        if !built.join(binary).is_file() {
            bail!(
                "no {binary} at {} — build it first with `cargo build{}`",
                built.join(binary).display(),
                if release { " --release" } else { "" }
            );
        }
    }

    let app = bundle_path();
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .with_context(|| format!("clearing the previous {}", app.display()))?;
    }
    let macos = macos_dir(&app);
    std::fs::create_dir_all(&macos).with_context(|| format!("creating {}", macos.display()))?;

    std::fs::copy(built.join(EXECUTABLE), executable_in(&app))
        .with_context(|| format!("copying {EXECUTABLE} into {}", macos.display()))?;
    std::fs::copy(built.join(CLI), macos.join(CLI))
        .with_context(|| format!("copying {CLI} into {}", macos.display()))?;
    std::fs::write(
        app.join("Contents/Info.plist"),
        info_plist(env!("CARGO_PKG_VERSION")),
    )
    .context("writing Info.plist")?;

    // The copy is what will run a VM, so it is the copy that has to carry the entitlement.
    sign::sign_binary_for_hypervisor(&macos.join(CLI))?;

    println!("bundle: {} runs as {APP_NAME}", app.display());
    Ok(())
}

/// The bundle's `Info.plist`: `CFBundleName` is the string the menu bar shows.
fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>dev.bsx.app</string>
    <key>CFBundleExecutable</key>
    <string>{EXECUTABLE}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
</dict>
</plist>
"#
    )
}

/// Where the assembled bundle lands, so a reader and a test name one path.
pub(crate) fn bundle_path() -> PathBuf {
    artifacts_dir().join(BUNDLE)
}

/// Where a bundle keeps what it runs, by the layout Apple defines.
fn macos_dir(app: &Path) -> PathBuf {
    app.join("Contents/MacOS")
}

/// The binary inside `app` that macOS starts.
fn executable_in(app: &Path) -> PathBuf {
    macos_dir(app).join(EXECUTABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plist names the app what a person calls it and starts the binary that is copied in:
    /// a `CFBundleName` that drifted from [`APP_NAME`] is a menu bar reading something else, and
    /// a `CFBundleExecutable` that drifted from the copied file is a bundle that will not open.
    #[test]
    fn the_plist_names_the_app_and_the_binary_it_starts() {
        let plist = info_plist("1.2.3");
        let pack = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
        let packed = pack(&plist);
        assert!(
            packed.contains(&pack(&format!(
                "<key>CFBundleName</key><string>{APP_NAME}</string>"
            ))),
            "the menu bar reads CFBundleName: {plist}"
        );
        assert!(
            packed.contains(&pack(&format!(
                "<key>CFBundleExecutable</key><string>{EXECUTABLE}</string>"
            ))),
            "the bundle starts the binary it carries: {plist}"
        );
        assert!(packed.contains("<string>1.2.3</string>"), "{plist}");
    }

    /// The layout is Apple's: the executable sits under `Contents/MacOS`, which is where
    /// `CFBundleExecutable` is resolved from and where `bsx` lands beside it.
    #[test]
    fn the_executable_sits_where_macos_looks_for_it() {
        let app = Path::new("/tmp/BSX.app");
        assert_eq!(
            executable_in(app),
            Path::new("/tmp/BSX.app/Contents/MacOS/bsx-app")
        );
        assert!(
            bundle_path().ends_with("artifacts/BSX.app"),
            "{:?}",
            bundle_path()
        );
    }
}
