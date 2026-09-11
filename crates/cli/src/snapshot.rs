//! `boxdesk snapshot`: sandboxes worth making again, kept by name.
//!
//! A snapshot is a posture with a name on it — what a sandbox boots, what it may touch, and how
//! much machine it gets. `new` writes one, `ls` lists them, `show` prints one the way `boxdesk
//! run --dry-run` prints a sandbox, and `rm` takes one away. A verb that boots takes `--snapshot
//! NAME` and starts from it.
//!
//! - **A snapshot is not a captured VM.** Nothing here pauses or copies a running sandbox; that
//!   is a different feature wearing the same word, and `ROADMAP.md` keeps the two apart.
//! - **`new` refuses to overwrite** unless told to. A template several sandboxes were made from
//!   is not something to replace by retyping its name.
//! - **The flags are `run`'s flags**, so a posture learned once is spelled one way everywhere.

use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;
use std::process::ExitCode;

use boxdesk_record::{Snapshot, SnapshotStore};
use boxdesk_supervisor::VmConfig;

use crate::EXIT_OPERATIONAL;
use crate::posture::{NetArg, RootFsArg};

#[derive(clap::Args)]
pub(crate) struct SnapshotArgs {
    #[command(subcommand)]
    cmd: SnapshotCmd,
}

#[derive(clap::Subcommand)]
enum SnapshotCmd {
    /// Write a snapshot: a named sandbox posture to make sandboxes from.
    New(NewArgs),
    /// List the snapshots on this machine.
    Ls(LsArgs),
    /// Print one snapshot: what a sandbox made from it would boot and could touch.
    Show(ShowArgs),
    /// Remove a snapshot. The sandboxes already made from it are untouched.
    Rm(RmArgs),
}

#[derive(clap::Args)]
pub(crate) struct NewArgs {
    /// The name it is reached by, as `--snapshot NAME`.
    #[arg(value_name = "NAME")]
    name: String,
    /// One line saying what this sandbox is for, shown beside the name in `ls`.
    #[arg(long, value_name = "TEXT")]
    about: Option<String>,
    /// The guest root a sandbox from this snapshot boots. Falls back like `boxdesk run`'s.
    #[arg(long, value_name = "DIR")]
    root: Option<PathBuf>,
    /// vCPUs a sandbox from it gets.
    #[arg(long, value_name = "N")]
    vcpus: Option<NonZeroU8>,
    /// Guest RAM in MiB a sandbox from it gets.
    #[arg(long, value_name = "MIB")]
    mem: Option<NonZeroU32>,
    /// A host directory made read-write at a guest path, as `GUESTDIR=HOSTDIR`. Repeatable.
    #[arg(long = "mount", value_name = "GUESTDIR=HOSTDIR")]
    mounts: Vec<String>,
    /// An extra virtiofs device, as `TAG=HOSTPATH`. Repeatable.
    #[arg(long = "share", value_name = "TAG=HOSTPATH")]
    shares: Vec<String>,
    /// Give sandboxes from it a display of `WIDTHxHEIGHT`, or `WIDTHxHEIGHT@HZ`.
    #[arg(long, value_name = "WIDTHxHEIGHT[@HZ]", value_parser = crate::run::parse_display)]
    display: Option<boxdesk_supervisor::Display>,
    /// The network posture: `none` (the default) or `tsi`.
    #[arg(long, value_name = "POSTURE")]
    net: Option<NetArg>,
    /// What a sandbox from it may do to its root: `read-only` (the default) or `writable`.
    #[arg(long, value_name = "POSTURE")]
    rootfs: Option<RootFsArg>,
    /// Give sandboxes from it a sound card.
    #[arg(long)]
    sound: bool,
    /// Give sandboxes from it the host GPU's 3D path.
    #[arg(long)]
    gpu: bool,
    /// Do not mount the run's results directory at `/results`.
    #[arg(long)]
    no_results: bool,
    /// Replace a snapshot of this name if there is one.
    #[arg(long)]
    force: bool,
    /// The command a sandbox from it runs when it is given none of its own, after `--`.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<String>,
}

#[derive(clap::Args)]
pub(crate) struct LsArgs {
    /// Print the snapshots as one JSON array.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct ShowArgs {
    /// The snapshot's name.
    #[arg(value_name = "NAME")]
    name: String,
    /// Print the snapshot as one JSON document.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct RmArgs {
    /// The snapshot's name.
    #[arg(value_name = "NAME")]
    name: String,
}

pub(crate) fn run(args: &SnapshotArgs) -> ExitCode {
    let store = match SnapshotStore::open() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("boxdesk snapshot: {e}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    let (verb, done) = match &args.cmd {
        SnapshotCmd::New(a) => ("new", create(&store, a)),
        SnapshotCmd::Ls(a) => ("ls", list(&store, a, &mut std::io::stdout())),
        SnapshotCmd::Show(a) => ("show", describe(&store, a, &mut std::io::stdout())),
        SnapshotCmd::Rm(a) => ("rm", forget(&store, a)),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("boxdesk snapshot {verb}: {msg}");
            ExitCode::from(EXIT_OPERATIONAL)
        }
    }
}

/// `snapshot new`: build the posture the flags describe and write it under `name`.
fn create(store: &SnapshotStore, args: &NewArgs) -> Result<(), String> {
    if !boxdesk_record::valid_id(&args.name) {
        return Err(format!(
            "{:?} is not a usable snapshot name: letters, digits, `-` and `_`",
            args.name
        ));
    }
    // Refusing rather than replacing: a template several sandboxes were made from is not
    // something to lose by retyping its name.
    if store.holds(&args.name) && !args.force {
        return Err(format!(
            "a snapshot named {:?} is already here (use --force to replace it)",
            args.name
        ));
    }

    // Through a `VmConfig` and `posture_of`, not by filling a `Posture` here: that is the one
    // place a posture is derived from a machine, and a second one would be a second answer to
    // "what could this touch" — the bug that doc comment was written about.
    let root = boxdesk::resolve_root(args.root.as_deref())?;
    let mut cfg = VmConfig::new(root, args.command.first().map_or("", String::as_str));
    cfg.args = args
        .command
        .iter()
        .skip(1)
        .map(std::ffi::OsString::from)
        .collect();
    cfg.net = args.net.unwrap_or_default().into_net();
    cfg.rootfs = args.rootfs.unwrap_or_default().into_rootfs();
    cfg.sound = args.sound;
    cfg.gpu = args.gpu;
    if let Some(vcpus) = args.vcpus {
        cfg.vcpus = vcpus;
    }
    if let Some(mem) = args.mem {
        cfg.mem_mib = mem;
    }
    for spec in &args.shares {
        let Some((tag, path)) = crate::vmm::split_share(spec) else {
            return Err(format!("--share {spec:?} is not TAG=HOSTPATH"));
        };
        cfg.shares.push((tag.to_string(), path.to_path_buf()));
    }
    for spec in &args.mounts {
        let Some((guest, host)) = crate::vmm::split_mount(spec) else {
            return Err(format!(
                "--mount {spec:?} is not GUESTDIR=HOSTDIR with an absolute guest path"
            ));
        };
        cfg.mounts.push((guest.to_path_buf(), host.to_path_buf()));
    }
    crate::run::apply_display(&mut cfg, args.display, None, None)?;
    let posture = boxdesk::posture_of(&cfg, !args.no_results);

    let mut snapshot = Snapshot::new(&args.name, posture, args.command.clone());
    if let Some(about) = &args.about {
        snapshot = snapshot.about(about);
    }
    store.save(&snapshot).map_err(|e| e.to_string())?;
    println!("{}", snapshot.name);
    Ok(())
}

/// `snapshot ls`: one line per snapshot, name first, so the column a reader scans is the one they
/// will type.
fn list(store: &SnapshotStore, args: &LsArgs, out: &mut impl std::io::Write) -> Result<(), String> {
    let snapshots = store.list();
    if args.json {
        let array: Vec<serde_json::Value> = snapshots.iter().map(json).collect();
        writeln!(out, "{}", serde_json::Value::Array(array)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if snapshots.is_empty() {
        writeln!(out, "no snapshots (write one with `boxdesk snapshot new`)")
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let widest = snapshots.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for snapshot in &snapshots {
        let about = if snapshot.about.is_empty() {
            summary(snapshot)
        } else {
            snapshot.about.clone()
        };
        writeln!(out, "{:widest$}  {about}", snapshot.name).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `snapshot show`: the posture, spelled the way `boxdesk run --dry-run` spells a sandbox's.
fn describe(
    store: &SnapshotStore,
    args: &ShowArgs,
    out: &mut impl std::io::Write,
) -> Result<(), String> {
    let snapshot = store.read(&args.name).map_err(|e| e.to_string())?;
    if args.json {
        writeln!(out, "{}", json(&snapshot)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let p = &snapshot.posture;
    writeln!(out, "name     {}", snapshot.name).map_err(|e| e.to_string())?;
    if !snapshot.about.is_empty() {
        writeln!(out, "about    {}", snapshot.about).map_err(|e| e.to_string())?;
    }
    writeln!(out, "root     {} {}", p.root.display(), p.rootfs.as_word())
        .map_err(|e| e.to_string())?;
    writeln!(out, "network  {}", p.network.as_word()).map_err(|e| e.to_string())?;
    if let Some(display) = p.display {
        writeln!(out, "display  {}", display.as_spec()).map_err(|e| e.to_string())?;
    }
    writeln!(out, "sound    {}", yes_no(p.sound)).map_err(|e| e.to_string())?;
    writeln!(out, "gpu      {}", yes_no(p.gpu)).map_err(|e| e.to_string())?;
    writeln!(out, "results  {}", yes_no(p.results)).map_err(|e| e.to_string())?;
    writeln!(out, "limits   {} vcpu, {} MiB", p.vcpus, p.mem_mib).map_err(|e| e.to_string())?;
    if !snapshot.command.is_empty() {
        writeln!(out, "command  {}", snapshot.command.join(" ")).map_err(|e| e.to_string())?;
    }
    writeln!(
        out,
        "created  {}",
        boxdesk_record::format_time(snapshot.created_ms)
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `snapshot rm`: take one away. The sandboxes already made from it are records of their own and
/// are not touched.
fn forget(store: &SnapshotStore, args: &RmArgs) -> Result<(), String> {
    store.remove(&args.name).map_err(|e| e.to_string())?;
    println!("{}", args.name);
    Ok(())
}

/// A snapshot as JSON, with the posture's keys spelled as `boxdesk show --json` spells a record's.
fn json(snapshot: &Snapshot) -> serde_json::Value {
    let p = &snapshot.posture;
    serde_json::json!({
        "name": snapshot.name,
        "about": snapshot.about,
        "command": snapshot.command,
        "root": p.root.display().to_string(),
        "rootfs": p.rootfs.as_word(),
        "network": p.network.as_word(),
        "display": p.display.map(|d| d.as_spec()),
        "sound": p.sound,
        "gpu": p.gpu,
        "results": p.results,
        "vcpus": p.vcpus.get(),
        "mem_mib": p.mem_mib.get(),
        "created_ms": snapshot.created_ms,
    })
}

/// What a snapshot with nothing said about it is, in one line: the shape of the machine and
/// whether it can reach anything, which is what a reader scanning `ls` wants.
fn summary(snapshot: &Snapshot) -> String {
    let p = &snapshot.posture;
    let net = if p.network == boxdesk_record::Network::Tsi {
        "network via host"
    } else {
        "no network"
    };
    format!("{} vcpu, {} MiB, {net}", p.vcpus, p.mem_mib)
}

fn yes_no(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use boxdesk_record::Posture;

    use super::*;

    /// A store of its own, so a test never reads or writes the machine's snapshots.
    fn scratch() -> (boxdesk_test_support::ScratchDir, SnapshotStore) {
        let dir = boxdesk_test_support::ScratchDir::created("cli-snapshot");
        let store = SnapshotStore::at(dir.path().join("snapshots")).expect("a store");
        (dir, store)
    }

    fn saved(store: &SnapshotStore, name: &str, about: &str) {
        let posture = Posture::new(
            PathBuf::from("/srv/guest"),
            NonZeroU8::new(1).expect("non-zero"),
            NonZeroU32::new(512).expect("non-zero"),
        );
        let mut snapshot = Snapshot::new(name, posture, vec!["true".to_string()]);
        if !about.is_empty() {
            snapshot = snapshot.about(about);
        }
        store.save(&snapshot).expect("saved");
    }

    /// `ls` puts the name first and pads it, so the column a reader scans is the one they type,
    /// and a snapshot with nothing said about it still says what shape of machine it is.
    ///
    /// Through `list` itself rather than a copy of its loop: a test that reimplements the thing
    /// it checks passes while the real one is wrong.
    #[test]
    fn ls_leads_with_the_name_and_says_something_about_every_one() {
        let (_dir, store) = scratch();
        saved(&store, "big", "the one with room");
        saved(&store, "tiny", "");

        let mut out = Vec::new();
        list(&store, &LsArgs { json: false }, &mut out).expect("listed");
        let text = String::from_utf8(out).expect("utf-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "got {text:?}");
        assert!(lines[0].starts_with("big "), "got {:?}", lines[0]);
        assert!(lines[0].contains("the one with room"));
        assert!(
            lines[1].contains("vcpu") && lines[1].contains("no network"),
            "a snapshot with no `about` should still describe itself: {:?}",
            lines[1]
        );
    }

    /// An empty store says so, and says where a snapshot comes from: a blank answer leaves a
    /// reader wondering whether the verb worked.
    #[test]
    fn ls_on_an_empty_store_says_how_to_make_one() {
        let (_dir, store) = scratch();
        let mut out = Vec::new();
        list(&store, &LsArgs { json: false }, &mut out).expect("listed");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("snapshot new"), "got {text:?}");
    }

    /// Every posture a snapshot carries reaches its JSON, under the names a record's JSON uses:
    /// a client reading both should not have to learn two spellings of one posture.
    #[test]
    fn the_json_names_every_posture_the_snapshot_holds() {
        let mut posture = Posture::new(
            PathBuf::from("/srv/guest"),
            NonZeroU8::new(1).expect("non-zero"),
            NonZeroU32::new(512).expect("non-zero"),
        );
        posture.network = boxdesk_record::Network::Tsi;
        let snapshot = Snapshot::new("devbox", posture, vec!["sleep".into()]).about("for editing");
        let value = json(&snapshot);
        for key in [
            "name",
            "about",
            "command",
            "root",
            "rootfs",
            "network",
            "display",
            "sound",
            "gpu",
            "results",
            "vcpus",
            "mem_mib",
            "created_ms",
        ] {
            assert!(
                value.get(key).is_some(),
                "the JSON says nothing about {key}"
            );
        }
        assert_eq!(value["network"], "tsi");
        assert_eq!(value["name"], "devbox");
    }

    /// `show` prints the posture in words rather than printing the file: a reader asking what a
    /// snapshot is should get the same words `boxdesk run --dry-run` answers with.
    #[test]
    fn show_prints_the_posture_in_words() {
        let (_dir, store) = scratch();
        saved(&store, "devbox", "for editing");
        let mut out = Vec::new();
        describe(
            &store,
            &ShowArgs {
                name: "devbox".to_string(),
                json: false,
            },
            &mut out,
        )
        .expect("described");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("root     /srv/guest read-only"), "got {text}");
        assert!(text.contains("network  none"), "got {text}");
        assert!(text.contains("about    for editing"), "got {text}");
        assert!(text.contains("command  true"), "got {text}");
    }

    /// A name that is not in the store is refused by `show` rather than answered with an empty
    /// posture.
    #[test]
    fn show_refuses_a_name_nobody_wrote() {
        let (_dir, store) = scratch();
        let mut out = Vec::new();
        let err = describe(
            &store,
            &ShowArgs {
                name: "nobody".to_string(),
                json: false,
            },
            &mut out,
        )
        .expect_err("there is no such snapshot");
        assert!(!err.is_empty(), "the refusal should say something");
    }

    /// `new` refuses to replace a snapshot unless it is told to: a template several sandboxes
    /// were made from is not something to lose by retyping its name.
    #[test]
    fn new_refuses_to_overwrite_without_being_told_to() {
        let (_dir, store) = scratch();
        saved(&store, "base", "the first one");
        let args = |force: bool| NewArgs {
            name: "base".to_string(),
            about: None,
            root: Some(PathBuf::from("/srv/other")),
            vcpus: None,
            mem: None,
            net: None,
            rootfs: None,
            sound: false,
            gpu: false,
            no_results: false,
            mounts: Vec::new(),
            shares: Vec::new(),
            display: None,
            force,
            command: vec!["true".to_string()],
        };
        let err = create(&store, &args(false)).expect_err("a name already taken");
        assert!(err.contains("--force"), "said {err} without saying how");
        assert_eq!(
            store.read("base").expect("still there").about,
            "the first one",
            "the refusal should have left the old one alone"
        );
    }
}
