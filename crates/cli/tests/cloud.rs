//! `tormoni cloud`: the console verbs, against a console.
//!
//! **The round trip is the property.** A push sends the bytes `tormoni export` writes and a pull
//! brings those bytes back; if they ever differ, the archive is no longer the wire format and the
//! cloud's own fixture is reading something this build does not write.
//!
//! That test needs a console, so it is `#[ignore]`d and driven by hand or by CI with one to point
//! at. What runs in the gate is everything that needs no server: the parser, and the refusals a
//! machine with no token makes before it opens a socket.

// A test binary: `expect` is the idiomatic assertion in helpers outside `#[test]`.
#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use tormoni_test_support::ScratchDir;

/// The console to drive the live test against, and the token to present.
const CONSOLE_ENV: &str = "TORMONI_CONSOLE";
const TOKEN_ENV: &str = "TORMONI_TOKEN";

/// The exit codes `cloud` gives a refusal, from `src/cloud.rs`.
const EXIT_OPERATIONAL: i32 = 2;

fn tormoni(runs: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tormoni"));
    cmd.env("TORMONI_RUNS_DIR", runs);
    cmd
}

/// A run to push: a record with one captured output file and one result.
fn planted(runs: &Path, id: &str) -> tormoni_record::Record {
    let store = tormoni_record::Store::at(runs.to_path_buf()).expect("a store");
    let mut record = tormoni_record::Record::begin(
        "pushee",
        tormoni_record::Verb::Run,
        vec!["true".into()],
        tormoni_record::Posture::new(
            "/img".into(),
            std::num::NonZeroU8::MIN,
            std::num::NonZeroU32::new(512).expect("non-zero"),
        ),
    );
    record.id = id.to_string();
    record.finish(tormoni_record::End::Exit(0));
    let run = store.create(&record).expect("created");
    store.save(&record).expect("the ended record");
    std::fs::write(run.stdout(), b"captured\n").expect("stdout");
    std::fs::write(run.results().join("note.txt"), b"kept\n").expect("a result");
    record
}

fn sha256(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("shasum runs on both platforms this builds for");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_string()
}

// ── Without a console ────────────────────────────────────────────────────────

/// A machine with no token says where to get one and never opens a socket.
///
/// `$HOME` and `$XDG_DATA_HOME` are pointed at an empty scratch dir, so this is the answer for a
/// person who has not signed in rather than for whoever is running the suite.
#[test]
fn a_machine_that_never_signed_in_is_told_where_to() {
    let scratch = ScratchDir::created("cloud-no-token");
    let runs = scratch.path().join("runs");
    let out = tormoni(&runs)
        .args(["cloud", "ls", "--console", "http://127.0.0.1:9"])
        .env("HOME", scratch.path())
        .env("XDG_DATA_HOME", scratch.path())
        .env_remove(TOKEN_ENV)
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("not signed in"), "{said}");
    assert!(said.contains(TOKEN_ENV), "names the way out: {said}");
}

/// An address that is not a console is refused before a request is built, so a typo cannot send
/// a bearer somewhere that was never meant to have one.
#[test]
fn an_origin_that_is_not_an_http_address_is_refused() {
    let scratch = ScratchDir::created("cloud-bad-origin");
    let out = tormoni(&scratch.path().join("runs"))
        .args(["cloud", "ls", "--console", "tormoni.ai"])
        .env(TOKEN_ENV, "tor_never_sent")
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("http://"), "{said}");
}

/// A console that is not listening is a sentence, not a panic (design rule 4).
#[test]
fn a_console_that_is_not_listening_says_so() {
    let scratch = ScratchDir::created("cloud-unreachable");
    let out = tormoni(&scratch.path().join("runs"))
        .args(["cloud", "ls", "--console", "http://127.0.0.1:9"])
        .env(TOKEN_ENV, "tor_never_answered")
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("could not be reached"), "{said}");
    assert!(!said.contains("panicked"), "{said}");
}

// ── With one ─────────────────────────────────────────────────────────────────

/// **A pulled archive is the bytes that were pushed.** The whole point of storing the export
/// whole: push, pull, and compare the digests of the two files.
///
/// Also drives the rest of the round trip, because each step is the next one's fixture: the
/// second push must upload nothing (it asked `HEAD` first), the run must appear in the list, and
/// the delete must take it out again.
///
/// By hand, against a console:
/// `TORMONI_CONSOLE=http://localhost:3000 TORMONI_TOKEN=tor_… cargo test -p tormoni --test cloud -- --ignored --nocapture`
#[test]
#[ignore = "needs a console: set $TORMONI_CONSOLE and $TORMONI_TOKEN"]
fn a_pulled_archive_is_the_bytes_that_were_pushed() {
    let console = std::env::var(CONSOLE_ENV).expect("a console to drive");
    let token = std::env::var(TOKEN_ENV).expect("a token to present");
    let scratch = ScratchDir::created("cloud-round-trip");
    let runs = scratch.path().join("runs");
    // Unique per invocation: a run id the console already holds would answer 200 on the first
    // push and prove nothing about the upload.
    let id = format!("{}-pushee", tormoni_record::now_ms());
    planted(&runs, &id);

    let cloud = |args: &[&str]| -> std::process::Output {
        tormoni(&runs)
            .arg("cloud")
            .args(args)
            .args(["--console", &console])
            .env(TOKEN_ENV, &token)
            .output()
            .expect("the binary runs")
    };
    let ok = |out: &std::process::Output, what: &str| {
        assert!(
            out.status.success(),
            "{what}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let pushed = cloud(&["push", &id]);
    ok(&pushed, "push");

    // The second push is the one that must cost nothing: `HEAD` answered 200, so no archive left
    // this machine.
    let again = cloud(&["push", &id]);
    ok(&again, "push again");
    let said = String::from_utf8_lossy(&again.stdout);
    assert!(
        said.contains("already stored"),
        "the upload was spent: {said}"
    );

    let listed = cloud(&["ls"]);
    ok(&listed, "ls");
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains(&id),
        "the pushed run is not in the list"
    );

    let exported = scratch.path().join("local.tar");
    let export = tormoni(&runs)
        .args(["export", &id, "--to"])
        .arg(&exported)
        .output()
        .expect("the binary runs");
    ok(&export, "export");

    let pulled = scratch.path().join("pulled.tar");
    let pull = cloud(&["pull", &id, "-o", &pulled.display().to_string()]);
    ok(&pull, "pull");

    assert_eq!(
        sha256(&exported),
        sha256(&pulled),
        "the archive that came back is not the archive that went"
    );

    ok(&cloud(&["rm", &id]), "rm");
    let after = cloud(&["ls"]);
    ok(&after, "ls after rm");
    assert!(
        !String::from_utf8_lossy(&after.stdout).contains(&id),
        "the removed run is still listed"
    );
}

/// A run this console does not hold is a 404 with the server's own sentence and its own exit
/// code, so a script tells it from a refusal without reading English.
#[test]
#[ignore = "needs a console: set $TORMONI_CONSOLE and $TORMONI_TOKEN"]
fn a_run_the_console_does_not_hold_exits_four() {
    let console = std::env::var(CONSOLE_ENV).expect("a console to drive");
    let token = std::env::var(TOKEN_ENV).expect("a token to present");
    let scratch = ScratchDir::created("cloud-404");
    let out = tormoni(&scratch.path().join("runs"))
        .args(["cloud", "show", "1-nothing-here", "--console", &console])
        .env(TOKEN_ENV, token)
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(4), "the not-found code");
    assert!(
        !String::from_utf8_lossy(&out.stderr).trim().is_empty(),
        "a refusal says something"
    );
}
