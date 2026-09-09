# Building guest images

Tormoni uses minimal Alpine Linux guest images containing only the components
required to run workloads and the static guest agent. Guest rootfs trees are built reproducibly
without root privileges.

## A tree to boot right away (`cargo xtask init`)

`cargo xtask init` puts the pinned Alpine minirootfs and the static guest agent where `tormoni` resolves
a root (`--root`, then `$TORMONI_GUEST_ROOT`, then `~/.local/share/tormoni/rootfs`), with the `/results`
mount point and a resolver beside them. It runs on either platform: the base is a tarball and the
agent is a static musl build, so neither step needs `apk`.

```console
cargo xtask init                      # this host's arch, into tormoni's default root
cargo xtask init --root DIR --force   # somewhere else, replacing a tree already there
```

`cargo xtask dist` writes the same tree for this host's guest and packs it into the release
artifact (`rootfs.tar.gz` under `Tormoni.app/Contents/Resources`, or under `share/tormoni` in the
Linux tarball), which `install.sh` unpacks to that default root. A release is whole without a
checkout.

It is a fixture, not the image: no runtimes, no locked closure, no reproducibility claim, and the
tree carries the invoking user's ownership rather than `0:0`. `--force` refuses a directory that
does not already look like a guest tree, so it cannot be pointed at a home directory.

## Building rootfs trees (`cargo xtask build-rootfs`)

Image builds are orchestrated through `cargo xtask build-rootfs`.

```console
cargo xtask build-rootfs               # minimal guest image (artifacts/rootfs-guest)
cargo xtask build-rootfs --desktop     # desktop image (artifacts/rootfs-desktop)
cargo xtask build-rootfs --arch aarch64 # target another architecture
cargo xtask build-rootfs --ml          # the ML scaffold (artifacts/rootfs-ml)
```

The builder is Linux, either architecture: what runs during a build is `apk.static`, a Linux ELF,
with `fakeroot` beside it. `--arch` picks the *guest's* architecture independently of the
builder's, because the install runs `--no-scripts` and nothing from the closure executes.

### Unprivileged rootfs assembly

Guest rootfs trees are constructed on Linux without requiring `root` or Docker:
- **`apk.static`**: Alpine's static package manager fetches and extracts `.apk` packages into the
  staged tree (`--no-scripts`).
- **`fakeroot`**: Wraps file creation so files in `artifacts/rootfs-guest` are owned by `uid 0`
  (root) in the filesystem metadata rather than the builder's user ID. That is what lets two builds
  on different machines produce identical trees and hashes, which `--verify` checks by building
  twice.
- **Cross-architecture builds**: Because package installation extracts static archives without
  executing guest scripts, a Linux builder of either architecture (`x86_64` or `aarch64`) can
  assemble an image for the other architecture.

## Image closures

### Minimal guest closure (`artifacts/rootfs-guest`)

The default sandbox image includes:
- Alpine Linux base packages (musl libc, busybox, standard POSIX utilities).
- Python 3 runtime for script execution.
- `guest-agent`: The static musl Rust binary baked at `/usr/local/bin/guest-agent`.

### Desktop guest closure (`artifacts/rootfs-desktop`)

The desktop sandbox image adds graphical and terminal session support for `--display` runs:
- **`cage`**: A minimal Wayland kiosk compositor based on wlroots.
- **`foot`**: A fast, lightweight Wayland terminal emulator.
- **`seatd` & `udev`**: Seat management and device node creation inside the guest.
- **`tormoni-session`**: A helper session supervisor that launches `seatd`, starts `cage`, and runs
  `foot` in a Wayland kiosk session.

### ML guest closure (`artifacts/rootfs-ml`)

`--ml` names a third closure: llama.cpp, `mesa-vulkan-virtio` (the Venus ICD), `vulkan-tools`, and
python3 with numpy for a CPU baseline. It is a scaffold: its package names are unverified, it has
no lockfile, and no host has built it, so it is not in the set `cargo xtask vendor` mirrors. It
joins that set with its first lockfile.

## Reproducibility and lockfiles

To ensure deterministic builds across hosts, package closures are locked:
- **Lockfiles**: `xtask/rootfs-packages.x86_64.lock` records the exact package versions and SHA-256
  hashes (with per-architecture lockfiles generated on build).
- **Verification**: `cargo xtask build-rootfs --verify` builds the image twice, asserting that the
  staged trees match byte-for-byte and that package versions match the lockfile.
- **Updating pins**: `cargo xtask build-rootfs --update-lock` re-pins the package closure when
  Alpine package versions update upstream.

## Offline vendoring (`cargo xtask vendor`)

For offline or air-gapped dev environments, `cargo xtask vendor` downloads all sha-pinned upstream
archives (Alpine base tarballs, `apk.static`, and `.apk` package closures) into a local `vendor/`
mirror directory.

- `cargo xtask vendor --verify` checks the local vendor mirror against its hash manifest without
  making network calls.
