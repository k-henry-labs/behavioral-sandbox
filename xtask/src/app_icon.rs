//! `cargo xtask app-icon`: the application's icon, cut from the product's mark.
//!
//! - **One source, two outputs.** `crates/app/icon/tormoni.svg` is the mark on a 1024 canvas at
//!   the extent macOS draws an icon to; from it come the `.icns` the bundle names and the PNG the
//!   Linux desktop entry names, both committed, as the fonts are.
//! - **macOS tooling.** `qlmanage` rasterises the SVG, `sips` scales it, `iconutil` packs the
//!   iconset. A dev step; elsewhere it says so and exits.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::bundle::APP_ID;
use crate::{artifacts_dir, run_tool, workspace_root};

/// The mark, on a 1024 canvas.
const SOURCE: &str = "crates/app/icon/tormoni.svg";

/// The icon the bundle names in its plist.
pub(crate) const ICNS: &str = "crates/app/icon/Tormoni.icns";

/// The icon the desktop entry names, which is by the app id.
pub(crate) fn png() -> PathBuf {
    workspace_root().join(format!("crates/app/icon/{APP_ID}.png"))
}

/// The sizes an iconset carries, each at 1x and 2x.
const SIZES: [u32; 5] = [16, 32, 128, 256, 512];

/// Cuts the icon files from the mark, or says why not on this host.
pub(crate) fn cut_app_icon() -> Result<()> {
    if !cfg!(target_os = "macos") {
        println!("app-icon: nothing to cut on this host (qlmanage, sips and iconutil are macOS's)");
        return Ok(());
    }
    let root = workspace_root();
    let source = root.join(SOURCE);
    let work = artifacts_dir().join("icon");
    let iconset = work.join("Tormoni.iconset");
    std::fs::create_dir_all(&iconset).with_context(|| format!("creating {}", iconset.display()))?;

    run_tool(
        "qlmanage",
        &[
            "-t".as_ref(),
            "-s".as_ref(),
            "1024".as_ref(),
            "-o".as_ref(),
            work.as_os_str(),
            source.as_os_str(),
        ],
    )?;
    let master = work.join("tormoni.svg.png");
    if !master.is_file() {
        bail!("qlmanage wrote no {}", master.display());
    }
    for size in SIZES {
        scale(
            &master,
            size,
            &iconset.join(format!("icon_{size}x{size}.png")),
        )?;
        scale(
            &master,
            size * 2,
            &iconset.join(format!("icon_{size}x{size}@2x.png")),
        )?;
    }
    run_tool(
        "iconutil",
        &[
            "--convert".as_ref(),
            "icns".as_ref(),
            "--output".as_ref(),
            root.join(ICNS).as_os_str(),
            iconset.as_os_str(),
        ],
    )?;
    scale(&master, 512, &png())?;
    println!("app-icon: wrote {ICNS} and {}", png().display());
    Ok(())
}

/// Scales `from` to a `size`-pixel square at `to`.
fn scale(from: &Path, size: u32, to: &Path) -> Result<()> {
    let size = size.to_string();
    run_tool(
        "sips",
        &[
            "-z".as_ref(),
            size.as_ref(),
            size.as_ref(),
            from.as_os_str(),
            "--out".as_ref(),
            to.as_os_str(),
        ],
    )
}
