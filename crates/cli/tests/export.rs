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
