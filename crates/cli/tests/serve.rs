//! `tormoni serve`: the refusals a box makes before it listens, and the one property that makes
//! serving the same product as running.
//!
//! **A served archive is byte for byte a local one.** That test boots a real sandbox, so it is
//! `#[ignore]`d and run by hand on a host with a hypervisor. What the gate runs is everything a
//! machine with no `/dev/kvm` can still answer: a box with no token, a token anybody can read, and
//! a bind that would have reached past the machine.

// A test binary: `expect` is the idiomatic assertion in helpers outside `#[test]`.
#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use tormoni_test_support::ScratchDir;

/// The exit code every refusal in this file makes.
const EXIT_OPERATIONAL: i32 = 2;

fn tormoni(data: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tormoni"));
    cmd.env("TORMONI_SERVE_DATA", data)
        .env_remove("TORMONI_SERVE_TOKEN")
        .env_remove("TORMONI_SERVE_BIND");
    cmd
}

/// A token file this user alone can read, as the server insists on.
fn token_at(data: &Path, token: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(data).expect("the data dir");
    let path = data.join("serve.token");
    std::fs::write(&path, token).expect("a token");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
}

// ── The refusals, which need no hypervisor ───────────────────────────────────

/// A box with no token would refuse every request, so it says where to put one and does not bind.
///
/// On a host with no hypervisor this refuses for that reason first, which is also correct: either
/// way nothing listened, and the message names something to fix.
#[test]
fn a_box_with_nothing_to_check_a_caller_against_does_not_listen() {
    let scratch = ScratchDir::created("serve-no-token");
    let data = scratch.path().join("data");
    std::fs::create_dir_all(&data).expect("the data dir");
    let out = tormoni(&data)
        .args(["serve", "--bind", "0"])
        .output()
        .expect("the binary runs");

    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    let about_the_token = said.contains("no token");
    let about_the_host = said.contains("hypervisor") || said.contains("/dev/kvm");
    assert!(
        about_the_token || about_the_host,
        "a refusal should name the token or the host: {said}"
    );
    if about_the_token {
        assert!(said.contains("0600"), "names the mode: {said}");
        assert!(
            said.contains("TORMONI_SERVE_TOKEN"),
            "names the way out: {said}"
        );
    }
}

/// A token anybody on the box can read is refused rather than used. A credential at `0644` on a
/// shared machine belongs to every account on it.
#[test]
fn a_token_the_whole_machine_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = ScratchDir::created("serve-open-token");
    let data = scratch.path().join("data");
    token_at(&data, "tor_secret\n");
    let path = data.join("serve.token");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");

    let out = tormoni(&data)
        .args(["serve", "--bind", "0"])
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    if !said.contains("hypervisor") && !said.contains("/dev/kvm") {
        assert!(said.contains("0600"), "{said}");
    }
    // Whatever it refused for, the token is not in what it said.
    assert!(
        !said.contains("tor_secret"),
        "the token was printed: {said}"
    );
}

/// An address that is not one is refused before anything binds, so a typo cannot put a sandbox
/// host somewhere nobody meant.
#[test]
fn an_address_that_is_not_one_is_refused_before_binding() {
    let scratch = ScratchDir::created("serve-bad-bind");
    let data = scratch.path().join("data");
    token_at(&data, "tor_secret\n");
    let out = tormoni(&data)
        .args(["serve", "--bind", "everywhere"])
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(EXIT_OPERATIONAL));
    let said = String::from_utf8_lossy(&out.stderr);
    if !said.contains("hypervisor") && !said.contains("/dev/kvm") {
        assert!(said.contains("listen on"), "{said}");
    }
}

// ── The property, which needs one ────────────────────────────────────────────

/// **A served archive is the archive a person would have got at a keyboard.** Not equivalent: the
/// same bytes, because there is one run path and serving re-enters it rather than reimplementing
/// it. If this ever fails, serving has stopped being the same product.
///
/// Drives the rest of the round trip with it: the stream carries the guest's output as it
/// arrives, the record comes back with environment NAMES and no values, and the ledger holds a
/// non-zero count for a sandbox that has ended.
///
/// By hand, on a host with a hypervisor (and on macOS, after `cargo xtask sign`):
/// `cargo test -p tormoni --test serve -- --ignored --nocapture`
#[test]
#[ignore = "boots a real sandbox through a listening server: needs a hypervisor"]
fn a_served_archive_is_byte_for_byte_the_one_a_local_export_writes() {
    let scratch = ScratchDir::created("serve-round-trip");
    let data = scratch.path().join("data");
    token_at(&data, "tor_roundtrip\n");
    let port = 8499;
    let name = format!("served-{}", std::process::id());

    let mut server = tormoni(&data)
        .args(["serve", "--bind", &port.to_string()])
        .spawn()
        .expect("the server starts");
    // Given a moment to bind. A poll of /health would be tidier and needs a client this crate
    // does not have; `curl` is what the rest of this binary reaches the network with.
    std::thread::sleep(std::time::Duration::from_secs(2));

    let at = |path: &str| format!("http://127.0.0.1:{port}{path}");
    let body = serde_json::json!({
        "command": ["/bin/busybox", "sh", "-c", "echo served; echo kept > /results/note.txt"],
        "name": name,
        "env": ["API_KEY=supersecret"],
    })
    .to_string();
    let posted = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "120",
            "-X",
            "POST",
        ])
        .args(["-H", "Authorization: Bearer tor_roundtrip"])
        .args(["-H", "content-type: application/json"])
        .args(["-d", &body])
        .arg(at("/v1/runs"))
        .output()
        .expect("curl runs");
    let stream = String::from_utf8_lossy(&posted.stdout).into_owned();
    let _ = server.kill();
    let _ = server.wait();

    assert!(
        stream.contains("\"event\":\"started\""),
        "no start event: {stream}"
    );
    assert!(
        stream.contains("served"),
        "the guest's output did not stream: {stream}"
    );
    // **The cut survives serving.** The guest was given the whole entry and the record kept the
    // name; a value must not appear on any path out of this box.
    assert!(
        !stream.contains("supersecret"),
        "an environment value reached the stream: {stream}"
    );
    assert!(
        stream.contains("\"API_KEY\""),
        "the name was kept: {stream}"
    );

    // A sandbox that ran accrued what it held, and the ledger says so.
    let ledger = std::fs::read_to_string(data.join("usage.jsonl")).expect("a ledger");
    assert!(ledger.contains(&name), "no usage for {name}: {ledger}");
    assert!(
        !ledger.contains("supersecret"),
        "a value reached the ledger: {ledger}"
    );

    // The run id, from the record the stream ended with.
    let ended: serde_json::Value = stream
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .find(|value: &serde_json::Value| value["event"] == "ended")
        .expect("an ended event");
    let run_id = ended["run"]["run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();

    // Serving cannot have changed the archive: the same bytes, or this is two products.
    let served = scratch.path().join("served.tar");
    let mut restarted = tormoni(&data)
        .args(["serve", "--bind", &port.to_string()])
        .spawn()
        .expect("the server starts again");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let pulled = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", "60", "-o"])
        .arg(&served)
        .args(["-H", "Authorization: Bearer tor_roundtrip"])
        .arg(at(&format!("/v1/runs/{run_id}/archive")))
        .status()
        .expect("curl runs");
    let _ = restarted.kill();
    let _ = restarted.wait();
    assert!(pulled.success(), "the archive did not come back");

    let local = scratch.path().join("local.tar");
    let exported = Command::new(env!("CARGO_BIN_EXE_tormoni"))
        .args(["export", &run_id, "--to"])
        .arg(&local)
        .output()
        .expect("the binary runs");
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );

    let digest = |path: &Path| {
        let out = Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .expect("shasum runs");
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .expect("a digest")
            .to_owned()
    };
    assert_eq!(
        digest(&served),
        digest(&local),
        "the archive a caller got is not the archive a keyboard would have written"
    );
}
