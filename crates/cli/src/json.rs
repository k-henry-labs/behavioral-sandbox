//! `--json`: one run, as a machine reads it.
//!
//! - **The record is the model, not a second one.** Every key here is a field of
//!   `tormoni_record::Record` or of its posture. Nothing is invented for the wire, so there is one
//!   vocabulary for a run whether it is read here or pulled back from a console.
//! - **The same keys the console serves.** `posture`, `command`, `end_kind`, `end_code`,
//!   `stdout_bytes`, `stderr_bytes`, `output_truncated` and `files` are spelled as the run store
//!   spells them, so an SDK deserializes local and remote into one type.
//! - **Times are the record's.** `started_ms` and `ended_ms` are the epoch milliseconds the record
//!   holds. A console serves the same instants as RFC 3339 `started_at`/`ended_at`, because a
//!   database gave it timestamps; converting here would need a calendar this binary has no reason
//!   to carry.
//! - **Output is inline and capped.** `stdout` and `stderr` are the captured text, which is what
//!   makes `run(...).stdout` work in a client. `output_truncated` says when the cap cut it, so a
//!   short string is never mistaken for the whole of one.

use serde_json::{Value, json};
use tormoni_record::{End, Record, RunDir};

/// One run with its captured output, for `run --json` and `show --json`.
pub(crate) fn run_json(record: &Record, dir: &RunDir) -> Value {
    let mut out = record_json(record);
    let text =
        |path: std::path::PathBuf| -> String { std::fs::read_to_string(path).unwrap_or_default() };
    let files: Vec<Value> = dir
        .result_files()
        .unwrap_or_default()
        .into_iter()
        .map(|(path, size)| json!({ "path": path.display().to_string(), "size_bytes": size }))
        .collect();
    if let Some(object) = out.as_object_mut() {
        object.insert("stdout".into(), Value::from(text(dir.stdout())));
        object.insert("stderr".into(), Value::from(text(dir.stderr())));
        object.insert("files".into(), Value::from(files));
        object.insert("dir".into(), Value::from(dir.path().display().to_string()));
    }
    out
}

/// One run without its output, for a list: reading every run's captured bytes to print a list of
/// them is a directory walk per row.
pub(crate) fn record_json(record: &Record) -> Value {
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
    json!({
        "run_id": record.id,
        "name": record.name,
        "verb": record.verb.as_word(),
        "command": record.command,
        "posture": posture_json(&record.posture),
        "started_ms": record.started_ms,
        "ended_ms": record.ended_ms,
        "end_kind": end_kind,
        "end_code": end_code,
        "pid": record.pid,
    })
}

/// The posture, spelled as the run store spells it so one type reads both.
fn posture_json(p: &tormoni_record::Posture) -> Value {
    json!({
        "root": p.root.display().to_string(),
        "rootfs": p.rootfs.as_word(),
        "mounts": p.mounts.iter()
            .map(|m| json!([m.guest.display().to_string(), m.host.display().to_string()]))
            .collect::<Vec<_>>(),
        "shares": p.shares.iter()
            .map(|s| json!([s.tag, s.host.display().to_string()]))
            .collect::<Vec<_>>(),
        "network": p.network.as_word(),
        "display": p.display.map(|d| d.as_spec()),
        "sound": p.sound,
        "gpu": p.gpu,
        "results": p.results,
        // Names only. A value never entered the record, so there is none to serve.
        "env": p.env,
        "vcpus": p.vcpus.get(),
        "mem_mib": p.mem_mib.get(),
    })
}

/// The sizes of what a run captured, and whether the cap cut either.
pub(crate) fn output_sizes(dir: &RunDir) -> (u64, u64, bool) {
    let size = |path: &std::path::Path| std::fs::metadata(path).map_or(0, |m| m.len());
    let cut = |path: &std::path::Path| path.with_extension("truncated").exists();
    let (out, err) = (dir.stdout(), dir.stderr());
    (size(&out), size(&err), cut(&out) || cut(&err))
}

/// A run with its output sizes filled in: what both `run --json` and `show --json` print.
pub(crate) fn complete(record: &Record, dir: &RunDir) -> Value {
    let mut value = run_json(record, dir);
    let (stdout_bytes, stderr_bytes, truncated) = output_sizes(dir);
    if let Some(object) = value.as_object_mut() {
        object.insert("stdout_bytes".into(), Value::from(stdout_bytes));
        object.insert("stderr_bytes".into(), Value::from(stderr_bytes));
        object.insert("output_truncated".into(), Value::from(truncated));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::{NonZeroU8, NonZeroU32};

    fn record() -> Record {
        let mut posture = tormoni_record::Posture::new(
            "/img".into(),
            NonZeroU8::MIN,
            NonZeroU32::new(512).expect("non-zero"),
        );
        posture.results = true;
        posture.env = vec!["CI".to_string()];
        let mut record = Record::begin(
            "shaped",
            tormoni_record::Verb::Run,
            vec!["sh".into(), "-c".into(), "echo hi".into()],
            posture,
        );
        record.id = "1756860007123-shaped".to_string();
        record
    }

    /// **Every key is one the console serves too**, so a client deserializes a local run and a
    /// pulled one into the same type. The list is spelled out rather than sampled: a key that
    /// quietly changes name is the drift this guards.
    #[test]
    fn the_shape_is_the_one_a_console_serves() {
        let value = record_json(&record());
        let object = value.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "command",
                "end_code",
                "end_kind",
                "ended_ms",
                "name",
                "pid",
                "posture",
                "run_id",
                "started_ms",
                "verb"
            ]
        );

        let posture = object["posture"].as_object().expect("a posture");
        let mut posture_keys: Vec<&str> = posture.keys().map(String::as_str).collect();
        posture_keys.sort_unstable();
        assert_eq!(
            posture_keys,
            [
                "display", "env", "gpu", "mem_mib", "mounts", "network", "results", "root",
                "rootfs", "shares", "sound", "vcpus"
            ]
        );
        assert_eq!(posture["network"], "none");
        assert_eq!(posture["rootfs"], "read-only");
        assert_eq!(posture["env"], json!(["CI"]));
        assert_eq!(object["command"], json!(["sh", "-c", "echo hi"]));
    }

    /// How a run ended is two fields, never one string to parse: a kind, and a number where the
    /// kind carries one.
    #[test]
    fn an_end_is_a_kind_and_a_number() {
        let cases = [
            (End::Exit(3), "exit", Some(3)),
            (End::Signal(9), "signal", Some(9)),
            (End::Stopped, "stopped", None),
            (End::Failed, "failed", None),
            (End::Gone, "gone", None),
        ];
        for (end, kind, code) in cases {
            let mut record = record();
            record.finish(end);
            let value = record_json(&record);
            assert_eq!(value["end_kind"], kind);
            assert_eq!(value["end_code"], code.map_or(Value::Null, Value::from));
            assert!(value["ended_ms"].is_number(), "an ended run says when");
        }
        // A run still going says so with nulls rather than by omitting the keys, so a client
        // reads one shape.
        let open = record_json(&record());
        assert_eq!(open["end_kind"], Value::Null);
        assert_eq!(open["ended_ms"], Value::Null);
    }
}
