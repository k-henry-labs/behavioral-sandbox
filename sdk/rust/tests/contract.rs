//! The two rules the whole contract rests on, and the ones most likely to be broken by a
//! well-meaning change: an environment value goes out to the guest but never comes back in a
//! record, and a guest command exiting non-zero is a record and not an error.
//!
//! These run against the REAL core. There is no binary to stand in for any more — the SDK calls
//! `execute_sandbox` in this process, so a stub would be testing itself. `dry_run` settles a
//! posture without booting, which is what lets the contract be checked on a machine with no
//! hypervisor.

use boxdesk::{Boxdesk, End, Error, Net, RootFs, Sandbox};
use std::num::{NonZeroU8, NonZeroU32};

/// A guest root that exists, because resolution refuses one that does not — the point here is the
/// posture, not where a tree lives.
fn rooted(command: [&str; 1]) -> Sandbox {
    Sandbox::new(command).root(std::env::temp_dir())
}

/// A value goes out to the guest whole, and only the NAME comes back.
///
/// The record is not the place for a secret, and this is the assertion that says so. It checks the
/// rendered record rather than the `env` field alone: a field added later that carried the value
/// would slip past an assertion naming only the fields that exist today.
#[test]
fn env_values_go_out_and_names_come_back() {
    const SECRET: &str = "s3cret-value-nobody-should-see";
    let outcome = Boxdesk::new()
        .dry_run(
            rooted(["env"])
                .env(format!("API_KEY={SECRET}"))
                .env("DEBUG=1"),
        )
        .expect("a dry run settles a posture");

    assert_eq!(outcome.record.posture.env, ["API_KEY", "DEBUG"]);
    let text = outcome.record.to_text();
    assert!(
        !text.contains(SECRET),
        "a value reached the record:\n{text}"
    );
}

/// A value may contain `=`; the cut is at the FIRST one.
#[test]
fn a_value_holding_an_equals_sign_keeps_its_name() {
    let outcome = Boxdesk::new()
        .dry_run(rooted(["env"]).env("TOKEN=a=b=c"))
        .expect("a dry run settles a posture");
    assert_eq!(outcome.record.posture.env, ["TOKEN"]);
}

/// The posture that comes back is the one that was asked for.
#[test]
fn the_posture_is_the_one_that_was_asked_for() {
    let outcome = Boxdesk::new()
        .dry_run(
            rooted(["true"])
                .vcpus(NonZeroU8::new(2).expect("non-zero"))
                .mem_mib(NonZeroU32::new(1024).expect("non-zero"))
                .net(Net::Tsi)
                .rootfs(RootFs::Writable)
                .mount("/mnt", std::env::temp_dir())
                .share("tag", std::env::temp_dir()),
        )
        .expect("a dry run settles a posture");

    let p = &outcome.record.posture;
    assert_eq!(p.vcpus.get(), 2);
    assert_eq!(p.mem_mib.get(), 1024);
    assert_eq!(p.network.as_word(), "tsi");
    assert_eq!(p.rootfs.as_word(), "writable");
    assert_eq!(p.mounts.len(), 1);
    assert_eq!(p.shares.len(), 1);
    assert_eq!(p.shares[0].tag, "tag");
}

/// Unset means the CLI's own default. This SDK adds none of its own, and a default that drifted
/// from the binary's would give a caller a sandbox the docs do not describe.
#[test]
fn the_defaults_are_the_clis_own() {
    let outcome = Boxdesk::new()
        .dry_run(rooted(["true"]))
        .expect("a dry run settles a posture");
    let p = &outcome.record.posture;
    assert_eq!(p.vcpus.get(), 1);
    assert_eq!(p.mem_mib.get(), 512);
    assert_eq!(p.network.as_word(), "none");
    assert_eq!(p.rootfs.as_word(), "read-only");
    assert!(p.results, "the results mount is on unless dropped");
}

/// A dry run has settled a posture and nothing more.
#[test]
fn a_dry_run_has_no_end_and_no_directory() {
    let outcome = Boxdesk::new()
        .dry_run(rooted(["true"]))
        .expect("a dry run settles a posture");
    assert_eq!(outcome.record.verb.as_word(), "run");
    assert!(outcome.record.end.is_none());
    assert!(outcome.record.ended_ms.is_none());
    assert!(outcome.dir.is_none());
    assert!(!outcome.ok(), "a run that has not ended did not succeed");
    assert_eq!(outcome.stdout().expect("no directory reads empty"), "");
    assert!(
        outcome
            .files()
            .expect("no directory lists nothing")
            .is_empty()
    );
}

/// A non-zero guest exit is a RESULT, not an error. `ok()` is the field to branch on, and it is
/// false for every end that is not a clean zero.
#[test]
fn only_a_clean_exit_is_ok() {
    let mut record = Boxdesk::new()
        .dry_run(rooted(["true"]))
        .expect("a dry run settles a posture")
        .record;

    for (end, ok) in [
        (End::Exit(0), true),
        (End::Exit(1), false),
        (End::Exit(3), false),
        (End::Signal(9), false),
        (End::Stopped, false),
        (End::Gone, false),
        (End::Failed, false),
    ] {
        record.finish(end);
        let outcome = boxdesk::Outcome {
            record: record.clone(),
            dir: None,
            code: 0,
        };
        assert_eq!(outcome.ok(), ok, "{end:?}");
    }
}

/// An empty command is refused with a sentence, not a panic on `command[0]`.
#[test]
fn an_empty_command_is_refused() {
    let err = Boxdesk::new()
        .run(Sandbox::new(Vec::<String>::new()))
        .expect_err("an empty command cannot run");
    assert!(matches!(err, Error::Posture(_)), "{err:?}");
    assert!(err.to_string().contains("empty"), "{err}");
}

/// A guest root that is not there is refused where it can still be explained, rather than failing
/// later as a boot that could not find its tree.
#[test]
fn a_missing_guest_root_is_refused() {
    let err = Boxdesk::new()
        .dry_run(Sandbox::new(["true"]).root("/definitely/not/a/guest/root"))
        .expect_err("a missing root cannot be run against");
    assert!(matches!(err, Error::Posture(_)), "{err:?}");
}

/// A run nobody filed is a refusal naming what was looked for.
#[test]
fn a_missing_run_names_what_was_asked_for() {
    let err = Boxdesk::new()
        .show("1-definitely-not-a-run")
        .expect_err("no such run");
    assert!(err.to_string().contains("no run"), "{err}");
}
