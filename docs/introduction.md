# Introduction

**Behavioral Sandbox** (**BSX**) is a local-first desktop sandbox for running untrusted code in
hardware isolation. Untrusted code runs inside a virtual machine, so the isolation boundary is the
CPU's, enforced by hardware virtualization: KVM on Linux, Hypervisor.framework on macOS. What a
sandbox can reach is settled before it starts, on the host side of that boundary.

It exists for the usual suspects: a third-party binary, a dependency's install script, an
AI-generated snippet, a sample under analysis. Everything stays on your own machine: no account, no
telemetry, no control plane, and nothing that stops working with the network off.

## What it does today

BSX runs on [libkrun](https://github.com/containers/libkrun), a library that makes the calling
process the virtual machine monitor. `krun_start_enter` never returns, so a VM **is** a process:
every sandbox is a helper this project spawned, tracked and reaped.

- **Run something.** `bsx run` runs one command in a fresh sandbox and exits with its status,
  `bsx shell` opens a session on a pty inside the guest, and `bsx up` starts a sandbox that
  outlives the command that started it, reached afterwards by name with `ls`, `exec` and `stop`.
- **See it.** `--display WIDTHxHEIGHT` gives the guest a virtio-gpu scanout shown in a window, and
  that window's keyboard and pointer reach the guest as two virtio-input devices. The desktop image
  boots to a terminal in a Wayland session under it, and `--sound` adds a virtio-snd card.
- **Keep it.** Every run leaves a record: the posture as settled, the captured output, and the
  directory the guest saw as `/results`. `bsx ls --all`, `show`, `rm` and `export` read, remove and
  package them, one ustar file per run.
- **Drive it from a window.** `bsx-app` is the notebook of those runs, live and past: a sidebar over
  the list, one run's record with its display and output, a start form that shows a sandbox's
  posture before it boots, and a shell in your own terminal. Its palette, interface scale and
  landing screen persist across launches.

Both platforms run the same sandboxes: KVM on Linux, and Hypervisor.framework on macOS ARM64, where
the tree also signs itself (`cargo xtask sign`) and bundles as `Behavioral Sandbox.app` (`cargo xtask bundle`).

**Status.** Pre-release: one maintainer, no external review, and no release to install. macOS's
libkrun builds neither the `--sound` nor the guest input backend, so a display there is viewed in
`bsx-app`. `--gpu` offers a guest the 3D path where libkrun reports the feature, but no host
measured so far carries a Venus-built renderer, so guest acceleration is unproven.

This book is short, and deliberately so: it describes the rules the project is built to, the crates
that are actually in the tree, and how a sandbox is run.

## Reading this book

- **[Running a sandbox](./running.md)**, the verbs, posture flags, configuration layering, what a
  run leaves behind, and the notebook.
- **[Architecture](./architecture.md)**, the six design rules with the mechanism serving each, and
  what is in the tree.
- **[Control socket & IPC](./control-ipc.md)**, local process discovery, display leasing, zero-copy
  memfd sharing, and host↔guest wire framing.
- **[Building guest images](./building-images.md)**, unprivileged rootfs assembly with `apk.static`
  and `fakeroot`, desktop closures, and lockfile verification.
- **[Security](./security.md)**, what is trusted, what counts as a security bug, and how to report
  one.

The repository's own operating manual is
[`AGENTS.md`](https://github.com/kendricklawton/behavioral-sandbox/blob/main/AGENTS.md) at the root:
the design rules, the repo layout, the build, and the commit conventions. It is written as standing
instructions for a coding agent and doubles as the developer reference.

## License

Apache-2.0.
