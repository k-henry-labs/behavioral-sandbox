//! `Tormoni`: the notebook. Runs on this machine, live and past, one row each; a run opens to
//! its record, and a live one to its display with the keyboard and pointer going in.
//!
//! - **Everything here the CLI can do.** The records are `tormoni-record`'s, read straight from the
//!   runs directory; starting, stopping and a shell go through the `tormoni` binary beside this one,
//!   so the app grows no verb the CLI lacks and an agent driving the CLI and a person at this
//!   window see one notebook.
//! - **Nothing leaves the machine.** The runs directory is local, the sockets are local, and the
//!   only processes started are `tormoni` and, for a shell, the operator's terminal.
//! - **Bounded.** The list is what retention keeps, the output pane shows the tail of a file up
//!   to a fixed size, the frame history is capped, and a display lease is shut down when its run
//!   is left, so nothing grows with time in the window.
//! - **The frame logs are the measurement.** `--log`, `--drawn-log` and `--input-log` record the
//!   display path on the host's monotonic clock, as `cargo xtask bench-frames --app` reads them.
#![deny(unsafe_code)]

mod account;
mod chrome;
mod cli;
mod device;
mod fonts;
mod frame;
mod icons;
mod lease;
mod screens;
mod state;
mod theme;
mod timer;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use iced::animation::Easing;
use iced::{Animation, Element, Size, Subscription, Task};

use tormoni_krun::SharedFrames;
use tormoni_record::{Record, Store};
use tormoni_supervisor::control::Damage;

/// Exit code for an operational failure, the CLI's convention.
const EXIT_OPERATIONAL: u8 = 2;

/// Presents the app remembers the damage of, so an upload after a run of missed redraws covers
/// exactly what changed since the frame it last uploaded.
const HISTORY: usize = 64;

/// The display a form offers before anyone changes it, and what a record without one falls back
/// to when it is re-run.
const DEFAULT_DISPLAY: &str = "640x480";

/// The limits a form offers before anyone changes it, and what a field nobody typed a number in
/// falls back to. Non-zero by type, as the record's own limits are.
pub(crate) const DEFAULT_VCPUS: std::num::NonZeroU8 = std::num::NonZeroU8::MIN;
pub(crate) const DEFAULT_MEM_MIB: std::num::NonZeroU32 = match std::num::NonZeroU32::new(512) {
    Some(mib) => mib,
    None => std::num::NonZeroU32::MIN,
};

/// Bytes of an output file the pane shows, from its end.
const OUTPUT_TAIL: u64 = 256 * 1024;

/// The shortest gap between two presents a thumbnail in the list is redrawn for. A thumbnail is
/// a glance, not a screen, and every present it takes is a whole window rebuild.
const THUMBNAIL_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// The most live displays the list leases at once, newest first. Each costs a thread, a socket
/// and a scanout mapping.
const MAX_THUMBNAILS: usize = 12;

/// The grid's leases plus the open run must each have a texture to upload into, or the cache
/// thrashes. The compiler holds the two constants in step, so neither can be raised alone.
const _: () = assert!(MAX_THUMBNAILS < frame::MAX_TEXTURES);

/// What the platform calls this application: the name the packaged executable carries, which
/// macOS reads for the menu bar and the Dock and a desktop entry names in `Exec`.
///
/// Not `CARGO_BIN_NAME`. Cargo writes every binary of a workspace into one directory, and the
/// default macOS filesystem is case-insensitive, so a `Tormoni` built beside `tormoni` would be
/// the same file; the build keeps them apart and `cargo xtask dist` renames this one.
pub(crate) const NAME: &str = "Tormoni";

/// The identifier the application registers under. The Linux window carries it, which is what
/// pairs the window with its desktop entry; xtask's bundle carries the same word as
/// `CFBundleIdentifier` and holds the two equal by reading this line.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const APP_ID: &str = "ai.tormoni.app";

#[derive(Parser)]
#[command(
    name = NAME,
    version,
    about = "The notebook: sandboxes on this machine, live and past, and their displays."
)]
struct Cli {
    /// Open straight onto this run, by id or by name (the newest of that name), instead of the
    /// list.
    name: Option<String>,
    /// Append one `frame_id<TAB>nanoseconds` line here per present record read.
    #[arg(long, value_name = "PATH")]
    log: Option<PathBuf>,
    /// Append one `frame_id<TAB>nanoseconds` line here per frame uploaded to the GPU.
    #[arg(long, value_name = "PATH")]
    drawn_log: Option<PathBuf>,
    /// Append each input line sent to the guest here, as it went down the session.
    #[arg(long, value_name = "PATH")]
    input_log: Option<PathBuf>,
    /// Exit when the opened run's lease ends, as a measurement run wants; the default keeps the
    /// notebook open.
    #[arg(long)]
    exit_with_lease: bool,
    /// The mode to draw in: `light`, `dark`, or `system`, which follows the desktop. Case is
    /// ignored. Falls back to `$TORMONI_THEME`, then to the pick in Settings, then to `system`. An
    /// unknown name is refused with the three.
    #[arg(long, value_name = "NAME")]
    theme: Option<String>,
    /// The console to sign in to and open pages of: an `http://` or `https://` address. Falls
    /// back to `$TORMONI_CONSOLE`, then to the product's own.
    #[arg(long, value_name = "URL")]
    console: Option<String>,
    /// Open on this screen instead of the menu.
    #[arg(long, value_name = "SCREEN", conflicts_with = "name")]
    open: Option<OpenScreen>,
}

/// The screens the command line can open on: every one that needs no run to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OpenScreen {
    List,
    New,
    Settings,
}

impl OpenScreen {
    /// The [`Screen`] this asks for: the one place the two enums meet.
    fn screen(self) -> Screen {
        match self {
            Self::List => Screen::List,
            Self::New => Screen::New,
            Self::Settings => Screen::Settings,
        }
    }

    /// The screen a saved `open` line names, through the flag's own parser.
    fn from_name(name: &str) -> Option<Self> {
        <Self as clap::ValueEnum>::from_str(name, true).ok()
    }
}

/// The flag's spelling, so the state file and `--open` share one grammar.
impl std::fmt::Display for OpenScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::List => "list",
            Self::New => "new",
            Self::Settings => "settings",
        })
    }
}

/// What the notebook's list is doing.
///
/// The selected ids live in the mode rather than beside it, so there is no selection to leave
/// behind when the list goes back to being read, and no third state where a stale set and a
/// cleared flag disagree. Only ended runs are ever in it: a live one is refused a delete anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListMode {
    /// Reading the list; pressing a row opens its run.
    Browsing,
    /// Selecting records to remove; pressing a row adds or removes it instead of opening it.
    Selecting(BTreeSet<String>),
}

/// What a destructive press is waiting on, and the only thing that draws the modal.
///
/// **Nothing is removed until [`Message::DeleteConfirmed`] answers one of these.** One value for
/// both paths, so a window cannot end up asking two questions at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// One run, pressed on its own pane or in its row.
    One(RunId),
    /// The list's selection, which a cancel leaves selected.
    Selected(BTreeSet<String>),
}

impl Confirm {
    /// How many records answering yes would remove.
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Selected(ids) => ids.len(),
        }
    }
}

impl ListMode {
    /// The ids selected so far, empty while browsing.
    pub(crate) fn selected(&self) -> &BTreeSet<String> {
        match self {
            Self::Browsing => {
                static NONE: std::sync::LazyLock<BTreeSet<String>> =
                    std::sync::LazyLock::new(BTreeSet::new);
                &NONE
            }
            Self::Selecting(ids) => ids,
        }
    }

    /// Whether a row should answer a press by being selected rather than opened.
    pub(crate) fn is_selecting(&self) -> bool {
        matches!(self, Self::Selecting(_))
    }
}

/// An interface scale Settings offers, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scale(pub(crate) u16);

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// The scales Settings offers.
const SCALES: [Scale; 4] = [Scale(90), Scale(100), Scale(110), Scale(125)];

/// The window as a source-list app opens one: on macOS the title is hidden and the titlebar is
/// transparent over the content, so the sidebar runs to the top with the traffic lights on it.
fn window_settings() -> iced::window::Settings {
    #[cfg(target_os = "macos")]
    let platform_specific = iced::window::settings::PlatformSpecific {
        title_hidden: true,
        titlebar_transparent: true,
        fullsize_content_view: true,
    };
    #[cfg(target_os = "linux")]
    let platform_specific = iced::window::settings::PlatformSpecific {
        application_id: APP_ID.to_string(),
        ..iced::window::settings::PlatformSpecific::default()
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let platform_specific = iced::window::settings::PlatformSpecific::default();
    iced::window::Settings {
        size: Size::new(1360.0, 860.0),
        min_size: Some(WINDOW_MIN),
        platform_specific,
        ..iced::window::Settings::default()
    }
}

/// The narrowest page this window is meant to draw: a card and the two gutters beside it. Below
/// this a line of prose is two words wide, which is not a layout but a failure of one.
const PAGE_MIN: f32 = 360.0;

/// The window will not be dragged narrower than the rail **and** a page beside it, nor shorter
/// than a card under a head. The rail is never folded for the person: a window that cannot hold
/// both is one this refuses to become, which is a floor rather than a layout that moves under
/// them.
const WINDOW_MIN: Size = Size::new(screens::SIDEBAR + PAGE_MIN + 2.0 * screens::GUTTER, 420.0);

/// Where a plain launch lands: the `--open` flag, else the saved pick, else the notebook.
fn landing(flag: Option<OpenScreen>, saved: Option<OpenScreen>) -> OpenScreen {
    flag.or(saved).unwrap_or(OpenScreen::List)
}

fn main() -> ExitCode {
    chrome::name_the_application();
    let cli = Cli::parse();
    // Before the adapter probe: a name this cannot resolve is a typo in an argument, and
    // answering it should not cost a GPU handle first.
    let asked = theme::asked_for(cli.theme.as_deref(), theme::from_env());
    let theme_overridden = asked.is_some();
    let saved = state::load();
    let (mode, theme_note) = match theme::startup(asked.as_deref(), saved.theme.as_deref()) {
        Ok(pair) => pair,
        Err(why) => {
            eprintln!("{NAME}: {why}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    let console = match account::console(cli.console.as_deref(), std::env::var(account::ENV).ok()) {
        Ok(origin) => origin,
        Err(why) => {
            eprintln!("{NAME}: {why}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    frame::report_adapter();
    let sinks = match frame::Sinks::open(cli.drawn_log.as_deref(), cli.input_log.as_deref()) {
        Ok(sinks) => Arc::new(sinks),
        Err(e) => {
            eprintln!("{NAME}: opening a log: {e}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    let store = match Store::open() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{NAME}: the runs directory: {e}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    let opening = cli.name.clone();
    let log = cli.log.clone();
    let exit_with_lease = cli.exit_with_lease;
    let open = cli.open;
    let scale = saved.scale.unwrap_or(100);
    let opens_on = saved.open.as_deref().and_then(OpenScreen::from_name);
    let boot = move || {
        let mut app = App::new(
            store.clone(),
            opening.clone(),
            log.clone(),
            Arc::clone(&sinks),
            exit_with_lease,
        );
        app.mode = mode;
        app.theme_overridden = theme_overridden;
        app.console = console.clone();
        app.scale = scale;
        app.opens_on = opens_on.unwrap_or(OpenScreen::List);
        if app.status.is_none() {
            app.status = theme_note.clone();
        }
        if opening.is_none() {
            app.set_screen(landing(open, opens_on).screen());
        }
        // Asked once here and followed by subscription after, so `System` is right from the
        // first frame rather than from the first change.
        (
            app,
            Task::batch([
                iced::system::theme().map(Message::DesktopTheme),
                // Both, because a window already open when this runs sends no `Opened` and one
                // opened after it is not `latest` yet.
                iced::window::latest()
                    .then(|id| id.map_or_else(Task::none, chrome::unify_titlebar)),
            ]),
        )
    };
    let ran = iced::application(boot, App::update, App::view)
        .subscription(App::subscription)
        .title(|app: &App| app.title())
        .theme(|app: &App| theme::theme(app.mode, app.desktop))
        // Before `.font`: `settings` replaces the whole set, fonts included.
        .settings(iced::Settings {
            default_text_size: iced::Pixels(screens::BODY),
            default_font: fonts::SANS,
            ..iced::Settings::default()
        })
        .font(icons::BYTES)
        .font(fonts::FACES[0])
        .font(fonts::FACES[1])
        .font(fonts::FACES[2])
        .font(fonts::FACES[3])
        .scale_factor(|app: &App| f32::from(app.scale) / 100.0)
        .window(window_settings())
        .run();
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{NAME}: {e}");
            ExitCode::from(EXIT_OPERATIONAL)
        }
    }
}

/// A run's id: `<started_ms>-<name>`, which is also its directory under the runs directory.
///
/// Distinct from [`RunName`] by type because the id *contains* the name, so the two are freely
/// confusable as `String` and a mix-up is a silent lookup miss: a dead button, or a display that
/// never arrives. The record is the only place either is minted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RunId(String);

impl RunId {
    /// The id of `record`.
    pub(crate) fn of(record: &Record) -> Self {
        Self(record.id.clone())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A run's name: what its VM answers to on the control socket, and what a lease asks for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RunName(String);

impl RunName {
    /// The name of `record`.
    pub(crate) fn of(record: &Record) -> Self {
        Self(record.name.clone())
    }

    /// The name a started run reported, which is the only name minted outside a record.
    pub(crate) fn started(name: String) -> Self {
        Self(name)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RunName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which screen the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Screen {
    /// The notebook: every run, newest first.
    List,
    /// One run's record, by id.
    Run(RunId),
    /// The form for a new run.
    New,
    /// Runs worth trying, each one press from a filled form.
    Cookbook,
    /// The notebook's own knobs.
    Settings,
}

/// Which captured file the output pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
    Shell,
    Exec,
}

impl Stream {
    /// The file this stream is in a run's directory.
    fn path(self, dir: &tormoni_record::RunDir) -> PathBuf {
        match self {
            Self::Stdout => dir.stdout(),
            Self::Stderr => dir.stderr(),
            Self::Shell => dir.shell_log(),
            Self::Exec => dir.exec_log(),
        }
    }

    /// The streams a run of `verb` has.
    pub(crate) fn of(verb: tormoni_record::Verb) -> &'static [Self] {
        match verb {
            tormoni_record::Verb::Run => &[Self::Stdout, Self::Stderr],
            tormoni_record::Verb::Shell => &[Self::Shell],
            tormoni_record::Verb::Up => &[Self::Exec],
            _ => &[],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Shell => "terminal",
            Self::Exec => "exec",
        }
    }
}

/// The tail of a captured file, as the pane shows it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Output {
    pub(crate) stream: Option<Stream>,
    pub(crate) text: String,
    /// Bytes the file holds in all.
    pub(crate) size: u64,
    /// Whether the record capped it.
    pub(crate) capped: bool,
}

/// The form for a new run, as text fields until it is started.
#[derive(Debug, Clone, Default)]
pub(crate) struct Form {
    pub(crate) name: String,
    pub(crate) root: String,
    pub(crate) writable_root: bool,
    pub(crate) command: String,
    pub(crate) mounts: String,
    pub(crate) shares: String,
    pub(crate) network: bool,
    pub(crate) display: bool,
    pub(crate) display_size: String,
    pub(crate) sound: bool,
    pub(crate) gpu: bool,
    pub(crate) results: bool,
    pub(crate) vcpus: String,
    pub(crate) mem_mib: String,
}

/// What a cookbook entry is filed under. **A shelf names its subject, not its moral**, so a
/// reader scanning the rail knows whether a run is about the network or the GPU before reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shelf {
    Basics,
    Network,
    Filesystem,
    Results,
    Gpu,
    Sizing,
    Failure,
}

impl Shelf {
    /// Every shelf, in the order the cookbook lists them.
    pub(crate) const ALL: [Self; 7] = [
        Self::Basics,
        Self::Network,
        Self::Filesystem,
        Self::Results,
        Self::Gpu,
        Self::Sizing,
        Self::Failure,
    ];

    /// The subject over its entries.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Basics => "THE BASICS",
            Self::Network => "NETWORK",
            Self::Filesystem => "THE FILESYSTEM",
            Self::Results => "RESULTS AND OUTPUT",
            Self::Gpu => "GPU",
            Self::Sizing => "CPU AND MEMORY",
            Self::Failure => "WHEN A RUN FAILS",
        }
    }

    /// One line under the subject, saying what its runs do.
    pub(crate) fn about(self) -> &'static str {
        match self {
            Self::Basics => "Whether it runs at all, and what the guest looks like from inside.",
            Self::Network => "What a sandbox reaches, and what it cannot until --net grants it.",
            Self::Filesystem => {
                "Which directories are there, which are writable, and what of \
                                 yours is not."
            }
            Self::Results => "Getting files and printed output back out of a run.",
            Self::Gpu => "What --gpu offers a guest, and what is there without it.",
            Self::Sizing => "What the posture gives the guest, which is not what this host has.",
            Self::Failure => "How a bad command reads from outside the sandbox.",
        }
    }
}

/// One cookbook entry: a run worth trying, and the one thing trying it shows.
///
/// **The posture is the data; every rendering is derived from it.** [`Example::cli`] builds the
/// `tormoni` line from these fields rather than storing a string, so a second rendering (a
/// `tormoni-js` or `tormoni-python` snippet) is a second function over the same table and cannot
/// drift from the form a press fills.
///
/// `command` is plain argv: [`cli::start`] splits the form's field on whitespace and does no
/// quoting, and the helper refuses an argument mixing a double quote with a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Example {
    pub(crate) shelf: Shelf,
    pub(crate) title: &'static str,
    pub(crate) shows: &'static str,
    pub(crate) command: &'static str,
    pub(crate) network: bool,
    pub(crate) gpu: bool,
    pub(crate) vcpus: Option<&'static str>,
    pub(crate) mem_mib: Option<&'static str>,
}

/// An entry with the default posture, which most of them have.
const fn plain(
    shelf: Shelf,
    title: &'static str,
    shows: &'static str,
    command: &'static str,
) -> Example {
    Example {
        shelf,
        title,
        shows,
        command,
        network: false,
        gpu: false,
        vcpus: None,
        mem_mib: None,
    }
}

/// The same, with a network granted.
const fn networked(title: &'static str, shows: &'static str, command: &'static str) -> Example {
    Example {
        network: true,
        ..plain(Shelf::Network, title, shows, command)
    }
}

impl Example {
    /// Every entry, in the order its shelf lists them. Each was run against the tree
    /// `cargo xtask init` writes before it was written down.
    pub(crate) const ALL: [Self; 23] = [
        plain(
            Shelf::Basics,
            "Hello from a virtual machine",
            "The guest answers Linux, whatever this host is.",
            "uname -a",
        ),
        plain(
            Shelf::Basics,
            "Look around the guest",
            "Its root is the guest tree, not this machine's.",
            "ls -la /",
        ),
        plain(
            Shelf::Basics,
            "Who the guest thinks you are",
            "Root inside the VM, which is nobody out here.",
            "id",
        ),
        plain(
            Shelf::Network,
            "Nothing reaches out, by default",
            "There is no resolver and no route, so this fails.",
            "wget -T5 -qO- http://example.com",
        ),
        networked(
            "A network, once granted",
            "The same command with --net tsi reaches what this host reaches.",
            "wget -T5 -qO- http://example.com",
        ),
        networked(
            "Names resolve too",
            "A granted network brings a resolver with it, not just a route.",
            "nslookup example.com",
        ),
        networked(
            "TLS works",
            "Certificates come from the guest tree, so https needs nothing extra.",
            "wget -T5 -qO- https://example.com",
        ),
        networked(
            "Fetch something and keep it",
            "The download lands in /results, so the record carries what came back.",
            "wget -T5 -O /results/page.html http://example.com",
        ),
        networked(
            "It is sockets, not the network",
            "tsi impersonates connections, so a raw ping has nothing to send on.",
            "ping -c1 -W2 1.1.1.1",
        ),
        networked(
            "Its loopback is your loopback",
            "With tsi the guest's 127.0.0.1 is this machine's: whatever you have bound there is \
             reachable from inside.",
            "wget -T5 -qO- http://127.0.0.1:8000/",
        ),
        plain(
            Shelf::Filesystem,
            "The root is read-only",
            "A write outside the run's own places is refused by the mount.",
            "touch /proof",
        ),
        plain(
            Shelf::Filesystem,
            "None of your directories are here",
            "Nothing of yours is in the guest until a --mount names it.",
            "ls -la /mnt",
        ),
        plain(
            Shelf::Filesystem,
            "Where a run can write",
            "/results is the one place a run is expected to put things.",
            "touch /results/proof",
        ),
        plain(
            Shelf::Results,
            "Bring a file back",
            "What lands in /results is collected into the record.",
            "cp /etc/hostname /results/hostname",
        ),
        plain(
            Shelf::Results,
            "Bring a directory back",
            "One archive in /results, listed by the record with its size.",
            "tar -cf /results/etc.tar /etc",
        ),
        plain(
            Shelf::Results,
            "Everything it prints is kept",
            "stdout and stderr are captured, capped, and shown beside the run.",
            "dmesg",
        ),
        plain(
            Shelf::Gpu,
            "No GPU, by default",
            "There is no render node in the guest at all until --gpu asks for one.",
            "ls -la /dev/dri",
        ),
        Example {
            gpu: true,
            ..plain(
                Shelf::Gpu,
                "The card the offer adds",
                "--gpu gives the guest card0 and renderD128. A driver to use them is the guest's \
                 own problem, and the stock tree has none.",
                "ls -la /dev/dri",
            )
        },
        plain(
            Shelf::Sizing,
            "What it was given",
            "One vCPU and 512 MiB, until a posture says otherwise.",
            "free -m",
        ),
        Example {
            vcpus: Some("4"),
            ..plain(
                Shelf::Sizing,
                "Give it four vCPUs",
                "The guest counts what the posture gave it, not this host's cores.",
                "nproc",
            )
        },
        Example {
            mem_mib: Some("2048"),
            ..plain(
                Shelf::Sizing,
                "Give it two gigabytes",
                "Guest RAM is what the posture says, and the record keeps the number.",
                "free -m",
            )
        },
        plain(
            Shelf::Failure,
            "A command that fails",
            "The run's end is the command's status, so a pipeline can read it.",
            "false",
        ),
        plain(
            Shelf::Failure,
            "A command that is not there",
            "The guest resolves the first word on its own PATH, never this host's.",
            "does-not-exist",
        ),
    ];

    /// The entries on one shelf, in order.
    pub(crate) fn on(shelf: Shelf) -> impl Iterator<Item = &'static Self> {
        Self::ALL.iter().filter(move |e| e.shelf == shelf)
    }

    /// The `tormoni` line this entry is, built from its posture rather than stored beside it.
    pub(crate) fn cli(&self) -> String {
        let mut line = String::from("tormoni run");
        if self.network {
            line.push_str(" --net tsi");
        }
        if self.gpu {
            line.push_str(" --gpu");
        }
        if let Some(vcpus) = self.vcpus {
            line.push_str(&format!(" --vcpus {vcpus}"));
        }
        if let Some(mem) = self.mem_mib {
            line.push_str(&format!(" --mem {mem}"));
        }
        format!("{line} -- {}", self.command)
    }

    /// The form a press leaves on the New run screen. What the entry does not name it leaves at
    /// the blank form's own default, so a cookbook press and a hand-filled form differ in nothing
    /// but the fields the entry is about.
    pub(crate) fn form(&self) -> Form {
        let mut form = Form::blank();
        form.command = self.command.to_string();
        form.network = self.network;
        form.gpu = self.gpu;
        if let Some(vcpus) = self.vcpus {
            form.vcpus = vcpus.to_string();
        }
        if let Some(mem) = self.mem_mib {
            form.mem_mib = mem.to_string();
        }
        form
    }
}

impl Form {
    fn blank() -> Self {
        Self {
            root: cli::default_root()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            display_size: DEFAULT_DISPLAY.to_string(),
            results: true,
            vcpus: DEFAULT_VCPUS.to_string(),
            mem_mib: DEFAULT_MEM_MIB.to_string(),
            ..Self::default()
        }
    }

    /// The form filled from a record, for a re-run: its command and posture again.
    fn from_record(record: &Record) -> Self {
        let p = &record.posture;
        Self {
            name: String::new(),
            root: p.root.display().to_string(),
            writable_root: p.rootfs == tormoni_record::Rootfs::Writable,
            command: record.command.join(" "),
            mounts: p
                .mounts
                .iter()
                .map(|m| format!("{}={}", m.guest.display(), m.host.display()))
                .collect::<Vec<_>>()
                .join(" "),
            shares: p
                .shares
                .iter()
                .map(|s| format!("{}={}", s.tag, s.host.display()))
                .collect::<Vec<_>>()
                .join(" "),
            network: p.network == tormoni_record::Network::Tsi,
            display: p.display.is_some(),
            display_size: p
                .display
                .map_or_else(|| DEFAULT_DISPLAY.to_string(), |d| d.as_spec()),
            sound: p.sound,
            gpu: p.gpu,
            results: p.results,
            vcpus: p.vcpus.to_string(),
            mem_mib: p.mem_mib.to_string(),
        }
    }
}

/// A field of the form, for one message that carries any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Root,
    Command,
    Mounts,
    Shares,
    DisplaySize,
    Vcpus,
    Mem,
}

/// A switch of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Switch {
    WritableRoot,
    Network,
    Display,
    Sound,
    Gpu,
    Results,
}

/// What the window reacts to.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A second passed: reread the notebook and the open run's output.
    Tick,
    /// A frame went up at this instant: the clock the sidebar's motion is read against.
    Drawn(std::time::Instant),
    /// A window is on screen: what its own chrome is settled on.
    Opened(iced::window::Id),
    /// The window changed size, which is also how it enters and leaves full screen.
    Resized(iced::window::Id),
    /// The head's own line was double-clicked, which is how a macOS window is zoomed.
    ZoomWindow,
    /// The head's quit control was pressed, where the platform draws no close button of its own.
    Quit,
    /// Whether the window is full screen, answered after a resize: the one thing that takes the
    /// platform's own buttons off the head's line while still drawing them elsewhere.
    Fullscreen(bool),
    Open(RunId),
    Back,
    List,
    Settings,
    /// A keyboard event every widget ignored: what the window's own chords read.
    Keyboard(iced::keyboard::Event),
    /// Draw in this mode from now on, and remember it.
    SetTheme(theme::Mode),
    /// The toolkit's report of what the desktop is showing, at start and on every change.
    DesktopTheme(iced::theme::Mode),
    /// Draw at this scale from now on, and remember it.
    SetScale(Scale),
    /// Open the next plain launch on this screen, and remember it.
    SetOpensOn(OpenScreen),
    /// Every setting back to what a fresh install has, and remembered so.
    ResetSettings,
    /// Fold the sidebar away, or bring it back.
    ToggleSidebar,
    NewRun,
    /// Open the cookbook.
    Cookbook,
    /// Fill the start form from a cookbook entry, and show it rather than start it.
    Example(Example),
    Field(Field, String),
    Switch(Switch, bool),
    Start,
    Started(Result<RunName, String>),
    Stop(RunName),
    Acted(Result<String, String>),
    Shell(RunName),
    Rerun(RunId),
    Delete(RunId),
    /// Start selecting records to remove.
    Select,
    /// Add or remove one record from the selection.
    SelectToggle(RunId),
    /// Select every ended run, or none of them.
    SelectAll(bool),
    /// Stop selecting and keep everything.
    SelectCancelled,
    /// Ask before removing what was selected.
    RemoveSelected,
    /// Remove what the modal is asking about.
    DeleteConfirmed,
    /// Put the modal away and remove nothing.
    DeleteCancelled,
    /// Write the run's directory as a tar file where a person can pick it up.
    Export(RunId),
    Show(Stream),
    /// A run's lease landed and its memfd is mapped.
    Mapped(RunName, Arc<SharedFrames>),
    /// A run presented a frame into `slot`.
    Presented {
        name: RunName,
        frame_id: u32,
        slot: u32,
        damage: Damage,
    },
    /// A run's input session is open: these are the lines its keyboard and pointer become.
    Input(
        RunName,
        iced::futures::channel::mpsc::UnboundedSender<String>,
    ),
    /// Start signing in: a device key is made and the console's connect page opens for it.
    SignIn,
    /// One ask of the console whether this device has been approved yet.
    Claimed(account::Claim),
    /// Open the pairing page again, for a browser that was closed before Connect was pressed.
    PairingPage,
    /// Stop signing in, keeping neither the key nor a token.
    SignInCancelled,
    /// A sign-in answered: the account's identity, or why there is none.
    SignedIn(Result<account::Identity, String>),
    /// Give up the account this window holds.
    SignOut,
    /// The token a sign-out gave up was handed back to the console, or was not.
    SignedOut(Result<(), String>),
    /// A token an earlier launch left behind was handed back, or was not.
    Retired(Result<(), String>),
    /// Open one of the console's pages in the browser: Manage and Upgrade.
    Console(account::Page),
    /// Something the operator should see in the window rather than on a stderr they may not have.
    Note(String),
    /// A run's lease ended, with why; the sandbox stopping is the ordinary case.
    Ended(RunName, String),
}

pub(crate) struct App {
    store: Store,
    screen: Screen,
    /// Every run, newest first, as of the last tick.
    runs: Vec<Record>,
    /// The names answering on their control sockets as of the last tick.
    live: BTreeSet<RunName>,
    /// Where `tormoni` and the guest root are, as of the last tick: what the menu reports.
    platform: cli::Platform,
    form: Form,
    /// The last thing worth telling the operator: an error, or what just happened.
    status: Option<String>,
    /// Who this window is signed in as. Nothing on any screen needs one.
    account: account::Account,
    /// The console the account is signed in to, and whose pages Manage and Upgrade open.
    console: String,
    /// Where this device's key and token live. A field, not a call, so a test never reaches the
    /// directory the person running it is signed in with.
    device_dir: PathBuf,
    output: Output,
    /// The shown run's result files, as of the last tick. Held here rather than read in `view`,
    /// which iced rebuilds once per message: with a guest presenting frames that is a directory
    /// walk per frame.
    results: Vec<(PathBuf, u64)>,
    log: Option<PathBuf>,
    sinks: Arc<frame::Sinks>,
    /// The display of every run this window is leasing, by name: the one on screen, and every
    /// live run with a display when the list is showing its grid.
    displays: BTreeMap<RunName, Display>,
    exit_with_lease: bool,
    /// The mode every view draws in; Settings changes it live.
    mode: theme::Mode,
    /// What the desktop is showing, as the toolkit last reported it: what `System` follows.
    desktop: iced::theme::Mode,
    /// Whether --theme or $TORMONI_THEME set it, which outranks a pick at the next launch.
    theme_overridden: bool,
    /// The interface scale in percent; Settings changes it live.
    scale: u16,
    /// The screen a plain launch opens on: the saved pick Settings shows and writes.
    opens_on: OpenScreen,
    /// Whether the list is asking "really clear the history?". Leaving the list disarms it.
    list: ListMode,
    /// The question a destructive press is waiting on. `None` is a window with nothing to answer,
    /// and is the only state in which anything can be removed.
    confirm: Option<Confirm>,
    /// Whether the sidebar is out, and where it stands while that is changing.
    sidebar: Animation<bool>,
    /// The instant the last frame was drawn at, which every animation is read at.
    now: std::time::Instant,
    /// The window this is drawing in, once it is open: what a zoom is asked of.
    window: Option<iced::window::Id>,
    /// Whether the window is full screen. Only macOS moves its buttons off the head's line for
    /// it, but the field is not `cfg`-gated: a screen asks [`lights`](Self::lights), and one
    /// answer for every platform is one layout to reason about.
    fullscreen: bool,
}

/// One leased display: what was mapped for it, the presents it has reported, and where its input
/// goes.
struct Display {
    frames: Arc<SharedFrames>,
    history: Arc<std::collections::VecDeque<frame::Present>>,
    input: Option<iced::futures::channel::mpsc::UnboundedSender<String>>,
    read: u64,
}

impl App {
    fn new(
        store: Store,
        opening: Option<String>,
        log: Option<PathBuf>,
        sinks: Arc<frame::Sinks>,
        exit_with_lease: bool,
    ) -> Self {
        let mut app = Self {
            store,
            screen: Screen::List,
            runs: Vec::new(),
            live: BTreeSet::new(),
            platform: cli::Platform::default(),
            form: Form::blank(),
            status: None,
            account: account::Account::default(),
            console: account::DEFAULT.to_string(),
            device_dir: account::dir().unwrap_or_default(),
            output: Output::default(),
            results: Vec::new(),
            log,
            sinks,
            displays: BTreeMap::new(),
            exit_with_lease,
            mode: theme::Mode::default(),
            desktop: iced::theme::Mode::None,
            theme_overridden: false,
            scale: 100,
            opens_on: OpenScreen::List,
            list: ListMode::Browsing,
            confirm: None,
            sidebar: Animation::new(true).quick().easing(Easing::EaseInOut),
            now: std::time::Instant::now(),
            window: None,
            // A window opens windowed; the first resize answers for the rest.
            fullscreen: false,
        };
        app.refresh();
        if let Some(key) = opening {
            let found = app
                .runs
                .iter()
                .find(|r| r.id == key)
                .or_else(|| app.runs.iter().find(|r| r.name == key))
                .map(RunId::of);
            match found {
                Some(id) => app.open(id),
                None => app.status = Some(format!("no run named or numbered {key:?}")),
            }
        }
        app
    }

    fn title(&self) -> String {
        match &self.screen {
            Screen::Settings => format!("{NAME} › settings"),
            Screen::List => format!("{NAME} › sandboxes"),
            Screen::New => format!("{NAME} › new run"),
            Screen::Cookbook => format!("{NAME} › cookbook"),
            Screen::Run(id) => format!(
                "{NAME} › {}",
                self.record(id).map_or(id.as_str(), |r| r.name.as_str())
            ),
        }
    }

    /// The room the window's own buttons take on the head's line, right now: none in full
    /// screen, where macOS hides the titlebar carrying them, and none where the platform never
    /// drew them on that line.
    pub(crate) fn lights(&self) -> f32 {
        if self.fullscreen { 0.0 } else { chrome::LIGHTS }
    }

    /// The ids a selection may hold: every run that has ended. A live run is refused a delete, so
    /// offering it would be offering something the press cannot do.
    pub(crate) fn removable(&self) -> impl Iterator<Item = String> + '_ {
        self.runs
            .iter()
            .filter(|r| !self.is_live(r))
            .map(|r| r.id.clone())
    }

    /// How far the sidebar is out: 0 folded away, 1 all the way, and between while it moves.
    pub(crate) fn sidebar_out(&self) -> f32 {
        self.sidebar.interpolate(0.0, 1.0, self.now)
    }

    /// The record with `id`, from the last tick.
    pub(crate) fn record(&self, id: &RunId) -> Option<&Record> {
        self.runs.iter().find(|r| r.id == id.as_str())
    }

    /// Whether the run with `id` is answering now.
    pub(crate) fn is_live(&self, record: &Record) -> bool {
        record.is_open() && self.live.contains(&RunName::of(record))
    }

    /// Rereads the notebook: the records, which names answer, and marks the open records whose
    /// VM does not answer as gone (the one bookkeeping a listing does, as `tormoni ls --all`).
    fn refresh(&mut self) {
        self.platform = cli::probe();
        self.live = tormoni_supervisor::discover::live()
            .map(|found| {
                found
                    .into_iter()
                    .map(|f| RunName::started(f.name))
                    .collect()
            })
            .unwrap_or_default();
        let mut runs = self.store.list().unwrap_or_default();
        settle_gone(&self.store, &mut runs, &self.live);
        self.runs = runs;
        if let Screen::Run(id) = &self.screen {
            let id = id.clone();
            self.reload_output(&id);
        }
        self.forget_unwatched();
    }

    /// Rereads the tail of the shown stream of run `id`, and the results the guest has written.
    fn reload_output(&mut self, id: &RunId) {
        let Some(record) = self.record(id) else {
            self.output = Output::default();
            self.results = Vec::new();
            return;
        };
        let streams = Stream::of(record.verb);
        let stream = match self.output.stream {
            Some(s) if streams.contains(&s) => Some(s),
            _ => streams.first().copied(),
        };
        let dir = self.store.dir_of(id.as_str());
        self.results = dir.result_files().unwrap_or_default();
        self.output = match stream {
            Some(stream) => {
                let path = stream.path(&dir);
                let (text, size) = tail_of(&path, OUTPUT_TAIL);
                Output {
                    stream: Some(stream),
                    text,
                    size,
                    capped: path.with_extension("truncated").exists(),
                }
            }
            None => Output::default(),
        };
    }

    /// Opens run `id`: the record, its output, and its display if it is live and has one.
    fn open(&mut self, id: RunId) {
        self.leave();
        self.set_screen(Screen::Run(id.clone()));
        self.output.stream = None;
        self.reload_output(&id);
    }

    /// Leaves whatever run is shown. The leases the next screen does not want end with their
    /// subscriptions, and [`Self::forget_unwatched`] drops what was mapped for them.
    fn leave(&mut self) {
        self.results = Vec::new();
    }

    /// Every live run with a display, newest first: what the list's grid shows a frame for.
    fn showing_displays(&self) -> Vec<&Record> {
        self.runs
            .iter()
            .filter(|r| self.is_live(r) && r.posture.display.is_some())
            .collect()
    }

    /// The runs to lease and how often each wants a present: the open run at the guest's pace,
    /// every other live display at [`THUMBNAIL_EVERY`].
    fn watches(&self) -> Vec<lease::Watch> {
        let open = match &self.screen {
            Screen::Run(id) => self.record(id).map(RunName::of),
            Screen::Settings | Screen::Cookbook => return Vec::new(),
            Screen::List | Screen::New => None,
        };
        let mut watches = Vec::new();
        if let Some(name) = &open {
            if self
                .record_by_name(name)
                .is_some_and(|r| self.is_live(r) && r.posture.display.is_some())
            {
                watches.push(lease::Watch {
                    name: name.clone(),
                    log: self.log.clone(),
                    every: std::time::Duration::ZERO,
                });
            }
            return watches;
        }
        for record in self.showing_displays().into_iter().take(MAX_THUMBNAILS) {
            watches.push(lease::Watch {
                name: RunName::of(record),
                log: None,
                every: THUMBNAIL_EVERY,
            });
        }
        watches
    }

    /// Moves to `screen` and settles what is leased for it.
    fn set_screen(&mut self, screen: Screen) {
        self.list = ListMode::Browsing;
        self.screen = screen;
        self.forget_unwatched();
    }

    /// What Settings persists, gathered whole so every save writes every knob.
    fn saved(&self) -> state::Saved {
        state::Saved {
            theme: Some(self.mode.to_string()),
            scale: Some(self.scale),
            open: Some(self.opens_on.to_string()),
        }
    }

    /// Drops what was mapped for a run this window no longer leases, so a display left behind
    /// does not keep its memfd, its input session or its history alive.
    fn forget_unwatched(&mut self) {
        let wanted: BTreeSet<RunName> = self.watches().into_iter().map(|w| w.name).collect();
        self.displays.retain(|name, _| wanted.contains(name));
    }

    /// Makes a device key, opens the console's page on it, and asks once whether it was
    /// approved. The key is new each time: the console hands a key its token once.
    fn start_pairing(&mut self) -> Result<Task<Message>, String> {
        let dir = self.device_dir.clone();
        if dir.as_os_str().is_empty() {
            return Err(
                "no HOME and no XDG_DATA_HOME, so there is nowhere to keep a device key"
                    .to_string(),
            );
        }
        let key = device::create(&dir)?;
        let device_name = account::device_name();
        let pairing = account::Pairing {
            device: device_name.clone(),
            line: key.public().line().to_string(),
            fingerprint: key.public().fingerprint().to_string(),
            issued_at: 0,
            started_ms: tormoni_record::now_ms(),
        };
        let url = account::connect_url(&self.console, &device_name, &pairing.line);
        self.account = account::Account::Pairing(pairing);
        account::open_url(&url)?;
        // Last, after every step that can fail: `?` above would drop this task, and the token
        // it took is off the disk by then, so a leftover would be neither held nor given back.
        Ok(Task::batch([self.retire_leftover(), self.ask_again(0)]))
    }

    /// Hands back a token a launch that never signed out left behind, so signing in again makes
    /// this machine one device on the console rather than one more.
    ///
    /// Nothing about the sign-in waits on it, and nothing it does reaches the key directory
    /// again: the token is off the disk before the task exists.
    fn retire_leftover(&self) -> Task<Message> {
        let Some(token) = device::take_token(&self.device_dir) else {
            return Task::none();
        };
        let console = self.console.clone();
        Task::perform(
            async move { account::retire(&console, token) },
            Message::Retired,
        )
    }

    /// Asks the console again in `after` seconds, from the second the last claim signed at.
    ///
    /// The wait is inside the task, so each one holds a pool thread for a couple of seconds
    /// rather than one holding it for the whole five minutes.
    fn ask_again(&self, after: u64) -> Task<Message> {
        let account::Account::Pairing(pairing) = &self.account else {
            return Task::none();
        };
        let (console, issued_at) = (self.console.clone(), pairing.issued_at);
        let dir = self.device_dir.clone();
        Task::perform(
            async move {
                if after > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(after));
                }
                account::claim(&console, &dir, issued_at)
            },
            Message::Claimed,
        )
    }

    /// Opens `page` of the console in the browser; what came of it lands as the operator's line.
    fn visit(&self, page: account::Page) -> Task<Message> {
        let console = self.console.clone();
        Task::perform(
            async move { account::open(&console, page) },
            |answer| match answer {
                Ok(line) | Err(line) => Message::Note(line),
            },
        )
    }

    /// The record with `name`, from the last tick.
    fn record_by_name(&self, name: &RunName) -> Option<&Record> {
        self.runs.iter().find(|r| r.name == name.as_str())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                self.refresh();
                Task::none()
            }
            Message::Open(id) => {
                self.open(id);
                self.status = None;
                Task::none()
            }
            Message::Back | Message::List => {
                self.leave();
                self.set_screen(Screen::List);
                self.status = None;
                Task::none()
            }
            Message::Settings => {
                self.leave();
                self.set_screen(Screen::Settings);
                self.status = None;
                Task::none()
            }
            Message::Keyboard(event) => {
                if let iced::keyboard::Event::KeyPressed { key, modifiers, .. } = event
                    && let Some(chord) = hotkey(&key, modifiers)
                {
                    return self.update(chord);
                }
                Task::none()
            }
            Message::SetTheme(mode) => {
                self.mode = mode;
                self.status = match state::save(&self.saved()) {
                    Ok(()) => Some(format!("drawing in {mode}")),
                    Err(e) => Some(format!("drawing in {mode} for this window; not saved: {e}")),
                };
                Task::none()
            }
            Message::ToggleSidebar => {
                // From the clock rather than the last frame: an idle window draws none, and a
                // motion begun in the past is over before it is seen.
                self.now = std::time::Instant::now();
                let out = self.sidebar.value();
                self.sidebar.go_mut(!out, self.now);
                Task::none()
            }
            Message::Drawn(at) => {
                self.now = at;
                Task::none()
            }
            Message::Opened(id) => {
                self.window = Some(id);
                chrome::unify_titlebar(id)
            }
            Message::Resized(id) => chrome::fit_fullscreen(id).map(Message::Fullscreen),
            Message::Fullscreen(full) => {
                self.fullscreen = full;
                Task::none()
            }
            Message::ZoomWindow => self
                .window
                .map_or_else(Task::none, iced::window::toggle_maximize),
            // The running sandboxes are helper processes of their own and keep running; what
            // this ends is the notebook looking at them.
            Message::Quit => iced::exit(),
            Message::ResetSettings => {
                self.mode = theme::Mode::default();
                self.scale = 100;
                self.opens_on = OpenScreen::List;
                self.status = match state::save(&self.saved()) {
                    Ok(()) => Some("settings reset".to_string()),
                    Err(e) => Some(format!("settings reset for this window; not saved: {e}")),
                };
                Task::none()
            }
            Message::DesktopTheme(desktop) => {
                self.desktop = desktop;
                Task::none()
            }
            Message::SetScale(Scale(pct)) => {
                self.scale = pct;
                self.status = match state::save(&self.saved()) {
                    Ok(()) => Some(format!("drawn at {}", Scale(pct))),
                    Err(e) => Some(format!(
                        "drawn at {} for this window; not saved: {e}",
                        Scale(pct)
                    )),
                };
                Task::none()
            }
            Message::SetOpensOn(open) => {
                self.opens_on = open;
                self.status = match state::save(&self.saved()) {
                    Ok(()) => Some(format!("a plain launch now opens on the {open} screen")),
                    Err(e) => Some(format!("not saved: {e}")),
                };
                Task::none()
            }
            Message::NewRun => {
                self.leave();
                self.form = Form::blank();
                self.set_screen(Screen::New);
                self.status = None;
                Task::none()
            }
            Message::Field(field, value) => {
                match field {
                    Field::Name => self.form.name = value,
                    Field::Root => self.form.root = value,
                    Field::Command => self.form.command = value,
                    Field::Mounts => self.form.mounts = value,
                    Field::Shares => self.form.shares = value,
                    Field::DisplaySize => self.form.display_size = value,
                    Field::Vcpus => self.form.vcpus = value,
                    Field::Mem => self.form.mem_mib = value,
                }
                Task::none()
            }
            Message::Switch(switch, on) => {
                match switch {
                    Switch::WritableRoot => self.form.writable_root = on,
                    Switch::Network => self.form.network = on,
                    Switch::Display => self.form.display = on,
                    Switch::Sound => self.form.sound = on,
                    Switch::Gpu => self.form.gpu = on,
                    Switch::Results => self.form.results = on,
                }
                Task::none()
            }
            Message::Cookbook => {
                self.set_screen(Screen::Cookbook);
                Task::none()
            }
            Message::Example(example) => {
                self.form = example.form();
                self.set_screen(Screen::New);
                Task::none()
            }
            Message::Start => {
                let form = self.form.clone();
                Task::perform(
                    async move { cli::start(&cli::tormoni_path(), &form) },
                    Message::Started,
                )
            }
            Message::Started(Ok(name)) => {
                self.status = Some(format!("started {name}"));
                self.refresh();
                match self.runs.iter().find(|r| r.name == name.as_str()) {
                    Some(record) => {
                        let id = RunId::of(record);
                        self.open(id);
                    }
                    None => self.set_screen(Screen::List),
                }
                Task::none()
            }
            Message::SignIn => {
                self.status = None;
                match self.start_pairing() {
                    Ok(task) => task,
                    Err(why) => {
                        self.status = Some(why);
                        Task::none()
                    }
                }
            }
            // A claim that lands after Cancel, or after a second Sign in, belongs to a key this
            // window no longer waits on: the state says which, so a stale one is dropped.
            Message::Claimed(claim) => {
                let account::Account::Pairing(pairing) = &mut self.account else {
                    return Task::none();
                };
                pairing.issued_at = claim.issued_at;
                match claim.outcome {
                    account::Claimed::Pending(_) if pairing.gave_up() => {
                        self.account = account::Account::SignedOut;
                        self.status = Some(
                            "nobody approved this device, so the sign-in was dropped".to_string(),
                        );
                        Task::none()
                    }
                    account::Claimed::Pending(after) => self.ask_again(after),
                    account::Claimed::Token(token) => {
                        let (console, dir) = (self.console.clone(), self.device_dir.clone());
                        Task::perform(
                            async move { account::finish(&console, &dir, token) },
                            Message::SignedIn,
                        )
                    }
                    account::Claimed::Refused(why) => {
                        self.account = account::Account::SignedOut;
                        self.status = Some(why);
                        Task::none()
                    }
                }
            }
            Message::PairingPage => {
                let account::Account::Pairing(pairing) = &self.account else {
                    return Task::none();
                };
                let url = account::connect_url(&self.console, &pairing.device, &pairing.line);
                Task::perform(
                    async move { account::open_url(&url) },
                    |answer| match answer {
                        Ok(line) | Err(line) => Message::Note(line),
                    },
                )
            }
            Message::SignInCancelled => {
                let _ = device::forget(&self.device_dir);
                self.account = account::Account::SignedOut;
                Task::none()
            }
            Message::SignedIn(Ok(identity)) => {
                self.status = Some(format!("signed in as {}", identity.email));
                self.account = account::Account::SignedIn(identity);
                Task::none()
            }
            Message::SignedIn(Err(why)) => {
                self.status = Some(why);
                self.account = account::Account::SignedOut;
                Task::none()
            }
            Message::SignOut => {
                // Both halves of the wipe happen HERE, before this returns, and not in the task
                // below: Sign in is on the screen the moment the account goes, and it writes a
                // new key into this same directory. A wipe still queued behind that press would
                // delete the key the sign-in had just made.
                let held = device::take_token(&self.device_dir);
                // A `Some` here means the token file is already gone, so a wipe that fails past
                // this leaves a SPENT key and never a credential: its one claim is used, and no
                // token names it any more. The token is handed back either way.
                let wiped = device::forget(&self.device_dir);
                self.account = account::Account::SignedOut;
                self.status = match (wiped, &held) {
                    (Err(why), _) => Some(why),
                    (Ok(()), None) => Some("signed out, and this device's key is gone".to_string()),
                    (Ok(()), Some(_)) => None,
                };
                let Some(token) = held else {
                    return Task::none();
                };
                let console = self.console.clone();
                Task::perform(
                    async move { account::retire(&console, token) },
                    Message::SignedOut,
                )
            }
            Message::SignedOut(Ok(())) => {
                self.status =
                    Some("signed out, and the console no longer lists this device".to_string());
                Task::none()
            }
            Message::SignedOut(Err(why)) => {
                self.status = Some(format!(
                    "signed out on this machine, but the console still lists this device: {why}"
                ));
                Task::none()
            }
            Message::Retired(Ok(())) => Task::none(),
            Message::Retired(Err(why)) => {
                self.status = Some(format!(
                    "the device an earlier sign-in left is still listed on the console: {why}"
                ));
                Task::none()
            }
            Message::Console(page) => self.visit(page),
            Message::Started(Err(why)) | Message::Acted(Err(why)) => {
                self.status = Some(why);
                Task::none()
            }
            Message::Acted(Ok(what)) => {
                self.status = Some(what);
                self.refresh();
                Task::none()
            }
            Message::Stop(name) => Task::perform(
                async move { cli::stop(&cli::tormoni_path(), name.as_str()) },
                Message::Acted,
            ),
            Message::Shell(name) => Task::perform(
                async move { cli::open_shell(&cli::tormoni_path(), name.as_str()) },
                Message::Acted,
            ),
            Message::Rerun(id) => {
                if let Some(record) = self.record(&id) {
                    self.form = Form::from_record(record);
                    self.leave();
                    self.set_screen(Screen::New);
                }
                Task::none()
            }
            Message::Delete(id) => {
                if self.record(&id).is_some_and(|r| self.is_live(r)) {
                    self.status = Some("stop the run before deleting its record".to_string());
                    return Task::none();
                }
                self.confirm = Some(Confirm::One(id));
                Task::none()
            }
            Message::Select => {
                self.list = ListMode::Selecting(BTreeSet::new());
                Task::none()
            }
            Message::SelectToggle(id) => {
                if let ListMode::Selecting(ids) = &mut self.list
                    && !ids.remove(id.as_str())
                {
                    ids.insert(id.as_str().to_string());
                }
                Task::none()
            }
            Message::SelectAll(all) => {
                let every: BTreeSet<String> = self.removable().collect();
                if let ListMode::Selecting(ids) = &mut self.list {
                    *ids = if all { every } else { BTreeSet::new() };
                }
                Task::none()
            }
            Message::SelectCancelled => {
                self.list = ListMode::Browsing;
                Task::none()
            }
            Message::RemoveSelected => {
                if let ListMode::Selecting(ids) = &self.list
                    && !ids.is_empty()
                {
                    self.confirm = Some(Confirm::Selected(ids.clone()));
                }
                Task::none()
            }
            Message::DeleteCancelled => {
                self.confirm = None;
                Task::none()
            }
            Message::DeleteConfirmed => {
                match self.confirm.take() {
                    None => return Task::none(),
                    Some(Confirm::One(id)) => {
                        self.leave();
                        self.status = match self.store.remove(id.as_str()) {
                            Ok(()) => Some(format!("removed {id}")),
                            Err(e) => Some(format!("removing {id}: {e}")),
                        };
                        self.set_screen(Screen::List);
                    }
                    Some(Confirm::Selected(ids)) => {
                        self.list = ListMode::Browsing;
                        let mut removed = 0usize;
                        let mut failed: Option<String> = None;
                        for id in &ids {
                            match self.store.remove(id) {
                                Ok(()) => removed += 1,
                                // The first failure is the one reported, and the rest of the
                                // selection is still attempted: one unreadable record does not
                                // strand the others.
                                Err(e) => {
                                    failed.get_or_insert_with(|| format!("removing {id}: {e}"));
                                }
                            }
                        }
                        self.status = Some(match failed {
                            Some(why) => format!("removed {}, then {why}", ended_runs(removed)),
                            None => format!("removed {}", ended_runs(removed)),
                        });
                    }
                }
                self.refresh();
                Task::none()
            }
            Message::Export(id) => {
                let store = self.store.clone();
                Task::perform(
                    async move {
                        let home = std::env::var_os("HOME").map(PathBuf::from);
                        let dest = export_destination(home, &store);
                        store
                            .export(id.as_str(), &dest)
                            .map(|path| format!("exported to {}", path.display()))
                            .map_err(|e| format!("exporting {id}: {e}"))
                    },
                    Message::Acted,
                )
            }
            Message::Show(stream) => {
                self.output.stream = Some(stream);
                if let Screen::Run(id) = &self.screen {
                    let id = id.clone();
                    self.reload_output(&id);
                }
                Task::none()
            }
            Message::Mapped(name, frames) => {
                let layout = frames.layout();
                eprintln!(
                    "{NAME}: mapped {name} {}x{} {:?}, stride {}, {} slots",
                    layout.width, layout.height, layout.format, layout.stride, layout.slots
                );
                // A new scanout, so the history starts again; a reconfigure leaves input open.
                let input = self.displays.remove(&name).and_then(|d| d.input);
                self.displays.insert(
                    name,
                    Display {
                        frames,
                        history: Arc::new(std::collections::VecDeque::with_capacity(HISTORY)),
                        input,
                        read: 0,
                    },
                );
                Task::none()
            }
            Message::Presented {
                name,
                frame_id,
                slot,
                damage,
            } => {
                // A present for a run this window has stopped leasing is dropped: its lease and
                // its mapping are already gone.
                let Some(display) = self.displays.get_mut(&name) else {
                    return Task::none();
                };
                display.read += 1;
                // `make_mut` copies only while the widget holds this for a draw.
                let history = Arc::make_mut(&mut display.history);
                if history.len() >= HISTORY {
                    history.pop_front();
                }
                history.push_back(frame::Present {
                    frame_id,
                    slot,
                    damage,
                });
                Task::none()
            }
            Message::Input(name, lines) => {
                if let Some(display) = self.displays.get_mut(&name) {
                    display.input = Some(lines);
                    eprintln!("{NAME}: the keyboard and pointer reach {name}");
                }
                Task::none()
            }
            Message::Note(what) => {
                self.status = Some(what);
                Task::none()
            }
            Message::Ended(name, why) => {
                let read = self.displays.get(&name).map_or(0, |d| d.read);
                eprintln!(
                    "{NAME}: {name}: {why}; read {read} presents, uploaded {} frames",
                    self.sinks.uploaded()
                );
                self.displays.remove(&name);
                if self.exit_with_lease {
                    return iced::exit();
                }
                self.refresh();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let content = match &self.screen {
            Screen::Settings => screens::settings(self),
            Screen::List => screens::list(self),
            Screen::New => screens::new_run(self, &self.form),
            Screen::Cookbook => screens::cookbook(self),
            Screen::Run(id) => screens::run(self, id),
        };
        screens::chrome(self, content)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![timer::every_second()];
        subs.push(iced::keyboard::listen().map(Message::Keyboard));
        subs.push(iced::system::theme_changes().map(Message::DesktopTheme));
        subs.push(iced::window::open_events().map(Message::Opened));
        subs.push(iced::window::resize_events().map(|(id, _)| Message::Resized(id)));
        // Only while something is moving: a frame subscription redraws the window on every
        // frame for as long as it is held.
        if self.sidebar.is_animating(self.now) {
            subs.push(iced::window::frames().map(Message::Drawn));
        }
        // Dropping a run's subscription cancels its lease and ends its thread.
        subs.extend(
            self.watches()
                .into_iter()
                .map(|watch| Subscription::run_with(watch, lease::stream)),
        );
        Subscription::batch(subs)
    }
}

/// Where an export goes: `$HOME/Downloads` when it exists, else home, else beside the runs
/// directory, which exists because the store opened.
fn export_destination(home: Option<PathBuf>, store: &Store) -> PathBuf {
    if let Some(home) = home {
        let downloads = home.join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        if home.is_dir() {
            return home;
        }
    }
    store
        .dir()
        .parent()
        .map_or_else(|| store.dir().to_path_buf(), Path::to_path_buf)
}

/// The chords the window answers anywhere: the platform's command with `,` opens Settings.
fn hotkey(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Option<Message> {
    match key {
        iced::keyboard::Key::Character(c) if c == "," && modifiers.command() => {
            Some(Message::Settings)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
            Some(Message::DeleteCancelled)
        }
        _ => None,
    }
}

/// Marks as gone, in `runs` and in the store, every open record that no VM in `live` belongs to.
///
/// **A name is reusable**, and `live` says only that *some* VM answers under one, so the newest
/// open run of a name is the one that VM is: the rule `tormoni ls --all` settles a name by, and the
/// one [`tormoni_record::Store::open_run`] reads a name by. `runs` is newest first, as
/// [`tormoni_record::Store::list`] returns it, which is what makes the first claim on a name the
/// newest rather than an arbitrary one.
fn settle_gone(store: &Store, runs: &mut [Record], live: &BTreeSet<RunName>) {
    let mut claimed = BTreeSet::new();
    for record in runs.iter_mut().filter(|r| r.is_open()) {
        let name = RunName::of(record);
        if claimed.insert(name.clone()) && live.contains(&name) {
            continue;
        }
        record.finish(tormoni_record::End::Gone);
        let _ = store.save(record);
    }
}

/// `n` runs, spelled with its plural. What a header asks about, where "ended" is already
/// implied: only an ended run can be selected.
pub(crate) fn runs(n: usize) -> String {
    format!("{n} run{}", if n == 1 { "" } else { "s" })
}

/// `n` ended runs, spelled with its plural: the status line says which kind went.
pub(crate) fn ended_runs(n: usize) -> String {
    format!("{n} ended run{}", if n == 1 { "" } else { "s" })
}

/// The last `max` bytes of `path` as text, and the file's whole size.
fn tail_of(path: &std::path::Path, max: u64) -> (String, u64) {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return (String::new(), 0);
    };
    let size = file.metadata().map_or(0, |m| m.len());
    let start = size.saturating_sub(max);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (String::new(), size);
    }
    let mut bytes = Vec::new();
    let _ = file.take(max).read_to_end(&mut bytes);
    (String::from_utf8_lossy(&bytes).into_owned(), size)
}

/// Where a run's frames, history, sinks and input go: the shader widget's program, or `None`
/// when this window holds no display for it yet.
pub(crate) fn frame_program(app: &App, name: &RunName) -> Option<frame::Program> {
    let display = app.displays.get(name)?;
    Some(frame::Program {
        run: Arc::from(name.as_str()),
        frames: Arc::clone(&display.frames),
        history: Arc::clone(&display.history),
        sinks: Arc::clone(&app.sinks),
        input: display.input.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The platform's command with `,` opens Settings; the pieces alone open nothing.
    #[test]
    fn the_platforms_command_and_comma_open_settings() {
        use iced::keyboard::{Key, Modifiers};
        let comma = Key::Character(",".into());
        assert!(matches!(
            hotkey(&comma, Modifiers::COMMAND),
            Some(Message::Settings)
        ));
        assert!(
            hotkey(&comma, Modifiers::empty()).is_none(),
            "bare comma types"
        );
        assert!(
            hotkey(&Key::Character("q".into()), Modifiers::COMMAND).is_none(),
            "no other chord is taken"
        );
    }

    /// One run is not "1 runs": both places that count them spell it through one helper.
    #[test]
    fn a_count_of_ended_runs_is_spelled_with_its_plural() {
        assert_eq!(ended_runs(1), "1 ended run");
        assert_eq!(ended_runs(2), "2 ended runs");
    }

    /// The archive goes where a person looks first: Downloads, else home, else beside
    /// the store.
    #[test]
    fn an_export_lands_in_downloads_then_home_then_beside_the_store() {
        let dir = tormoni_test_support::ScratchDir::created("app-export-dest");
        let store = Store::at(dir.path().join("data/runs")).expect("a store");
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("Downloads")).expect("a downloads dir");
        assert_eq!(
            export_destination(Some(home.clone()), &store),
            home.join("Downloads")
        );
        std::fs::remove_dir(home.join("Downloads")).expect("removed");
        assert_eq!(export_destination(Some(home.clone()), &store), home);
        assert_eq!(export_destination(None, &store), dir.path().join("data"));
    }

    /// The pane shows the tail of a file and its whole size, and an absent file is empty.
    #[test]
    fn the_output_pane_shows_the_tail() {
        let dir = std::env::temp_dir().join(format!("tormoni-app-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a dir");
        let path = dir.join("stdout");
        std::fs::write(&path, "0123456789").expect("written");
        assert_eq!(tail_of(&path, 4), ("6789".to_string(), 10));
        assert_eq!(tail_of(&path, 100), ("0123456789".to_string(), 10));
        assert_eq!(tail_of(&dir.join("none"), 4), (String::new(), 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run with a display, live or not, for the watch-set tests.
    fn displayed(name: &str, with_display: bool) -> Record {
        let mut p = tormoni_record::Posture::new(
            PathBuf::from("/img"),
            std::num::NonZeroU8::MIN,
            std::num::NonZeroU32::new(512).expect("non-zero"),
        );
        p.display = with_display
            .then(|| tormoni_record::DisplayMode::parse("640x480"))
            .flatten();
        Record::begin(name, tormoni_record::Verb::Run, vec!["true".into()], p)
    }

    fn app_with(runs: Vec<Record>, live: &[&str]) -> App {
        let dir = tormoni_test_support::ScratchDir::created("app-watches");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let sinks = Arc::new(frame::Sinks::open(None, None).expect("sinks"));
        let mut app = App::new(store, None, None, sinks, false);
        app.runs = runs;
        app.live = live
            .iter()
            .map(|n| RunName::started((*n).to_string()))
            .collect();
        // The scratch dir is dropped at the end of the test; nothing here touches it again.
        std::mem::forget(dir);
        app
    }

    /// The pairing loop as the window drives it: a claim nobody has approved keeps the block
    /// waiting, one that is refused stops and says why, and a claim that lands after Cancel is
    /// dropped rather than signing a window in that gave up.
    ///
    /// Driven through the messages the console's answers arrive as, so no console is needed.
    #[test]
    fn a_pairing_waits_then_stops_when_the_console_refuses() {
        let mut app = app_with(vec![], &[]);
        assert_eq!(app.account, account::Account::SignedOut, "a fresh launch");

        // A claim with nothing waiting for it changes nothing: the state is what says whether
        // this window still cares about that key.
        let stray = account::Claim {
            issued_at: 7,
            outcome: account::Claimed::Pending(2),
        };
        let _ = app.update(Message::Claimed(stray));
        assert_eq!(
            app.account,
            account::Account::SignedOut,
            "no pairing to answer"
        );

        app.account = account::Account::Pairing(pairing());
        let _ = app.update(Message::Claimed(account::Claim {
            issued_at: 11,
            outcome: account::Claimed::Pending(2),
        }));
        assert_eq!(
            waiting_second(&app.account),
            Some(11),
            "a pending claim keeps the block waiting, carrying the second it signed at"
        );

        let _ = app.update(Message::Claimed(account::Claim {
            issued_at: 13,
            outcome: account::Claimed::Refused("this device key already collected".to_string()),
        }));
        assert_eq!(app.account, account::Account::SignedOut);
        assert_eq!(
            app.status.as_deref(),
            Some("this device key already collected")
        );
    }

    /// A device nobody approved inside the window is given up on rather than asked about for
    /// ever, and the operator is told which happened.
    #[test]
    fn a_pairing_nobody_approves_is_dropped() {
        let mut app = app_with(vec![], &[]);
        let mut stale = pairing();
        stale.started_ms = tormoni_record::now_ms() - 10 * 60 * 1000;
        app.account = account::Account::Pairing(stale);
        let _ = app.update(Message::Claimed(account::Claim {
            issued_at: 17,
            outcome: account::Claimed::Pending(2),
        }));
        assert_eq!(app.account, account::Account::SignedOut);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("nobody approved")),
            "{:?}",
            app.status
        );
    }

    /// The second the block's pairing last signed at, or `None` where it is not pairing.
    fn waiting_second(account: &account::Account) -> Option<i64> {
        match account {
            account::Account::Pairing(pairing) => Some(pairing.issued_at),
            _ => None,
        }
    }

    /// A pairing this window is waiting on, for the tests above.
    fn pairing() -> account::Pairing {
        account::Pairing {
            device: "a laptop".to_string(),
            line: "ssh-ed25519 AAAA".to_string(),
            fingerprint: "SHA256:abc".to_string(),
            issued_at: 0,
            started_ms: tormoni_record::now_ms(),
        }
    }

    /// The signed-in state the loop lands in, and the way back out of it. Driven through the
    /// message the console's answer arrives as, so no console is needed here.
    #[test]
    fn a_signed_in_window_shows_the_account_and_can_sign_out() {
        let mut app = app_with(vec![], &[]);
        let identity = account::Identity {
            email: "someone@example.com".to_string(),
            display_name: Some("Someone Else".to_string()),
        };
        let _ = app.update(Message::SignedIn(Ok(identity.clone())));
        assert_eq!(app.account, account::Account::SignedIn(identity));
        assert_eq!(app.account.title(), "Someone Else");
        assert_eq!(app.account.line(&app.console), "someone@example.com");
        assert_eq!(
            app.status.as_deref(),
            Some("signed in as someone@example.com")
        );

        // Pressing Sign out gives up this window's account there and then; what the console
        // says about the device arrives afterwards, as its own message.
        let _ = app.update(Message::SignOut);
        assert_eq!(app.account, account::Account::SignedOut);

        let _ = app.update(Message::SignedOut(Ok(())));
        assert_eq!(
            app.status.as_deref(),
            Some("signed out, and the console no longer lists this device")
        );
    }

    /// **The wipe is done before Sign out returns, not by the task it starts.** Sign in is on
    /// the screen from that moment and writes a new key into this same directory, so a wipe
    /// still queued behind that press would delete the key the sign-in had just made.
    #[test]
    fn signing_out_empties_the_key_directory_before_it_asks_the_console_anything() {
        let scratch = tormoni_test_support::ScratchDir::created("app-sign-out");
        let mut app = app_with(vec![], &[]);
        app.device_dir = scratch.path().join("device");
        device::create(&app.device_dir).expect("a key");
        device::save_token(&app.device_dir, "tor_secret").expect("a token");
        let _ = app.update(Message::SignedIn(Ok(account::Identity {
            email: "someone@example.com".to_string(),
            display_name: None,
        })));

        // The task the press returns is dropped unrun, which is what a Sign in landing first
        // would do to it.
        drop(app.update(Message::SignOut));
        assert_eq!(app.account, account::Account::SignedOut);
        for name in ["token", "device.key", "device.pub"] {
            assert!(
                !app.device_dir.join(name).exists(),
                "{name} outlived the press"
            );
        }
    }

    /// A sign-out the console never heard still signs this window out, and says what is left
    /// listed rather than reading as a failure to sign out.
    #[test]
    fn a_sign_out_the_console_refused_still_leaves_the_window_signed_out() {
        let mut app = app_with(vec![], &[]);
        let _ = app.update(Message::SignedIn(Ok(account::Identity {
            email: "someone@example.com".to_string(),
            display_name: None,
        })));
        let _ = app.update(Message::SignOut);
        let _ = app.update(Message::SignedOut(Err("it answered 503".to_string())));
        assert_eq!(app.account, account::Account::SignedOut);
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("still lists this device")),
            "{:?}",
            app.status
        );
    }

    /// Only the newest open run of a name is the one a VM answering under it belongs to. An
    /// older one is an abandoned run whose name a later sandbox took: shown as running, and
    /// leased for a display it does not have, until it is written back as gone.
    #[test]
    fn an_open_run_whose_name_was_taken_again_is_not_shown_as_live() {
        let dir = tormoni_test_support::ScratchDir::created("app-settle-gone");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let mut abandoned = displayed("web", true);
        abandoned.started_ms -= 10;
        abandoned.id = format!("{}-web", abandoned.started_ms);
        let current = displayed("web", true);
        let orphan = displayed("solo", true);
        let mut runs = vec![current.clone(), abandoned.clone(), orphan.clone()];
        for record in &runs {
            store.create(record).expect("created");
        }

        let live: BTreeSet<RunName> = [RunName::started("web".to_string())].into_iter().collect();
        settle_gone(&store, &mut runs, &live);

        assert!(runs[0].is_open(), "the newest `web` is the VM answering");
        assert_eq!(
            (runs[1].end, runs[2].end),
            (
                Some(tormoni_record::End::Gone),
                Some(tormoni_record::End::Gone)
            ),
            "the older `web` and the unanswered `solo` are gone"
        );
        assert_eq!(
            store.read(&abandoned.id).expect("read").end,
            Some(tormoni_record::End::Gone),
            "and written back, so the notebook says so next time too"
        );

        // The point of the bookkeeping: the list neither shows nor leases the abandoned run.
        let mut app = app_with(runs, &["web"]);
        app.screen = Screen::List;
        assert_eq!(
            app.runs.iter().filter(|r| app.is_live(r)).count(),
            1,
            "one sandbox is running, not two"
        );
        assert_eq!(
            app.watches().len(),
            1,
            "and one display is leased, not two under one name"
        );
    }

    /// The list leases every live display, each at the thumbnail rate; opening one run leases
    /// that one alone, at the guest's own pace. A run without a display is never leased, and
    /// neither is one that has ended.
    #[test]
    fn the_list_watches_every_live_display_and_a_run_screen_watches_one() {
        let runs = vec![
            displayed("alpha", true),
            displayed("beta", true),
            displayed("nodisplay", false),
            displayed("ended", true),
        ];
        let open_id = RunId::of(&runs[0]);
        let mut app = app_with(runs, &["alpha", "beta", "nodisplay"]);
        app.screen = Screen::List;

        let mut watched: Vec<(RunName, std::time::Duration)> = app
            .watches()
            .into_iter()
            .map(|w| (w.name, w.every))
            .collect();
        watched.sort();
        assert_eq!(
            watched,
            [
                (RunName::started("alpha".to_string()), THUMBNAIL_EVERY),
                (RunName::started("beta".to_string()), THUMBNAIL_EVERY),
            ],
            "the list watches both live displays, and only those, at the thumbnail rate"
        );

        app.screen = Screen::Run(open_id);
        let watched: Vec<(RunName, std::time::Duration)> = app
            .watches()
            .into_iter()
            .map(|w| (w.name, w.every))
            .collect();
        assert_eq!(
            watched,
            [(
                RunName::started("alpha".to_string()),
                std::time::Duration::ZERO
            )],
            "an open run is the only lease, and it takes every present"
        );
    }

    /// A display this window has stopped watching is dropped, so its mapping, its history and
    /// its input session go with the lease rather than outliving it.
    #[test]
    fn a_display_no_longer_watched_is_forgotten() {
        let runs = vec![displayed("alpha", true), displayed("beta", true)];
        let mut app = app_with(runs, &["alpha", "beta"]);
        app.screen = Screen::List;
        // A real mapping, so what is dropped is the memfd and the region, not a stand-in.
        let frames = {
            use tormoni_krun::DisplayBackend as _;
            let mut fb = tormoni_krun::MemoryFramebuffer::shared();
            fb.configure_scanout(0, 64, 32, 64, 32, tormoni_krun::PixelFormat::B8G8R8X8Unorm)
                .expect("a scanout");
            let (fd, layout) = fb.share(0).expect("shareable").expect("a scanout");
            Arc::new(tormoni_krun::SharedFrames::map(fd, layout).expect("mapped"))
        };
        for name in ["alpha", "beta"] {
            app.displays.insert(
                RunName::started(name.to_string()),
                Display {
                    frames: Arc::clone(&frames),
                    history: Arc::new(std::collections::VecDeque::new()),
                    input: None,
                    read: 0,
                },
            );
        }
        app.live.remove(&RunName::started("beta".to_string()));
        app.forget_unwatched();
        assert_eq!(
            app.displays.keys().collect::<Vec<_>>(),
            [&RunName::started("alpha".to_string())],
            "the run that stopped answering is no longer held"
        );
    }

    /// The window opens on the menu; naming a run on the command line skips straight to it.
    #[test]
    fn the_window_opens_on_the_notebook_and_a_deep_link_skips_it() {
        let dir = tormoni_test_support::ScratchDir::created("app-boot");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let record = displayed("opened", false);
        store.create(&record).expect("created");
        let sinks = Arc::new(frame::Sinks::open(None, None).expect("sinks"));
        let app = App::new(store.clone(), None, None, Arc::clone(&sinks), false);
        assert_eq!(app.screen, Screen::List, "nothing asked, so the notebook");
        let app = App::new(store, Some("opened".to_string()), None, sinks, false);
        assert_eq!(app.screen, Screen::Run(RunId::of(&record)));
    }

    /// A saved `open` line is spelled exactly as the flag spells it, and parses back through the
    /// flag's own parser, so the state file and `--open` share one grammar.
    #[test]
    fn the_open_names_share_the_flag_grammar() {
        for open in [OpenScreen::List, OpenScreen::New, OpenScreen::Settings] {
            let flag = clap::ValueEnum::to_possible_value(&open).expect("every screen is a value");
            assert_eq!(open.to_string(), flag.get_name(), "one spelling");
            assert_eq!(OpenScreen::from_name(&open.to_string()), Some(open));
        }
        assert_eq!(OpenScreen::from_name("nowhere"), None);
    }

    #[test]
    fn a_scale_is_spelled_in_percent() {
        assert_eq!(Scale(110).to_string(), "110%");
    }

    #[test]
    fn a_plain_launch_lands_on_the_flag_then_the_saved_pick() {
        assert_eq!(
            landing(Some(OpenScreen::List), Some(OpenScreen::New)),
            OpenScreen::List,
            "the flag wins"
        );
        assert_eq!(
            landing(None, Some(OpenScreen::New)),
            OpenScreen::New,
            "else the saved pick"
        );
        assert_eq!(landing(None, None), OpenScreen::List);
    }

    /// Every screen the flag can name maps to itself, so `--open list` is the list.
    #[test]
    fn the_open_flag_maps_to_its_screens() {
        assert_eq!(OpenScreen::List.screen(), Screen::List);
        assert_eq!(OpenScreen::New.screen(), Screen::New);
        assert_eq!(OpenScreen::Settings.screen(), Screen::Settings);
    }

    /// Settings leases nothing: no thumbnail spins up behind a screen with no display on it.
    #[test]
    fn settings_leases_no_displays() {
        let mut app = app_with(vec![displayed("alpha", true)], &["alpha"]);
        app.screen = Screen::Settings;
        assert!(app.watches().is_empty(), "settings asks for no leases");
    }

    /// A cookbook entry fills the form and stops there: the posture sentence is read before
    /// anything boots, so a press starts no VM and writes no record.
    #[test]
    fn a_cookbook_entry_fills_the_form_and_starts_nothing() {
        let dir = tormoni_test_support::ScratchDir::created("app-cookbook");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let sinks = Arc::new(frame::Sinks::open(None, None).expect("sinks"));
        let mut app = App::new(store.clone(), None, None, sinks, false);

        let networked = Example::ALL
            .iter()
            .find(|e| e.network)
            .expect("an entry that grants a network");
        let _ = app.update(Message::Example(*networked));
        assert_eq!(app.screen, Screen::New, "an entry shows the form");
        assert_eq!(app.form.command, networked.command);
        assert!(app.form.network, "the one that grants a network says so");
        assert!(
            store.list().expect("listed").is_empty(),
            "filling a form records nothing"
        );

        // An entry that says nothing about a field leaves the blank form's own default there.
        let plain = Example::ALL
            .iter()
            .find(|e| !e.network && !e.gpu && e.vcpus.is_none() && e.mem_mib.is_none())
            .expect("a default-posture entry");
        let _ = app.update(Message::Example(*plain));
        assert!(!app.form.network, "and the default posture");
        assert!(!app.form.gpu, "which offers no gpu either");
        assert_eq!(app.form.vcpus, Form::blank().vcpus);
        assert_eq!(app.form.mem_mib, Form::blank().mem_mib);
    }

    /// Every entry is plain argv. `cli::start` splits the command field on whitespace and does no
    /// quoting, and the helper refuses an argument mixing a double quote with a space, so an entry
    /// carrying either would be a button that cannot run.
    #[test]
    fn every_cookbook_entry_is_argv_the_form_can_split() {
        for example in &Example::ALL {
            let (title, command) = (example.title, example.command);
            assert!(
                !command.contains('"') && !command.contains('\''),
                "{title}: {command} carries a quote the form does not honour"
            );
            assert!(
                !command
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .is_empty(),
                "{title}: no command at all"
            );
            assert_eq!(
                example.form().command,
                command,
                "{title}: the form carries the command the card shows"
            );
        }
    }

    /// The line an entry shows is the posture it fills in, so what a reader copies into a terminal
    /// and what the form starts cannot say different things. The same table renders a `tormoni-js`
    /// or `tormoni-python` snippet later, and this is what holds every rendering to the fields.
    #[test]
    fn the_line_an_entry_shows_is_the_posture_it_fills_in() {
        for example in &Example::ALL {
            let line = example.cli();
            let form = example.form();
            assert!(
                line.starts_with("tormoni run "),
                "{}: {line} is not a run",
                example.title
            );
            assert!(
                line.ends_with(&format!(" -- {}", example.command)),
                "{}: {line} does not end in its command",
                example.title
            );
            assert_eq!(
                line.contains("--net tsi"),
                form.network,
                "{}: the line and the form disagree about the network",
                example.title
            );
            assert_eq!(
                line.contains("--vcpus"),
                example.vcpus.is_some(),
                "{}: the line and the entry disagree about vcpus",
                example.title
            );
            assert_eq!(
                line.contains("--mem"),
                example.mem_mib.is_some(),
                "{}: the line and the entry disagree about memory",
                example.title
            );
            assert_eq!(
                line.contains("--gpu"),
                form.gpu,
                "{}: the line and the form disagree about the gpu",
                example.title
            );
        }
    }

    /// Every shelf carries entries, and every entry is on a shelf the cookbook lists: a shelf
    /// added without entries draws an empty heading, and an entry on no listed shelf is
    /// unreachable from the screen.
    #[test]
    fn every_shelf_is_filled_and_every_entry_is_shelved() {
        let mut counted = 0;
        for shelf in Shelf::ALL {
            let on = Example::on(shelf).count();
            assert!(on > 0, "{:?} has no entries", shelf);
            counted += on;
        }
        assert_eq!(
            counted,
            Example::ALL.len(),
            "an entry is on no listed shelf"
        );
    }

    /// A shelf names its subject and says what its runs do, which is the whole reason a reader can
    /// scan the cookbook: a blank line under a heading leaves the entries to speak for themselves.
    #[test]
    fn every_shelf_says_what_its_runs_do() {
        for shelf in Shelf::ALL {
            let (title, about) = (shelf.title(), shelf.about());
            assert_eq!(title, title.to_uppercase(), "{title} is not a heading");
            assert!(about.len() > 20, "{title}: {about:?} says too little");
            assert!(about.ends_with('.'), "{title}: {about:?} is not a sentence");
        }
    }

    /// One run's Delete asks too, and a live run is never asked about: pressing it reports why
    /// instead of raising a question whose only honest answer is no.
    #[test]
    fn deleting_one_run_asks_first_and_never_asks_about_a_live_one() {
        let dir = tormoni_test_support::ScratchDir::created("app-delete-one");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let name = format!("delete-live-{}", std::process::id());
        let sock = tormoni_supervisor::socket::path_for(&name).expect("a socket path");
        let _ = std::fs::remove_file(&sock);
        let listener = std::os::unix::net::UnixListener::bind(&sock).expect("a live socket");

        let mut ended = displayed("ended", false);
        ended.finish(tormoni_record::End::Exit(0));
        let live = displayed(&name, false);
        for r in [&ended, &live] {
            store.create(r).expect("created");
        }

        let sinks = Arc::new(frame::Sinks::open(None, None).expect("sinks"));
        let mut app = App::new(store.clone(), None, None, sinks, false);

        // A live run is refused at the press, so no question is ever raised about it.
        let _ = app.update(Message::Delete(RunId(live.id.clone())));
        assert!(app.confirm.is_none(), "a live run raises no question");
        assert_eq!(
            app.status.as_deref(),
            Some("stop the run before deleting its record")
        );

        // An ended one asks, and asking alone removes nothing.
        let _ = app.update(Message::Delete(RunId(ended.id.clone())));
        assert_eq!(
            app.confirm,
            Some(Confirm::One(RunId(ended.id.clone()))),
            "the press asks rather than removes"
        );
        assert_eq!(
            store.list().expect("listed").len(),
            2,
            "asking removes none"
        );

        // Escape is the same answer as Cancel, and it keeps the record.
        let _ = app.update(Message::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }));
        assert!(app.confirm.is_none(), "escape puts the question away");
        assert_eq!(store.list().expect("listed").len(), 2, "and keeps the run");

        // Answering it is the only thing that removes.
        let _ = app.update(Message::Delete(RunId(ended.id.clone())));
        let _ = app.update(Message::DeleteConfirmed);
        let left: Vec<String> = store
            .list()
            .expect("listed")
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(left, vec![live.name.clone()], "only the answered one went");
        assert!(app.confirm.is_none(), "the question goes with it");
        drop(listener);
        let _ = std::fs::remove_file(&sock);
    }

    /// A selection removes exactly what was selected, only behind the confirm, and never a live run;
    /// a second press unselects, and leaving the list drops the selection.
    #[test]
    fn a_selection_removes_what_was_selected_and_only_behind_the_confirm() {
        let dir = tormoni_test_support::ScratchDir::created("app-clear");
        let store = Store::at(dir.path().join("runs")).expect("a store");
        let name = format!("clear-live-{}", std::process::id());
        let sock = tormoni_supervisor::socket::path_for(&name).expect("a socket path");
        let _ = std::fs::remove_file(&sock);
        let listener = std::os::unix::net::UnixListener::bind(&sock).expect("a live socket");

        let mut gone = displayed("gone", false);
        gone.finish(tormoni_record::End::Exit(0));
        let mut failed = displayed("failed", false);
        failed.finish(tormoni_record::End::Failed);
        let live = displayed(&name, false);
        for r in [&gone, &failed, &live] {
            store.create(r).expect("created");
        }

        let sinks = Arc::new(frame::Sinks::open(None, None).expect("sinks"));
        let mut app = App::new(store.clone(), None, None, sinks, false);
        // A selection offers only what a delete would accept: the live run is not in it.
        let _ = app.update(Message::Select);
        let _ = app.update(Message::SelectAll(true));
        assert_eq!(
            app.list.selected().len(),
            2,
            "the live run is not selectable"
        );
        assert_eq!(
            store.list().expect("listed").len(),
            3,
            "selecting removes nothing"
        );
        let _ = app.update(Message::SelectCancelled);
        assert_eq!(app.list, ListMode::Browsing);
        assert_eq!(
            store.list().expect("listed").len(),
            3,
            "neither does cancelling"
        );

        // One of the two, which is the whole point: not all, and not one at a time.
        let one = gone.id.clone();
        let _ = app.update(Message::Select);
        let _ = app.update(Message::SelectToggle(RunId(one.clone())));
        assert_eq!(app.list.selected().len(), 1);
        let _ = app.update(Message::RemoveSelected);
        assert!(
            matches!(app.confirm, Some(Confirm::Selected(_))),
            "asking before removing"
        );
        assert_eq!(
            store.list().expect("listed").len(),
            3,
            "asking removes nothing"
        );
        // Cancelling gives the selection back rather than dropping it: the way out of the question
        // is not the way out of the selection that raised it.
        let _ = app.update(Message::DeleteCancelled);
        assert!(app.confirm.is_none(), "cancelling puts the question away");
        assert_eq!(
            app.list.selected().len(),
            1,
            "cancelling keeps what was selected"
        );
        assert_eq!(
            store.list().expect("listed").len(),
            3,
            "neither does cancelling the question"
        );
        let _ = app.update(Message::RemoveSelected);
        let _ = app.update(Message::DeleteConfirmed);
        assert_eq!(app.list, ListMode::Browsing);
        let left: Vec<String> = store
            .list()
            .expect("listed")
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(left.len(), 2, "one went, the other two stayed");
        assert!(!left.contains(&gone.name), "the selected one went");
        assert!(left.contains(&failed.name), "the unselected one stayed");
        assert_eq!(app.status.as_deref(), Some("removed 1 ended run"));

        // Pressing the same row twice takes it back out.
        let _ = app.update(Message::Select);
        let two = failed.id.clone();
        let _ = app.update(Message::SelectToggle(RunId(two.clone())));
        let _ = app.update(Message::SelectToggle(RunId(two)));
        assert!(app.list.selected().is_empty(), "a second press unselects");

        app.set_screen(Screen::Settings);
        assert_eq!(app.list, ListMode::Browsing, "leaving the list disarms");
        drop(listener);
        let _ = std::fs::remove_file(&sock);
    }

    /// A re-run's form is the record's command and posture again.
    #[test]
    fn a_rerun_form_is_the_records_posture_again() {
        let mut p = tormoni_record::Posture::new(
            PathBuf::from("/img"),
            std::num::NonZeroU8::new(2).expect("non-zero"),
            std::num::NonZeroU32::new(768).expect("non-zero"),
        );
        p.rootfs = tormoni_record::Rootfs::Writable;
        p.mounts.push(tormoni_record::Mount::new(
            PathBuf::from("/mnt"),
            PathBuf::from("/home/x/out"),
        ));
        p.network = tormoni_record::Network::Tsi;
        p.display = tormoni_record::DisplayMode::parse("800x600");
        p.results = false;
        let record = tormoni_record::Record::begin(
            "r",
            tormoni_record::Verb::Run,
            vec!["python3".into(), "x.py".into()],
            p,
        );
        let form = Form::from_record(&record);
        assert_eq!(form.command, "python3 x.py");
        assert!(form.writable_root && form.network && form.display && !form.results);
        assert_eq!(form.mounts, "/mnt=/home/x/out");
        assert_eq!(
            (
                form.vcpus.as_str(),
                form.mem_mib.as_str(),
                form.display_size.as_str()
            ),
            ("2", "768", "800x600")
        );
    }
}
