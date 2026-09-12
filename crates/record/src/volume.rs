//! A volume: a directory with a life of its own, mounted into sandboxes by name.
//!
//! **A run's writes end with the run.** What it keeps is `results/` inside its own record, tied to
//! that one run; a host directory given with `--mount` outlives it but is a path the caller has to
//! remember, and nothing here manages. A volume is the third thing: a named directory this project
//! makes, keeps and can list, mounted into any sandbox that asks for it by name.
//!
//! - **Local, and only here.** No object store and no cloud: the cloud was removed from this tree
//!   deliberately. A volume is a directory on this machine, under `$BOXDESK_VOLUMES_DIR`, else
//!   `$XDG_DATA_HOME/boxdesk/volumes`, else `~/.local/share/boxdesk/volumes`, created `0700`.
//! - **A name is the directory name**, by [`crate::valid_id`]'s allow-list, because the name is
//!   joined to a path and [`VolumeStore::remove`] hands the result to `remove_dir_all`.
//! - **The data is a directory beside the record**, never the record's directory itself, so a
//!   guest that fills a volume cannot write over the file that describes it.
//! - **Removing one is removing what is in it.** [`VolumeStore::remove`] refuses a volume that
//!   holds anything unless it is told twice, because the whole point of a volume is that it is the
//!   thing that was not ephemeral.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use crate::{ParseError, checked_id, id_rule, line, now_ms, temp_name, unescape, valid_id};

/// The volume format's version, the first line of every volume file.
pub const VOLUME_FORMAT: u32 = 1;

/// The lines a volume cannot be read without.
const REQUIRED: [&str; 2] = ["volume", "name"];

/// What a volume's directory holds: the file that describes it, and the tree a guest sees.
const RECORD_FILE: &str = "volume";
const DATA_DIR: &str = "data";

/// A directory with a life of its own, mounted into sandboxes by name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Volume {
    /// The name it is mounted and listed by, and the directory it is kept in.
    pub name: String,
    /// One line saying what is in it, or empty.
    pub about: String,
    /// When it was created, milliseconds since the Unix epoch.
    pub created_ms: u64,
}

impl Volume {
    /// A volume named `name`, created now.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            about: String::new(),
            created_ms: now_ms(),
        }
    }

    /// The same, with a line saying what is in it.
    #[must_use]
    pub fn about(mut self, about: &str) -> Self {
        self.about = about.to_string();
        self
    }

    /// The volume as the text its file holds.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        line(&mut out, "volume", &VOLUME_FORMAT.to_string());
        line(&mut out, "name", &self.name);
        if !self.about.is_empty() {
            line(&mut out, "about", &self.about);
        }
        line(&mut out, "created", &self.created_ms.to_string());
        out
    }

    /// A volume read back from its text.
    ///
    /// # Errors
    ///
    /// A line that will not parse, a format this build does not read, a missing required line, or
    /// a name that is not a usable directory name.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut volume = Self {
            name: String::new(),
            about: String::new(),
            created_ms: 0,
        };
        let mut seen: Vec<&str> = Vec::new();
        for (n, row) in text.lines().enumerate() {
            if row.is_empty() {
                continue;
            }
            let (key, value) = row.split_once(' ').unwrap_or((row, ""));
            let bad = || ParseError(format!("line {}: {row:?}", n + 1));
            let value = &unescape(value).ok_or_else(bad)?;
            if let Some(key) = REQUIRED.iter().find(|k| **k == key) {
                seen.push(key);
            }
            match key {
                "volume" => {
                    let format: u32 = value.parse().map_err(|_| bad())?;
                    if format != VOLUME_FORMAT {
                        return Err(ParseError(format!(
                            "volume format {format}; this build reads {VOLUME_FORMAT}"
                        )));
                    }
                }
                "name" => volume.name = value.to_string(),
                "about" => volume.about = value.to_string(),
                "created" => volume.created_ms = value.parse().map_err(|_| bad())?,
                // A key this build does not know is one a later build wrote: carried past rather
                // than refused, so an older reader still lists the volume.
                _ => {}
            }
        }
        if let Some(missing) = REQUIRED.iter().find(|k| !seen.contains(k)) {
            return Err(ParseError(format!("no `{missing}` line")));
        }
        if !valid_id(&volume.name) {
            return Err(ParseError(format!(
                "{:?} is not a usable volume name: {}",
                volume.name,
                id_rule()
            )));
        }
        Ok(volume)
    }
}

/// The volumes directory as a store of them.
#[derive(Debug, Clone)]
pub struct VolumeStore {
    dir: PathBuf,
}

impl VolumeStore {
    /// The store at the volumes directory the environment names, created if absent.
    ///
    /// # Errors
    ///
    /// No directory can be named, or it cannot be created.
    pub fn open() -> io::Result<Self> {
        Self::at(volumes_dir()?)
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

    /// Every volume in the store, by name. A directory that will not parse is skipped rather than
    /// fatal, as a snapshot's is.
    #[must_use]
    pub fn list(&self) -> Vec<Volume> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Volume> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| valid_id(name))
            .filter_map(|name| self.read(&name).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The volume named `name`.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, the record is missing, or its text will not parse.
    pub fn read(&self, name: &str) -> io::Result<Volume> {
        let text = std::fs::read_to_string(self.dir_of(name)?.join(RECORD_FILE))?;
        let volume = Volume::parse(&text).map_err(io::Error::other)?;
        if volume.name != name {
            return Err(io::Error::other(format!(
                "the volume in {name} says it is {}",
                volume.name
            )));
        }
        Ok(volume)
    }

    /// Whether a volume of this name is in the store.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.dir_of(name)
            .is_ok_and(|d| d.join(RECORD_FILE).is_file())
    }

    /// Creates the volume's directory and its `data/`, and writes the record.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the directories cannot be made.
    pub fn create(&self, volume: &Volume) -> io::Result<PathBuf> {
        use std::os::unix::fs::DirBuilderExt;
        let dir = self.dir_of(&volume.name)?;
        let data = dir.join(DATA_DIR);
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder.create(&data)?;
        self.save(volume)?;
        Ok(data)
    }

    /// Rewrites a volume's record, atomically.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the file cannot be written.
    pub fn save(&self, volume: &Volume) -> io::Result<()> {
        let dir = self.dir_of(&volume.name)?;
        let tmp = dir.join(temp_name("volume"));
        let written = std::fs::write(&tmp, volume.to_text())
            .and_then(|()| std::fs::rename(&tmp, dir.join(RECORD_FILE)));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    }

    /// The directory a sandbox sees when it mounts this volume.
    ///
    /// **Not the volume's own directory**: a guest that filled a volume would otherwise write over
    /// the record that describes it.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or there is no volume of that name.
    pub fn data_of(&self, name: &str) -> io::Result<PathBuf> {
        let data = self.dir_of(name)?.join(DATA_DIR);
        if data.is_dir() {
            Ok(data)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no volume named {name:?}"),
            ))
        }
    }

    /// What a volume holds, in bytes, walking its data directory.
    ///
    /// **A number a person can act on**, which is the one thing a volume list should say that a
    /// name and a date do not. Symlinks are counted as themselves and never followed, so a link
    /// into the host cannot make a volume look enormous or loop forever.
    #[must_use]
    pub fn size_of(&self, name: &str) -> u64 {
        let Ok(data) = self.data_of(name) else {
            return 0;
        };
        let mut total = 0;
        let mut stack = vec![data];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
        total
    }

    /// Whether a volume has anything in it, which is what makes removing one a question.
    #[must_use]
    pub fn is_empty(&self, name: &str) -> bool {
        self.data_of(name)
            .and_then(std::fs::read_dir)
            .is_ok_and(|mut entries| entries.next().is_none())
    }

    /// Removes a volume and everything in it.
    ///
    /// **`force` is not a formality.** A volume exists because its contents were worth keeping
    /// past the run that made them; removing one that holds something is the one destructive act
    /// in this module, so it is refused until it is asked for twice.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, there is no such volume, it holds something and `force` is
    /// not set, or the directory cannot be removed.
    pub fn remove(&self, name: &str, force: bool) -> io::Result<()> {
        let dir = self.dir_of(name)?;
        if !dir.join(RECORD_FILE).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no volume named {name:?}"),
            ));
        }
        if !force && !self.is_empty(name) {
            return Err(io::Error::other(format!(
                "the volume {name:?} is not empty; removing it removes what is in it"
            )));
        }
        std::fs::remove_dir_all(&dir)
    }

    /// The directory a volume of this name lives in, refusing a name that is not one entry.
    fn dir_of(&self, name: &str) -> io::Result<PathBuf> {
        Ok(self.dir.join(checked_id(name)?))
    }
}

/// The volumes directory, from the environment: `$BOXDESK_VOLUMES_DIR`, else
/// `$XDG_DATA_HOME/boxdesk/volumes`, else `~/.local/share/boxdesk/volumes`.
///
/// # Errors
///
/// None of the three is set, so there is nowhere to put one.
pub fn volumes_dir() -> io::Result<PathBuf> {
    volumes_dir_from(
        std::env::var_os("BOXDESK_VOLUMES_DIR"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
    .ok_or_else(|| {
        io::Error::other("no volumes directory: set BOXDESK_VOLUMES_DIR, XDG_DATA_HOME or HOME")
    })
}

/// [`volumes_dir`] with the environment reads lifted out.
#[must_use]
pub fn volumes_dir_from(
    volumes: Option<OsString>,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    volumes
        .map(PathBuf::from)
        .or_else(|| xdg_data.map(|d| PathBuf::from(d).join("boxdesk/volumes")))
        .or_else(|| home.map(|h| PathBuf::from(h).join(".local/share/boxdesk/volumes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (boxdesk_test_support::ScratchDir, VolumeStore) {
        let dir = boxdesk_test_support::ScratchDir::created("volume-store");
        let store = VolumeStore::at(dir.path().join("volumes")).expect("a store");
        (dir, store)
    }

    /// Everything a volume holds survives being written and read.
    #[test]
    fn a_volume_reads_back_as_what_was_written() {
        let before = Volume::new("datasets").about("the corpus, unpacked");
        let after = Volume::parse(&before.to_text()).expect("its own text parses");
        assert_eq!(after, before);
    }

    /// **A guest writes into `data/`, never into the volume's own directory.** A volume whose
    /// record sat beside the files a guest could write would be a volume a guest could rewrite the
    /// description of.
    #[test]
    fn what_a_guest_mounts_is_beside_the_record_and_not_over_it() {
        let (_dir, store) = scratch();
        let data = store.create(&Volume::new("datasets")).expect("created");

        assert!(data.is_dir(), "a volume is a directory a guest can write");
        assert_eq!(data.file_name().and_then(|n| n.to_str()), Some(DATA_DIR));
        assert_eq!(
            store.data_of("datasets").expect("the same directory"),
            data,
            "what a mount resolves to is what was created"
        );
        let record = data
            .parent()
            .expect("the volume's own directory")
            .join(RECORD_FILE);
        assert!(record.is_file(), "the record sits beside the data");
        assert!(
            !record.starts_with(&data),
            "and never inside what a guest can write"
        );
    }

    /// A volume exists because what is in it outlived the run that made it, so removing a full one
    /// is refused until it is asked for twice.
    #[test]
    fn removing_a_volume_that_holds_something_is_refused_once() {
        let (_dir, store) = scratch();
        let data = store.create(&Volume::new("datasets")).expect("created");
        assert!(store.is_empty("datasets"), "a fresh volume holds nothing");

        std::fs::write(data.join("corpus.bin"), vec![0u8; 2048]).expect("wrote");
        assert!(!store.is_empty("datasets"));
        assert_eq!(store.size_of("datasets"), 2048, "and says how much");

        let err = store
            .remove("datasets", false)
            .expect_err("a full volume is not removed by asking once");
        assert!(err.to_string().contains("not empty"), "said {err}");
        assert!(store.holds("datasets"), "and it is still here");

        store.remove("datasets", true).expect("asked twice");
        assert!(!store.holds("datasets"));
    }

    /// An empty one goes without the second ask: there is nothing to lose.
    #[test]
    fn removing_an_empty_volume_needs_no_second_ask() {
        let (_dir, store) = scratch();
        store.create(&Volume::new("scratch")).expect("created");
        store.remove("scratch", false).expect("nothing to lose");
        assert!(!store.holds("scratch"));
        assert!(
            store.remove("scratch", false).is_err(),
            "and removing what is not there says so"
        );
    }

    /// Listing is by name, so two machines showing one store show it the same way.
    #[test]
    fn the_store_lists_in_name_order() {
        let (_dir, store) = scratch();
        for name in ["models", "cache", "datasets"] {
            store.create(&Volume::new(name)).expect("created");
        }
        assert_eq!(
            store
                .list()
                .iter()
                .map(|v| v.name.as_str())
                .collect::<Vec<_>>(),
            vec!["cache", "datasets", "models"]
        );
    }

    /// A name that is not one directory entry is refused when it is read, because the name is
    /// joined to a path and the remove hands the result to `remove_dir_all`.
    #[test]
    fn a_name_that_is_not_a_directory_name_is_refused() {
        let text = Volume::new("ok")
            .to_text()
            .replace("name ok", "name ../etc");
        let err = Volume::parse(&text).expect_err("a traversal is not a name");
        assert!(err.to_string().contains("usable volume name"), "said {err}");

        let (_dir, store) = scratch();
        assert!(
            store.data_of("../etc").is_err(),
            "and the store refuses one on the way to a path"
        );
    }

    /// The three places a volumes directory can come from, in the order they are asked.
    #[test]
    fn the_volumes_directory_comes_from_the_first_of_three() {
        let some = |s: &str| Some(OsString::from(s));
        assert_eq!(
            volumes_dir_from(some("/tmp/vols"), some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/tmp/vols"))
        );
        assert_eq!(
            volumes_dir_from(None, some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/xdg/boxdesk/volumes"))
        );
        assert_eq!(
            volumes_dir_from(None, None, some("/home/u")),
            Some(PathBuf::from("/home/u/.local/share/boxdesk/volumes"))
        );
        assert_eq!(volumes_dir_from(None, None, None), None);
    }
}
