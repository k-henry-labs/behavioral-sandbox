//! The `.app` the window runs as, so macOS names it what a person calls it.
//!
//! - **The executable is named `Tormoni`, and the platform reads that name.** macOS names the
//!   menu bar, the About, Hide and Quit items under it, and the Dock from the executable's file
//!   name (measured on this host, macOS 26.6.2, 2026-09-09), so a bare `target/debug/Tormoni`
//!   reads right; the bundle adds what a file name cannot carry: the identifier and the icon.
//!   `the_bundle_runs_the_binary_the_manifest_names` holds `CFBundleExecutable` to the
//!   manifest's `[[bin]]`.
//! - **The pair travels together, laid out as Ollama lays its own out.** `Tormoni` is the
//!   executable under `Contents/MacOS`; `tormoni` sits under `Contents/Resources`, where
//!   `tormoni_path` looks and where an installer's `/usr/local/bin/tormoni` symlink points.
//!   Finder starts the app with a login `PATH` no `cargo` layout is on, so the bundle is the one
//!   place a double-clicked app can rely on.
//! - **The CLI copy is signed first, then the bundle is sealed.** The seal hashes every file
//!   under `Contents/Resources`, so the entitlement goes on before it; `codesign` writes a new
//!   inode, so entitling the one in `target/` would not travel.
//! - **Assembled, never edited in place.** The bundle is removed and rebuilt, so a stale binary
//!   from an older build cannot survive inside it.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{app_icon, artifacts_dir, cargo, sign, target_dir, workspace_root};

/// The application's name, which is also its executable's: what the menu bar, the Dock, Finder
/// and the About window call it. The command line binary keeps the lowercase `tormoni`, where a
/// person types it.
pub(crate) const APP: &str = "Tormoni";

/// The bundle's own directory name, which Finder shows as the application's.
const BUNDLE: &str = "Tormoni.app";

/// The command line binary, copied in beside the application. Its name is the command a person
/// types, so it is also what cargo builds it as.
const CLI: &str = "tormoni";

/// What cargo builds the application as, which is **not** what it ships as.
///
/// One target directory holds every binary of the workspace, and macOS's default filesystem is
/// case-insensitive, so building a `Tormoni` beside `tormoni` would write one file and the
/// release would carry whichever was linked last.
/// `the_two_binaries_cannot_collide_in_one_directory` is the guard.
pub(crate) const BUILT_APP: &str = "tormoni-app";

/// The icon inside the bundle, which the plist names.
const ICON: &str = "Tormoni.icns";

/// The identifier the application registers under: the bundle's `CFBundleIdentifier`, the Linux
/// window's app id, and the desktop entry's file name. `the_app_id_is_the_one_the_window_carries`
/// holds it to the app's own copy.
pub(crate) const APP_ID: &str = "ai.tormoni.app";

/// Assembles `artifacts/Tormoni.app` from the built binaries, or explains why there is
/// nothing to do.
pub(crate) fn bundle_app(release: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        // Said rather than passed over: a step that silently does nothing reads as a step that
        // worked, and a `.app` is a macOS shape no other platform reads.
        println!("bundle: nothing to bundle on this host (a .app is macOS's)");
        return Ok(());
    }
    assemble(release, &[]).map(|_| ())
}

/// Assembles the bundle from the built binaries, with `extras` copied into `Contents/Resources`
/// under the names given, and answers where it landed. The order at the end is the mechanism:
/// the CLI copy is entitled, then the bundle is sealed over it.
pub(crate) fn assemble(release: bool, extras: &[(&Path, &str)]) -> Result<PathBuf> {
    let built = target_dir().join(if release { "release" } else { "debug" });
    for binary in [BUILT_APP, CLI] {
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
    let resources = resources_dir(&app);
    for dir in [&macos, &resources] {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }

    std::fs::copy(built.join(BUILT_APP), executable_in(&app))
        .with_context(|| format!("copying {BUILT_APP} into {} as {APP}", macos.display()))?;
    std::fs::copy(built.join(CLI), cli_in(&app))
        .with_context(|| format!("copying {CLI} into {}", resources.display()))?;
    for (from, name) in extras {
        std::fs::copy(from, resources.join(name))
            .with_context(|| format!("copying {} into {}", from.display(), resources.display()))?;
    }
    std::fs::copy(workspace_root().join(app_icon::ICNS), resources.join(ICON))
        .with_context(|| format!("copying {} into the bundle", app_icon::ICNS))?;
    std::fs::write(
        app.join("Contents/Info.plist"),
        info_plist(env!("CARGO_PKG_VERSION")),
    )
    .context("writing Info.plist")?;

    // The copy is what will run a VM, so it is the copy that has to carry the entitlement, and
    // the seal that follows hashes it as it then is.
    sign::sign_binary_for_hypervisor(&cli_in(&app))?;
    sign::seal_bundle(&app)?;

    println!("bundle: {} runs as {APP}", app.display());
    Ok(app)
}

/// The bundle's `Info.plist`: `CFBundleName` is the string the menu bar shows.
fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{APP}</string>
    <key>CFBundleDisplayName</key>
    <string>{APP}</string>
    <key>CFBundleIdentifier</key>
    <string>{APP_ID}</string>
    <key>CFBundleExecutable</key>
    <string>{APP}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>CFBundleIconFile</key>
    <string>{ICON}</string>
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

/// Builds the pair, assembles the bundle, and starts the notebook from inside it, so the
/// platform reads its name from `CFBundleName`. Arguments in `args` reach the app.
///
/// **The bundle is the mechanism, not a packaging step.** The Dock's label and the menu bar name
/// a bare executable by its file name, and only a bundle carries a name of its own; running the
/// copy inside one is what lends the process that identity while its output stays on the
/// terminal, which `open` would take away.
pub(crate) fn run_app(release: bool, args: &[String]) -> Result<()> {
    let mut build = vec!["build", "-p", "tormoni-app", "-p", "tormoni"];
    if release {
        build.push("--release");
    }
    cargo(&build)?;
    if cfg!(target_os = "macos") {
        bundle_app(release)?;
    }
    let built = target_dir().join(if release { "release" } else { "debug" });
    let program = app_program(&built);
    println!("$ {}", program.display());
    let status = Command::new(&program)
        .args(args)
        .status()
        .with_context(|| format!("starting {}", program.display()))?;
    if !status.success() {
        bail!("{} exited {status}", program.display());
    }
    Ok(())
}

/// Where the app is started from: the copy inside the bundle on macOS, since that is what names
/// it, and the built binary on a platform that has no bundle to run from.
fn app_program(built: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        executable_in(&bundle_path())
    } else {
        built.join(BUILT_APP)
    }
}

/// Where the assembled bundle lands, so a reader and a test name one path.
fn bundle_path() -> PathBuf {
    artifacts_dir().join(BUNDLE)
}

/// Where a bundle keeps what it runs, by the layout Apple defines.
fn macos_dir(app: &Path) -> PathBuf {
    app.join("Contents/MacOS")
}

/// The binary inside `app` that macOS starts.
fn executable_in(app: &Path) -> PathBuf {
    macos_dir(app).join(APP)
}

/// Where a bundle keeps what it carries beside its executable.
fn resources_dir(app: &Path) -> PathBuf {
    app.join("Contents/Resources")
}

/// The command line binary inside `app`: where Ollama keeps its own, and where an installer's
/// symlink on `PATH` points.
pub(crate) fn cli_in(app: &Path) -> PathBuf {
    resources_dir(app).join(CLI)
}

/// The desktop entry a Linux launcher reads: it starts the executable by name, so `bin/` must be
/// on `PATH`, and names the icon and the window by [`APP_ID`], which is what pairs a window
/// with this entry.
pub(crate) fn desktop_entry() -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={APP}\nComment=Sandboxes on this machine, live \
         and past\nExec={APP}\nIcon={APP_ID}\nTerminal=false\nCategories=Development;\n\
         StartupWMClass={APP_ID}\n"
    )
}

/// What the Linux release carries, by path under its one top-level directory: the two binaries
/// under `bin/`, and under `share/` the desktop entry, the icon, and the guest tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Payload {
    /// A built binary, by its file name under the build directory.
    Binary(&'static str),
    /// The desktop entry, written from [`desktop_entry`].
    Desktop,
    /// The icon, from the tree.
    Icon,
    /// The guest tree, as the archive `dist` wrote.
    Rootfs,
}

/// The Linux layout: each relative path and what fills it.
pub(crate) fn linux_layout() -> Vec<(String, Payload)> {
    vec![
        (format!("bin/{CLI}"), Payload::Binary(CLI)),
        (format!("bin/{APP}"), Payload::Binary(BUILT_APP)),
        (
            format!("share/applications/{APP_ID}.desktop"),
            Payload::Desktop,
        ),
        (
            format!("share/icons/hicolor/512x512/apps/{APP_ID}.png"),
            Payload::Icon,
        ),
        ("share/tormoni/rootfs.tar.gz".to_string(), Payload::Rootfs),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plist names the app what a person calls it and starts the binary that is copied in:
    /// a `CFBundleName` that drifted from [`APP`] is a menu bar reading something else, and
    /// a `CFBundleExecutable` that drifted from the copied file is a bundle that will not open.
    #[test]
    fn the_plist_names_the_app_and_the_binary_it_starts() {
        let plist = info_plist("1.2.3");
        let pack = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
        let packed = pack(&plist);
        assert!(
            packed.contains(&pack(&format!(
                "<key>CFBundleName</key><string>{APP}</string>"
            ))),
            "the menu bar reads CFBundleName: {plist}"
        );
        assert!(
            packed.contains(&pack(&format!(
                "<key>CFBundleExecutable</key><string>{APP}</string>"
            ))),
            "the bundle starts the binary it carries: {plist}"
        );
        assert!(packed.contains("<string>1.2.3</string>"), "{plist}");
    }

    /// The string literal following `marker` in `source`, empty where there is none.
    fn quoted_after(source: &str, marker: &str) -> String {
        let found = source
            .split_once(marker)
            .and_then(|(_, rest)| rest.strip_prefix('"'))
            .and_then(|rest| rest.split_once('"'));
        found
            .map(|(literal, _)| literal.to_string())
            .unwrap_or_default()
    }

    /// One name: the manifest's `[[bin]]` is the file cargo writes and `CFBundleExecutable` is
    /// the file macOS starts, so a drift between them is a bundle that will not open, and a
    /// menu bar reading something else. Read as text, because xtask does not build the GUI crate.
    #[test]
    fn the_bundle_runs_the_binary_the_manifest_names() {
        let manifest =
            std::fs::read_to_string(crate::workspace_root().join("crates/app/Cargo.toml"))
                .expect("the app's manifest");
        let (_, bin) = manifest.split_once("[[bin]]").expect("a [[bin]] table");
        assert_eq!(
            quoted_after(bin, "name = "),
            BUILT_APP,
            "crates/app/Cargo.toml builds a binary the bundle would not find"
        );
        assert!(
            info_plist("1.2.3").contains(&format!("<string>{APP}</string>")),
            "the bundle starts what it copied in as {APP}"
        );
    }

    /// The two binaries share one target directory, and the default macOS filesystem folds case,
    /// so names differing only in case would be one file: the release would carry the same
    /// program twice, and which one is a race between two linker runs. This is not theory; it
    /// shipped a bundle whose `tormoni` was the notebook.
    #[test]
    fn the_two_binaries_cannot_collide_in_one_directory() {
        assert_ne!(
            BUILT_APP.to_lowercase(),
            CLI.to_lowercase(),
            "cargo would write {BUILT_APP} and {CLI} to one path where the filesystem folds case"
        );
    }

    /// The verb exists because a binary started out of `target/` is named by its file name
    /// wherever the platform shows it, so what it starts on macOS has to be the copy inside the
    /// bundle. Starting the built one would leave the Dock reading the build directory's copy,
    /// which is the whole of what the verb is for.
    #[test]
    fn the_app_is_started_from_inside_the_bundle_where_there_is_one() {
        let built = Path::new("/tmp/target/debug");
        let program = app_program(built);
        if cfg!(target_os = "macos") {
            assert_eq!(program, executable_in(&bundle_path()), "{program:?}");
            assert!(
                program.starts_with(artifacts_dir()),
                "it must be the assembled copy, not the built one: {program:?}"
            );
        } else {
            assert_eq!(program, built.join(APP), "{program:?}");
        }
    }

    /// The plist names an icon the tree holds, as an `.icns`; a renamed or missing file would
    /// be a bundle with the generic icon and nothing saying so.
    #[test]
    fn the_plist_names_an_icon_the_tree_holds() {
        assert!(
            info_plist("1.2.3").contains(&format!("<string>{ICON}</string>")),
            "the plist does not name {ICON}"
        );
        let bytes = std::fs::read(crate::workspace_root().join(app_icon::ICNS))
            .expect("crates/app/icon/Tormoni.icns: run `cargo xtask app-icon`");
        assert_eq!(&bytes[..4], b"icns", "not an icns file");
        let png =
            std::fs::read(app_icon::png()).expect("the desktop icon: run `cargo xtask app-icon`");
        assert_eq!(&png[..4], b"\x89PNG", "not a PNG");
    }

    /// The CLI sits under `Contents/Resources`, as Ollama keeps its own: the place the app
    /// looks, and the place an installer's symlink names.
    #[test]
    fn the_cli_sits_in_resources_as_ollama_keeps_its_own() {
        assert_eq!(
            cli_in(Path::new("/tmp/Tormoni.app")),
            Path::new("/tmp/Tormoni.app/Contents/Resources/tormoni")
        );
    }

    /// The identifier the plist registers is the one the window carries on Linux, read from
    /// the app's source as text, since xtask does not build the GUI crate.
    #[test]
    fn the_app_id_is_the_one_the_window_carries() {
        let source =
            std::fs::read_to_string(crate::workspace_root().join("crates/app/src/main.rs"))
                .expect("the app's main");
        assert_eq!(
            quoted_after(&source, "const APP_ID: &str = "),
            APP_ID,
            "crates/app/src/main.rs registers the window under another id than the bundle"
        );
        assert!(
            info_plist("1.2.3").contains(&format!("<string>{APP_ID}</string>")),
            "the plist does not carry {APP_ID}"
        );
    }

    /// The desktop entry starts the executable by its name and pairs the icon and the window
    /// with the entry through the one app id, which is also its file name in the layout.
    #[test]
    fn the_desktop_entry_starts_the_binary_and_names_the_icon_by_the_app_id() {
        let entry = desktop_entry();
        let field = |key: &str| {
            entry
                .lines()
                .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(field("Exec"), APP);
        assert_eq!(field("Name"), APP);
        assert_eq!(field("Icon"), APP_ID);
        assert_eq!(field("StartupWMClass"), APP_ID);
        let layout = linux_layout();
        assert!(
            layout.contains(&(
                format!("share/applications/{APP_ID}.desktop"),
                Payload::Desktop
            )),
            "{layout:?}"
        );
        assert!(
            layout.contains(&(format!("bin/{APP}"), Payload::Binary(BUILT_APP))),
            "{layout:?}"
        );
    }

    /// The layout is Apple's: the executable sits under `Contents/MacOS`, which is where
    /// `CFBundleExecutable` is resolved from and where `tormoni` lands beside it.
    #[test]
    fn the_executable_sits_where_macos_looks_for_it() {
        let app = Path::new("/tmp/Tormoni.app");
        assert_eq!(
            executable_in(app),
            Path::new("/tmp/Tormoni.app/Contents/MacOS/Tormoni")
        );
        assert!(
            bundle_path().ends_with("artifacts/Tormoni.app"),
            "{:?}",
            bundle_path()
        );
    }
}
