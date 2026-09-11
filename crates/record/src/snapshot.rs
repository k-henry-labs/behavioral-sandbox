//! A snapshot: a sandbox worth making again, kept by name.
//!
//! **A snapshot is a posture with a name on it.** It says what a sandbox made from it boots, what
//! that sandbox may touch, and how much machine it gets — and then nothing happens until someone
//! creates one. Nothing here captures a running VM: that is a different feature wearing the same
//! word, and `ROADMAP.md` keeps it apart.
//!
//! - **It is the same posture a record carries**, written by the same lines. A snapshot describes
//!   what a sandbox *will* be able to touch and a [`Record`](crate::Record) what one *could*; the
//!   two are one sentence about two moments, so a posture grown in one reaches the other.
//! - **A name is the file name**, by [`crate::valid_id`]'s allow-list, because the name
//!   is joined to a path and [`SnapshotStore::remove`] hands the result to the filesystem.
//! - **No environment values, ever.** A posture carries the *names* of environment entries and
//!   never what they are set to, and a snapshot is a file that gets copied around and shared. The
//!   rule is the record's, inherited rather than restated.
//! - **Local, and only here.** `$BOXDESK_SNAPSHOTS_DIR`, else `$XDG_DATA_HOME/boxdesk/snapshots`,
//!   else `~/.local/share/boxdesk/snapshots`, created `0700`.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::{
    ParseError, Posture, checked_id, id_rule, line, now_ms, posture_key, posture_text, temp_name,
    unescape, valid_id,
};

/// The snapshot format's version, the first line of every snapshot file.
pub const SNAPSHOT_FORMAT: u32 = 1;

/// The lines a snapshot cannot be read without. The posture's own required lines are here for the
/// same reason they are in a record: a file missing one describes a sandbox nobody could boot.
const REQUIRED: [&str; 7] = [
    "snapshot", "name", "root", "network", "sound", "gpu", "limits",
];

/// A named sandbox to make again: what it boots, what it may touch, and how much machine it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Snapshot {
    /// The name it is created and reached by, and the file it is kept in.
    pub name: String,
    /// One line saying what this sandbox is for, or empty. A list of names with no words beside
    /// them is a list nobody reads twice.
    pub about: String,
    /// The command a sandbox from this snapshot runs when it is given none of its own, one word
    /// per element. Empty means the sandbox is started to be entered rather than to run something.
    pub command: Vec<String>,
    /// What a sandbox from it boots, what it may touch, and how much machine it gets.
    pub posture: Posture,
    /// When it was created, milliseconds since the Unix epoch.
    pub created_ms: u64,
}

impl Snapshot {
    /// A snapshot named `name`, created now, over `posture`.
    #[must_use]
    pub fn new(name: &str, posture: Posture, command: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            about: String::new(),
            command,
            posture,
            created_ms: now_ms(),
        }
    }

    /// The same, with a line saying what it is for.
    #[must_use]
    pub fn about(mut self, about: &str) -> Self {
        self.about = about.to_string();
        self
    }

    /// The snapshot as the text its file holds.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        line(&mut out, "snapshot", &SNAPSHOT_FORMAT.to_string());
        line(&mut out, "name", &self.name);
        if !self.about.is_empty() {
            line(&mut out, "about", &self.about);
        }
        for arg in &self.command {
            line(&mut out, "arg", arg);
        }
        out.push_str(&posture_text(&self.posture));
        line(&mut out, "created", &self.created_ms.to_string());
        out
    }

    /// A snapshot read back from its text.
    ///
    /// # Errors
    ///
    /// A line that will not parse, a format this build does not read, a missing required line, or
    /// a name that is not a usable file name.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut snapshot = Self {
            name: String::new(),
            about: String::new(),
            command: Vec::new(),
            posture: Posture::default(),
            created_ms: 0,
        };
        let mut seen: Vec<&str> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            let bad = || ParseError(format!("line {}: {line:?}", n + 1));
            let value = &unescape(value).ok_or_else(bad)?;
            if let Some(key) = REQUIRED.iter().find(|k| **k == key) {
                seen.push(key);
            }
            match key {
                "snapshot" => {
                    let format: u32 = value.parse().map_err(|_| bad())?;
                    if format != SNAPSHOT_FORMAT {
                        return Err(ParseError(format!(
                            "snapshot format {format}; this build reads {SNAPSHOT_FORMAT}"
                        )));
                    }
                }
                "name" => snapshot.name = value.to_string(),
                "about" => snapshot.about = value.to_string(),
                "arg" => snapshot.command.push(value.to_string()),
                "created" => snapshot.created_ms = value.parse().map_err(|_| bad())?,
                // Either one of the posture's own lines, or a key this build does not know: one a
                // later build wrote, carried past rather than refused.
                _ => {
                    posture_key(&mut snapshot.posture, key, value).map_err(|()| bad())?;
                }
            }
        }
        if let Some(missing) = REQUIRED.iter().find(|k| !seen.contains(k)) {
            return Err(ParseError(format!("no `{missing}` line")));
        }
        if !valid_id(&snapshot.name) {
            return Err(ParseError(format!(
                "{:?} is not a usable snapshot name: {}",
                snapshot.name,
                id_rule()
            )));
        }
        Ok(snapshot)
    }
}

/// The snapshots directory as a store of them.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    /// The store at the snapshots directory the environment names, created if absent.
    ///
    /// # Errors
    ///
    /// No directory can be named, or it cannot be created.
    pub fn open() -> io::Result<Self> {
        Self::at(snapshots_dir()?)
    }

    /// The store at `dir`, created `0700` if absent.
    ///
    /// # Errors
    ///
    /// The directory cannot be created.
    pub fn at(dir: PathBuf) -> io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder.create(&dir)?;
        Ok(Self { dir })
    }

    /// Where the store is.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every snapshot in the store, by name.
    ///
    /// **A file that will not parse is skipped, not fatal.** One unreadable snapshot must not be
    /// a store nobody can list; the verb that reads it by name is where the error belongs.
    #[must_use]
    pub fn list(&self) -> Vec<Snapshot> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Snapshot> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| valid_id(name))
            .filter_map(|name| self.read(&name).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The snapshot named `name`.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, the file is missing, or its text will not parse. A file that
    /// names a different snapshot is refused: one copied under a new name would otherwise be
    /// listed as the one it came from, and removed as it.
    pub fn read(&self, name: &str) -> io::Result<Snapshot> {
        let text = std::fs::read_to_string(self.path_of(name)?)?;
        let snapshot = Snapshot::parse(&text).map_err(io::Error::other)?;
        if snapshot.name != name {
            return Err(io::Error::other(format!(
                "the snapshot in {name} says it is {}",
                snapshot.name
            )));
        }
        Ok(snapshot)
    }

    /// Whether a snapshot of this name is in the store.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.path_of(name).is_ok_and(|p| p.is_file())
    }

    /// Writes `snapshot` over whatever was under its name.
    ///
    /// Written to a temporary file and renamed over the old one, as a record is: a reader sees the
    /// old text or the new, never a torn one.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the file cannot be written.
    pub fn save(&self, snapshot: &Snapshot) -> io::Result<()> {
        let path = self.path_of(&snapshot.name)?;
        let tmp = self.dir.join(temp_name("snapshot"));
        let written =
            std::fs::write(&tmp, snapshot.to_text()).and_then(|()| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    }

    /// Removes the snapshot named `name`.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the file cannot be removed.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        std::fs::remove_file(self.path_of(name)?)
    }

    /// The file a snapshot of this name lives in, refusing a name that is not one directory entry.
    fn path_of(&self, name: &str) -> io::Result<PathBuf> {
        Ok(self.dir.join(checked_id(name)?))
    }
}

/// The snapshots directory, from the environment: `$BOXDESK_SNAPSHOTS_DIR`, else
/// `$XDG_DATA_HOME/boxdesk/snapshots`, else `~/.local/share/boxdesk/snapshots`.
///
/// # Errors
///
/// None of the three is set, so there is nowhere to put one.
pub fn snapshots_dir() -> io::Result<PathBuf> {
    snapshots_dir_from(
        std::env::var_os("BOXDESK_SNAPSHOTS_DIR"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
    .ok_or_else(|| {
        io::Error::other("no snapshots directory: set BOXDESK_SNAPSHOTS_DIR, XDG_DATA_HOME or HOME")
    })
}

/// [`snapshots_dir`] with the environment reads lifted out.
#[must_use]
pub fn snapshots_dir_from(
    snapshots: Option<OsString>,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    snapshots
        .map(PathBuf::from)
        .or_else(|| xdg_data.map(|d| PathBuf::from(d).join("boxdesk/snapshots")))
        .or_else(|| home.map(|h| PathBuf::from(h).join(".local/share/boxdesk/snapshots")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mount, Network, Rootfs};

    /// A snapshot with something set in every corner of its posture, so a round trip that loses
    /// one is a round trip that fails.
    fn furnished() -> Snapshot {
        let posture = Posture {
            root: PathBuf::from("/srv/guest"),
            rootfs: Rootfs::Writable,
            network: Network::Tsi,
            sound: true,
            gpu: true,
            results: true,
            mounts: vec![Mount::new(
                PathBuf::from("/work"),
                PathBuf::from("/Users/me/project"),
            )],
            ..Posture::default()
        };
        Snapshot::new("devbox", posture, vec!["sleep".into(), "infinity".into()])
            .about("the one with the project mounted")
    }

    /// Everything a snapshot holds survives being written and read, which is the only promise a
    /// file format makes.
    #[test]
    fn a_snapshot_reads_back_as_what_was_written() {
        let before = furnished();
        let after = Snapshot::parse(&before.to_text()).expect("its own text parses");
        assert_eq!(after, before);
    }

    /// **The posture is written by the record's own writer**, so a field added there and missed
    /// here would be a snapshot quietly narrower than the sandbox it names. This is that check:
    /// the posture lines of a snapshot are exactly the posture lines of a record.
    #[test]
    fn a_snapshot_spells_its_posture_the_way_a_record_does() {
        let snapshot = furnished();
        let record = crate::Record::begin(
            "devbox",
            crate::Verb::Up,
            snapshot.command.clone(),
            snapshot.posture.clone(),
        );
        let lines = |text: &str, drop: &[&str]| -> Vec<String> {
            text.lines()
                .filter(|l| !drop.iter().any(|k| l.split(' ').next() == Some(k)))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            lines(
                &snapshot.to_text(),
                &["snapshot", "name", "about", "arg", "created"]
            ),
            lines(
                &record.to_text(),
                &[
                    "record", "id", "name", "verb", "arg", "started", "pid", "ended", "end"
                ]
            ),
            "the two files describe one posture in two ways"
        );
    }

    /// A snapshot whose name would not be one directory entry is refused when it is read, not
    /// when it is used: the name is joined to a path.
    #[test]
    fn a_name_that_is_not_a_file_name_is_refused() {
        let text = furnished().to_text().replace("name devbox", "name ../etc");
        let err = Snapshot::parse(&text).expect_err("a traversal is not a name");
        assert!(
            err.to_string().contains("usable snapshot name"),
            "said {err} instead"
        );
    }

    /// A file this build cannot read is refused by version rather than by the first line that
    /// surprises it.
    #[test]
    fn a_later_format_is_refused_by_its_version() {
        let text = furnished().to_text().replace("snapshot 1", "snapshot 2");
        let err = Snapshot::parse(&text).expect_err("a format from the future");
        assert!(err.to_string().contains("this build reads"), "said {err}");
    }

    /// Every required line is required: a file missing one describes a sandbox nobody could boot,
    /// and the error says which line rather than which field defaulted.
    #[test]
    fn every_required_line_is_missed_by_name() {
        let whole = furnished().to_text();
        for key in REQUIRED {
            let without: String = whole
                .lines()
                .filter(|l| l.split(' ').next() != Some(key))
                .map(|l| format!("{l}\n"))
                .collect();
            let err = Snapshot::parse(&without).expect_err("{key} should be required");
            assert!(
                err.to_string().contains(key),
                "dropping `{key}` said {err} instead of naming it"
            );
        }
    }

    /// Saving, reading back, listing and removing, through the store's own names.
    #[test]
    fn the_store_keeps_a_snapshot_under_its_name() {
        let dir = boxdesk_test_support::ScratchDir::created("snapshot-store");
        let store = SnapshotStore::at(dir.path().join("snapshots")).expect("a store");
        assert!(store.list().is_empty(), "a fresh store holds nothing");

        let snapshot = furnished();
        store.save(&snapshot).expect("saved");
        assert!(store.holds("devbox"));
        assert_eq!(store.read("devbox").expect("read back"), snapshot);
        assert_eq!(
            store
                .list()
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["devbox"]
        );

        store.remove("devbox").expect("removed");
        assert!(!store.holds("devbox"));
        assert!(store.list().is_empty());
    }

    /// A store lists by name, not by the order the filesystem hands entries back: two machines
    /// showing one store in two orders is a list nobody can scan.
    #[test]
    fn the_store_lists_in_name_order() {
        let dir = boxdesk_test_support::ScratchDir::created("snapshot-order");
        let store = SnapshotStore::at(dir.path().join("snapshots")).expect("a store");
        for name in ["small", "big", "medium"] {
            store
                .save(&Snapshot::new(name, Posture::default(), Vec::new()))
                .expect("saved");
        }
        assert_eq!(
            store
                .list()
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["big", "medium", "small"]
        );
    }

    /// One unreadable file must not be a store nobody can list: the rest still come back.
    #[test]
    fn a_file_that_will_not_parse_is_skipped_rather_than_fatal() {
        let dir = boxdesk_test_support::ScratchDir::created("snapshot-junk");
        let store = SnapshotStore::at(dir.path().join("snapshots")).expect("a store");
        store
            .save(&Snapshot::new("good", Posture::default(), Vec::new()))
            .expect("saved");
        std::fs::write(store.dir().join("junk"), "not a snapshot\n").expect("wrote junk");

        assert_eq!(
            store
                .list()
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["good"],
            "the good one should still list"
        );
        assert!(
            store.read("junk").is_err(),
            "and reading the bad one says so"
        );
    }

    /// A file copied under another name is refused rather than answered as the snapshot it came
    /// from, which is the rule a record keeps for the same reason.
    #[test]
    fn a_snapshot_read_under_the_wrong_name_is_refused() {
        let dir = boxdesk_test_support::ScratchDir::created("snapshot-copy");
        let store = SnapshotStore::at(dir.path().join("snapshots")).expect("a store");
        let snapshot = furnished();
        store.save(&snapshot).expect("saved");
        std::fs::copy(store.dir().join("devbox"), store.dir().join("copy")).expect("copied");

        let err = store.read("copy").expect_err("a copy is not the original");
        assert!(err.to_string().contains("says it is devbox"), "said {err}");
    }

    /// The three places a snapshots directory can come from, in the order they are asked, which is
    /// the order the runs directory asks its own.
    #[test]
    fn the_snapshots_directory_comes_from_the_first_of_three() {
        let some = |s: &str| Some(OsString::from(s));
        assert_eq!(
            snapshots_dir_from(some("/tmp/snaps"), some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/tmp/snaps")),
            "the explicit one wins"
        );
        assert_eq!(
            snapshots_dir_from(None, some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/xdg/boxdesk/snapshots"))
        );
        assert_eq!(
            snapshots_dir_from(None, None, some("/home/u")),
            Some(PathBuf::from("/home/u/.local/share/boxdesk/snapshots"))
        );
        assert_eq!(
            snapshots_dir_from(None, None, None),
            None,
            "with none of the three there is nowhere to put one"
        );
    }
}
