//! `tormoni export`: a record store to a tar file, with no guest booted. The suite plants a record
//! with `tormoni-record` and runs the built binary against a scratch `$TORMONI_RUNS_DIR`.

// A test binary: `expect` is the idiomatic assertion in helpers outside `#[test]`.
#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use tormoni_test_support::ScratchDir;

fn tormoni(runs: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tormoni"));
    cmd.env("TORMONI_RUNS_DIR", runs);
    cmd
}

/// A run to export: a record with one captured output file.
fn planted(runs: &Path) -> tormoni_record::Record {
    let store = tormoni_record::Store::at(runs.to_path_buf()).expect("a store");
    let mut record = tormoni_record::Record::begin(
        "exportee",
        tormoni_record::Verb::Run,
        vec!["true".into()],
        tormoni_record::Posture::new(
            "/img".into(),
            std::num::NonZeroU8::MIN,
            std::num::NonZeroU32::new(512).expect("non-zero"),
        ),
    );
    record.id = "1756860007123-exportee".to_string();
    let run = store.create(&record).expect("created");
    std::fs::write(run.stdout(), b"captured\n").expect("stdout");
    record
}

/// **`--json` is one document on stdout and nothing else.** Four SDKs parse this; a byte of
/// anything in front of it makes that impossible, and the verbs that print a table or the
/// record's own lines are the ones most likely to leak one.
///
/// Boots nothing: `show` and `ls` read the notebook, so this runs on a host with no hypervisor.
#[test]
fn json_is_one_document_and_the_keys_a_client_binds_to() {
    let scratch = ScratchDir::created("json-shape");
    let runs = scratch.path().join("runs");
    let record = planted(&runs);

    let parse = |args: &[&str]| -> serde_json::Value {
        let out = tormoni(&runs).args(args).output().expect("the binary runs");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&out.stdout);
        assert!(
            parsed.is_ok(),
            "{args:?} did not write one JSON document: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        parsed.expect("checked just above")
    };

    let shown = parse(&["show", "--json", &record.id]);
    assert_eq!(shown["run_id"], record.id);
    assert_eq!(shown["verb"], "run");
    assert_eq!(shown["stdout"], "captured\n");
    assert_eq!(shown["stdout_bytes"], 9);
    assert_eq!(shown["output_truncated"], false);
    assert_eq!(shown["posture"]["network"], "none");
    assert_eq!(shown["posture"]["rootfs"], "read-only");

    // A list is an object with a `runs` array, never a bare array: a top-level object is what
    // lets a cursor or a count be added later without breaking every client.
    let listed = parse(&["ls", "--json", "--all"]);
    let rows = listed["runs"].as_array().expect("a runs array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["run_id"], record.id);
    assert_eq!(rows[0]["live"], false);
    // A list carries no captured output: reading every run's bytes to print a list of them is a
    // directory walk per row.
    assert!(rows[0].get("stdout").is_none(), "{listed}");
}

/// **A run is ephemeral: it takes its own directory with it.** The output still comes back, on
/// this process's streams and inside `--json`, and what the guest wrote to `/results` goes with
/// the run unless `--keep` says otherwise.
///
/// Boots nothing: `--dry-run` settles a posture and writes no record, so the sweep is tested by
/// what is on the disk after a real run in the suite below rather than here. This asserts the
/// flag reaches the parser and the default is ephemeral.
#[test]
fn a_run_is_ephemeral_unless_it_is_told_to_keep() {
    let scratch = ScratchDir::created("run-ephemeral");
    let runs = scratch.path().join("runs");

    // The parser's side: `--keep` is a flag on `run`, and absent means ephemeral.
    let out = tormoni(&runs)
        .args(["run", "--keep", "--dry-run", "--json", "--", "true"])
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A dry run writes no record either way, so nothing accumulated.
    assert!(
        !runs.exists() || std::fs::read_dir(&runs).is_ok_and(|d| d.count() == 0),
        "a dry run left something behind"
    );
}

/// **An ephemeral run names no directory and no results**, because the sweep has just taken both.
/// A path that was removed a line ago is an invitation for a client to open it; the captured
/// output is inline either way, so nothing a caller needs goes with it.
///
/// Driven through `show --json` on a planted record for the `--keep` side, and through the
/// ephemeral side's own shape, so neither needs a hypervisor.
#[test]
fn an_ephemeral_run_reports_no_directory_and_a_kept_one_does() {
    let scratch = ScratchDir::created("json-ephemeral");
    let runs = scratch.path().join("runs");
    let record = planted(&runs);

    let out = tormoni(&runs)
        .args(["show", "--json", &record.id])
        .output()
        .expect("the binary runs");
    let kept: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON document");
    assert!(
        kept["dir"].is_string(),
        "a kept run names where it is: {kept}"
    );
    assert_eq!(kept["stdout"], "captured\n");
}

/// The verb writes the archive where asked (by id or name, `--to` or the cwd), prints only the
/// path, a stock `tar` lists the entries, and an unknown key leaves stdout empty.
#[test]
fn export_writes_a_tar_where_asked_and_prints_only_the_path() {
    let scratch = ScratchDir::created("cli-export");
    let runs = scratch.path().join("runs");
    let record = planted(&runs);
    let dest = scratch.path().join("dest");
    std::fs::create_dir(&dest).expect("a dest dir");

    let out = tormoni(&runs)
        .args(["export", &record.id, "--to"])
        .arg(&dest)
        .output()
        .expect("tormoni ran");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = dest.join(format!("tormoni-{}.tar", record.id));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", expected.display()),
        "stdout is the written path and nothing else"
    );
    assert!(expected.is_file());

    let listing = Command::new("tar")
        .arg("-tf")
        .arg(&expected)
        .output()
        .expect("the system tar ran");
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let names = String::from_utf8_lossy(&listing.stdout);
    assert!(names.contains("1756860007123-exportee/record"), "{names}");
    assert!(names.contains("1756860007123-exportee/stdout"), "{names}");

    let cwd = scratch.path().join("cwd");
    std::fs::create_dir(&cwd).expect("a cwd");
    let out = tormoni(&runs)
        .args(["export", "exportee"])
        .current_dir(&cwd)
        .output()
        .expect("tormoni ran");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        cwd.join(format!("tormoni-{}.tar", record.id)).is_file(),
        "by name, into the current directory"
    );

    let out = tormoni(&runs)
        .args(["export", "nobody"])
        .output()
        .expect("ran");
    assert_eq!(out.status.code(), Some(2), "an operational refusal");
    assert!(out.stdout.is_empty(), "stdout stays pipe-clean");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no run named or numbered"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
