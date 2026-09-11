# Contributing to Boxdesk

Thanks for your interest. **Outside contributions are welcome.** A few things are worth knowing
before you spend time on one.

**Open an issue first for anything non-trivial.** Bug fixes, tests, and documentation can go
straight to a pull request. For a new capability, a change to a public API, or a refactor that moves
code between crates, open an issue and settle the shape first. That is not gatekeeping: the project
is pre-1.0 and its surface still moves (the `boxdesk-channel` framing, the `boxdesk-supervisor` spawn and
discovery API, and the crate names all change without notice until the first supported release,
`v0.1.0`), and an issue is how you avoid building against a shape that is about to change under
you.

**Five design rules govern every change**, and the first question in review is which rule a change
touches. They are in [Architecture and design](docs/architecture.md), with the reasoning behind
each. A change that breaks one is declined as a design error rather than weighed as a trade-off,
however good the code is, so they are worth reading before starting anything large.

**Sign your commits off.** `git commit -s` adds a `Signed-off-by:` line: your assertion, under the
[Developer Certificate of Origin](https://developercertificate.org/), that you wrote the patch or
otherwise have the right to submit it under the project's license. Contributions are licensed under
**Apache-2.0**, the project's license (see [`LICENSE`](LICENSE)).

**What a pull request needs.** `cargo xtask ci` green locally (fmt, the prose-drift lint, clippy
`-D warnings`, build, tests, docs, and `deny`), and
[Conventional Commits](https://www.conventionalcommits.org/) subjects. New behavior needs a test,
and the repo's standard for a test is that it was watched failing before it passed: break the
behavior under test, see the assertion fire, then revert.

**`cargo xtask ci` is the gate and needs no privilege.** The tests that boot a guest are
`#[ignore]`d and each names its own prerequisite (`/dev/kvm` and a guest tree from
`cargo xtask build-rootfs`), because a test whose prerequisite is missing skips itself and cargo
counts a skipped test as a pass; run them with `cargo test -p boxdesk --test e2e -- --ignored`.
`cargo xtask setup` reports what your host can do.

**Expect review to take a while.** One maintainer, no service commitment, and a security-sensitive
core that gets read slowly on purpose.

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md). Suspected vulnerabilities go to the
private advisory form described in [`SECURITY.md`](SECURITY.md), never to a public issue or pull
request.

**Building it** is [DEVELOPMENT.md](DEVELOPMENT.md): prerequisites, the xtask verbs, testing, and
how a release is cut. **What it is and why it is shaped this way** is the book under
[`docs/`](docs/SUMMARY.md) — the design rules, the architecture, the guest image build, and the two
protocols. This file is the process.
