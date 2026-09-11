# Introduction

**Tormoni** is a local-first desktop sandbox for running untrusted code in
hardware isolation. Untrusted code runs inside a virtual machine, so the isolation boundary is the
CPU's, enforced by hardware virtualization: KVM on Linux, Hypervisor.framework on macOS. What a
sandbox can reach is settled before it starts, on the host side of that boundary.

It exists for the usual suspects: a third-party binary, a dependency's install script, an
AI-generated snippet, a sample under analysis. Everything stays on your own machine: no telemetry,
no daemon, and a sandbox that needs no network. Signing in to the console is the one thing that
does, and no sandbox needs it.

## What it does today

Tormoni runs on [libkrun](https://github.com/libkrun/libkrun), a library that makes the calling
process the virtual machine monitor. `krun_start_enter` never returns, so a VM **is** a process:
every sandbox is a helper this project spawned, tracked and reaped.

- **Run something.** `tormoni run` runs one command in a fresh sandbox and exits with its status,
  `tormoni shell` opens a session on a pty inside the guest, and `tormoni up` starts a sandbox that
  outlives the command that started it, reached afterwards by name with `ls`, `exec` and `stop`.
- **See it.** `--display WIDTHxHEIGHT` gives the guest a virtio-gpu scanout shown in a window, and
  that window's keyboard and pointer reach the guest as two virtio-input devices. The desktop image
  boots to a terminal in a Wayland session under it, and `--sound` asks for a virtio-snd card where
  libkrun was built with one, which no measured host has been.
- **Keep it.** Every run leaves a record: the posture as settled, the captured output, and the
  directory the guest saw as `/results`. `tormoni ls --all`, `show`, `rm` and `export` read, remove and
  package them, one ustar file per run.
- **Drive it from a window.** `Tormoni` is the notebook of those runs, live and past: a sidebar over
  the list, one run's record with its display and output, a start form that shows a sandbox's
  posture before it boots, and a shell in your own terminal. Its palette, interface scale and
  landing screen persist across launches.

Both platforms run the same sandboxes: KVM on Linux, and Hypervisor.framework on macOS ARM64, where
the tree also signs itself (`cargo xtask sign`) and bundles as `Tormoni.app` (`cargo xtask bundle`).

**Status.** Pre-release: one maintainer, no external review. A release installs with
`curl -fsSL https://raw.githubusercontent.com/kendricklawton/tormoni/main/install.sh | sh` on macOS ARM64 and Linux x86_64; [Running a
sandbox](./running.md#installing) says what that does and cannot do. macOS's
libkrun builds neither the `--sound` nor the guest input backend, so a display there is viewed in
`Tormoni`. `--gpu` offers a guest the 3D path where libkrun reports the feature, but no host
measured so far carries a Venus-built renderer, so guest acceleration is unproven.

This book is short, and deliberately so: it describes the rules the project is built to, the crates
that are actually in the tree, and how a sandbox is run.

## Reading this book

- **[Running a sandbox](./running.md)**, the verbs, posture flags, configuration layering, what a
  run leaves behind, and the notebook.
- **[Architecture](./architecture.md)**, the five design rules with the mechanism serving each, and
  what is in the tree.
- **[Control socket & IPC](./control-ipc.md)**, local process discovery, display leasing, zero-copy
  memfd sharing, and host↔guest wire framing.
- **[Building guest images](./building-images.md)**, unprivileged rootfs assembly with `apk.static`
  and `fakeroot`, desktop closures, and lockfile verification.
- **[Security](./security.md)**, what is trusted, what counts as a security bug, and how to report
  one.

## License

Apache-2.0.
