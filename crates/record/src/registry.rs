//! A registry: where images come from, and who this machine is when it asks.
//!
//! - **An address, never a secret.** A record here holds the host, the project and the username —
//!   the things you would read off a page. The password is read from `$BOXDESK_REGISTRY_PASSWORD`
//!   at the moment a pull needs it and is never written down, for the same reason a posture keeps
//!   the *names* of environment entries and never their values: a file this tool writes is a file
//!   that gets copied, backed up and shared.
//! - **A name is the file name**, by [`crate::valid_id`]'s allow-list, because the name is joined
//!   to a path and [`RegistryStore::remove`] hands the result to the filesystem.
//! - **Local, and only here.** `$BOXDESK_REGISTRIES_DIR`, else
//!   `$XDG_DATA_HOME/boxdesk/registries`, else `~/.local/share/boxdesk/registries`, created
//!   `0700`.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::{ParseError, checked_id, id_rule, line, now_ms, temp_name, unescape, valid_id};

/// The registry format's version, the first line of every registry file.
pub const REGISTRY_FORMAT: u32 = 1;

/// The lines a registry cannot be read without: without a host there is nowhere to ask.
const REQUIRED: [&str; 3] = ["registry", "name", "url"];

/// Where images come from, and who this machine is when it asks for one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Registry {
    /// The short handle this is reached and listed by, and the file it is kept in.
    pub name: String,
    /// The registry host, as it appears in an image reference: `ghcr.io`, `docker.io`.
    pub url: String,
    /// The namespace or organisation images sit under there, or empty.
    pub project: String,
    /// Who this machine signs in as, or empty for anonymous pulls.
    ///
    /// **There is no password field, and that is deliberate.** See the module's first bullet.
    pub username: String,
    /// When it was added, milliseconds since the Unix epoch.
    pub added_ms: u64,
}

impl Registry {
    /// A registry named `name` at `url`, added now.
    #[must_use]
    pub fn new(name: &str, url: &str) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            project: String::new(),
            username: String::new(),
            added_ms: now_ms(),
        }
    }

    /// The same, under a project and a username.
    #[must_use]
    pub fn signed_in_as(mut self, project: &str, username: &str) -> Self {
        self.project = project.to_string();
        self.username = username.to_string();
        self
    }

    /// The registry as the text its file holds.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        line(&mut out, "registry", &REGISTRY_FORMAT.to_string());
        line(&mut out, "name", &self.name);
        line(&mut out, "url", &self.url);
        if !self.project.is_empty() {
            line(&mut out, "project", &self.project);
        }
        if !self.username.is_empty() {
            line(&mut out, "username", &self.username);
        }
        line(&mut out, "added", &self.added_ms.to_string());
        out
    }

    /// A registry read back from its text.
    ///
    /// # Errors
    ///
    /// A line that will not parse, a format this build does not read, a missing required line, or
    /// a name that is not a usable file name.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut registry = Self {
            name: String::new(),
            url: String::new(),
            project: String::new(),
            username: String::new(),
            added_ms: 0,
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
                "registry" => {
                    let format: u32 = value.parse().map_err(|_| bad())?;
                    if format != REGISTRY_FORMAT {
                        return Err(ParseError(format!(
                            "registry format {format}; this build reads {REGISTRY_FORMAT}"
                        )));
                    }
                }
                "name" => registry.name = value.to_string(),
                "url" => registry.url = value.to_string(),
                "project" => registry.project = value.to_string(),
                "username" => registry.username = value.to_string(),
                "added" => registry.added_ms = value.parse().map_err(|_| bad())?,
                // A key this build does not know is one a later build wrote: carried past rather
                // than refused, so an older reader still lists the registry.
                //
                // **A `password` line would land here and be ignored**, which is the behaviour to
                // want: nothing this build writes puts one in a file, and nothing it reads would
                // use one that somebody else put there.
                _ => {}
            }
        }
        if let Some(missing) = REQUIRED.iter().find(|k| !seen.contains(k)) {
            return Err(ParseError(format!("no `{missing}` line")));
        }
        if !valid_id(&registry.name) {
            return Err(ParseError(format!(
                "{:?} is not a usable registry name: {}",
                registry.name,
                id_rule()
            )));
        }
        if registry.url.is_empty() {
            return Err(ParseError("no url".to_string()));
        }
        Ok(registry)
    }
}

/// The registries directory as a store of them.
#[derive(Debug, Clone)]
pub struct RegistryStore {
    dir: PathBuf,
}

impl RegistryStore {
    /// The store at the registries directory the environment names, created if absent.
    ///
    /// # Errors
    ///
    /// No directory can be named, or it cannot be created.
    pub fn open() -> io::Result<Self> {
        Self::at(registries_dir()?)
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

    /// Every registry in the store, by name. A file that will not parse is skipped rather than
    /// fatal, as a snapshot's is.
    #[must_use]
    pub fn list(&self) -> Vec<Registry> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Registry> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| valid_id(name))
            .filter_map(|name| self.read(&name).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The registry named `name`.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, the file is missing, or its text will not parse.
    pub fn read(&self, name: &str) -> io::Result<Registry> {
        let text = std::fs::read_to_string(self.path_of(name)?)?;
        let registry = Registry::parse(&text).map_err(io::Error::other)?;
        if registry.name != name {
            return Err(io::Error::other(format!(
                "the registry in {name} says it is {}",
                registry.name
            )));
        }
        Ok(registry)
    }

    /// The registry serving `host`, if this machine knows one.
    ///
    /// **By host, not by name.** A pull has an image reference in hand and wants to know who to be
    /// when it asks; what the person called the entry is not in the reference.
    #[must_use]
    pub fn serving(&self, host: &str) -> Option<Registry> {
        self.list().into_iter().find(|r| r.url == host)
    }

    /// Whether a registry of this name is in the store.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.path_of(name).is_ok_and(|p| p.is_file())
    }

    /// Writes `registry` over whatever was under its name, atomically.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the file cannot be written.
    pub fn save(&self, registry: &Registry) -> io::Result<()> {
        let path = self.path_of(&registry.name)?;
        let tmp = self.dir.join(temp_name("registry"));
        let written =
            std::fs::write(&tmp, registry.to_text()).and_then(|()| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    }

    /// Removes the registry named `name`.
    ///
    /// # Errors
    ///
    /// The name is not a usable one, or the file cannot be removed.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        std::fs::remove_file(self.path_of(name)?)
    }

    /// The file a registry of this name lives in, refusing a name that is not one directory entry.
    fn path_of(&self, name: &str) -> io::Result<PathBuf> {
        Ok(self.dir.join(checked_id(name)?))
    }
}

/// The registries directory, from the environment: `$BOXDESK_REGISTRIES_DIR`, else
/// `$XDG_DATA_HOME/boxdesk/registries`, else `~/.local/share/boxdesk/registries`.
///
/// # Errors
///
/// None of the three is set, so there is nowhere to put one.
pub fn registries_dir() -> io::Result<PathBuf> {
    registries_dir_from(
        std::env::var_os("BOXDESK_REGISTRIES_DIR"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
    .ok_or_else(|| {
        io::Error::other(
            "no registries directory: set BOXDESK_REGISTRIES_DIR, XDG_DATA_HOME or HOME",
        )
    })
}

/// [`registries_dir`] with the environment reads lifted out.
#[must_use]
pub fn registries_dir_from(
    registries: Option<OsString>,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    registries
        .map(PathBuf::from)
        .or_else(|| xdg_data.map(|d| PathBuf::from(d).join("boxdesk/registries")))
        .or_else(|| home.map(|h| PathBuf::from(h).join(".local/share/boxdesk/registries")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn furnished() -> Registry {
        Registry::new("work", "ghcr.io").signed_in_as("acme", "buildbot")
    }

    /// Everything a registry holds survives being written and read.
    #[test]
    fn a_registry_reads_back_as_what_was_written() {
        let before = furnished();
        let after = Registry::parse(&before.to_text()).expect("its own text parses");
        assert_eq!(after, before);
    }

    /// **No password reaches the file, and none is read back out of one.**
    ///
    /// This is the rule the whole module is arranged around, so it is checked from both sides: a
    /// registry writes no secret, and a file that somebody else put a `password` line into is
    /// parsed without one — the key falls through to the unknown-key arm and is dropped.
    #[test]
    fn a_password_neither_reaches_the_file_nor_comes_back_out_of_one() {
        // One instance, compared against itself: `furnished()` stamps the moment it was called,
        // so two of them differ by a millisecond and the comparison would be a flake.
        let registry = furnished();
        let text = registry.to_text();
        assert!(
            !text.to_lowercase().contains("password"),
            "a registry file must never carry one: {text:?}"
        );

        let planted = format!("{text}password hunter2\n");
        let read = Registry::parse(&planted).expect("an unknown key is carried past");
        assert_eq!(read, registry, "the planted line changed nothing");
    }

    /// A pull has a reference in hand, not a name somebody chose, so the lookup that matters is by
    /// host.
    #[test]
    fn the_registry_serving_a_host_is_found_by_that_host() {
        let dir = boxdesk_test_support::ScratchDir::created("registry-serving");
        let store = RegistryStore::at(dir.path().join("registries")).expect("a store");
        store.save(&furnished()).expect("saved");
        store
            .save(&Registry::new("hub", "docker.io"))
            .expect("saved");

        let found = store.serving("ghcr.io").expect("the one at ghcr.io");
        assert_eq!(found.name, "work");
        assert_eq!(found.username, "buildbot");
        assert!(
            store.serving("quay.io").is_none(),
            "a host nobody configured is nobody's"
        );
    }

    /// Saving, reading back, listing in name order, and removing.
    #[test]
    fn the_store_keeps_a_registry_under_its_name() {
        let dir = boxdesk_test_support::ScratchDir::created("registry-store");
        let store = RegistryStore::at(dir.path().join("registries")).expect("a store");
        assert!(store.list().is_empty(), "a fresh store holds nothing");

        for (name, url) in [
            ("work", "ghcr.io"),
            ("acme", "quay.io"),
            ("hub", "docker.io"),
        ] {
            store.save(&Registry::new(name, url)).expect("saved");
        }
        assert_eq!(
            store
                .list()
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            vec!["acme", "hub", "work"],
            "two machines showing one store in two orders is a list nobody can scan"
        );

        assert!(store.holds("work"));
        assert_eq!(store.read("work").expect("read back").url, "ghcr.io");
        store.remove("work").expect("removed");
        assert!(!store.holds("work"));
    }

    /// A registry with nowhere to ask is not one, and a name that is not a file name is refused
    /// when it is read rather than when it is used.
    #[test]
    fn a_registry_without_a_host_or_a_usable_name_is_refused() {
        let no_url: String = furnished()
            .to_text()
            .lines()
            .filter(|l| !l.starts_with("url "))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(Registry::parse(&no_url).is_err(), "nowhere to ask");

        let bad_name = furnished().to_text().replace("name work", "name ../etc");
        let err = Registry::parse(&bad_name).expect_err("a traversal is not a name");
        assert!(
            err.to_string().contains("usable registry name"),
            "said {err}"
        );
    }

    /// The three places a registries directory can come from, in the order they are asked.
    #[test]
    fn the_registries_directory_comes_from_the_first_of_three() {
        let some = |s: &str| Some(OsString::from(s));
        assert_eq!(
            registries_dir_from(some("/tmp/regs"), some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/tmp/regs"))
        );
        assert_eq!(
            registries_dir_from(None, some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/xdg/boxdesk/registries"))
        );
        assert_eq!(
            registries_dir_from(None, None, some("/home/u")),
            Some(PathBuf::from("/home/u/.local/share/boxdesk/registries"))
        );
        assert_eq!(registries_dir_from(None, None, None), None);
    }
}
