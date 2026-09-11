#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! The Rust SDK for [Boxdesk], which runs untrusted code inside a hardware-isolated virtual
//! machine on your own machine.
//!
//! This crate implements no virtualization and speaks no HTTP. It calls the same
//! [`execute_sandbox`] the `boxdesk` binary calls, in this process, so a run started here leaves
//! the record a run started at a keyboard leaves — there is one run path, not two that agree
//! today.
//!
//! ```no_run
//! use boxdesk::{Sandbox, Boxdesk};
//!
//! let outcome = Boxdesk::new().run(Sandbox::new(["echo", "hi"]))?;
//! println!("{}", outcome.stdout()?);
//! # Ok::<(), boxdesk::Error>(())
//! ```
//!
//! [Boxdesk]: https://github.com/kendricklawton/boxdesk

use std::ffi::OsString;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::{Path, PathBuf};

// Named one by one rather than glob-re-exported. Two `pub use ...::*` lines put every item of two
// crates into this namespace, so anything either adds later lands in this SDK's public API without
// anybody deciding it should.
pub use boxdesk_record::{
    DisplayMode, End, Mount, Network, Posture, Record, Rootfs, RunDir, Share, Verb,
};
pub use boxdesk_supervisor::{Net, RootFs};

/// What went wrong. A guest command exiting non-zero is **not** one of these: that is an
/// [`Outcome`] whose [`ok`](Outcome::ok) is false.
#[derive(Debug)]
pub enum Error {
    /// The posture could not be settled — no guest root, a limit of zero, an unknown word.
    Posture(String),
    /// The sandbox itself failed to run.
    Sandbox(String),
    /// A run's directory could not be read back.
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Posture(why) | Self::Sandbox(why) => f.write_str(why),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The posture to run under: what the sandbox may touch, and with how much.
///
/// Every field is optional and unset means the CLI's own default, so this adds no defaults of its
/// own. It is the argument to [`Boxdesk::run`].
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    command: Vec<String>,
    name: Option<String>,
    root: Option<PathBuf>,
    vcpus: Option<NonZeroU8>,
    mem_mib: Option<NonZeroU32>,
    workdir: Option<PathBuf>,
    mounts: Vec<(PathBuf, PathBuf)>,
    shares: Vec<(String, PathBuf)>,
    net: Option<Net>,
    rootfs: Option<RootFs>,
    env: Vec<OsString>,
    no_results: bool,
    keep: bool,
    gpu: bool,
    sound: bool,
}

impl Sandbox {
    /// A sandbox that runs `command`, whose first word the GUEST's `PATH` resolves.
    #[must_use]
    pub fn new<I, S>(command: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            command: command.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Names the sandbox. Unset, one is named after this process.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The guest root tree. Unset: `$BOXDESK_GUEST_ROOT`, then the per-user data directory.
    #[must_use]
    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = Some(root.into());
        self
    }

    /// vCPUs. Non-zero by type: a machine with no cpu never booted.
    #[must_use]
    pub fn vcpus(mut self, vcpus: NonZeroU8) -> Self {
        self.vcpus = Some(vcpus);
        self
    }

    /// Guest RAM in MiB, non-zero for [`vcpus`](Self::vcpus)' reason.
    #[must_use]
    pub fn mem_mib(mut self, mem_mib: NonZeroU32) -> Self {
        self.mem_mib = Some(mem_mib);
        self
    }

    /// The guest working directory.
    #[must_use]
    pub fn workdir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workdir = Some(dir.into());
        self
    }

    /// A host directory, read-write, at a guest path.
    #[must_use]
    pub fn mount(mut self, guest: impl Into<PathBuf>, host: impl Into<PathBuf>) -> Self {
        self.mounts.push((guest.into(), host.into()));
        self
    }

    /// An extra virtiofs device the guest mounts by tag.
    #[must_use]
    pub fn share(mut self, tag: impl Into<String>, host: impl Into<PathBuf>) -> Self {
        self.shares.push((tag.into(), host.into()));
        self
    }

    /// The network posture. Default [`Net::None`]: no network at all.
    #[must_use]
    pub fn net(mut self, net: Net) -> Self {
        self.net = Some(net);
        self
    }

    /// What the guest may do to its root. Default [`RootFs::ReadOnly`].
    #[must_use]
    pub fn rootfs(mut self, rootfs: RootFs) -> Self {
        self.rootfs = Some(rootfs);
        self
    }

    /// One `KEY=VALUE` guest environment entry.
    ///
    /// The guest receives the whole entry. The record keeps **only the name**, so a value set here
    /// is never readable back off a [`Record`].
    #[must_use]
    pub fn env(mut self, entry: impl Into<OsString>) -> Self {
        self.env.push(entry.into());
        self
    }

    /// Drops the default `/results` mount.
    #[must_use]
    pub fn no_results(mut self) -> Self {
        self.no_results = true;
        self
    }

    /// Files the run's record instead of sweeping it.
    ///
    /// A run is ephemeral by default: its output comes back in the [`Outcome`] and the directory
    /// it worked in goes with it. Keep it to read what the guest wrote to `/results`, or to reach
    /// the run again with [`Boxdesk::show`].
    #[must_use]
    pub fn keep(mut self) -> Self {
        self.keep = true;
        self
    }

    /// Offers the host GPU's 3D path. A host whose libkrun lacks the backend refuses the run.
    #[must_use]
    pub fn gpu(mut self) -> Self {
        self.gpu = true;
        self
    }

    /// Offers a sound card, with the same portability caveat as [`gpu`](Self::gpu).
    #[must_use]
    pub fn sound(mut self) -> Self {
        self.sound = true;
        self
    }
}

/// What a run left behind: the record, and the directory when one was kept.
///
/// A named struct rather than a tuple, because `(Record, Option<RunDir>, u8)` reads as three
/// unrelated values at a call site and says nothing about which `u8` it is.
#[derive(Debug)]
pub struct Outcome {
    /// Everything the run store knows about the run.
    pub record: Record,
    /// The run's directory, when [`Sandbox::keep`] filed one.
    pub dir: Option<RunDir>,
    /// The guest command's own exit status, which is what `boxdesk run` exits with.
    pub code: u8,
}

impl Outcome {
    /// Whether the guest command itself succeeded.
    #[must_use]
    pub fn ok(&self) -> bool {
        matches!(self.record.end, Some(End::Exit(0)))
    }

    /// What the guest wrote to stdout. Empty for a swept run, which kept no directory.
    pub fn stdout(&self) -> Result<String, Error> {
        self.captured(RunDir::stdout)
    }

    /// What the guest wrote to stderr, with [`stdout`](Self::stdout)'s caveat.
    pub fn stderr(&self) -> Result<String, Error> {
        self.captured(RunDir::stderr)
    }

    /// The files the guest left in `/results`, empty unless the run was kept.
    pub fn files(&self) -> Result<Vec<(PathBuf, u64)>, Error> {
        self.dir.as_ref().map_or_else(
            || Ok(Vec::new()),
            |dir| dir.result_files().map_err(Error::Io),
        )
    }

    fn captured(&self, which: fn(&RunDir) -> PathBuf) -> Result<String, Error> {
        let Some(dir) = self.dir.as_ref() else {
            return Ok(String::new());
        };
        match std::fs::read_to_string(which(dir)) {
            Ok(text) => Ok(text),
            // A run that wrote nothing to a stream leaves no file for it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(Error::Io(e)),
        }
    }
}

/// A handle on the Boxdesk core.
#[derive(Debug, Clone, Copy, Default)]
pub struct Boxdesk;

impl Boxdesk {
    /// A new handle. It holds nothing; the run store is opened per call.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Boot a fresh sandbox, run the command, wait, and return what it left.
    ///
    /// A non-zero guest exit is not an error: it is an [`Outcome`] whose [`ok`](Outcome::ok) is
    /// false. Only a failure of Boxdesk itself is an [`Error`].
    pub fn run(&self, sandbox: Sandbox) -> Result<Outcome, Error> {
        self.execute(sandbox, false)
    }

    /// Settle and return the posture without booting anything. The record has no end.
    pub fn dry_run(&self, sandbox: Sandbox) -> Result<Outcome, Error> {
        self.execute(sandbox, true)
    }

    /// One filed run, by id or by name.
    pub fn show(&self, key: &str) -> Result<Outcome, Error> {
        let store = boxdesk_record::Store::open()?;
        let record = store
            .find(key)?
            .ok_or_else(|| Error::Sandbox(format!("no run {key:?} in the run store")))?;
        let dir = store.dir_of(&record.id);
        let dir = dir.path().is_dir().then_some(dir);
        Ok(Outcome {
            record,
            dir,
            code: 0,
        })
    }

    /// The filed runs, newest first. Without `all`, only the ones still open.
    pub fn runs(&self, all: bool) -> Result<Vec<Record>, Error> {
        let store = boxdesk_record::Store::open()?;
        Ok(store
            .list()?
            .into_iter()
            .filter(|record| all || record.is_open())
            .collect())
    }

    fn execute(&self, sandbox: Sandbox, dry_run: bool) -> Result<Outcome, Error> {
        let Some((program, rest)) = sandbox.command.split_first() else {
            return Err(Error::Posture(
                "command is empty: pass at least the program to run".to_string(),
            ));
        };
        let root = boxdesk_core::resolve_root(sandbox.root.as_deref().map(Path::new))
            .map_err(Error::Posture)?;

        let mut cfg = boxdesk_supervisor::VmConfig::new(root, program);
        cfg.args = rest.iter().map(OsString::from).collect();
        // The whole entry goes to the guest; `boxdesk_core::posture_of` is what cuts it down to a
        // name for the record, so this SDK never holds both halves.
        cfg.env = sandbox.env;
        cfg.workdir = sandbox.workdir;
        cfg.mounts = sandbox.mounts;
        cfg.shares = sandbox.shares;
        cfg.gpu = sandbox.gpu;
        cfg.sound = sandbox.sound;
        if let Some(v) = sandbox.vcpus {
            cfg.vcpus = v;
        }
        if let Some(m) = sandbox.mem_mib {
            cfg.mem_mib = m;
        }
        if let Some(n) = sandbox.net {
            cfg.net = n;
        }
        if let Some(r) = sandbox.rootfs {
            cfg.rootfs = r;
        }

        let opts = boxdesk_core::SandboxOptions {
            name: sandbox
                .name
                .unwrap_or_else(|| format!("run-{}", std::process::id())),
            command: sandbox.command,
            cfg,
            results: !sandbox.no_results,
            keep: sandbox.keep,
            dry_run,
            // The caller reads the output off the Outcome; writing it to this process's stdout as
            // well would put guest bytes into the host program's own streams.
            quiet: true,
        };
        let (record, dir, code) = boxdesk_core::execute_sandbox(opts).map_err(Error::Sandbox)?;
        Ok(Outcome { record, dir, code })
    }
}
