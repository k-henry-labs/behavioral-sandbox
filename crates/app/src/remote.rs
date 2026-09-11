//! Runs that happened somewhere else.
//!
//! - **The same record, so the same screens.** A console serves a run in the shape this machine
//!   writes one, so a remote run becomes a [`Record`] and every screen renders it unchanged. There
//!   is no second view model and no second output pane.
//! - **Through the `tormoni` beside this app**, like everything else here. `cloud ls`, `cloud show`
//!   and `cloud output` are already the client; this parses what they print.
//! - **The start time comes out of the id.** A run id is `<started_ms>-<name>` by construction, so
//!   the millis are in hand without a calendar. The console serves RFC 3339 timestamps because a
//!   database gave it some, and nothing here has to read one.
//! - **What cannot cross a network is not offered.** A display is a sealed memfd over a local
//!   socket, and stopping or shelling into a sandbox reaches its control socket. Those belong to
//!   the machine a run is on; the notebook reads a remote run rather than driving it.

use std::collections::BTreeSet;

use tormoni_record::{End, Posture, Record, Verb};

/// The ids this window got from a console rather than from its own notebook.
pub(crate) type Remote = BTreeSet<String>;

/// Every run the console holds, as records.
///
/// One page. A console bounds its list, and a notebook showing the newest of somebody's runs is
/// the same thing the local list does with `$TORMONI_RUNS_KEEP`.
pub(crate) fn list(cli: &std::path::Path, console: Option<&str>) -> Result<Vec<Record>, String> {
    let answer = ask(cli, console, &["ls"])?;
    let json: serde_json::Value = serde_json::from_str(&answer)
        .map_err(|e| format!("the console's list is not JSON: {e}"))?;
    let runs = json
        .get("runs")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "the console's list carries no `runs`".to_string())?;
    Ok(runs.iter().filter_map(record_of).collect())
}

/// What a run printed, as the console kept it.
pub(crate) fn output(
    cli: &std::path::Path,
    console: Option<&str>,
    id: &str,
) -> Result<String, String> {
    let answer = ask(cli, console, &["output", id])?;
    let json: serde_json::Value = serde_json::from_str(&answer)
        .map_err(|e| format!("the console's output is not JSON: {e}"))?;
    Ok(["stdout", "stderr"]
        .into_iter()
        .filter_map(|key| json.get(key)?.as_str())
        .collect::<Vec<_>>()
        .join(""))
}

/// Writes one run's archive where asked, for Export.
pub(crate) fn pull(
    cli: &std::path::Path,
    console: Option<&str>,
    id: &str,
    to: &std::path::Path,
) -> Result<String, String> {
    ask(cli, console, &["pull", id, "-o", &to.display().to_string()])
}

/// One `tormoni cloud` call, and what it printed.
fn ask(cli: &std::path::Path, console: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new(cli);
    cmd.arg("cloud").args(args);
    if let Some(console) = console {
        cmd.args(["--console", console]);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("run {}: {e}", cli.display()))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .trim()
            .trim_start_matches("tormoni cloud: ")
            .to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A console's run as a [`Record`], or nothing where it is not one.
///
/// The fields a console and this machine spell alike are taken straight across. Two are not:
/// `started_ms` comes out of the id, and `ended_ms` is left absent because the console serves an
/// RFC 3339 instant and no screen needs a duration badly enough to carry a calendar for it. What
/// decides whether a run is over is [`Record::end`], never the absence of a time.
pub(crate) fn record_of(value: &serde_json::Value) -> Option<Record> {
    let id = value.get("run_id")?.as_str()?.to_string();
    let started_ms = started_of(&id)?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let verb = value
        .get("verb")
        .and_then(serde_json::Value::as_str)
        .and_then(Verb::from_word)
        .unwrap_or(Verb::Run);
    let command = value
        .get("command")
        .and_then(serde_json::Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut record = Record::begin(&name, verb, command, posture_of(value.get("posture")));
    record.id = id;
    record.started_ms = started_ms;
    record.end = end_of(value);
    Some(record)
}

/// The milliseconds a run id opens with. `<started_ms>-<name>` is how every id is built, so this
/// is the record's own number rather than a second one derived from a timestamp.
fn started_of(id: &str) -> Option<u64> {
    id.split_once('-')?.0.parse().ok()
}

/// How a run ended, from the kind and the number the console serves as two fields.
fn end_of(value: &serde_json::Value) -> Option<End> {
    let kind = value.get("end_kind")?.as_str()?;
    let code = || {
        value
            .get("end_code")
            .and_then(serde_json::Value::as_i64)
            .and_then(|c| i32::try_from(c).ok())
            .unwrap_or_default()
    };
    match kind {
        "exit" => Some(End::Exit(code())),
        "signal" => Some(End::Signal(code())),
        "stopped" => Some(End::Stopped),
        "gone" => Some(End::Gone),
        // `failed`, and anything a later console names that this build does not know: a run that
        // is over, however it got there. Reading it as still running would be the worse mistake.
        _ => Some(End::Failed),
    }
}

/// The posture a console serves, in this machine's shape. Every key it does not carry keeps the
/// default, so an older console's answer still opens.
fn posture_of(value: Option<&serde_json::Value>) -> Posture {
    let mut posture = Posture::default();
    let Some(value) = value else {
        return posture;
    };
    let word = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let flag = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_default()
    };
    if let Some(root) = word("root") {
        posture.root = root.into();
    }
    if let Some(rootfs) = word("rootfs").and_then(tormoni_record::Rootfs::from_word) {
        posture.rootfs = rootfs;
    }
    if let Some(network) = word("network").and_then(tormoni_record::Network::from_word) {
        posture.network = network;
    }
    posture.display = word("display").and_then(tormoni_record::DisplayMode::parse);
    posture.sound = flag("sound");
    posture.gpu = flag("gpu");
    posture.results = flag("results");
    posture.mounts = pairs(value.get("mounts"))
        .into_iter()
        .map(|(guest, host)| tormoni_record::Mount::new(guest.into(), host.into()))
        .collect();
    posture.shares = pairs(value.get("shares"))
        .into_iter()
        .map(|(tag, host)| tormoni_record::Share::new(tag, host.into()))
        .collect();
    // Names only, because that is all a record ever held.
    posture.env = value
        .get("env")
        .and_then(serde_json::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if let Some(vcpus) = value
        .get("vcpus")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u8::try_from(n).ok())
        .and_then(std::num::NonZeroU8::new)
    {
        posture.vcpus = vcpus;
    }
    if let Some(mem) = value
        .get("mem_mib")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .and_then(std::num::NonZeroU32::new)
    {
        posture.mem_mib = mem;
    }
    posture
}

/// The `[a, b]` pairs a console serves mounts and shares as.
fn pairs(value: Option<&serde_json::Value>) -> Vec<(String, String)> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_array()?;
                    Some((
                        row.first()?.as_str()?.to_string(),
                        row.get(1)?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A console's answer, as its run store serves one. The path to that store is in another
    /// repository, so it is described rather than linked.
    fn served() -> serde_json::Value {
        serde_json::json!({
            "run_id": "1789085063489-nightly",
            "name": "nightly",
            "verb": "run",
            "command": ["sh", "-c", "echo hi"],
            "started_at": "2026-09-10T18:27:17.726133Z",
            "ended_at": "2026-09-10T18:27:18.000000Z",
            "end_kind": "exit",
            "end_code": 3,
            "posture": {
                "root": "/home/someone/.local/share/tormoni/rootfs",
                "rootfs": "writable",
                "mounts": [["/mnt", "/srv/code"]],
                "shares": [["data", "/srv/data"]],
                "network": "tsi",
                "display": null,
                "sound": false,
                "gpu": true,
                "results": true,
                "env": ["API_KEY", "CI"],
                "vcpus": 4,
                "mem_mib": 2048
            }
        })
    }

    /// **A console's run becomes this machine's record**, so every screen renders a remote run
    /// with the code that renders a local one. The posture especially: what a sandbox somewhere
    /// else could reach is shown the same way, or a reader would have two things to learn.
    #[test]
    fn a_served_run_is_the_record_this_machine_would_have_written() {
        let record = record_of(&served()).expect("a record");
        assert_eq!(record.id, "1789085063489-nightly");
        assert_eq!(record.name, "nightly");
        assert_eq!(record.verb, Verb::Run);
        assert_eq!(record.command, ["sh", "-c", "echo hi"]);
        assert_eq!(record.end, Some(End::Exit(3)));
        assert!(!record.is_open(), "a run that ended is not still going");

        let p = &record.posture;
        assert_eq!(p.rootfs, tormoni_record::Rootfs::Writable);
        assert_eq!(p.network, tormoni_record::Network::Tsi);
        assert!(p.gpu && p.results && !p.sound);
        assert_eq!(p.vcpus.get(), 4);
        assert_eq!(p.mem_mib.get(), 2048);
        assert_eq!(p.env, ["API_KEY", "CI"], "names, and never values");
        assert_eq!(p.mounts.len(), 1);
        assert_eq!(p.mounts[0].guest.display().to_string(), "/mnt");
        assert_eq!(p.shares[0].tag, "data");
    }

    /// **The start time is the id's, not a timestamp's.** A run id opens with the milliseconds it
    /// began at, so a console serving RFC 3339 costs this window no calendar. The list sorts on
    /// this number, so getting it from anywhere else would be a second source for one fact.
    #[test]
    fn the_start_time_comes_out_of_the_id() {
        assert_eq!(started_of("1789085063489-nightly"), Some(1_789_085_063_489));
        assert_eq!(
            started_of("1789085063489-run-with-dashes"),
            Some(1_789_085_063_489)
        );
        assert_eq!(started_of("not-a-time-nightly"), None);
        assert_eq!(started_of("nightly"), None);

        let record = record_of(&served()).expect("a record");
        assert_eq!(record.started_ms, 1_789_085_063_489);
        // Left absent on purpose: the console's `ended_at` is an RFC 3339 instant, and `end` is
        // what says a run is over.
        assert_eq!(record.ended_ms, None);
    }

    /// An answer that is not a run is dropped rather than shown as an empty row, and a console
    /// that names an end this build does not know is read as over rather than as running.
    #[test]
    fn an_answer_that_is_not_a_run_is_left_out() {
        assert!(record_of(&serde_json::json!({})).is_none());
        assert!(record_of(&serde_json::json!({ "run_id": "no-millis-here" })).is_none());

        let mut odd = served();
        odd["end_kind"] = serde_json::json!("something-later");
        let record = record_of(&odd).expect("a record");
        assert_eq!(record.end, Some(End::Failed));
        assert!(
            !record.is_open(),
            "an end this build cannot name is still an end"
        );

        // A run the console says is still going has no end, and reads as open.
        let mut going = served();
        going["end_kind"] = serde_json::Value::Null;
        assert!(record_of(&going).expect("a record").is_open());
    }

    /// A console that says nothing about a posture field leaves this machine's default, so an
    /// older console's answer still opens rather than refusing.
    #[test]
    fn a_posture_a_console_did_not_spell_keeps_the_default() {
        let bare = record_of(&serde_json::json!({
            "run_id": "1789085063489-bare",
            "name": "bare",
            "verb": "up",
        }))
        .expect("a record");
        assert_eq!(bare.verb, Verb::Up);
        assert_eq!(bare.posture, Posture::default());
        assert!(bare.posture.env.is_empty());
    }
}
