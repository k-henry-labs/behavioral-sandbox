//! The record as JavaScript sees it.
//!
//! `#[napi(object)]` generates both the plain JS object and its TypeScript declaration from these
//! structs, so the `.d.ts` a caller types against is derived from `boxdesk_record::Record` rather
//! than written next to it. [`convert_record`] is the one place a field is read; rename a field
//! upstream and this file stops compiling.
//!
//! **Times and sizes are `i64`, not `u64`.** napi maps `u64` to a BigInt, which is contagious at
//! the call site — `run.startedMs - t0` throws on a BigInt/number mix. Every value here is far
//! inside the 2^53 a JS number holds exactly: epoch milliseconds run out in 287,000 years.

use napi_derive::napi;
use boxdesk_record::{End, Record, RunDir};

#[napi(object)]
pub struct JsPosture {
    pub root: String,
    pub rootfs: String,
    pub mounts: Vec<Vec<String>>,
    pub shares: Vec<Vec<String>>,
    pub network: String,
    pub display: Option<String>,
    pub sound: bool,
    pub gpu: bool,
    pub results: bool,
    pub env: Vec<String>,
    pub vcpus: u32,
    pub mem_mib: u32,
}

#[napi(object)]
pub struct JsFile {
    pub path: String,
    pub size_bytes: i64,
}

#[napi(object)]
pub struct JsRun {
    pub run_id: String,
    pub name: String,
    pub verb: String,
    pub command: Vec<String>,
    pub posture: JsPosture,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub end_kind: Option<String>,
    pub end_code: Option<i64>,
    pub pid: Option<u32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub stdout_bytes: Option<i64>,
    pub stderr_bytes: Option<i64>,
    pub output_truncated: Option<bool>,
    pub files: Option<Vec<JsFile>>,
    pub dir: Option<String>,
    pub live: Option<bool>,
    /// Whether the guest command itself succeeded. A non-zero exit is a result, not an error, so
    /// this is the field a caller branches on.
    pub ok: bool,
}

/// The record, and the directory's contents when there is still a directory.
///
/// Spelled to match `crates/cli/src/json.rs` key for key, so a caller reading a local run and one
/// pulled from a console deserialises into one type.
pub fn convert_record(record: &Record, dir: Option<&RunDir>, live: Option<bool>) -> JsRun {
    let (end_kind, end_code) = match record.end {
        Some(End::Exit(code)) => (Some("exit"), Some(i64::from(code))),
        Some(End::Signal(sig)) => (Some("signal"), Some(i64::from(sig))),
        Some(End::Stopped) => (Some("stopped"), None),
        Some(End::Gone) => (Some("gone"), None),
        Some(End::Failed) => (Some("failed"), None),
        // `End` is `#[non_exhaustive]`: a kind added later reads as one this build cannot name,
        // rather than as a kind it is not.
        Some(_) => (Some("unknown"), None),
        None => (None, None),
    };

    let p = &record.posture;
    let posture = JsPosture {
        root: p.root.display().to_string(),
        rootfs: p.rootfs.as_word().to_string(),
        mounts: p
            .mounts
            .iter()
            .map(|m| vec![m.guest.display().to_string(), m.host.display().to_string()])
            .collect(),
        shares: p
            .shares
            .iter()
            .map(|s| vec![s.tag.clone(), s.host.display().to_string()])
            .collect(),
        network: p.network.as_word().to_string(),
        display: p.display.map(|d| d.as_spec()),
        sound: p.sound,
        gpu: p.gpu,
        results: p.results,
        // Names only. A value never entered the record, so there is none to serve.
        env: p.env.clone(),
        vcpus: u32::from(p.vcpus.get()),
        mem_mib: p.mem_mib.get(),
    };

    let mut run = JsRun {
        run_id: record.id.clone(),
        name: record.name.clone(),
        verb: record.verb.as_word().to_string(),
        command: record.command.clone(),
        posture,
        started_ms: i64::try_from(record.started_ms).unwrap_or(i64::MAX),
        ended_ms: record
            .ended_ms
            .map(|ms| i64::try_from(ms).unwrap_or(i64::MAX)),
        end_kind: end_kind.map(str::to_string),
        end_code,
        pid: record.pid,
        stdout: None,
        stderr: None,
        stdout_bytes: None,
        stderr_bytes: None,
        output_truncated: None,
        files: None,
        dir: None,
        live,
        ok: matches!(record.end, Some(End::Exit(0))),
    };

    if let Some(d) = dir {
        let text = |path: std::path::PathBuf| std::fs::read_to_string(path).unwrap_or_default();
        let size = |path: &std::path::Path| {
            i64::try_from(std::fs::metadata(path).map_or(0, |m| m.len())).unwrap_or(i64::MAX)
        };
        // The same marker `crates/cli/src/json.rs::output_sizes` reads: the capper writes a
        // sibling file rather than editing the stream it already flushed.
        let cut = |path: &std::path::Path| path.with_extension("truncated").exists();
        let (out, err) = (d.stdout(), d.stderr());
        run.stdout = Some(text(d.stdout()));
        run.stderr = Some(text(d.stderr()));
        run.stdout_bytes = Some(size(&out));
        run.stderr_bytes = Some(size(&err));
        run.output_truncated = Some(cut(&out) || cut(&err));
        run.files = Some(
            d.result_files()
                .unwrap_or_default()
                .into_iter()
                .map(|(path, size)| JsFile {
                    path: path.display().to_string(),
                    size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
                })
                .collect(),
        );
        run.dir = Some(d.path().display().to_string());
    }

    run
}
