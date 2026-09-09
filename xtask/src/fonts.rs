//! The text faces the app compiles in, cut to the characters it draws.
//!
//! - **The same families the web app serves.** `web/app/layout.tsx` self-hosts Inter and Geist
//!   Mono through `next/font`, so a desktop window and a page read as one product rather than as
//!   two that happen to share a name.
//! - **Static instances, not the variable file.** `fontdb` indexes a face by the weight it
//!   declares, and a variable font declares one; the app asks for `Semibold` by weight, so the
//!   two weights it uses are two files.
//! - **Cut to the `latin` subset the web asks for**, plus the few characters the app draws
//!   outside it, so both ends carry one coverage. Anything else a guest prints falls back to a
//!   system face, which is what a missing glyph does anyway.
//! - **Each release is pinned by sha256** and each licence travels with its font, as the icon cut
//!   does; `cargo xtask icons` is the same job for the icon face, driven by the app's own
//!   literals instead of a fixed range.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::artifacts::unpack_release;
use crate::workspace_root;

/// The releases these are cut from. Each URL is replaceable; each sha256 is the contract.
const INTER_VERSION: &str = "4.1";
const INTER_SHA256: &str = "9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e";
const GEIST_VERSION: &str = "1.7.2";
const GEIST_SHA256: &str = "7fc800d2ac6b92844895196e5041aca55d814c15db70c44f79b3b83ab82b04e2";

/// Where the cut faces and their licences sit, beside the icon font.
const FONT_DIR: &str = "crates/app/fonts";

/// One face to cut: where it is in the unpacked release, and what it is called here.
struct Face {
    /// The path inside the unpacked release, which is the upstream's own layout.
    source: &'static str,
    /// The file this writes, which is what `crates/app/src/fonts.rs` compiles in.
    dest: &'static str,
}

const INTER_FACES: [Face; 2] = [
    Face {
        source: "extras/ttf/Inter-Regular.ttf",
        dest: "Inter-Regular.ttf",
    },
    Face {
        source: "extras/ttf/Inter-SemiBold.ttf",
        dest: "Inter-SemiBold.ttf",
    },
];

const GEIST_FACES: [Face; 2] = [
    Face {
        source: "geist-font/GeistMono/ttf/GeistMono-Regular.ttf",
        dest: "GeistMono-Regular.ttf",
    },
    Face {
        source: "geist-font/GeistMono/ttf/GeistMono-SemiBold.ttf",
        dest: "GeistMono-SemiBold.ttf",
    },
];

/// `cargo xtask fonts`: cut the pinned text faces to [`coverage`] and put them where the app
/// compiles them in.
pub(crate) fn cut_text_fonts() -> Result<()> {
    let root = workspace_root();
    let wanted = coverage();

    let inter = unpack_release(
        &format!(
            "https://github.com/rsms/inter/releases/download/v{INTER_VERSION}/Inter-{INTER_VERSION}.zip"
        ),
        INTER_SHA256,
        &format!("Inter-{INTER_VERSION}"),
    )?;
    let geist = unpack_release(
        &format!(
            "https://github.com/vercel/geist-font/releases/download/v{GEIST_VERSION}/geist-font-v{GEIST_VERSION}.zip"
        ),
        GEIST_SHA256,
        &format!("geist-font-{GEIST_VERSION}"),
    )?;

    for (unpacked, faces) in [(&inter, &INTER_FACES), (&geist, &GEIST_FACES)] {
        for face in faces {
            cut(
                &unpacked.join(face.source),
                &root.join(FONT_DIR).join(face.dest),
                &wanted,
            )?;
        }
    }

    // The licence travels with the font, as `crates/app/fonts/LICENSE-lucide` does.
    install_licence(
        &inter.join("LICENSE.txt"),
        &root.join(FONT_DIR).join("LICENSE-Inter"),
    )?;
    install_licence(
        &geist.join("geist-font/OFL.txt"),
        &root.join(FONT_DIR).join("LICENSE-GeistMono"),
    )?;

    println!("fonts: `cargo test -p tormoni-app` checks each cut declares what the app asks for");
    Ok(())
}

/// Every character the cut faces answer for: the `latin` subset `next/font` serves on the web,
/// plus what the app draws outside it.
fn coverage() -> BTreeSet<char> {
    // The `latin` subset's own ranges, as Google Fonts publishes them, so a word rendered here
    // and the same word rendered on a page are the same glyphs.
    let latin: [(u32, u32); 17] = [
        (0x0000, 0x00FF),
        (0x0131, 0x0131),
        (0x0152, 0x0153),
        (0x02BB, 0x02BC),
        (0x02C6, 0x02C6),
        (0x02DA, 0x02DA),
        (0x02DC, 0x02DC),
        (0x0304, 0x0304),
        (0x0308, 0x0308),
        (0x0329, 0x0329),
        (0x2000, 0x206F),
        (0x2074, 0x2074),
        (0x20AC, 0x20AC),
        (0x2122, 0x2122),
        (0x2191, 0x2191),
        (0x2193, 0x2193),
        (0x2212, 0x2215),
    ];
    // `←` spells a mount in a run's row and `●` is the status dot; neither is in `latin`, and a
    // missing one would fall back to a system face mid-line. `→` for its pair, since `latin`
    // carries `↑` and `↓` and a half-set of arrows is the gap nobody expects.
    let extra = ['\u{2190}', '\u{2192}', '\u{25CF}'];
    latin
        .iter()
        .flat_map(|(lo, hi)| (*lo..=*hi).filter_map(char::from_u32))
        .chain(extra)
        .collect()
}

/// Cuts one face to `wanted` and writes it, reporting what it saved.
fn cut(source: &Path, dest: &Path, wanted: &BTreeSet<char>) -> Result<()> {
    let before = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let after = oxifont_subset::subset_font(&before, wanted)
        .map_err(|e| anyhow::anyhow!("subsetting {}: {e}", source.display()))?;
    std::fs::write(dest, &after).with_context(|| format!("writing {}", dest.display()))?;
    println!(
        "fonts: {} KiB -> {} KiB, at {}",
        before.len() / 1024,
        after.len() / 1024,
        dest.strip_prefix(workspace_root())
            .unwrap_or(dest)
            .display()
    );
    Ok(())
}

/// Copies a licence out of the release, so the tree carries the terms it redistributes under.
fn install_licence(source: &Path, dest: &Path) -> Result<()> {
    let text = std::fs::read_to_string(source)
        .with_context(|| format!("reading {}, the font's licence", source.display()))?;
    if !text.contains("SIL Open Font License") {
        bail!(
            "{} is not the OFL this expects to be redistributing under",
            source.display()
        );
    }
    std::fs::write(dest, text).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coverage is the web's `latin` subset plus what the app draws outside it: the
    /// characters a row and a heading are actually made of.
    #[test]
    fn the_coverage_carries_every_character_the_app_draws() {
        let cover = coverage();
        for c in "abzABZ0189 /=-_.:@%".chars() {
            assert!(cover.contains(&c), "{c:?} is not covered");
        }
        // Each of these is in the app's own source, and the `\u{2190}` and `\u{25CF}` are the two
        // that the `latin` subset does not carry.
        for c in [
            '\u{00B7}', '\u{00D7}', '\u{2026}', '\u{203A}', '\u{2190}', '\u{2192}', '\u{25CF}',
        ] {
            assert!(cover.contains(&c), "U+{:04X} is not covered", c as u32);
        }
        // Cut, not merely renamed: a face answering every character would save nothing.
        assert!(!cover.contains(&'\u{4E00}'), "the cut is not a whole font");
    }
}
