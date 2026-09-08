//! What the notebook remembers between launches: one `key value` file beside the runs
//! directory, written whole to a temporary file and renamed, read leniently (an unknown key is
//! a later build's; a file this build cannot read is nothing saved).

use std::io;
use std::path::{Path, PathBuf};

/// The state format's version, the first line of the file.
const FORMAT: u32 = 1;

/// The file's name, a sibling of the runs directory.
const FILE: &str = "app-state";

/// The percent bounds a scale line must sit in to be believed.
const SCALE: std::ops::RangeInclusive<u16> = 50..=200;

/// Everything the notebook remembers, one `key value` line each; an absent key is no pick.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Saved {
    /// The palette, by the name the toolkit prints it under.
    pub(crate) theme: Option<String>,
    /// The interface scale, in percent.
    pub(crate) scale: Option<u16>,
    /// The screen a plain launch opens on, in the `--open` flag's spelling.
    pub(crate) open: Option<String>,
}

/// The state file's path: beside the runs directory, or inside it when there is no beside.
fn beside(runs: &Path) -> PathBuf {
    runs.parent()
        .map_or_else(|| runs.join(FILE), |parent| parent.join(FILE))
}

/// The saved picks; a file that is absent, unreadable or a later format is nothing saved.
pub(crate) fn load() -> Saved {
    tormoni_record::runs_dir()
        .ok()
        .and_then(|runs| std::fs::read_to_string(beside(&runs)).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Saves every pick, whole: the temporary is renamed over the file, never left.
pub(crate) fn save(saved: &Saved) -> io::Result<()> {
    save_at(&beside(&tormoni_record::runs_dir()?), saved)
}

fn save_at(path: &Path, saved: &Saved) -> io::Result<()> {
    let tmp = tmp_of(path);
    std::fs::write(&tmp, render(saved))?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// This process's temporary: picks in one window serialize through `update`, so the pid alone
/// keeps two windows from interleaving one file.
fn tmp_of(path: &Path) -> PathBuf {
    path.with_extension(format!("{}.tmp", std::process::id()))
}

fn render(saved: &Saved) -> String {
    let mut out = format!("state {FORMAT}\n");
    if let Some(theme) = &saved.theme {
        out.push_str(&format!("theme {theme}\n"));
    }
    if let Some(scale) = saved.scale {
        out.push_str(&format!("scale {scale}\n"));
    }
    if let Some(open) = &saved.open {
        out.push_str(&format!("open {open}\n"));
    }
    out
}

fn parse(text: &str) -> Saved {
    let mut lines = text.lines();
    if lines.next() != Some(format!("state {FORMAT}").as_str()) {
        return Saved::default();
    }
    let mut saved = Saved::default();
    for line in lines {
        if let Some(theme) = line.strip_prefix("theme ") {
            saved.theme = Some(theme.to_owned());
        } else if let Some(scale) = line.strip_prefix("scale ") {
            saved.scale = scale.parse().ok().filter(|pct| SCALE.contains(pct));
        } else if let Some(open) = line.strip_prefix("open ") {
            saved.open = Some(open.to_owned());
        }
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_pick() -> Saved {
        Saved {
            theme: Some("Nord".to_owned()),
            scale: Some(110),
            open: Some("list".to_owned()),
        }
    }

    /// What `save` writes parses back; a later format, garbage, or a missing format line is
    /// nothing saved; a key this build does not know is passed over.
    #[test]
    fn the_state_file_round_trips_and_a_file_this_build_cannot_read_is_nothing_saved() {
        assert_eq!(
            parse(&render(&every_pick())),
            every_pick(),
            "its own text parses"
        );
        assert_eq!(
            parse(&render(&Saved::default())),
            Saved::default(),
            "no picks is a bare format line"
        );
        assert_eq!(
            parse("state 2\ntheme Nord\n"),
            Saved::default(),
            "a later format"
        );
        assert_eq!(parse("theme Nord\n"), Saved::default(), "no format line");
        assert_eq!(parse("not even lines"), Saved::default());
        assert_eq!(
            parse("state 1\nfuture x\ntheme Nord\n").theme.as_deref(),
            Some("Nord"),
            "an unknown key is passed over"
        );
    }

    /// A scale outside [`SCALE`], or one that is not a number, is no pick at all.
    #[test]
    fn a_scale_the_window_could_not_survive_is_not_believed() {
        assert_eq!(
            parse("state 1\nscale 200\n").scale,
            Some(200),
            "the bound itself holds"
        );
        assert_eq!(parse("state 1\nscale 999\n").scale, None);
        assert_eq!(parse("state 1\nscale 0\n").scale, None);
        assert_eq!(parse("state 1\nscale huge\n").scale, None);
    }

    /// The file sits beside the runs directory, so `$TORMONI_RUNS_DIR` isolation carries over.
    #[test]
    fn the_state_file_sits_beside_the_runs_directory() {
        assert_eq!(
            beside(Path::new("/x/tormoni/runs")),
            Path::new("/x/tormoni/app-state")
        );
        assert_eq!(
            beside(Path::new("/")),
            Path::new("/app-state"),
            "no beside falls inside"
        );
    }

    /// A pick lands whole under the final name, with no temporary left.
    #[test]
    fn a_pick_is_saved_whole_with_no_tmp_left() {
        let dir = tormoni_test_support::ScratchDir::created("app-state");
        let path = dir.path().join(FILE);
        save_at(&path, &every_pick()).expect("saved");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "state 1\ntheme Nord\nscale 110\nopen list\n"
        );
        assert!(!tmp_of(&path).exists(), "no temporary stays");
        let blocked = dir.path().join("a-directory");
        std::fs::create_dir(&blocked).expect("a directory in the way");
        save_at(&blocked, &every_pick()).expect_err("cannot rename over a directory");
        assert!(
            !tmp_of(&blocked).exists(),
            "a failed rename does not leave its temporary"
        );
    }
}
