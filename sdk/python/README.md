# tormoni-python

The Python SDK for Tormoni.

Tormoni runs untrusted code inside a **hardware-isolated virtual machine** on your own machine —
KVM on Linux, Hypervisor.framework on macOS, via libkrun. It is not a container. Every run leaves
a record: the posture it was given, its captured output, and whatever it wrote to `/results`.

This package is a thin, faithful wrapper around the `tormoni` CLI. It spawns the binary with
`--json`, parses the one document the binary prints, and hands it back. No HTTP client, no async
runtime, no third-party dependencies — the standard library only.

## Install

```bash
pip install tormoni
```

You also need the `tormoni` CLI itself. The SDK looks for it on `PATH`, honours the `TORMONI_CLI`
environment variable (the same one Tormoni's desktop app uses), and takes an explicit path:
`Tormoni(executable="/opt/tormoni/bin/tormoni")`.

## Three lines

```python
import tormoni

run = tormoni.Tormoni().run(["python3", "-c", "print(6*7)"])
print(run.stdout)   # "42\n"
print(run.ok)       # True
```

## A failed command is not an error

This is the one thing to understand about the API.

`tormoni run` exits with the *guest command's* own exit status, so the process exit code says
nothing about whether Tormoni worked. The SDK draws the line where the CLI does:

- **The guest command exited non-zero.** Not an exception. You get a `Run` whose `ok` is false and
  whose `end_code` carries the status.
- **Tormoni itself failed** — binary missing, guest root absent, hypervisor not answering. That
  raises, carrying the CLI's own message unchanged.

```python
run = client.run(["sh", "-c", "exit 3"])
run.ok         # False  -- no exception was raised
run.end_kind   # "exit"
run.end_code   # 3
```

Never parse a single string to learn how a run ended. `end_kind` is one of `"exit"`, `"signal"`,
`"stopped"`, `"gone"`, `"failed"`, `"unknown"`, or `None` while the run is still going, and
`end_code` carries a number only for `"exit"` and `"signal"`.

```python
import tormoni

try:
    run = tormoni.Tormoni().run(["cargo", "build"], mounts={"/src": "/home/you/project"})
except tormoni.TormoniNotFound as error:
    ...                 # the CLI is not installed
except tormoni.TormoniError as error:
    print(error.stderr) # Tormoni's own words, never reworded here
    print(error.exit_code)
```

A document that parses but is not a run record — a missing `run_id`, a `posture` of the wrong
shape — is also a `TormoniError`. Tormoni failing to hold up its end of the contract is an
operational failure like any other, so nothing raw (`KeyError`, `TypeError`) ever escapes the SDK.

Nothing is retried. A failure is an answer.

## Truncated output

When the capture cap cuts the output, `output_truncated` is true and `stdout` is a **prefix**, not
the whole thing. `stdout_bytes` tells you how much the guest actually wrote.

```python
if run.output_truncated:
    print(f"showing {len(run.stdout)} of {run.stdout_bytes} bytes")
```

## Options

Every option maps onto exactly one CLI flag. The SDK adds no options and no defaults of its own:
leave one unset and the CLI's default stands.

| Option | Flag | Meaning |
|---|---|---|
| `root` | `--root DIR` | Guest root tree. Default `$TORMONI_GUEST_ROOT`, then `~/.local/share/tormoni/rootfs` |
| `vcpus` | `--vcpus N` | vCPUs. Default 1 |
| `mem_mib` | `--mem MIB` | Guest RAM in MiB. Default 512 |
| `workdir` | `--workdir DIR` | Guest working directory |
| `mounts` | `--mount GUESTDIR=HOSTDIR` | A host directory, read-write, at a guest path. Repeatable |
| `shares` | `--share TAG=HOSTPATH` | An extra virtiofs device a guest mounts by tag. Repeatable |
| `net` | `--net none\|tsi` | `none` (the default) means no network at all. `tsi` lets the guest reach what the host can |
| `rootfs` | `--rootfs read-only\|writable` | Default `read-only` |
| `env` | `--env KEY=VALUE` | One guest environment entry. Repeatable |
| `name` | `--name NAME` | Names the sandbox |
| `no_results` | `--no-results` | Drops the default `/results` mount |
| `gpu` | `--gpu` | Optional device |
| `sound` | `--sound` | Optional device |
| `display` | `--display WxH[@HZ]` | Optional device |

`gpu`, `sound` and `display` are the least portable part of the surface: a host whose libkrun was
built without a backend refuses them, and `--display` does not currently boot on macOS. The SDK
passes them through and lets the CLI's refusal reach you rather than probing for support itself.

The repeatable options take a mapping, `(key, value)` pairs, or literal `"KEY=VALUE"` strings —
whichever reads better. Any iterable works, including a generator or `dict.items()`:

```python
client.run(["make"], mounts={"/src": "/home/you/project"}, env={"CI": "1"})
client.run(["make"], mounts=[("/src", "/home/you/project")])
client.run(["make"], mounts=["/src=/home/you/project"])
```

A malformed shape is rejected where you wrote it, not silently turned into malformed flags —
`mounts=("/src", "/home/you/project")` (a bare pair, missing its enclosing list) raises rather
than becoming `--mount /src --mount /home/you/project`.

The first word of the command is resolved by the **guest's** `PATH`, not the host's.

### Environment values are never recorded

The guest receives the whole `KEY=VALUE` entry, but the record keeps only the name. `posture.env`
is a list of names — there is no field anywhere that holds a value, by design.

```python
run = client.run(["env"], env={"API_KEY": "s3cret"})
run.posture.env   # ["API_KEY"]
```

## The rest of the API

```python
client = tormoni.Tormoni()

# Settle and inspect the posture without booting anything.
plan = client.dry_run(["cargo", "build"], vcpus=4, net="tsi")
plan.posture.vcpus     # 4
plan.end_kind          # None -- nothing ran

# The record of a run that already happened, by id or name.
client.show("1789085063489-run-81523")

# List runs. Without all=True, only live sandboxes.
for row in client.runs(all=True):
    print(row.run_id, row.end_kind, row.live)
```

`runs()` rows carry `live` and **omit** the captured output — reading every run's bytes to list
them would be a directory walk per row — so `row.stdout` is `None` there. Don't mistake that for
an empty stdout; call `show()` for the full record.

Options can also be gathered up and reused:

```python
from tormoni import RunOptions

build = RunOptions(vcpus=4, mem_mib=2048, mounts={"/src": "/home/you/project"}, workdir="/src")
client.run_with(["cargo", "build"], build)
client.run_with(["cargo", "test"], build)
```

`Sandbox` is an alias for `Tormoni`, for when it reads better at a call site.

## Development

```bash
pip install -e ".[dev]"
pytest          # no hypervisor, no network, no /dev/kvm
mypy --strict
```

The suite never boots a VM. It writes a stub `tormoni` binary to a temporary directory, puts it at
the front of `PATH`, and asserts the exact argv the client built and how it reads the document that
comes back. One integration test uses a real install and is skipped unless you ask for it:

```bash
TORMONI_INTEGRATION=1 pytest tests/test_integration.py
```

## License

Apache-2.0, with the rest of the repository. See [LICENSE](../../LICENSE).
