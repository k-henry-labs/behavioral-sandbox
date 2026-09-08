//! The icon font, cut to the glyphs the app names.
//!
//! - **One list, and it lives in the app.** `crates/app/src/icons.rs` names every glyph as a
//!   `\u{…}` literal; this reads them back out of that file, so the font and the code that draws
//!   from it cannot come to name different sets.
//! - **The upstream release is pinned by sha256**, like every other fetched input, so a cut is
//!   reproducible: the same release and the same list give the same bytes.
//! - **Cutting is a dev step, not a build step.** The cut font is committed, because the app
//!   compiles it in and the gate builds with no network.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{artifacts, artifacts_dir, workspace_root};

/// The release this is cut from. The URL is replaceable; the sha256 is the contract.
const VERSION: &str = "1.41.0";
const SHA256: &str = "df17b2ca43256d0bd4eb695e42892164a1497a5a8c61510191c6290217cc4279";

/// Where the app names its glyphs, and where the cut font and its licence sit beside it.
const NAMES: &str = "crates/app/src/icons.rs";
const FONT: &str = "crates/app/fonts/lucide.ttf";
const LICENCE: &str = "crates/app/fonts/LICENSE-lucide";

/// `cargo xtask icons`: cut the pinned upstream font to what `crates/app/src/icons.rs` names.
pub(crate) fn cut_icon_font() -> Result<()> {
    let root = workspace_root();
    if !root.join(LICENCE).is_file() {
        bail!("{LICENCE} is missing: the font's licence travels with the font");
    }

    let zip = artifacts_dir().join(format!("lucide-font-{VERSION}.zip"));
    artifacts::fetch_one(&artifacts::Artifact {
        url: format!(
            "https://github.com/lucide-icons/lucide/releases/download/{VERSION}/lucide-font-{VERSION}.zip"
        ),
        sha256: SHA256,
        dest: zip.clone(),
    })?;

    let unpacked = artifacts_dir().join(format!("lucide-font-{VERSION}"));
    let _ = std::fs::remove_dir_all(&unpacked);
    let out = Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&zip)
        .arg("-d")
        .arg(&unpacked)
        .output()
        .context("running unzip (the release ships a zip)")?;
    if !out.status.success() {
        bail!(
            "unzip failed on {}: {}",
            zip.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let whole = find_ttf(&unpacked)?;

    let source = std::fs::read_to_string(root.join(NAMES))
        .with_context(|| format!("reading {NAMES}, which names the glyphs"))?;
    let wanted = named_codepoints(&source)?;
    if wanted.is_empty() {
        bail!("{NAMES} names no glyphs, so there is nothing to cut to");
    }

    let before = std::fs::read(&whole).with_context(|| format!("reading {}", whole.display()))?;
    // Not `subsetter`, which cuts `cmap` out because a PDF carries its own: a font without one
    // answers no lookup by character, which is the only way the app asks for a glyph.
    let after = oxifont_subset::subset_font(&before, &wanted)
        .map_err(|e| anyhow::anyhow!("subsetting the font: {e}"))?;

    let dest = root.join(FONT);
    std::fs::write(&dest, &after).with_context(|| format!("writing {}", dest.display()))?;
    println!(
        "icons: {} glyphs, {} KiB -> {} KiB, at {FONT}",
        wanted.len(),
        before.len() / 1024,
        after.len() / 1024
    );
    println!("icons: `cargo test -p tormoni-app` checks the cut against what the app names");
    Ok(())
}

/// Every `'\u{…}'` literal in `source`, which is how the app spells an icon.
fn named_codepoints(source: &str) -> Result<BTreeSet<char>> {
    const OPEN: &str = "'\\u{";
    let mut found = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find(OPEN) {
        rest = &rest[at + OPEN.len()..];
        let Some(end) = rest.find('}') else { break };
        let hex = &rest[..end];
        let code = u32::from_str_radix(hex, 16)
            .with_context(|| format!("{hex:?} is not a hex codepoint"))?;
        let glyph = char::from_u32(code).with_context(|| format!("U+{hex} is not a character"))?;
        found.insert(glyph);
        rest = &rest[end..];
    }
    Ok(found)
}

/// The one `.ttf` in the unpacked release: the format both the app and `ttf-parser` read.
fn find_ttf(dir: &std::path::Path) -> Result<PathBuf> {
    for entry in walk(dir)? {
        if entry.extension().is_some_and(|e| e == "ttf") {
            return Ok(entry);
        }
    }
    bail!("no .ttf under {}", dir.display())
}

/// Every file under `dir`, one level of nesting being all the release has.
fn walk(dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            found.extend(walk(&path)?);
        } else {
            found.push(path);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's own spelling is what is read: a `\u{…}` literal, however it is named, and
    /// nothing else in the file.
    #[test]
    fn the_glyphs_are_read_from_the_apps_own_literals() {
        let source = "\
            const PANEL_LEFT: char = '\\u{e12a}';\n\
            const GRID: char = '\\u{e0ff}';\n\
            // a comment mentioning e999 and \"e888\"\n\
            let s = format!(\"{icon:?}\");\n";
        let found = named_codepoints(source).expect("the literals parse");
        assert_eq!(
            found,
            BTreeSet::from(['\u{e12a}', '\u{e0ff}']),
            "only the literals, and each once"
        );
        assert!(
            named_codepoints("nothing here")
                .expect("no literals")
                .is_empty(),
            "a file naming none is empty, not an error"
        );
    }

    /// A literal that is not a codepoint is refused rather than skipped, since skipping it would
    /// cut a glyph the app draws.
    #[test]
    fn a_literal_that_is_not_a_codepoint_is_refused() {
        assert!(named_codepoints("'\\u{zzzz}'").is_err());
    }
}
