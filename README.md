<div align="center">
  <h1>Boxdesk</h1>

  <p>
    <strong>A local-first desktop sandbox for running untrusted code in a hardware-isolated
    virtual machine, on
    <a href="https://github.com/containers/libkrun">libkrun</a></strong>
  </p>

  <p>
    <a href="https://github.com/kendricklawton/boxdesk/actions/workflows/ci.yml"><img src="https://github.com/kendricklawton/boxdesk/actions/workflows/ci.yml/badge.svg" alt="build status" /></a>
    <img src="https://img.shields.io/badge/status-pre--release-orange.svg" alt="pre-release" />
    <img src="https://img.shields.io/badge/rustc-1.98%2B-green.svg" alt="supported rustc 1.98+" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg" alt="Apache-2.0" /></a>
  </p>

  <h3>
    <a href="https://kendricklawton.github.io/boxdesk/">Docs</a>
    <span> | </span>
    <a href="docs/architecture.md#design-rules">Design rules</a>
    <span> | </span>
    <a href="DEVELOPMENT.md">Developing</a>
    <span> | </span>
    <a href="CONTRIBUTING.md">Contributing</a>
  </h3>
</div>

## What it is

A desktop application for running untrusted code, on one person's machine, with a CLI beside it.
Untrusted code runs inside a virtual machine, so the isolation boundary is the CPU's, enforced by
hardware virtualization: KVM on Linux, Hypervisor.framework on macOS ARM64.

libkrun makes the calling process the virtual machine monitor. `krun_start_enter` never returns, so
a VM **is** a process: every VM is a helper the supervisor spawned and reaps.

## What it does today

On a host whose hypervisor answers (`/dev/kvm` on Linux, Hypervisor.framework on macOS ARM64) and a
guest image the tree builds:

* **Run something.** `boxdesk run` runs one command in a sandbox and exits with its status, `boxdesk shell`
  opens a session on a pty inside the guest, and `boxdesk up` starts a sandbox that outlives the command
  that started it. `boxdesk ls`, `boxdesk exec` and `boxdesk stop` reach a sandbox this process did not start.
* **See it.** `--display WIDTHxHEIGHT` shows a guest's screen in a window whose keyboard and pointer
  go to the guest, the desktop image boots to a terminal in a Wayland session there, and `--sound`
  adds a virtio-snd card.
* **Keep it.** Every run leaves a record: the posture as settled, the captured output, and the
  guest's `/results`. `boxdesk show`, `boxdesk rm` and `boxdesk export` read, remove and package them, one ustar
  file per run.
* **Drive it from a window.** `Boxdesk` is the notebook: every run on the machine, live and past; a
  live run's display with your keyboard and pointer going in; a form that shows a sandbox's posture
  before it boots. A sidebar reaches its screens, and its palette, interface scale and landing
  screen persist across launches.

macOS ARM64 builds, signs (`cargo xtask sign`), bundles as `Boxdesk.app` (`cargo xtask bundle`) and
boots the same sandboxes under Hypervisor.framework.

**Status.** Pre-release: one maintainer, no external review. A release installs with
`curl -fsSL https://raw.githubusercontent.com/kendricklawton/boxdesk/main/install.sh | sh` on macOS ARM64 and Linux x86_64 (the
[documentation](https://docs.boxdesk.dev) says what it does and cannot do). macOS's
libkrun builds neither the `--sound` nor the guest input backend, and the display helper's own
window is compiled out there, so a display on macOS is viewed in `Boxdesk`. `--gpu` offers a guest
the 3D path (virgl + Venus) where libkrun reports the feature, but no host measured so far carries a
Venus-built renderer, so guest acceleration is unproven.

## Design rules

Five rules. A change that breaks one is a design error, not a trade-off. Each states an intent and
the mechanism serving it; the full text is
[Architecture and design](https://docs.boxdesk.dev/architecture/#design-rules).

* **Isolation is hardware, not software**: untrusted code runs in a VM under KVM or
  Hypervisor.framework, never behind a guest-side check.
* **Deny by default**: no explicit configuration means no shared directory and no network. What is
  shared *is* the policy, settled before the VM starts.
* **An application, not a platform**: a program on one person's machine. There is no tenant, no
  account, and no fleet. An AI model is a caller, never a component.
* **No panic, hang, or leak on the host path**: a hostile guest or a dead helper surfaces as a typed
  error. The rule the code is written against; an aim, not a proven property.
* **Measure rather than assert**: percentiles with the host and date, and a number that cannot be
  defended is withdrawn. libkrun has no snapshot surface, so every boot is a cold boot.

The host path is `#![forbid(unsafe_code)]`, enforced by the compiler and checked by
`every_crate_forbids_unsafe` in the gate. Two crates are excepted, each for a library written in
another language: `boxdesk-krun`, because libkrun is C, and `boxdesk-app`'s `chrome`, because AppKit is
Objective-C. The gate asserts that list exactly, so a third cannot appear quietly.

## Building

On Linux, `/dev/kvm` must be readable and writable by your user, which usually means membership of
the `kvm` group. On macOS, Hypervisor.framework refuses a process without the
`com.apple.security.hypervisor` entitlement: `cargo xtask sign` applies it ad hoc, and a signature
does not reliably outlive the next cargo build, so re-sign after building. No part of the build or
the run needs root on either platform.

`cargo xtask init` is what gets a sandbox booting: it puts the pinned Alpine minirootfs and the
static agent where `boxdesk` looks for a root, on either platform, since neither step needs `apk`. The
runtimes and the locked package closure come from `cargo xtask build-rootfs`, which runs on Linux.

libkrun and its kernel payload install from the system package manager (`pacman -S libkrun
libkrunfw` on Arch; `brew tap slp/krun && brew trust slp/krun && brew install libkrun libkrunfw`
on macOS): a C library and a shared object holding a Linux kernel, so neither arrives through
cargo. The guest image is built on Linux, for either architecture (`--arch aarch64`), because
what executes during the build is `apk.static`, a Linux binary.

```console
cargo xtask setup            # what this host can and cannot do
cargo xtask init             # a guest tree where boxdesk looks for one, so a sandbox can boot
cargo xtask ci               # the gate: fmt, prose drift, clippy, build, test, docs, deny
cargo xtask sign             # macOS: re-entitle the built boxdesk after any other build
cargo xtask bundle           # macOS: assemble artifacts/Boxdesk.app from the built pair
cargo xtask dist             # this host's release under dist/, as install.sh downloads it
cargo xtask build-rootfs     # the guest image (Alpine + runtimes + the static agent)
cargo xtask build-rootfs --desktop   # the desktop image (+ a Wayland compositor and a terminal)
```

## Repo layout

Directories stay short and packages carry the `boxdesk-` prefix, so a package is its directory plus that
prefix, with one exception: `crates/cli` builds `boxdesk`, the bare name going to the command a user
types. `cargo … -p` takes the package, a path takes the directory.

| Path | Package | Role |
|------|---------|------|
| `crates/supervisor` | `boxdesk-supervisor` | Spawn, track, stop and reap the helper processes that are VMs. One value per live VM; `Drop` tears it down. |
| `crates/krun` | `boxdesk-krun` | The safe wrapper over libkrun, with the raw declarations private beneath it. The one crate that may use `unsafe`, because the library is C. |
| `crates/channel` | `boxdesk-channel` | The host↔guest wire protocol: nearly dependency-free length-prefixed framing (`zeroize`, for the post-send secret wipe, is the one dependency), shared by both ends. |
| `crates/guest-agent` | `boxdesk-guest-agent` | The in-guest agent: runs one command per connection, streams stdout/stderr/exit. Exec/IO only, not the trust boundary. |
| `crates/record` | `boxdesk-record` | The run record the notebook keeps: posture, captured output, and the guest's `/results`, one directory per run, exportable as one tar file. |
| `crates/input` | `boxdesk-input` | The guest's keyboard and pointer: device shapes, reports, and the line grammar the replay file and the control socket feed. |
| `crates/cli` | `boxdesk` | The `boxdesk` CLI and its verbs. The binary on `PATH` is `boxdesk`. |
| `crates/serve` | `boxdesk-serve` | `boxdesk serve`: one box that runs sandboxes for a caller over HTTP, and the meter that says what one held. One tenant, one token; it decides nothing about who is asking. |
| `crates/app` | `boxdesk-app` | The GUI application, `Boxdesk`, on iced: the notebook of runs reached from a sidebar, a run's record with its display and output, a start form, stop, re-run, delete, export, clear history, a persisted palette, scale and landing screen, and a shell in your terminal. One AppKit call gives its window a toolbar, which is what puts the window's own buttons on the line its head is drawn to. |
| `docs` | | This documentation, as an mdBook. |
| `crates/test-support` | `boxdesk-test-support` | Shared test fixtures: a self-reclaiming scratch dir, a log sink, a deterministic generator. Dev-only, never shipped. |
| `xtask` | `xtask` | Dev orchestration: `cargo xtask ci`, the guest image build, the vendor mirror. Never shipped. |

## Verified on

The gate (`cargo xtask ci`: build, tests, lints, docs, dependency audit) runs in CI on Ubuntu 24.04
`x86_64` and macOS ARM64 (`macos-15`) on every change and needs no privilege, and a smoke lane runs
the wire-protocol fuzz targets from their committed seeds. **No CI lane boots a VM.** The suites
that boot one run where a hypervisor answers, and skip saying so where none does. Development
happens on Arch Linux `x86_64` and macOS ARM64.

## Releases and scope

A `v*` tag builds a release: `.github/workflows/release.yml` runs `cargo xtask dist` on macOS ARM64
and on Linux x86_64 and publishes the two artifacts, `SHA256SUMS` and `install.sh`, which
`install.sh` at the repository root installs. There is no published roadmap and no promised date.
A capability becomes a feature when a test exercises it end to end, and is not announced before
that. The first
supported release, `v0.1.0`, will pin the host↔guest wire framing and the supervisor API; until then
everything, including the crate names, changes without notice.

The project is **open to outside pull requests**, though everything here is pre-`v0.1.0` and
changes without notice. A pull request signs its commits off (`git commit -s`). The terms are in
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
