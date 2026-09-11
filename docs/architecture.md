# Architecture

What Tormoni is, the rules it holds itself to, and what is in the tree today.

## Scope

### What this is

Tormoni is a local-first desktop sandbox. Untrusted code runs inside a virtual machine, with the
isolation boundary enforced by the CPU through hardware virtualization: KVM on Linux,
Hypervisor.framework on macOS. It is a GUI application with a CLI beside it, both on one machine.

### What the tree does today

Tormoni runs on [libkrun](https://github.com/libkrun/libkrun), a library that makes the calling
process the virtual machine monitor.

In the tree: the host/guest wire framing (`tormoni-channel`), the in-guest agent (`tormoni-guest-agent`),
the safe libkrun wrapper (`tormoni-krun`), the process supervisor (`tormoni-supervisor`), the `tormoni` CLI and
its verbs, the run record and its ustar export (`tormoni-record`), the guest image build and the gate
(`xtask`), and the display path: a virtio-gpu scanout landing in host RAM and shown in a window,
with that window's keyboard and pointer going back as two virtio-input devices, a second guest
image that boots a Wayland compositor on it, and an opt-in virtio-snd card. `Tormoni` is the
notebook of those runs: a sidebar over the list, a live run's display and input in the window, a
start form that shows the posture before boot, export, a selection of ended runs removed behind a
confirm, and a Settings screen whose palette, scale and landing screen persist. On macOS ARM64 the
tree signs (`cargo xtask sign`), bundles (`cargo xtask bundle`) and boots the same sandboxes under
Hypervisor.framework.

**Status.** Pre-release: one maintainer, no external review; a `v*` tag releases the tree through
`cargo xtask dist` and `install.sh`. macOS's libkrun builds
neither the `--sound` nor the guest input backend, and the display helper's own window is compiled
out there, so a display on macOS is viewed in `Tormoni`. `--gpu` offers a guest the 3D path behind
`krun_has_feature`; measured acceleration exists nowhere yet, for reasons below.

### Design rules

These are the rules the project holds itself to, stated so a change that breaks one is recognisable
as a design error rather than a trade-off. They describe intent and the mechanism serving it, not a
verified outcome.

1. **Isolation is hardware, not software.** Untrusted code runs in a VM under KVM or
   Hypervisor.framework. A change that moves the boundary into guest-side software is a design
   error, not an optimisation, and a shared-kernel shortcut taken to simplify things is the same
   error.
2. **Deny by default.** A sandbox with no explicit configuration shares no host directory and has no
   network. What is shared **is** the policy: no in-kernel enforcer sits behind it, so the set of
   shared directories and the network backend are settled before the VM starts and are visible to
   the person starting it.
3. **An application, not a platform.** The product is a program on one person's machine. The unit is
   the sandbox; there is no tenant, no account, and no fleet. Mechanism that makes one machine's
   sandboxes work belongs here, and anything that must know who is paying is a different product.
   An AI model is a caller, never a component: it drives the app from outside.
4. **No panic, hang, or leak on the host path.** A hostile or crashing guest, or a helper that dies,
   should surface as a typed error. A leak here is a stranded VM holding somebody's laptop RAM, not
   a server you can reboot. This is what the code is written against and what the
   confinement suite exercises; an aim, not a proven property.
5. **Measure rather than assert.** Boot, memory, and frame timings are reported as nearest-rank
   percentiles with the host and date they were taken on. Where a number cannot be defended, it is
   withdrawn rather than published. libkrun has no snapshot surface, so every boot is a cold boot.

## Index of crates

Directories stay short and packages carry the `tormoni-` prefix, so a package is its directory plus that
prefix, with one exception: `crates/cli` builds `tormoni`, the bare name going to the command a user
types. `cargo … -p` takes the **package**, a path takes the **directory**.

| Crate | Directory | Role |
|---|---|---|
| `tormoni-supervisor` | `crates/supervisor` | Spawn, track, stop and reap the helper processes that are VMs. One value per live VM; `Drop` tears it down. |
| `tormoni-krun` | `crates/krun` | The safe wrapper over libkrun, with the raw declarations private beneath it. One of the two crates that may use `unsafe`, because the library is C. |
| `tormoni-channel` | `crates/channel` | The host/guest wire protocol. Nearly dependency-free framing (`zeroize`, for the post-send secret wipe, is the one dependency), shared verbatim by both ends. |
| `tormoni-guest-agent` | `crates/guest-agent` | The in-guest agent. One command per connection, static musl, baked into the guest image. Not a security boundary. Its binary keeps the bare name `guest-agent`. |
| `tormoni-record` | `crates/record` | The run record: posture, captured output and the guest's `/results`, one directory per run under the local data dir, written by the CLI, read by both binaries, and exported as one ustar file. |
| `tormoni-input` | `crates/input` | The guest's keyboard and pointer: device shapes, reports, and the line grammar the replay file and the control socket's `input` session feed. |
| `tormoni` | `crates/cli` | The `tormoni` binary and its verbs. Package, binary, and command all share the name. |
| `tormoni-serve` | `crates/serve` | `tormoni serve`: one box that runs sandboxes for a caller over HTTP, and the meter that says what one held. One tenant, one token; it decides nothing about who is asking. |
| `tormoni-app` | `crates/app` | The GUI application, on iced: the notebook of runs from `tormoni-record` reached from a sidebar, a run's record with its display (leased over the control socket, uploaded to a wgpu texture) and output, a start form, stop, re-run, delete, export, a selection of ended runs to remove, a persisted palette, scale and landing screen, and a shell in the operator's terminal through `tormoni`. |
| `tormoni-test-support` | `crates/test-support` | Test fixtures: a self-reclaiming scratch dir, a log sink, and the deterministic generator the in-gate fuzz suites use. |
| `xtask` | `xtask` | Dev orchestration: the gate, the guest image build, the vendor mirror. Never shipped, and never renamed: `cargo xtask` is a `--package xtask` alias. |

## How a sandbox runs

One verb, end to end, with the boundary each step sits on:

1. **The posture is settled on the host, before anything boots.** Flags, then `TORMONI_*`, then the
   nearest `.tormoni.toml`, then defaults, resolve to one set of virtiofs tags, a network backend and a
   device list. `--dry-run` prints it without booting. Nothing decides this later, because there is
   no in-kernel enforcer behind it to decide with.
2. **The CLI writes the record and spawns a helper.** `tormoni-supervisor` writes the helper's argv and
   spawns `tormoni __vmm`; `tormoni-record` writes the run's directory with the posture as settled. One
   `Vm` value tracks the child, and `Drop` tears it down.
3. **The helper becomes the VM.** It configures libkrun through `tormoni-krun`'s builder, whose types
   carry the library's call-ordering rules, then calls `krun_start_enter`, which does not return.
   From here the process *is* the guest's monitor: there is no daemon above it.
4. **The guest is reached over vsock.** `tormoni-guest-agent`, static musl and baked into the image,
   serves repeated execs from one session directory. `tormoni exec` and `tormoni shell` speak
   `tormoni-channel`'s framing to it. The agent does no init work and is not the security boundary.
5. **A display leaves the guest as pixels.** The guest draws into a dumb buffer; its `virtio_gpu`
   driver transfers and flushes; the frame lands in host RAM in a sealed memfd of equal slots. A
   second process leases that scanout over the control socket, maps it, and uploads by damage
   rectangle into a wgpu texture, which is how `Tormoni` shows a run another process started.
6. **Input goes back the same way.** Window events become `tormoni-input` reports and travel as
   `kbd|ptr TYPE CODE VALUE` lines down an `input` session on the control socket, arriving as two
   virtio-input devices. The replay file and the socket speak one grammar, so both feeders are the
   same code path.
7. **The run ends in the record.** The captured output (capped), the guest's `/results`, and the
   end state land in the run's directory, which both binaries read afterwards.

The control socket and the wire framing are on their own page: [Control socket &
IPC](./control-ipc.md).

## What a sandbox costs on an M1

Measured 2026-09-05 on one host: MacBook Air M1 (MacBookAir10,1), macOS 26.6.2, libkrun 1.19.4
and libkrunfw 5.5.0 from Homebrew, a debug build. The guest is the aarch64 Alpine 3.24.1 spike
tree, not `build-rootfs`'s product. One host, one date; nothing here is claimed for any other.

Every boot is a cold boot, because libkrun has no snapshot surface, so the boot number is the
whole verb: `tormoni run --root <tree> -- /bin/true` from spawn to exit, record written, timed around
the subprocess. Three warmups discarded, then one hundred runs, nearest-rank percentiles:

| What | Number |
|---|---|
| Cold boot, wall clock, n=100 | p50 157 ms, p90 162 ms, p99 169 ms (min 150, max 170) |
| An idle `tormoni up` sandbox, 512 MiB configured | 102 MiB resident, steady over three samples |
| `Tormoni` idling (on the menu screen it had that day) | 101 MiB resident |

Resident is what was touched, not what was granted: the helper holds a 512 MiB guest with a
102 MiB resident set.

## What crosses the GPU boundary

Measured 2026-09-04 on one host: Intel Core i5-10310U with UHD Graphics (i915), Linux 7.2.2,
libkrun 1.19.4, libkrunfw 5.5.0, virglrenderer 1.3.0, Mesa 26.1.6. One host, one date; nothing here
is claimed for any other.

**Only 2D pixels cross, in one direction.** A guest draws into a DRM dumb buffer, its kernel
`virtio_gpu` driver sends the transfer and flush, and the frame arrives in host RAM. No guest
process has ever issued a 3D command, on this host, because no userspace driver in either image can.

### A display brings the host GPU in

A `--display` VM's monitor process holds seven file descriptors on `/dev/dri/renderD128` (the i915
render node) and maps Mesa's EGL, GLES and GBM alongside `libvirglrenderer`. A headless VM on the
same image holds none, and maps neither EGL nor GLES:

```console
$ tormoni up --name gpuprobe --root artifacts/rootfs-guest --display 640x480
$ ls -l /proc/$(pgrep -f 'tormoni __vmm')/fd | grep -c renderD128
7
$ awk '{print $6}' /proc/$(pgrep -f 'tormoni __vmm')/maps | grep -E 'libEGL|libGLES|virgl' | sort -u
/usr/lib/libEGL_mesa.so.0.0.0
/usr/lib/libGLESv2.so.2.1.0
/usr/lib/libvirglrenderer.so.1.11.0
```

So `--display` is what brings the host GPU into a sandbox's blast radius. This is a property of the
host renderer, not a capability granted to the guest, and it is the reason virglrenderer belongs in
the trusted set on the [Security](./security.md) page.

### What the guest is offered

The guest gets a `virtio_gpu` card and a render node, and the device advertises 3D. Asking the host
renderer for each capset (rather than decoding the `SUPPORTED_CAPSET_IDs` bitmask) is answered for
VIRGL, VIRGL2, VENUS, CROSS_DOMAIN and DRM. `crates/cli/tests/gpu_probe.py` reports it, and
`a_display_guest_is_offered_a_3d_virtio_gpu_it_has_no_driver_for` pins it:

```text
PROBE card0_driver virtio_gpu 0.1.0 (virtio GPU)
PROBE card0_param_3D_FEATURES 1
PROBE card0_capsets_answered VIRGL(1) VIRGL2(2) VENUS(4) CROSS_DOMAIN(5) DRM(6)
```

That list is not a statement about what the host can serve. Setting `NO_VIRGL`, which makes every
guest `ResourceCreate2d` fail, leaves the same five capsets answered.

### What `--gpu` changes, measured on this Mac

Measured 2026-09-05 on one host: Apple M1 (MacBook Air), macOS 26.6.2, Homebrew libkrun 1.19.4,
virglrenderer 1.3.0, a debug build. The guest is the pinned Alpine 3.24.1 aarch64 minirootfs. One
host, one date; nothing here is claimed for any other, and the Linux rows are not yet measured.

| What | Command | Result |
|---|---|---|
| Venus in the host renderer | `nm -gU $(brew --prefix virglrenderer)/lib/libvirglrenderer.dylib \| grep -c vkr_` | 0, and `otool -L` shows no Vulkan: built without Venus, the same wall the Arch box showed |
| The feature probe | `cargo xtask setup` | the gpu feature answers yes |
| Boot with `--gpu`, no display | `tormoni run --gpu -- /bin/true` | exit 0: `krun_set_gpu_options2` with Venus in the flags is tolerated by a Venus-less renderer |
| DRM nodes under `--gpu` | `tormoni run --gpu -- sh -c 'ls /dev/dri'` | `card0`, `renderD128` |
| No DRM nodes without it | `tormoni run -- sh -c 'ls /dev/dri'` | `none`: the offer exists exactly when asked for |
| The refusal, feature absent | `tormoni run --gpu` on a gpu-less libkrun | not reproducible here (this libkrun answers the feature); the pinned text is "--gpu needs a libkrun built with the gpu feature, which this one lacks" |

### What guest acceleration still needs

Two things, neither of them a Tormoni code change:

- **A guest Mesa carrying the virgl Gallium driver.** Alpine's `mesa-dri-gallium` is built with
  Iris, llvmpipe and radeonsi only: `strings libgallium-26.1.6.so` finds no virgl driver. A guest
  with Mesa installed prints `virtio_gpu: driver missing`, falls back to `kms_swrast`, and reports
  `OpenGL core profile renderer: llvmpipe`. Alpine packages no virgl Gallium driver at all, so it
  is not reachable by adding a package.
- **A host virglrenderer built with Venus.** `mesa-vulkan-virtio` installs its ICD in the guest,
  but `vulkaninfo` gets `ERROR_INITIALIZATION_FAILED` with no physical device. The cause is the
  host: virglrenderer 1.3.0 exports zero `vkr_` symbols and does not link `libvulkan` on both
  measured hosts (Arch, and Homebrew above). Passing Venus in the flags changes nothing, which is
  how the host was established as the limit rather than the flag.

Compute of any kind follows from those two: no GL, no Vulkan, no guest driver. Both guest-side
results were taken against a throwaway image built by installing Mesa into a copy of the guest
tree; that image is a spike and is not in the repo.

`--gpu` changed the mechanism, not the finding: it asks for Venus over `krun_set_gpu_options2` with
an SHM window, and both prerequisites still gate any acceleration. Until they are met, a display is
a 2D scanout and the guest's own rendering is software. No guest workload using the offer has been
measured anywhere (`vulkaninfo`, llama.cpp's Vulkan backend, a Venus frame), and the ML guest image
that would run them is a scaffold no host has built.
