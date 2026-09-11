#![deny(clippy::all)]

//! The JS SDK's native half.
//!
//! **This file maps JavaScript arguments onto a `VmConfig` and nothing else.** It does not build a
//! posture, does not decide a guest root, and does not serialise a record: `tormoni_core` owns all
//! three, and each of them was wrong here when this crate owned a copy. What crosses the boundary
//! is a `#[napi(object)]` built by `models::convert_record`, so a renamed field of
//! `tormoni_record::Record` is a compile error rather than a key that quietly stops appearing.

use napi_derive::napi;
use std::ffi::OsString;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;
use tormoni_core::{SandboxOptions, execute_sandbox, resolve_root};
use tormoni_record::Store;
use tormoni_supervisor::{Net, RootFs, VmConfig};

mod models;
use models::convert_record;
pub use models::{JsFile, JsPosture, JsRun};

/// A `String` from the core as the exception a caller catches.
fn failed(message: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(message.to_string())
}

/// A pair arriving as a two-element array, which is how the posture spells mounts and shares.
fn pair(of: &str, raw: &[String]) -> napi::Result<(String, String)> {
    match raw {
        [first, second] => Ok((first.clone(), second.clone())),
        other => Err(failed(format!(
            "each {of} is a two-element array, and one had {} element(s)",
            other.len()
        ))),
    }
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn run_sandbox(
    name: Option<String>,
    command: Vec<String>,
    root: Option<String>,
    vcpus: Option<u32>,
    mem_mib: Option<u32>,
    workdir: Option<String>,
    mounts: Vec<Vec<String>>,
    shares: Vec<Vec<String>>,
    net: Option<String>,
    rootfs: Option<String>,
    env: Vec<String>,
    no_results: bool,
    keep: bool,
    dry_run: bool,
    gpu: bool,
    sound: bool,
) -> napi::Result<JsRun> {
    let Some((program, rest)) = command.split_first() else {
        return Err(failed("command is empty: pass at least the program to run"));
    };

    // The same order the CLI resolves in — the argument, then `$TORMONI_GUEST_ROOT`, then the
    // per-user data directory — because a binding with a default of its own would look somewhere
    // `cargo xtask init` never writes.
    let root = resolve_root(root.as_ref().map(std::path::Path::new)).map_err(failed)?;

    let mut cfg = VmConfig::new(root, program);
    cfg.args = rest.iter().map(OsString::from).collect();
    // The whole `KEY=VALUE` entry goes to the guest. Only the NAME comes back, and the cut is made
    // once in `tormoni_core::posture_of` rather than here.
    cfg.env = env.iter().map(OsString::from).collect();
    cfg.workdir = workdir.map(PathBuf::from);
    cfg.gpu = gpu;
    cfg.sound = sound;
    if let Some(v) = vcpus {
        let v = u8::try_from(v).ok().and_then(NonZeroU8::new);
        cfg.vcpus = v.ok_or_else(|| failed("vcpus is a whole number from 1 to 255"))?;
    }
    if let Some(m) = mem_mib {
        cfg.mem_mib =
            NonZeroU32::new(m).ok_or_else(|| failed("memMib must be at least 1, not 0"))?;
    }
    cfg.mounts = mounts
        .iter()
        .map(|raw| pair("mount", raw).map(|(g, h)| (PathBuf::from(g), PathBuf::from(h))))
        .collect::<napi::Result<_>>()?;
    cfg.shares = shares
        .iter()
        .map(|raw| pair("share", raw).map(|(t, h)| (t, PathBuf::from(h))))
        .collect::<napi::Result<_>>()?;
    // Unknown words are refused rather than silently read as the safe default: a caller who typed
    // "writeable" means to have asked for something, and a sandbox that quietly ignores a posture
    // word is the one defect this product cannot afford.
    if let Some(word) = rootfs {
        cfg.rootfs = match word.as_str() {
            "read-only" => RootFs::ReadOnly,
            "writable" => RootFs::Writable,
            other => {
                return Err(failed(format!(
                    "rootfs is \"read-only\" or \"writable\", not {other:?}"
                )));
            }
        };
    }
    if let Some(word) = net {
        cfg.net = match word.as_str() {
            "none" => Net::None,
            "tsi" => Net::Tsi,
            other => return Err(failed(format!("net is \"none\" or \"tsi\", not {other:?}"))),
        };
    }

    let opts = SandboxOptions {
        name: name.unwrap_or_else(|| format!("run-{}", std::process::id())),
        command,
        cfg,
        results: !no_results,
        keep,
        dry_run,
        // The caller reads the output off the returned object; writing it to this process's stdout
        // as well would put guest bytes into the host program's own streams.
        quiet: true,
    };

    let (record, dir, _code) = execute_sandbox(opts).map_err(failed)?;
    Ok(convert_record(&record, dir.as_ref(), None))
}

/// One filed run, by id or by name — the lookup `tormoni show` makes.
#[napi]
pub fn show(id: String) -> napi::Result<JsRun> {
    let store = Store::open().map_err(failed)?;
    let Some(record) = store.find(&id).map_err(failed)? else {
        return Err(failed(format!("no run {id:?} in the run store")));
    };
    let dir = store.dir_of(&record.id);
    let live = record.is_open();
    // A swept run has a record and no directory left; asking the directory for output it does not
    // have would report empty bytes as though the run had produced none.
    let dir = dir.path().is_dir().then_some(dir);
    Ok(convert_record(&record, dir.as_ref(), Some(live)))
}

/// The filed runs, newest first. Without `all`, only the ones still open.
#[napi]
pub fn runs(all: Option<bool>) -> napi::Result<Vec<JsRun>> {
    let all = all.unwrap_or(false);
    let store = Store::open().map_err(failed)?;
    Ok(store
        .list()
        .map_err(failed)?
        .into_iter()
        .filter(|record| all || record.is_open())
        .map(|record| {
            let live = record.is_open();
            // No directory: reading every run's captured bytes to list them is a directory walk
            // per row, which is the reason `tormoni ls` omits them too.
            convert_record(&record, None, Some(live))
        })
        .collect())
}
