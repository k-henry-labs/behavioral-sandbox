# Developing Tormoni

Everything needed to build, test and release Tormoni from source.

For contribution process — issue first, commit sign-off, what a pull request needs, and the five
design rules every change is reviewed against — see [CONTRIBUTING.md](CONTRIBUTING.md). For what the
product does and how a person uses it, see <docs/SUMMARY.md>.

## Prerequisites

**A host that virtualises.** macOS on Apple silicon (Hypervisor.framework) or Linux on `x86_64`
with `/dev/kvm` readable and writable by your user, which usually means membership of the `kvm`
group. Most cloud VMs cannot do this; nested virtualisation is off by default nearly everywhere.

`cargo xtask setup` asks this host its own question and names what is missing — KVM on Linux,
Hypervisor.framework on macOS, rather than a device the other one has not got. Run it first.

**libkrun and libkrunfw.** A C library and a shared object holding a Linux kernel, so neither
arrives through cargo:

```console
brew tap libkrun/krun && brew install libkrun libkrunfw   # macOS
sudo pacman -S libkrun libkrunfw                          # Arch
sudo dnf install libkrun libkrunfw                        # Fedora
```

Debian and Ubuntu package neither.

> **macOS: take virglrenderer from the tap, not from core.** The tap's `libkrun.rb` declares
> `depends_on "virglrenderer"` unqualified, so Homebrew resolves it to homebrew-core's stock build,
> which does not export `virgl_renderer_resource_get_map_ptr`. The CLI survives that — an executable
> binds lazily and a headless run never reaches the GPU path — but anything that loads libkrun with
> eager binding fails outright, which is how the Python SDK's extension module was found refusing to
> import. Fix it with:
>
> ```console
> brew uninstall --ignore-dependencies virglrenderer
> brew install libkrun/krun/virglrenderer
> ```

**Rust.** Pinned exactly by `rust-toolchain.toml`, currently 1.98.0 with `rustfmt` and `clippy`.
rustup reads that file, so no manual toolchain step is needed. The pin is exact on purpose: a
floating stable means a lint that passes on your machine can fail in CI.

**Nothing else is required to build.** The gate needs no privilege and boots no VM.

## Initial setup

```console
git clone https://github.com/kendricklawton/tormoni.git
cd tormoni
cargo xtask setup            # what this host can and cannot do
cargo xtask init             # a guest tree where tormoni looks for one
cargo build                  # the CLI and the app
```

`cargo xtask init` puts the pinned Alpine minirootfs and the static guest agent at
`~/.local/share/tormoni/rootfs` (or `$TORMONI_GUEST_ROOT`). It runs on either platform — the base is
a tarball and the agent is a static musl build, so neither step needs `apk`. Without it nothing
boots, because there is no guest to boot.

On macOS a binary must carry the hypervisor entitlement before it can start a VM, and **any later
`cargo build` or `cargo test` replaces the binary and drops the signature with it**:

```console
cargo xtask sign             # re-entitle the built tormoni; run it after a build, not once
```

## Build and run

```console
cargo build                            # debug
cargo build --release                  # release; the benches require this
cargo run -p tormoni -- run -- echo hi # the CLI
cargo xtask app                        # build, bundle and start the GUI
```

`cargo xtask app` exists because the Dock's label and the menu bar name a bare executable by its
file name, and only a bundle carries a name of its own. It starts the copy inside
`artifacts/Tormoni.app` so the platform calls it `Tormoni`; its output still reaches your terminal.
Anything after `--` reaches the app. Off macOS it starts the built binary.

### The xtask verbs

| Command | What it does |
|---|---|
| `cargo xtask ci` | The gate: fmt, prose-drift, clippy `-D warnings`, build, test, docs, `cargo deny`, sign |
| `cargo xtask setup` | What this host can and cannot do |
| `cargo xtask init` | A bootable guest tree where `tormoni` looks for one, on any host |
| `cargo xtask sign` | macOS: re-entitle the built `tormoni` for Hypervisor.framework |
| `cargo xtask bundle` | macOS: assemble `artifacts/Tormoni.app` from the built pair |
| `cargo xtask app` | Build, bundle and start the GUI under its own name |
| `cargo xtask dist` | This host's release under `dist/`, as `install.sh` downloads it |
| `cargo xtask build-rootfs` | The guest image: Alpine + runtimes + the static agent (Linux only) |
| `cargo xtask vendor` | Mirror every sha-pinned upstream input for offline builds |
| `cargo xtask icons` | Cut the Lucide release to the glyphs `crates/app/src/icons.rs` names |
| `cargo xtask fonts` | Cut Inter and Geist Mono to what `crates/app/src/fonts.rs` compiles in |
| `cargo xtask app-icon` | Cut `crates/app/icon/tormoni.svg` into the `.icns` and PNG the bundle names |
| `cargo xtask bench-boot` | Cold-boot latency as percentiles. Needs `/dev/kvm` and a **release** build |
| `cargo xtask bench-footprint` | Per-sandbox memory footprint of a cohort of idle VMs |
| `cargo xtask bench-frames` | The guest-to-host frame path, headless |
| `cargo xtask fuzz` | `cargo fuzz` over the untrusted-input decoders. Nightly; never part of `ci` |
| `cargo xtask fuzz-smoke` | Every fuzz target briefly, fail-fast — the per-PR smoke |
| `cargo xtask semver-check` | The pinned API surface against a baseline rev. Needs `cargo-semver-checks` |

`icons`, `fonts` and `app-icon` are dev steps whose **outputs are committed**, because the app
compiles them in and the gate builds with no network.

## Project structure

### Workspace crates

Directories stay short and packages carry the `tormoni-` prefix, so a package is its directory plus
that prefix — with one exception: `crates/cli` builds `tormoni`, the bare name going to the command
a person types.

| Path | Package | Role |
|---|---|---|
| `crates/supervisor` | `tormoni-supervisor` | Spawns, tracks, stops and reaps the helper processes that **are** VMs |
| `crates/krun` | `tormoni-krun` | The safe wrapper over libkrun. **The one crate that may use `unsafe`**, because the library is C |
| `crates/channel` | `tormoni-channel` | The host↔guest wire protocol, shared by both ends |
| `crates/guest-agent` | `tormoni-guest-agent` | The in-guest agent. Builds to nothing off Linux |
| `crates/record` | `tormoni-record` | The run record: posture, captured output, `/results`, one directory per run |
| `crates/input` | `tormoni-input` | The guest's keyboard and pointer |
| `crates/cli` | `tormoni` | The CLI, its verbs, and `execute_sandbox` — the one run path |
| `crates/serve` | `tormoni-serve` | `tormoni serve`: one box, one token, and the meter |
| `crates/app` | `tormoni-app` | The GUI, `Tormoni`, on iced |
| `crates/test-support` | `tormoni-test-support` | Shared test fixtures. Dev-only, never shipped |
| `xtask` | `xtask` | Dev orchestration. Never shipped |

### SDKs

Not workspace members — `sdk/rust`, `sdk/python` and `sdk/js` are in the workspace `exclude` list
and carry their own lockfiles.

| Path | Binding | Notes |
|---|---|---|
| `sdk/rust` | Direct | Depends on `crates/cli` by path, so it is `publish = false` until the core is published |
| `sdk/python` | PyO3 + maturin | One `abi3` wheel per platform serves every CPython from 3.9 up |
| `sdk/js` | napi-rs | `index.d.ts` is **generated** from the Rust structs, never hand-written. Ships as five packages: see below |
| `sdk/go` | Subprocess | The one SDK still built on an argv; cgo's costs were judged not worth it |

Because Go builds a command line by hand, `every_run_flag_is_one_the_sdks_know` in
`xtask/src/lints.rs` fails when `tormoni run` grows a flag `sdk/go` does not pass. The other three
get that from the compiler.

### Other directories

- `fuzz` — the `cargo fuzz` harness. Its own detached workspace, nightly, never in the gate.
- `install.sh` — what `curl -fsSL https://raw.githubusercontent.com/kendricklawton/tormoni/main/install.sh | sh` runs.

### How the JS SDK ships

Five npm packages, the shape esbuild and napi both use. The main package `tormoni` carries **no
binary at all** — 11 kB of JavaScript and types — and declares four `optionalDependencies`:

| Package | `os` / `cpu` |
|---|---|
| `@tormoni/js-darwin-arm64` | darwin / arm64 |
| `@tormoni/js-darwin-x64` | darwin / x64 |
| `@tormoni/js-linux-x64-gnu` | linux / x64 |
| `@tormoni/js-linux-arm64-gnu` | linux / arm64 |

npm installs only the one matching the host, because each manifest declares `os` and `cpu`. The
generated loader in `index.js` tries a `.node` beside itself first — which is what makes a local
`npx vitest` work without publishing anything — and falls back to the scoped package.

Their manifests live in `sdk/js/npm/<triple>/` and **are committed**; the `.node` files copied in
beside them at release time are not. `napi create-npm-dir -t .` regenerates the manifests from the
`napi` block in `package.json`, and `napi version` bumps them.

**`napi.package.name` must stay set to `@tormoni/js`.** Without it the loader falls back to
unscoped names like `tormoni-darwin-arm64`, which squats four global npm names instead of four
inside a scope you own. The `@tormoni` scope has to exist on npm before any of this publishes.

**Do not add `*.node` back to the main package's `files`.** It shipped the host's own binary to
every consumer, which is 1.6 MB nobody on another platform can load.

## Guest images

Guest rootfs trees are minimal Alpine, holding only what a workload needs and the static guest
agent. They are built **without root**, and `--verify` builds one twice and compares the two trees
byte for byte.

### A tree to boot right away

`cargo xtask init` is described under [Initial setup](#initial-setup). It is a fixture, not the
image: no runtimes, no locked closure, no reproducibility claim, and the tree carries the invoking
user's ownership rather than `0:0`. `--force` refuses a directory that does not already look like a
guest tree, so it cannot be pointed at a home directory.

`cargo xtask dist` writes the same tree for this host's guest and packs it into the release
artifact, which `install.sh` unpacks to that default root. A release is whole without a checkout.

### Building a closure

```console
cargo xtask build-rootfs                # minimal guest image
cargo xtask build-rootfs --desktop      # desktop image, for `--display` runs
cargo xtask build-rootfs --arch aarch64 # target another architecture
cargo xtask build-rootfs --ml           # the ML scaffold
```

**The builder is Linux, either architecture.** What runs during a build is `apk.static`, a Linux
ELF, with `fakeroot` beside it. `--arch` picks the *guest's* architecture independently of the
builder's, because the install runs `--no-scripts` and nothing from the closure executes — which is
also what makes cross-architecture assembly possible at all.

`fakeroot` is what gives the staged files `uid 0` in their metadata rather than the builder's user
id, and that is what lets two builds on different machines produce identical trees.

Three closures:

- **Minimal**: the Alpine base (musl, busybox, the POSIX utilities), the runtimes `GUEST_PACKAGES`
  names in `xtask/src/rootfs.rs`, and `guest-agent` baked at `/usr/local/bin/guest-agent`.
- **Desktop**: adds `cage` (a wlroots kiosk compositor), `foot` (a Wayland terminal), `seatd` and
  `eudev`, `xkeyboard-config` and one font. No Mesa driver — the session renders with pixman. Plus
  `tormoni-session`, not a package but a program the build writes, which starts `seatd`, then
  `cage`, and runs `foot` in it.
- **ML**: llama.cpp, the Venus ICD, `vulkan-tools`, and python3 with numpy for a CPU baseline. **A
  scaffold**: its package names are unverified, it has no lockfile, and no host has built it, so
  `cargo xtask vendor` does not mirror it. It joins that set with its first lockfile.

### Pins and offline builds

`xtask/rootfs-packages.x86_64.lock` records exact package versions and SHA-256 hashes, one lockfile
per architecture. `--verify` builds twice and asserts both that the trees match byte for byte and
that versions match the lockfile; `--update-lock` re-pins when Alpine moves upstream.

`cargo xtask vendor` mirrors every sha-pinned upstream archive locally, and `--verify` checks that
mirror against its manifest without touching the network.

## The control socket and the guest wire

Two tiers: a host control socket for discovery and display leasing, and a vsock wire protocol for
running a command inside a guest.

### There is no daemon

A running sandbox **is** a helper process (`tormoni __vmm`) listening on a Unix socket under the
user's runtime directory. Sockets live at `$XDG_RUNTIME_DIR/tormoni/<name>.sock`, falling back to
`$TMPDIR` and then `/tmp`. The directory is created `0700`, and its ownership and mode are checked
at runtime before a socket in it is trusted — because those fallbacks are shared.

The socket directory is the registry. `tormoni ls` scans it, but **file existence is not the
liveness test**: `socket::is_live` makes a non-blocking connect, and a socket whose process died is
cleared by `socket::clear_if_stale`. The agent socket sits alongside at `<name>.agent`, and a
detached run's log at `<name>.log`.

### The control protocol

A request is one word on one line; the answer begins `ok` or `err <why>`, and a caller reads at most
4096 bytes. A word the VM does not know is answered with the words it *does* speak, rather than a
closed connection — `an_unknown_request_is_answered_with_what_this_vm_speaks` holds that.

- `info` — `ok`, then the machine's shape as `key value` lines (`proto`, `pid`, `vcpus`, `mem_mib`,
  `net`, `rootfs`, `channel`), which is the row `tormoni ls` prints.
- `stop` — `ok` **first**, and the process exits after, so a caller learns the request was accepted
  rather than inferring it from a dropped connection.
- `display` — leases the scanout. The answer carries the sealed memfd holding the frame slots and
  their layout over `SCM_RIGHTS`, and the connection then streams one record per present until the
  caller closes it. Refused by a VM with no display.
- `input` — after `ok`, the connection carries `kbd|ptr TYPE CODE VALUE` lines until the caller
  closes it, and whatever those lines left pressed is released then. Refused by a VM with no
  display, which has no devices.

### The guest wire

`crates/channel` is length-prefixed framing over AF_VSOCK (port 1024), with a Unix socket fallback.
A session opens with the magic `AGCH` and a `u16` version; a mismatch is rejected immediately.
Frames are then `tag(u8) · len(u32-le) · payload`.

**`len` is checked against `MAX_PAYLOAD` (1 MiB) before anything is allocated**, so a length the
guest chose cannot size a host allocation.

| Tag | Name | Direction | Payload |
|---|---|---|---|
| 1 | `Exec` | Host → Guest | Command, arguments, working directory, environment pairs. |
| 2 | `Stdout` | Guest → Host | Bytes from the command's stdout. |
| 3 | `Stderr` | Guest → Host | Bytes from the command's stderr. |
| 4 | `Exit` | Guest → Host | Exit code (`i32`). |
| 5 | `Error` | Guest → Host | Agent error message, sanitized. |
| 6 | `PutFile` | Host → Guest | Injected file path and content. |
| 7 | `File` | Guest → Host | A file from the guest's `/results`. |
| 8 | `TimedOut` | Guest → Host | The command outran its deadline. |
| 9 | `ExecPty` | Host → Guest | Interactive shell, with PTY `cols` and `rows`. |
| 10 | `Stdin` | Host → Guest | Bytes for stdin, or for a PTY session. |
| 11 | `Resize` | Host → Guest | New PTY `cols` and `rows`. |

Two things the framing does beyond framing. Sensitive environment values and payload buffers
`zeroize` on drop, so a secret does not linger in freed memory. And a guest's error message is
capped at 4 KiB and escaped for ASCII control characters and Unicode bidirectional control code
points — the ones a terminal would otherwise act on, and the ones Trojan Source relies on.

### The agent is not the boundary

`crates/guest-agent` is a static musl binary baked into the image at `/usr/local/bin/guest-agent`.
It serves `Exec` and `ExecPty`, manages process lifetimes, attaches PTYs, and reads and writes
`/results`.

It runs **inside** the guest and is **not** part of the host isolation boundary. What contains a
compromised agent is the CPU, through KVM or Hypervisor.framework — not anything the agent does.

## Testing

```console
cargo test --workspace                            # everything the gate runs
cargo test -p tormoni-record                      # one crate
cargo test -p tormoni-supervisor spawn            # one test by name
cargo test -p tormoni --test e2e -- --ignored     # the ones that boot a guest
```

**The tests that boot a guest are `#[ignore]`d, and each names its own prerequisite** — `/dev/kvm`
and a guest tree. A test whose prerequisite is missing skips itself, and cargo counts a skipped test
as a pass, which is the failure mode that rule exists to avoid.

Some suites compile to nothing on macOS and say so at the end of a gate run: `tormoni-guest-agent`
reaps through a pidfd and listens on AF_VSOCK, the helper's own window needs a thread other than the
main one, and the benches read `/proc`.

### The SDK suites

Each SDK has its own runner, and none is part of `cargo xtask ci`:

```console
cd sdk/rust   && cargo test
cd sdk/go     && go test ./...
cd sdk/js     && npx napi build --platform && npx vitest run
cd sdk/python && maturin build --out /tmp/wheels && pip install --force-reinstall /tmp/wheels/*.whl && pytest
```

The Python and JS suites need their native module **rebuilt** before they run: a stale `.so` or
`.node` tests the previous commit. Neither artifact is committed.

## Benchmarking

`bench-boot`, `bench-footprint` and `bench-frames` each need `/dev/kvm`, a guest tree and a
**release** `tormoni`. They report nearest-rank percentiles with the host and date, per design rule
5 — a number that cannot be defended is withdrawn rather than published.

## Code quality

`cargo xtask ci` is the gate, and it needs no privilege:

```console
cargo xtask ci
```

It runs, in order: `cargo fmt --all --check`, the prose-drift lint, the `install.sh` shell check,
the toolchain/MSRV agreement check, the fuzz lockfile check, `clippy -D warnings`, build and test
under `RUSTFLAGS=-D warnings`, `cargo doc` under `RUSTDOCFLAGS=-D warnings`, `cargo deny check`, and
finally `cargo xtask sign` — last, because a signature does not reliably outlive a later cargo
command.

**The prose-drift lint reads `git ls-files`.** A backticked repo path in any `.rs` or `.md` file
must exist, so a rename that rots a comment fails the gate rather than being discovered later.

`cargo deny` carries a closed licence allow-list in `deny.toml`. A new dependency's licence needs an
entry **with a reason**, in the style of the ones already there.

The SDK crates are excluded from the workspace, so `cargo fmt --all` does not reach them:

```console
for d in sdk/rust sdk/python sdk/js; do (cd $d && cargo fmt --all); done
```

## Releasing

**A human makes the tag.** Nothing in CI decides to release; the tag is the decision.

`.github/workflows/release.yml` triggers on a `v*` tag and runs four jobs:

1. `check-version` — the tag equals the workspace version in `Cargo.toml` and is **annotated**,
   since the notes come from the annotation.
2. `dist-macos` — `cargo xtask dist` on `macos-15`, producing `Tormoni-macos-aarch64.zip`.
3. `dist-linux` — `cargo xtask dist` in a Fedora container, producing `tormoni-linux-x86_64.tgz`.
4. `release` — concatenates the checksums, adds `install.sh`, verifies, and publishes.

A tag containing `-` is marked a prerelease, which `releases/latest` skips, so `v0.0.5-rc1`
exercises the whole pipeline without moving what `install.sh` downloads.

Two limits worth knowing before promising a release to anyone:

- **The Linux artifact's glibc floor is the build container's.** Building on a newer distro than
  your users run means the binary will not load for them.
- **There is no Linux ARM64 target.** `install.sh` and `xtask/src/dist.rs` accept
  `("macos","aarch64")` and `("linux","x86_64")` and refuse everything else.

## Additional resources

- [CONTRIBUTING.md](CONTRIBUTING.md) — how to contribute, and the five design rules
- [README.md](README.md) — what Tormoni is
- [SECURITY.md](SECURITY.md) — reporting a vulnerability
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
