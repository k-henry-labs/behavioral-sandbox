//! `boxdesk run`: boot a sandbox, run one command in it, exit with the command's status.
//!
//! The whole verb is a thin shape over the supervisor: build a [`boxdesk_supervisor::VmConfig`], spawn
//! the helper that becomes the VM, wait, and translate how the helper ended into this process's
//! exit code. The guest's output is this process's output because the helper inherits stdio, so
//! `boxdesk run -- make test 2>/dev/null` behaves like the command it wraps.
//!
//! **Every `run` is a cold boot** (~300 ms on the development laptop, `scratch/ROADMAP.md` 2.9):
//! libkrun has no snapshot surface, so there is no warm path to hide it. A sequence of commands
//! against one VM is 3.9's long-lived mode, not this verb.

use std::ffi::OsString;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;

use boxdesk_record::RESULTS_GUEST_PATH;
use boxdesk_supervisor::{Display, VmConfig};

use crate::EXIT_OPERATIONAL;
use crate::posture::{NetArg, RootFsArg};

/// The `--display` value parser, so a geometry the helper would refuse is refused by the parser
/// instead, the way `--vcpus 0` already is by [`NonZeroU8`].
///
/// Shared by every verb that boots, so the refusal is one message.
pub(crate) fn parse_display(spec: &str) -> Result<Display, String> {
    let Some((width, height, refresh)) = crate::vmm::split_display(spec) else {
        // clap prints the flag and the offending value around this, so neither is repeated here.
        return Err("not WIDTHxHEIGHT or WIDTHxHEIGHT@HZ, all non-zero".to_string());
    };
    let display = Display::new(width, height);
    Ok(match refresh {
        Some(hz) => display.with_refresh(hz),
        None => display,
    })
}

/// Run one command in a fresh sandbox.
#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// The guest root directory (a tree from `cargo xtask build-rootfs`). Falls back to
    /// `$BOXDESK_GUEST_ROOT`, then `~/.local/share/boxdesk/rootfs`.
    #[arg(long, value_name = "DIR")]
    pub(crate) root: Option<PathBuf>,
    /// vCPUs for this sandbox. Falls back to `$BOXDESK_VCPUS`, then 1.
    #[arg(long, value_name = "N")]
    pub(crate) vcpus: Option<NonZeroU8>,
    /// Guest RAM in MiB. Falls back to `$BOXDESK_MEM_MIB`, then 512.
    #[arg(long, value_name = "MIB")]
    pub(crate) mem: Option<NonZeroU32>,
    /// The guest working directory.
    #[arg(long, value_name = "DIR")]
    pub(crate) workdir: Option<PathBuf>,
    /// A host directory made read-write at a guest path, as `GUESTDIR=HOSTDIR`: the project
    /// case, where edits land on the host. Repeatable.
    #[arg(long = "mount", value_name = "GUESTDIR=HOSTDIR")]
    pub(crate) mounts: Vec<String>,
    /// An extra virtiofs device, as `TAG=HOSTPATH`, for a guest that mounts by tag itself.
    /// Repeatable; `--mount` is the one that also mounts.
    #[arg(long = "share", value_name = "TAG=HOSTPATH")]
    pub(crate) shares: Vec<String>,
    /// Start from a snapshot: a named posture written by `boxdesk snapshot new`. Every flag given
    /// beside it speaks over it.
    #[arg(long, value_name = "NAME")]
    pub(crate) snapshot: Option<String>,
    /// Boot an image pulled by `boxdesk pull`, as `NAME[:TAG]`, instead of a guest root on disk.
    #[arg(long, value_name = "REF", conflicts_with = "root")]
    pub(crate) image: Option<String>,
    /// Mount a volume made by `boxdesk volume new`, as `NAME:GUESTDIR`. Repeatable. The guest
    /// path must exist in the image, as `--mount`'s must.
    #[arg(long = "volume", value_name = "NAME:GUESTDIR")]
    pub(crate) volumes: Vec<String>,
    /// The network posture: `none` (the default) or `tsi`.
    #[arg(long, value_name = "POSTURE")]
    pub(crate) net: Option<NetArg>,
    /// What the guest may do to its root: `read-only` (the default) or `writable`.
    #[arg(long, value_name = "POSTURE")]
    pub(crate) rootfs: Option<RootFsArg>,
    /// A `KEY=VALUE` entry for the guest environment. Repeatable.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub(crate) env: Vec<String>,
    /// The VM's name while it runs, visible to discovery. Defaults to `run-<pid>`.
    #[arg(long, value_name = "NAME")]
    pub(crate) name: Option<String>,
    /// Print what this sandbox would share and exit, without booting anything.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Keep the run's record after it ends: its posture, its captured output, and whatever the
    /// guest wrote to `/results`.
    ///
    /// A run is ephemeral by default. It boots, its output comes back on this process's streams
    /// (or inside `--json`), and the directory it worked in goes with it, so nothing accumulates
    /// on a machine that only wanted an answer.
    #[arg(long)]
    pub(crate) keep: bool,
    /// Give the guest a display of `WIDTHxHEIGHT`, shown in a window for as long as the sandbox
    /// runs; `WIDTHxHEIGHT@HZ` also tells the guest its refresh rate. Closing the window stops
    /// the sandbox.
    #[arg(long, value_name = "WIDTHxHEIGHT[@HZ]", value_parser = crate::run::parse_display)]
    pub(crate) display: Option<Display>,
    /// Keep PATH holding the display's latest frame as a binary PPM. Needs `--display`.
    #[arg(long, value_name = "PATH")]
    pub(crate) screenshot: Option<PathBuf>,
    /// Append one `frame_id<TAB>nanoseconds` line to PATH per frame the display thread sees, for
    /// measuring the frame path. Needs `--display`.
    #[arg(long, value_name = "PATH")]
    pub(crate) frame_log: Option<PathBuf>,
    /// Give the guest a virtio-snd sound card, backed by the host's audio server. Off by default:
    /// audio is a two-way hole, so the guest playing to your speakers and capturing from your
    /// microphone is opened only when asked.
    #[arg(long)]
    pub(crate) sound: bool,
    /// Let the guest use the host GPU for its own rendering: a 3D-capable virtio-gpu (virgl +
    /// Venus) into the host renderer, with or without --display. Off by default: what the guest
    /// submits, the host renderer executes. The default image ships no driver to use it.
    #[arg(long)]
    pub(crate) gpu: bool,
    /// Do not mount the run's results directory at `/results` in the guest. Every run gets one
    /// by default: the record's own empty directory, where the guest's results land.
    #[arg(long)]
    pub(crate) no_results: bool,
    /// Print the run as one JSON document when it ends, and keep the guest's own output off
    /// this process's streams: it is captured either way, and comes back inside the document.
    #[arg(long)]
    pub(crate) json: bool,
    /// The command, after `--`. The first word is resolved by the guest (its `PATH`, not the
    /// host's), so `echo` runs the guest's `echo`.
    #[arg(last = true, value_name = "COMMAND")]
    pub(crate) command: Vec<String>,
}

pub(crate) fn run(args: &RunArgs) -> ExitCode {
    match execute(args) {
        Ok(code) => ExitCode::from(code),
        Err(msg) => {
            eprintln!("boxdesk run: {msg}");
            ExitCode::from(EXIT_OPERATIONAL)
        }
    }
}

/// The verb's fallible body, one error path, one printer: the same shape as `shell`'s `session`.
fn execute(args: &RunArgs) -> Result<u8, String> {
    let snapshot = crate::posture::asked_for(args.snapshot.as_deref())?;
    let snapshot = snapshot.as_ref();
    // The root a snapshot names stands in for the flag, so it goes through the same resolution:
    // a snapshot pointing at a tree that is not there should fail the way `--root` does.
    // An image outranks a snapshot's root and is refused if it was never pulled: `--image` names
    // a thing, and quietly booting a different tree because that thing is absent is the wrong
    // answer to a caller who said which one they wanted.
    let image = crate::image::asked_for(args.image.as_deref())?;
    let root = boxdesk::resolve_root(
        image
            .as_deref()
            .or(args.root.as_deref())
            .or_else(|| snapshot.map(|s| s.posture.root.as_path())),
    )?;
    let command = crate::posture::command_for(&args.command, snapshot)?;
    let cfg = to_config(args, root, snapshot, &command)?;
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| format!("run-{}", std::process::id()));
    crate::check_name(&name)?;
    // `--no-results` subtracts from whatever the snapshot said, because it is the one posture
    // flag that takes away rather than adds.
    let results = snapshot.is_none_or(|s| s.posture.results) && !args.no_results;

    if args.dry_run && !args.json {
        print_posture(&name, &cfg, results, &mut std::io::stdout()).map_err(|e| e.to_string())?;
        return Ok(0);
    }

    let opts = boxdesk::SandboxOptions {
        name: name.clone(),
        command: command.clone(),
        cfg,
        results,
        keep: args.keep,
        dry_run: args.dry_run,
        quiet: args.json,
    };

    let (record, run_opt, exit_code) = boxdesk::execute_sandbox(opts)?;

    if args.json {
        let mut value = if let Some(run) = run_opt {
            crate::json::complete(&record, &run)
        } else {
            crate::json::record_json(&record)
        };
        if !args.keep
            && let Some(object) = value.as_object_mut()
        {
            object.insert("dir".into(), serde_json::Value::Null);
            object.insert("files".into(), serde_json::Value::Array(Vec::new()));
        }
        println!("{value}");
    }

    Ok(exit_code)
}

/// Writes what this sandbox shares, one element to a line, in the order the guest meets them.
///
/// Design rule 2's second half: the posture is visible to whoever starts it. To stdout, as a
/// run's structured result.
pub(crate) fn print_posture(
    name: &str,
    cfg: &VmConfig,
    results: bool,
    out: &mut impl std::io::Write,
) -> std::io::Result<()> {
    writeln!(out, "name     {name}")?;
    writeln!(
        out,
        "root     {} {}",
        cfg.root.display(),
        cfg.rootfs.as_flag()
    )?;
    for (guest, host) in &cfg.mounts {
        writeln!(
            out,
            "mount    {} <- {} writable",
            guest.display(),
            host.display()
        )?;
    }
    for (tag, host) in &cfg.shares {
        writeln!(out, "share    {tag} <- {} writable", host.display())?;
    }
    if results {
        writeln!(
            out,
            "results  {RESULTS_GUEST_PATH} <- the run's own record directory, writable"
        )?;
    }
    if let Some((port, path)) = &cfg.vsock {
        writeln!(out, "channel  guest vsock {port} <- {}", path.display())?;
    }
    for entry in &cfg.env {
        writeln!(
            out,
            "env      {} is set in the guest",
            boxdesk_record::env_key(&entry.to_string_lossy())
        )?;
    }
    if let Some(display) = cfg.display {
        writeln!(out, "display  {} in a window", display.as_spec())?;
    }
    if let Some(path) = &cfg.screenshot {
        writeln!(out, "screenshot {}", path.display())?;
    }
    if let Some(path) = &cfg.frame_log {
        writeln!(out, "frame-log {}", path.display())?;
    }
    if cfg.sound {
        writeln!(
            out,
            "sound    a virtio-snd card to the host audio server (play and capture)"
        )?;
    }
    if cfg.gpu {
        writeln!(
            out,
            "gpu      a 3D virtio-gpu into the host renderer (virgl + Venus offered)"
        )?;
    }
    writeln!(out, "network  {}", cfg.net.as_flag())?;
    writeln!(
        out,
        "limits   {} vcpu, {} MiB",
        cfg.vcpus.get(),
        cfg.mem_mib.get()
    )?;
    writeln!(out, "exec     {}", cfg.exec.display())
}

/// A resource limit from its flag, else its `BOXDESK_*` variable, else `None` (the supervisor's
/// default). The same flag-then-env order as the guest root, with the config-file layer still
/// deferred with it.
pub(crate) fn resolve_limit<T: std::str::FromStr>(
    flag: Option<T>,
    var: &'static str,
) -> Result<Option<T>, String> {
    resolve_limit_from(flag, var, std::env::var_os(var))
}

/// [`resolve_limit`] with the environment read lifted out, for [`resolve_root_from`]'s reason.
/// A variable that is set but does not parse is refused loudly rather than ignored: a typo'd
/// limit that silently falls back is a config that lies about what it configured.
fn resolve_limit_from<T: std::str::FromStr>(
    flag: Option<T>,
    var: &'static str,
    env: Option<OsString>,
) -> Result<Option<T>, String> {
    if flag.is_some() {
        return Ok(flag);
    }
    let Some(raw) = env else {
        return Ok(None);
    };
    let Some(text) = raw.to_str() else {
        return Err(format!("{var} is set but is not valid UTF-8"));
    };
    text.parse().map(Some).map_err(|_| {
        format!("{var}={text:?} is not a usable limit (a non-zero number that fits the knob)")
    })
}

/// Puts a `--display`, `--screenshot` and `--frame-log` on `cfg`, refusing the spellings the
/// helper would. Shared by every verb that boots, so the refusal is one message.
pub(crate) fn apply_display(
    cfg: &mut VmConfig,
    display: Option<Display>,
    screenshot: Option<&Path>,
    frame_log: Option<&Path>,
) -> Result<(), String> {
    cfg.display = display;
    if cfg.display.is_none() {
        if screenshot.is_some() {
            return Err("--screenshot needs a --display to take a frame from".to_string());
        }
        if frame_log.is_some() {
            return Err("--frame-log needs a --display to log frames of".to_string());
        }
    }
    cfg.screenshot = screenshot.map(Path::to_path_buf);
    cfg.frame_log = frame_log.map(Path::to_path_buf);
    Ok(())
}

/// The [`VmConfig`] for `args`, against `root`. Split from [`run`] so the flag-to-field mapping is
/// testable without booting anything.
fn to_config(
    args: &RunArgs,
    root: PathBuf,
    snapshot: Option<&boxdesk_record::Snapshot>,
    command: &[String],
) -> Result<VmConfig, String> {
    let Some((program, rest)) = command.split_first() else {
        return Err("no command after `--`".to_string());
    };
    let mut cfg = VmConfig::new(root, program);
    // The snapshot first, as the base every flag below speaks over.
    if let Some(snapshot) = snapshot {
        crate::posture::lay_snapshot(&mut cfg, snapshot);
    }
    if let Some(net) = args.net {
        cfg.net = net.into_net();
    }
    if let Some(rootfs) = args.rootfs {
        cfg.rootfs = rootfs.into_rootfs();
    }
    cfg.sound |= args.sound;
    cfg.gpu |= args.gpu;
    apply_display(
        &mut cfg,
        args.display
            .or_else(|| crate::posture::snapshot_display(snapshot)),
        args.screenshot.as_deref(),
        args.frame_log.as_deref(),
    )?;
    if let Some(v) = crate::posture::limit(
        args.vcpus,
        snapshot.map(|s| s.posture.vcpus),
        "BOXDESK_VCPUS",
    )? {
        cfg.vcpus = v;
    }
    if let Some(m) = crate::posture::limit(
        args.mem,
        snapshot.map(|s| s.posture.mem_mib),
        "BOXDESK_MEM_MIB",
    )? {
        cfg.mem_mib = m;
    }
    cfg.workdir = args.workdir.clone();
    cfg.args = rest.iter().map(OsString::from).collect();
    cfg.env = args.env.iter().map(OsString::from).collect();
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
    // A volume is an ordinary mount, resolved to one here: nothing downstream — the config, the
    // record, the posture a run prints — needs to know the word.
    cfg.mounts.extend(crate::volume::mounts_for(&args.volumes)?);
    Ok(cfg)
}

/// A guest's `i32` exit code as this process's `u8` one, shared by both verbs. Out-of-range
/// values cannot come from a Unix wait status, but a lossy cast that quietly wrapped one would
/// report a wrong code as a right one, so they saturate loudly instead.
pub(crate) fn guest_code(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {

    // `panic!` is the assertion in the let-else arms below; the tree-wide deny is for the host
    // path, and this module is not on it.
    #![allow(clippy::panic)]

    use std::path::Path;

    use clap::Parser;

    use super::*;
    use crate::{Cli, Cmd};

    /// The roadmap's own example, `boxdesk run -- echo hello`, must parse with the command intact and
    /// hyphens in the command untouched, since everything after `--` belongs to the guest.
    #[test]
    fn the_command_after_the_separator_is_taken_verbatim() {
        let cli = Cli::parse_from(["boxdesk", "run", "--", "sh", "-c", "echo hi"]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        assert_eq!(args.command, ["sh", "-c", "echo hi"]);
        assert!(args.root.is_none());
    }

    /// Every flag lands in the config field it names, and the command splits into the guest
    /// program and its arguments.
    #[test]
    fn the_flags_land_in_the_config_fields_they_name() {
        let cli = Cli::parse_from([
            "boxdesk",
            "run",
            "--vcpus",
            "2",
            "--mem",
            "1024",
            "--workdir",
            "/w",
            "--share",
            "data=/tmp",
            "--env",
            "K=v",
            "--mount",
            "/project=/srv/code",
            "--",
            "prog",
            "-x",
        ]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        let cfg = to_config(&args, PathBuf::from("/root-tree"), None, &args.command)
            .expect("a well-formed config");
        assert_eq!(cfg.root, Path::new("/root-tree"));
        assert_eq!(cfg.exec, Path::new("prog"));
        assert_eq!(cfg.args, [OsString::from("-x")]);
        assert_eq!(cfg.vcpus.get(), 2);
        assert_eq!(cfg.mem_mib.get(), 1024);
        assert_eq!(cfg.workdir.as_deref(), Some(Path::new("/w")));
        assert_eq!(cfg.env, [OsString::from("K=v")]);
        assert_eq!(cfg.shares, [("data".to_string(), PathBuf::from("/tmp"))]);
        assert_eq!(
            cfg.mounts,
            [(PathBuf::from("/project"), PathBuf::from("/srv/code"))]
        );
    }

    /// A limit resolves flag first, then its variable, and a variable that does not parse is a
    /// loud refusal naming it, never a silent fall-back to the default.
    #[test]
    fn a_limit_resolves_flag_then_env_and_refuses_a_typo() {
        let flag = NonZeroU8::new(4);
        let env = Some(OsString::from("2"));
        assert_eq!(
            resolve_limit_from(flag, "BOXDESK_VCPUS", env.clone()).expect("the flag wins"),
            flag
        );
        assert_eq!(
            resolve_limit_from::<NonZeroU8>(None, "BOXDESK_VCPUS", env).expect("the env fills in"),
            NonZeroU8::new(2)
        );
        assert_eq!(
            resolve_limit_from::<NonZeroU8>(None, "BOXDESK_VCPUS", None)
                .expect("unset means unset"),
            None
        );
        for bad in ["zero-is-not-a-machine", "0", "-1", ""] {
            let err =
                resolve_limit_from::<NonZeroU8>(None, "BOXDESK_VCPUS", Some(OsString::from(bad)))
                    .expect_err("a set-but-broken limit must refuse");
            assert!(err.contains("BOXDESK_VCPUS"), "names the variable: {err}");
        }
    }

    /// `run` with no `--net` asks for no network: the whole point of the task is that "say
    /// nothing" means no network, against libkrun's own default. The crossing itself is
    /// `every_posture_defaults_closed_and_crosses_to_its_own_variant`, beside the enum.
    #[test]
    fn the_net_posture_defaults_to_none() {
        let cli = Cli::parse_from(["boxdesk", "run", "--", "true"]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        assert_eq!(args.net, None, "no --net means the posture is unsaid");
    }

    /// `run` with no `--rootfs` cannot write the image tree every sandbox on the host boots
    /// from, and the config it builds carries that posture through.
    #[test]
    fn the_root_posture_defaults_to_read_only() {
        let cli = Cli::parse_from(["boxdesk", "run", "--", "true"]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        assert_eq!(args.rootfs, None, "no --rootfs means the posture is unsaid");
        let cfg = to_config(&args, PathBuf::from("/r"), None, &args.command)
            .expect("a well-formed config");
        assert_eq!(cfg.rootfs, boxdesk_supervisor::RootFs::ReadOnly);
    }

    /// The posture print names every way into and out of the sandbox, each with its direction,
    /// and the root line carries the posture rather than only the path: a reader must be able to
    /// tell a writable image tree from a read-only one without booting it.
    #[test]
    fn the_posture_print_names_every_shared_thing_and_its_direction() {
        let cli = Cli::parse_from([
            "boxdesk",
            "run",
            "--rootfs",
            "writable",
            "--mount",
            "/mnt=/srv/code",
            "--share",
            "data=/srv/data",
            "--net",
            "tsi",
            "--gpu",
            "--",
            "true",
        ]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        let cfg = to_config(&args, PathBuf::from("/root-tree"), None, &args.command)
            .expect("a well-formed config");
        let mut out = Vec::new();
        print_posture("vm-under-test", &cfg, false, &mut out).expect("a Vec never fails to write");
        let text = String::from_utf8(out).expect("the printer writes UTF-8");
        for line in [
            "name     vm-under-test",
            "root     /root-tree writable",
            "mount    /mnt <- /srv/code writable",
            "share    data <- /srv/data writable",
            "network  tsi",
            "gpu      a 3D virtio-gpu into the host renderer (virgl + Venus offered)",
            "limits   1 vcpu, 512 MiB",
            "exec     true",
        ] {
            assert!(text.contains(line), "{line:?} missing from:\n{text}");
        }
    }

    /// **`--env` reaches the guest whole and the record by name only.** The VM is given
    /// `KEY=VALUE`, because that is what it is for; what a person may later export, push or paste
    /// gets the name and nothing else.
    #[test]
    fn an_env_value_reaches_the_guest_and_never_the_record_or_the_posture_print() {
        let cli = Cli::parse_from([
            "boxdesk",
            "run",
            "--env",
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI",
            "--env",
            "CI=1",
            "--",
            "true",
        ]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        let cfg = to_config(&args, PathBuf::from("/root-tree"), None, &args.command)
            .expect("a well-formed config");
        assert_eq!(
            cfg.env,
            [
                OsString::from("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI"),
                OsString::from("CI=1")
            ],
            "the guest is given the whole entry"
        );

        let posture = boxdesk::posture_of(&cfg, false);
        assert_eq!(posture.env, ["AWS_SECRET_ACCESS_KEY", "CI"]);
        let record = boxdesk_record::Record::begin(
            "vm-under-test",
            boxdesk_record::Verb::Run,
            vec!["true".to_string()],
            posture,
        );
        assert!(
            !record.to_text().contains("wJalrXUtnFEMI"),
            "{}",
            record.to_text()
        );

        let mut out = Vec::new();
        print_posture("vm-under-test", &cfg, false, &mut out).expect("a Vec never fails to write");
        let text = String::from_utf8(out).expect("the printer writes UTF-8");
        assert!(
            text.contains("env      AWS_SECRET_ACCESS_KEY is set in the guest"),
            "{text}"
        );
        assert!(!text.contains("wJalrXUtnFEMI"), "{text}");
    }

    /// A display lands in the config as two non-zero numbers, a screenshot needs one, and the
    /// posture print names both: a window is a way out of the sandbox a reader should see.
    #[test]
    fn a_display_and_screenshot_land_in_the_config_and_the_posture() {
        let mut cfg = VmConfig::new("/r", "true");
        let err = apply_display(&mut cfg, None, Some(Path::new("/tmp/f.ppm")), None)
            .expect_err("a screenshot with no display");
        assert!(err.contains("--display"), "{err}");
        // The geometry is the parser's job now, so a zero side never reaches `apply_display`.
        let err = parse_display("0x600").expect_err("zero is not a display");
        assert!(
            err.contains("WIDTHxHEIGHT") && err.contains("non-zero"),
            "{err}"
        );
        assert!(parse_display("800").is_err(), "a width is not a display");
        assert!(parse_display("800x600@0").is_err(), "zero Hz is not a rate");
        apply_display(
            &mut cfg,
            Some(parse_display("800x600").expect("a display")),
            Some(Path::new("/tmp/f.ppm")),
            None,
        )
        .expect("both");
        assert_eq!(
            cfg.display.map(|d| d.as_spec()),
            Some("800x600".to_string())
        );
        assert_eq!(cfg.screenshot.as_deref(), Some(Path::new("/tmp/f.ppm")));
        let err = apply_display(
            &mut VmConfig::new("/r", "true"),
            None,
            None,
            Some(Path::new("/l")),
        )
        .expect_err("a frame log needs a display");
        assert!(err.contains("--frame-log"), "{err}");
        let mut rated = VmConfig::new("/r", "true");
        apply_display(
            &mut rated,
            Some(parse_display("800x600@120").expect("a rated display")),
            None,
            Some(Path::new("/tmp/frames.tsv")),
        )
        .expect("a rate and a log");
        assert_eq!(
            rated.display.map(|d| d.as_spec()).as_deref(),
            Some("800x600@120")
        );
        assert_eq!(
            rated.frame_log.as_deref(),
            Some(Path::new("/tmp/frames.tsv"))
        );
        let mut out = Vec::new();
        print_posture("vm", &cfg, true, &mut out).expect("a Vec never fails");
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("display  800x600 in a window"), "{text}");
        assert!(text.contains("screenshot /tmp/f.ppm"), "{text}");
    }

    /// A malformed share is refused here, before a VM is spawned to die on it.
    #[test]
    fn a_malformed_share_is_refused_before_spawn() {
        let cli = Cli::parse_from(["boxdesk", "run", "--share", "nopath", "--", "true"]);
        let Cmd::Run(args) = cli.cmd else {
            panic!("run must parse");
        };
        let err = to_config(&args, PathBuf::from("/r"), None, &args.command)
            .expect_err("half a share is refused");
        assert!(err.contains("nopath"), "{err}");
    }
    /// **A flag speaks over the snapshot, including when the flag is the closed one.**
    ///
    /// This is the reason `--net` and `--rootfs` stopped carrying a clap `default_value`: with
    /// one, a caller who typed `--net none` was indistinguishable from a caller who typed
    /// nothing, so a snapshot asking for `tsi` would have opened a network the caller had just
    /// said no to. There is no worse bug available in this file.
    #[test]
    fn a_posture_flag_speaks_over_the_snapshot_even_when_it_closes_one() {
        let mut posture = boxdesk_record::Posture::new(
            PathBuf::from("/srv/guest"),
            std::num::NonZeroU8::new(1).expect("non-zero"),
            std::num::NonZeroU32::new(512).expect("non-zero"),
        );
        posture.network = boxdesk_record::Network::Tsi;
        posture.rootfs = boxdesk_record::Rootfs::Writable;
        let snapshot = boxdesk_record::Snapshot::new("open", posture, vec!["true".to_string()]);

        let parsed = |argv: &[&str]| {
            let Cmd::Run(args) = Cli::parse_from(argv).cmd else {
                panic!("run parses as run")
            };
            args
        };

        let open = parsed(&["boxdesk", "run", "--", "true"]);
        let cfg = to_config(&open, PathBuf::from("/r"), Some(&snapshot), &open.command)
            .expect("a well-formed config");
        assert_eq!(
            cfg.net,
            boxdesk_supervisor::Net::Tsi,
            "with no --net the snapshot's posture should stand"
        );
        assert_eq!(cfg.rootfs, boxdesk_supervisor::RootFs::Writable);

        let shut = parsed(&[
            "boxdesk",
            "run",
            "--net",
            "none",
            "--rootfs",
            "read-only",
            "--",
            "true",
        ]);
        let cfg = to_config(&shut, PathBuf::from("/r"), Some(&snapshot), &shut.command)
            .expect("a well-formed config");
        assert_eq!(
            cfg.net,
            boxdesk_supervisor::Net::None,
            "typing --net none must close a network the snapshot opened"
        );
        assert_eq!(
            cfg.rootfs,
            boxdesk_supervisor::RootFs::ReadOnly,
            "and typing --rootfs read-only must close the root"
        );
    }
}
