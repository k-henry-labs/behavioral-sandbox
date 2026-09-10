//! `cargo xtask app-icon`: the application's icon, cut from the product's mark.
//!
//! - **One source, two outputs.** `crates/app/icon/tormoni.svg` is the mark on a 1024 canvas at
//!   the extent macOS draws an icon to; from it come the `.icns` the bundle names and the PNG the
//!   Linux desktop entry names, both committed, as the fonts are.
//! - **macOS tooling.** `sips` rasterises the SVG and scales it, `iconutil` packs the iconset. A
//!   dev step; elsewhere it says so and exits.

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
        println!("app-icon: nothing to cut on this host (sips and iconutil are macOS's)");
        return Ok(());
    }
    let root = workspace_root();
    let source = root.join(SOURCE);
    let work = artifacts_dir().join("icon");
    let iconset = work.join("Tormoni.iconset");
    std::fs::create_dir_all(&iconset).with_context(|| format!("creating {}", iconset.display()))?;

    let master = work.join("master.png");
    render(&source, &master)?;
    if !master.is_file() {
        bail!("sips wrote no {}", master.display());
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

/// Rasterises the mark to `to` at the canvas its SVG declares, keeping the transparent surround.
/// `qlmanage` is the other way to do this and flattens onto white, which puts a box behind the icon.
fn render(from: &Path, to: &Path) -> Result<()> {
    run_tool(
        "sips",
        &[
            "-s".as_ref(),
            "format".as_ref(),
            "png".as_ref(),
            from.as_os_str(),
            "--out".as_ref(),
            to.as_os_str(),
        ],
    )
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

#[cfg(test)]
mod tests {
    use tormoni_test_support::ScratchDir;

    use super::*;

    /// A launcher sets an icon on its own background, so the tile's surround must stay transparent.
    /// `qlmanage` cut the icon onto white until 2026-09-09, which shipped it inside a white box.
    #[test]
    fn nothing_is_drawn_behind_the_icons_tile() {
        if !cfg!(target_os = "macos") {
            println!("skipped: reading a pixel back needs sips, which is macOS's");
            return;
        }
        let scratch = ScratchDir::created("app-icon");
        for icon in [workspace_root().join(ICNS), png()] {
            let flat = scratch.path().join("flat.tiff");
            run_tool(
                "sips",
                &[
                    "-s".as_ref(),
                    "format".as_ref(),
                    "tiff".as_ref(),
                    icon.as_os_str(),
                    "--out".as_ref(),
                    flat.as_os_str(),
                ],
            )
            .expect("sips could not read the committed icon");
            let bytes = std::fs::read(&flat).expect("sips wrote no tiff");
            assert_eq!(
                first_pixel(&bytes)[3],
                0,
                "{} has a background behind its tile: re-cut with `cargo xtask app-icon`",
                icon.display()
            );
        }
    }

    /// The top-left pixel of a `sips`-written TIFF, as RGBA. It asserts the shape it reads rather
    /// than handling every TIFF, so a change in what `sips` writes fails here instead of passing.
    fn first_pixel(tiff: &[u8]) -> [u8; 4] {
        assert!(
            tiff.starts_with(b"II") || tiff.starts_with(b"MM"),
            "sips wrote a tiff that starts {:?}, which is neither byte order",
            &tiff[..2]
        );
        let intel = tiff.starts_with(b"II");
        let short = |at: usize| {
            let raw = tiff[at..at + 2].try_into().unwrap();
            if intel {
                u16::from_le_bytes(raw)
            } else {
                u16::from_be_bytes(raw)
            }
        };
        let long = |at: usize| {
            let raw = tiff[at..at + 4].try_into().unwrap();
            let value = if intel {
                u32::from_le_bytes(raw)
            } else {
                u32::from_be_bytes(raw)
            };
            value as usize
        };
        let ifd = long(4);
        let mut strip = None;
        for entry in 0..usize::from(short(ifd)) {
            let at = ifd + 2 + entry * 12;
            let (tag, kind, count, value) = (short(at), short(at + 2), long(at + 4), at + 8);
            match tag {
                258 => assert!(
                    (0..4).all(|s| short(long(value) + s * 2) == 8),
                    "the tiff is not eight bits a sample"
                ),
                259 => assert_eq!(short(value), 1, "the tiff is compressed"),
                277 => assert_eq!(short(value), 4, "the tiff is not rgba"),
                273 => {
                    assert_eq!(kind, 4, "the strip offsets are not longs");
                    strip = Some(if count == 1 {
                        long(value)
                    } else {
                        long(long(value))
                    });
                }
                _ => {}
            }
        }
        let strip = strip.expect("the tiff names no strip");
        tiff[strip..strip + 4].try_into().unwrap()
    }
}
