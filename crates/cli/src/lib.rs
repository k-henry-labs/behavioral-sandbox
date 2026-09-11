#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tormoni_record::{End, Posture, Record, RunDir, Store, Verb};
use tormoni_supervisor::{Console, Exit, Vm, VmConfig};

pub struct SandboxOptions {
    pub name: String,
    pub command: Vec<String>,
    pub cfg: VmConfig,
    pub results: bool,
    pub keep: bool,
    pub dry_run: bool,
    pub quiet: bool,
}

/// The record's spelling of a config's root posture.
///
/// The two enums are separate because `tormoni-record` is dependency-free and cannot name a type
/// from the crate that links libkrun; `the_record_and_the_config_spell_the_posture_alike` holds
/// them in step, and the wildcard is what `#[non_exhaustive]` requires of a match from another
/// crate.
fn record_rootfs(rootfs: tormoni_supervisor::RootFs) -> tormoni_record::Rootfs {
    match rootfs {
        tormoni_supervisor::RootFs::Writable => tormoni_record::Rootfs::Writable,
        _ => tormoni_record::Rootfs::ReadOnly,
    }
}

/// The record's spelling of a config's network posture.
fn record_network(net: tormoni_supervisor::Net) -> tormoni_record::Network {
    match net {
        tormoni_supervisor::Net::Tsi => tormoni_record::Network::Tsi,
        _ => tormoni_record::Network::None,
    }
}

/// The record's spelling of a config's display.
fn record_display(display: tormoni_supervisor::Display) -> tormoni_record::DisplayMode {
    let mode = tormoni_record::DisplayMode::new(display.width, display.height);
    match display.refresh {
        Some(hz) => mode.with_refresh(hz),
        None => mode,
    }
}

/// The posture a config describes, which is what a record keeps of a run.
///
/// **Derived here and nowhere else, and deliberately not a field a caller supplies.** It was one,
/// and the Python binding built its own by hand: it dropped the display, never filled `cfg.env`
/// at all, and assigned the raw `KEY=VALUE` entries to `env` — putting the caller's secrets in
/// the record and handing them back out. A posture that is computed from the config cannot
/// disagree with the machine that actually booted.
pub fn posture_of(cfg: &VmConfig, results: bool) -> Posture {
    let mut p = Posture::new(cfg.root.clone(), cfg.vcpus, cfg.mem_mib);
    p.rootfs = record_rootfs(cfg.rootfs);
    p.mounts = cfg
        .mounts
        .iter()
        .map(|(guest, host)| tormoni_record::Mount::new(guest.clone(), host.clone()))
        .collect();
    p.shares = cfg
        .shares
        .iter()
        .map(|(tag, host)| tormoni_record::Share::new(tag.clone(), host.clone()))
        .collect();
    p.network = record_network(cfg.net);
    p.display = cfg.display.map(record_display);
    p.sound = cfg.sound;
    p.gpu = cfg.gpu;
    p.results = results;
    // The names, never what they are set to: a value is the caller's secret often enough that
    // the record is not the place for one. `tormoni_record::env_key` is the same cut the record
    // writer makes.
    p.env = cfg
        .env
        .iter()
        .map(|entry| tormoni_record::env_key(&entry.to_string_lossy()).to_string())
        .collect();
    p
}

pub fn execute_sandbox(mut opts: SandboxOptions) -> Result<(Record, Option<RunDir>, u8), String> {
    let posture = posture_of(&opts.cfg, opts.results);
    if opts.dry_run {
        let record = Record::begin(&opts.name, Verb::Run, opts.command.clone(), posture);
        return Ok((record, None, 0));
    }

    let store = Store::open().map_err(|e| e.to_string())?;
    let mut record = Record::begin(&opts.name, Verb::Run, opts.command, posture);
    let run = store.create(&record).map_err(|e| e.to_string())?;

    if opts.results {
        opts.cfg.mounts.push((
            std::path::PathBuf::from(tormoni_record::RESULTS_GUEST_PATH),
            run.results(),
        ));
    }
    opts.cfg.console = Console::Piped;

    let mut vm = Vm::spawn(opts.name.clone(), &opts.cfg).map_err(|e| e.to_string())?;
    record.pid = Some(vm.pid());
    store.save(&record).map_err(|e| e.to_string())?;

    let (to_out, to_err): (
        Box<dyn std::io::Write + Send>,
        Box<dyn std::io::Write + Send>,
    ) = if opts.quiet {
        (Box::new(std::io::sink()), Box::new(std::io::sink()))
    } else {
        (Box::new(std::io::stdout()), Box::new(std::io::stderr()))
    };

    let out = tee(
        vm.take_stdout(),
        to_out,
        run.append(&run.stdout()).map_err(|e| e.to_string())?,
    );
    let err = tee(
        vm.take_stderr(),
        to_err,
        run.append(&run.stderr()).map_err(|e| e.to_string())?,
    );

    let waited = vm.wait();
    let _ = out.join();
    let _ = err.join();

    let exit = match waited {
        Ok(exit) => exit,
        Err(e) => {
            record.finish(End::Failed);
            let _ = store.save(&record);
            return Err(e.to_string());
        }
    };

    record.finish(match exit {
        Exit::Code(code) => End::Exit(code),
        Exit::Signal(sig) => End::Signal(sig),
        _ => End::Failed,
    });
    store.save(&record).map_err(|e| e.to_string())?;

    let code = match exit {
        Exit::Code(code) => u8::try_from(code).unwrap_or(u8::MAX),
        Exit::Signal(sig) => 128u8.saturating_add(u8::try_from(sig).unwrap_or(u8::MAX)),
        _ => 2, // EXIT_OPERATIONAL
    };

    if !opts.keep
        && let Err(e) = store.remove(&record.id)
    {
        eprintln!("tormoni run: {} was left behind: {e}", record.id);
    }

    Ok((record, Some(run), code))
}

fn tee(
    from: Option<impl std::io::Read + Send + 'static>,
    mut to: impl std::io::Write + Send + 'static,
    mut keep: tormoni_record::Capped,
) -> std::thread::JoinHandle<()> {
    use std::io::Write;
    std::thread::spawn(move || {
        let Some(mut from) = from else { return };
        let mut buf = [0u8; 8192];
        loop {
            let n = match from.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let _ = keep.write_all(&buf[..n]);
            if to.write_all(&buf[..n]).is_err() || to.flush().is_err() {
                break;
            }
        }
    })
}

/// The guest root: the flag, else `$TORMONI_GUEST_ROOT`, else the per-user data directory. The same
/// order as every other layered knob here (flag, then env, then default), with the config file
/// layer deliberately absent until phase 3's config work decides its shape.
pub fn resolve_root(flag: Option<&Path>) -> Result<PathBuf, String> {
    resolve_root_from(
        flag.map(Path::to_path_buf),
        std::env::var_os("TORMONI_GUEST_ROOT"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
}

/// [`resolve_root`] with the environment reads lifted out, so the precedence is a pure decision a
/// test can drive without mutating the test process's environment (which is `unsafe` in this
/// edition).
fn resolve_root_from(
    flag: Option<PathBuf>,
    env_root: Option<OsString>,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, String> {
    let root = flag
        .or_else(|| env_root.map(PathBuf::from))
        .or_else(|| data_dir(xdg_data, home).map(|d| d.join("tormoni/rootfs")));
    let Some(root) = root else {
        return Err(
            "no guest root: pass --root, set TORMONI_GUEST_ROOT, or install a tree at \
             ~/.local/share/tormoni/rootfs (a checkout puts one there with `cargo xtask init`, or \
             builds the full image on Linux with `cargo xtask build-rootfs`)"
                .to_string(),
        );
    };
    if !root.is_dir() {
        return Err(format!(
            "the guest root {} is not a directory (a checkout builds one with \
             `cargo xtask build-rootfs`)",
            root.display()
        ));
    }
    Ok(root)
}

/// `$XDG_DATA_HOME`, else `$HOME/.local/share`, else nothing to derive a default from.
fn data_dir(xdg_data: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    xdg_data
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".local/share")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Precedence is flag, then env, then the data-dir default, and the error path names all
    /// three sources rather than reporting an empty hand.
    #[test]
    fn the_root_resolves_flag_then_env_then_data_dir() {
        let flag = Some(PathBuf::from("/tmp"));
        let env = Some(OsString::from("/nonexistent-env-root"));
        let got = resolve_root_from(flag, env.clone(), None, None).expect("the flag wins");
        assert_eq!(got, Path::new("/tmp"));
        let got = resolve_root(Some(Path::new("/tmp"))).expect("the borrowed form agrees");
        assert_eq!(got, Path::new("/tmp"));

        let err = resolve_root_from(None, env, None, None)
            .expect_err("an env root that is not a directory is refused");
        assert!(err.contains("/nonexistent-env-root"), "{err}");

        let err = resolve_root_from(None, None, None, None)
            .expect_err("nothing to resolve from is an error, not a guess");
        assert!(err.contains("--root"), "{err}");
        assert!(err.contains("TORMONI_GUEST_ROOT"), "{err}");

        let home = Some(OsString::from("/nonexistent-home"));
        let err = resolve_root_from(None, None, None, home)
            .expect_err("the derived default is still checked for existence");
        assert!(err.contains(".local/share/tormoni/rootfs"), "{err}");
    }

    /// The record's posture words are the config's flag words. Two crates spell this vocabulary
    /// because `tormoni-record` is dependency-free, so the pairing is asserted rather than assumed:
    /// a record saying `read-only` for a writable root would misreport what a sandbox could do.
    #[test]
    fn the_record_and_the_config_spell_the_posture_alike() {
        for rootfs in [
            tormoni_supervisor::RootFs::ReadOnly,
            tormoni_supervisor::RootFs::Writable,
        ] {
            assert_eq!(
                rootfs.as_flag(),
                record_rootfs(rootfs).as_word(),
                "{rootfs:?}"
            );
        }
        for net in [tormoni_supervisor::Net::None, tormoni_supervisor::Net::Tsi] {
            assert_eq!(net.as_flag(), record_network(net).as_word(), "{net:?}");
        }
        let hd = std::num::NonZeroU32::new(1920).expect("non-zero");
        let vd = std::num::NonZeroU32::new(1080).expect("non-zero");
        let hz = std::num::NonZeroU32::new(60).expect("non-zero");
        for display in [
            tormoni_supervisor::Display::new(hd, vd),
            tormoni_supervisor::Display::new(hd, vd).with_refresh(hz),
        ] {
            assert_eq!(
                display.as_spec(),
                record_display(display).as_spec(),
                "{display:?}"
            );
        }
    }
}
