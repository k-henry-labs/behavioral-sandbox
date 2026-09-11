// The whole tree is `#![forbid(unsafe_code)]`, which an FFI crate cannot be: PyO3's macros
// expand into `unsafe fn` bodies. `deny(unsafe_op_in_unsafe_fn)` is the strongest thing that
// still compiles, and it holds every `unsafe` to an explicit block with a reason. There is no
// `unsafe` token in this file; the lint covers what the macros expand to.
//
// PyO3 0.22 could not even manage that — its expansion tripped this lint nine times in code
// this crate does not write. The 0.24.1 bump that RUSTSEC-2025-0020 forced also fixed that, so
// the `allow` this line replaced is gone.
//
// Note that `every_crate_forbids_unsafe` never sees this file: that lint walks `crates/` only,
// so the tree's guarantee stops at the workspace boundary rather than covering the SDKs.
#![deny(unsafe_op_in_unsafe_fn)]

//! The Python SDK's native half.
//!
//! **This file maps Python arguments onto a `VmConfig` and nothing else.** It does not build a
//! posture, does not decide a guest root, and does not serialise a record: `tormoni_core` owns
//! all three, and each of them was wrong here when this crate owned a copy. What crosses the
//! boundary is a `#[pyclass]` built by `models::convert_record`, so a renamed field of
//! `tormoni_record::Record` is a compile error rather than a key that quietly stops appearing.

use pyo3::prelude::*;
use std::ffi::OsString;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;
use tormoni_core::{SandboxOptions, execute_sandbox, resolve_root};
use tormoni_record::Store;
use tormoni_supervisor::{Net, RootFs, VmConfig};

mod models;
use models::{PyFile, PyPosture, PyRun, convert_record};

/// Turns a `String` error from the core into the exception a caller sees.
fn failed(message: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(message.to_string())
}

#[pyfunction]
#[pyo3(signature = (name, command, root, vcpus, mem_mib, workdir, mounts, shares, net, rootfs, env, no_results, keep, dry_run, gpu, sound))]
#[allow(clippy::too_many_arguments)]
fn run_sandbox(
    name: Option<String>,
    command: Vec<String>,
    root: Option<String>,
    vcpus: Option<u8>,
    mem_mib: Option<u32>,
    workdir: Option<String>,
    mounts: Vec<(String, String)>,
    shares: Vec<(String, String)>,
    net: Option<String>,
    rootfs: Option<String>,
    env: Vec<String>,
    no_results: bool,
    keep: bool,
    dry_run: bool,
    gpu: bool,
    sound: bool,
) -> PyResult<PyRun> {
    let Some((program, rest)) = command.split_first() else {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "command is empty: pass at least the program to run",
        ));
    };

    // The same order the CLI resolves in — the argument, then `$TORMONI_GUEST_ROOT`, then the
    // per-user data directory — because a binding with a default of its own would look somewhere
    // `cargo xtask init` never writes.
    let root = resolve_root(root.as_ref().map(std::path::Path::new)).map_err(failed)?;

    let mut cfg = VmConfig::new(root, program);
    cfg.args = rest.iter().map(OsString::from).collect();
    // The whole `KEY=VALUE` entry goes to the guest. Only the NAME comes back, and the cut is
    // made once in `tormoni_core::posture_of` rather than here.
    cfg.env = env.iter().map(OsString::from).collect();
    cfg.workdir = workdir.map(PathBuf::from);
    cfg.gpu = gpu;
    cfg.sound = sound;
    if let Some(v) = vcpus {
        cfg.vcpus = NonZeroU8::new(v).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("vcpus must be at least 1, not 0")
        })?;
    }
    if let Some(m) = mem_mib {
        cfg.mem_mib = NonZeroU32::new(m).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("mem_mib must be at least 1, not 0")
        })?;
    }
    cfg.mounts = mounts
        .into_iter()
        .map(|(guest, host)| (PathBuf::from(guest), PathBuf::from(host)))
        .collect();
    cfg.shares = shares
        .into_iter()
        .map(|(tag, host)| (tag, PathBuf::from(host)))
        .collect();
    // Unknown words are refused rather than silently read as the safe default: a caller who
    // typed "writeable" means to have asked for something, and a sandbox that quietly ignores a
    // posture word is the one defect this product cannot afford.
    if let Some(word) = rootfs {
        cfg.rootfs = match word.as_str() {
            "read-only" => RootFs::ReadOnly,
            "writable" => RootFs::Writable,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "rootfs is \"read-only\" or \"writable\", not {other:?}"
                )));
            }
        };
    }
    if let Some(word) = net {
        cfg.net = match word.as_str() {
            "none" => Net::None,
            "tsi" => Net::Tsi,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "net is \"none\" or \"tsi\", not {other:?}"
                )));
            }
        };
    }

    let opts = SandboxOptions {
        name: name.unwrap_or_else(|| format!("run-{}", std::process::id())),
        command,
        cfg,
        results: !no_results,
        keep,
        dry_run,
        // The caller reads the output off the returned object; writing it to this process's
        // stdout as well would put guest bytes into the host program's own streams.
        quiet: true,
    };

    let (record, dir, _code) = execute_sandbox(opts).map_err(failed)?;
    Ok(convert_record(&record, dir.as_ref(), None))
}

/// One filed run, by id or by name — the lookup `tormoni show` makes.
#[pyfunction]
fn show(id: String) -> PyResult<PyRun> {
    let store = Store::open().map_err(failed)?;
    let Some(record) = store.find(&id).map_err(failed)? else {
        return Err(pyo3::exceptions::PyKeyError::new_err(format!(
            "no run {id:?} in the run store"
        )));
    };
    let dir = store.dir_of(&record.id);
    let live = record.is_open();
    // A swept run has a record and no directory left; asking the directory for output it does not
    // have would report empty bytes as though the run had produced none.
    let dir = dir.path().is_dir().then_some(dir);
    Ok(convert_record(&record, dir.as_ref(), Some(live)))
}

/// The filed runs, newest first. Without `all`, only the ones still open.
#[pyfunction]
#[pyo3(signature = (all=false))]
fn runs(all: bool) -> PyResult<Vec<PyRun>> {
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

#[pymodule]
fn _tormoni_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_sandbox, m)?)?;
    m.add_function(wrap_pyfunction!(show, m)?)?;
    m.add_function(wrap_pyfunction!(runs, m)?)?;
    m.add_class::<PyRun>()?;
    m.add_class::<PyPosture>()?;
    m.add_class::<PyFile>()?;
    Ok(())
}
