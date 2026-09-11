# tormoni-rust

[![crates.io](https://img.shields.io/crates/v/tormoni.svg)](https://crates.io/crates/tormoni)
[![docs.rs](https://docs.rs/tormoni/badge.svg)](https://docs.rs/tormoni)
[![CI](https://github.com/tormoni/tormoni-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/tormoni/tormoni-rust/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../../LICENSE)

The Rust SDK for [Tormoni](https://github.com/kendricklawton/tormoni), which runs untrusted code inside a
hardware-isolated virtual machine on your own machine — KVM on Linux, Hypervisor.framework on
macOS, via libkrun. It is not a container. Every run leaves a record: the posture it was given,
its captured output, and whatever it wrote to `/results`.

This crate is a thin, faithful wrapper around the `tormoni` command-line tool. It does not
implement virtualization and it does not speak HTTP — it spawns a subprocess and parses one JSON
document. Standard library, `serde` and `serde_json`; nothing else.

## Install

You need the `tormoni` CLI on your `PATH` (or pointed at by `TORMONI_CLI`), then:

```toml
[dependencies]
tormoni = "0.1"
```

## Use

```rust
let run = tormoni::Tormoni::new().run(["echo", "hi"])?;
assert_eq!(run.stdout, "hi\n");
assert!(run.ok());
```

Options mirror `tormoni run`'s flags one-for-one, and a configured client is reusable:

```rust
use tormoni::{Net, Rootfs, Tormoni};

let sandbox = Tormoni::new()
    .vcpus(2)
    .mem(1024)
    .net(Net::Tsi)
    .rootfs(Rootfs::Writable)
    .mount("/work", "/home/you/project")
    .env("API_KEY", std::env::var("API_KEY")?);

let run = sandbox.run(["sh", "-c", "cd /work && make test > /results/log.txt"])?;

println!("{}", run.stdout);
for file in &run.files {
    println!("{} ({} bytes)", file.path, file.size_bytes);
}
```

| Method | Flag / verb |
|---|---|
| `run(command)` | `tormoni run --json -- COMMAND...` |
| `dry_run(command)` | `tormoni run --json --dry-run -- COMMAND...` — settles a posture without booting |
| `show(id)` | `tormoni show --json ID\|NAME` |
| `runs(all)` | `tormoni ls --json [--all]` |
| `.root(dir)` | `--root DIR` |
| `.vcpus(n)` | `--vcpus N` |
| `.mem(mib)` | `--mem MIB` |
| `.workdir(dir)` | `--workdir DIR` |
| `.mount(guest, host)` | `--mount GUESTDIR=HOSTDIR`, repeatable |
| `.share(tag, host)` | `--share TAG=HOSTPATH`, repeatable |
| `.net(Net::None \| Net::Tsi)` | `--net none\|tsi` |
| `.rootfs(Rootfs::ReadOnly \| Rootfs::Writable)` | `--rootfs read-only\|writable` |
| `.env(key, value)` | `--env KEY=VALUE`, repeatable |
| `.name(name)` | `--name NAME` |
| `.no_results()` | `--no-results` |
| `.gpu()`, `.sound()`, `.display(geometry)` | `--gpu`, `--sound`, `--display WxH[@HZ]` |

The SDK adds no defaults of its own: an option you do not set is absent from the command line, and
the CLI's own default stands.

`--gpu`, `--sound` and `--display` are the least portable part of the surface. A host whose libkrun
was built without the backend refuses them, and `--display` does not currently boot on macOS at
all. The SDK passes them through without probing; the CLI's refusal reaches you as
`Error::Failed`.

## A failed command is not an error

`tormoni run` exits with the guest command's own exit status, so the child's exit code tells you
nothing about whether Tormoni itself worked. The rule is the contract's:

- **If stdout parses as one JSON document, the sandbox ran.** A guest that exits non-zero is an
  `Ok(Run)` whose `ok()` is `false`. Nothing is raised.
- **If stdout does not parse, it is an operational failure** — a missing binary, an absent guest
  root, a hypervisor that will not answer. `Error::Failed` carries Tormoni's own stderr text,
  unchanged.

```rust
use tormoni::{End, Error, Tormoni};

match Tormoni::new().run(["sh", "-c", "exit 3"]) {
    Ok(run) if run.ok() => println!("{}", run.stdout),
    Ok(run) => match run.end {
        Some(End::Exit(code)) => println!("exited {code}: {}", run.stderr),
        Some(End::Signal(sig)) => println!("killed by signal {sig}"),
        other => println!("ended {other:?}"),
    },
    Err(Error::NotFound { .. }) => println!("install the tormoni CLI"),
    // Tormoni's messages are written for a person and maintained upstream. Do not re-word them.
    Err(e) => println!("{e}"),
}
```

Never parse a single string to learn how a run ended: `end` is reconstructed from the
`end_kind` / `end_code` pair, both of which stay available on the `Run`.

## Two things worth knowing about a record

- **`posture.env` is a list of names, never values.** Tormoni deliberately never writes an
  environment value to a record, so there is nowhere in this SDK for one to come back.
- **`output_truncated`** is true when the capture cap cut the output. When it is, `stdout` and
  `stderr` are prefixes, not the whole thing; `stdout_bytes` and `stderr_bytes` are the honest
  sizes.

## Finding the binary

In order: the path passed to `Tormoni::binary()`, then `$TORMONI_CLI` (the same variable Tormoni's
desktop app uses), then `tormoni` on `PATH`.

## Examples

Runnable, in `examples/`:

```sh
cargo run --example plan     # settles a posture without booting anything — works with no hypervisor
cargo run --example hello    # boots a sandbox and runs one command
cargo run --example list     # lists the runs on disk
```

## Tests

The suite never boots a VM — it points the client at a stub `tormoni` script in a temporary
directory that records its argv and prints a canned document. No hypervisor, no network:

```sh
cargo test
```

One integration test does boot a real sandbox. It is ignored by default and needs a working
install:

```sh
TORMONI_TEST_LIVE=1 cargo test --test live -- --ignored
```

## Scope

`run` is one-shot; there is no `Sandbox` object with a lifecycle, because Tormoni has no such
thing. `tormoni up`, for long-lived sandboxes, is not covered in v1. There are no retries — a
failure is an answer — and no local cache or index, because the records directory is the state.

## License

Apache-2.0, with the rest of the repository. See [LICENSE](../../LICENSE).
