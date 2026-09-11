# tormoni-go

The Go SDK for Tormoni.

Tormoni runs untrusted code inside a hardware-isolated virtual machine on your own machine — KVM on
Linux, Hypervisor.framework on macOS, via libkrun. It is not a container. Every run leaves a record:
the posture it was given, its captured output, and whatever it wrote to `/results`.

This package is a thin wrapper around the `tormoni` CLI: a subprocess and a JSON parse. Standard
library only, no dependencies.

> **Note:** the module path below uses a placeholder organisation. Change `kendricklawton` to the
> org that will actually publish this repository, in `go.mod` and in the import paths.

## Install

```sh
go get github.com/kendricklawton/tormoni/sdk/go
```

You also need the `tormoni` CLI on your `PATH`.

## Use

```go
c := tormoni.New()
run, err := c.Run(context.Background(), []string{"echo", "hi"}, nil)
if err != nil { log.Fatal(err) }
fmt.Print(run.Stdout) // "hi\n"
fmt.Println(run.OK()) // true
```

The guest's stdout and stderr do not reach your process's streams. They are captured and come back
inside the record.

## Errors versus failed runs

`tormoni run` exits with the guest command's own status, so the subprocess exit code tells you
nothing about whether Tormoni worked. The rule this package follows is the CLI's own:

- **If stdout parses as one JSON document, the sandbox ran.** You get a `*Run` and a `nil` error.
  Read `EndKind` and `EndCode` to learn how the command finished — never infer it from one string.
- **If stdout does not parse, Tormoni itself failed.** You get an error.

So a guest command exiting non-zero is not an error:

```go
run, err := c.Run(ctx, []string{"sh", "-c", "exit 3"}, nil)
// err == nil
// run.OK() == false, run.EndKind == tormoni.EndExit, *run.EndCode == 3
```

while a missing binary or an unanswering hypervisor is:

```go
if errors.Is(err, tormoni.ErrNotFound) {
    // The tormoni binary is not installed, or not where we looked.
}
var tErr *tormoni.Error
if errors.As(err, &tErr) {
    // tErr.Stderr is Tormoni's own message, verbatim. tErr.ExitCode is the
    // binary's exit status. This package never rewords upstream's wording.
}
```

`err` also unwraps to `context.Canceled` and `context.DeadlineExceeded`, so cancelling the context
you passed is distinguishable from a real failure.

`*Error` carries `Args`, the argument list the CLI was given, for debugging. The value of every
`--env` entry in it is replaced with `<redacted>` — Tormoni deliberately never writes an environment
value to a record, and an error from this package will not undo that by spilling one into your logs.
The variable names are left intact, and the guest still receives the real values.

## Options

`RunOptions` maps one-to-one onto the CLI's flags. Every field is optional; a zero value means "do
not pass the flag", and the CLI's own default stands. This package adds no defaults of its own.

| Field | Flag | Notes |
|---|---|---|
| `Root` | `--root DIR` | Guest root tree. Unset: `$TORMONI_GUEST_ROOT`, then `~/.local/share/tormoni/rootfs` |
| `VCPUs` | `--vcpus N` | `*int`, so a deliberate `0` is distinguishable from unset. CLI default 1 |
| `MemMiB` | `--mem MIB` | `*int`, same reason. CLI default 512 |
| `Workdir` | `--workdir DIR` | Guest working directory |
| `Mounts` | `--mount GUESTDIR=HOSTDIR` | Repeats, in order. A host directory, read-write, at a guest path |
| `Shares` | `--share TAG=HOSTPATH` | Repeats, in order. A virtiofs device the guest mounts by tag |
| `Net` | `--net none\|tsi` | `none` (CLI default) is no network at all; `tsi` reaches what the host can |
| `Rootfs` | `--rootfs read-only\|writable` | CLI default `read-only` |
| `Env` | `--env KEY=VALUE` | Repeats, in order. Passed whole and verbatim |
| `Name` | `--name NAME` | Names the sandbox |
| `NoResults` | `--no-results` | Drops the default `/results` mount |
| `GPU` | `--gpu` | Optional device; see below |
| `Sound` | `--sound` | Optional device; see below |
| `Display` | `--display WxH[@HZ]` | Optional device; see below |

```go
vcpus, mem := 4, 2048
run, err := c.Run(ctx, []string{"sh", "-c", "make -j4 > /results/build.log"}, &tormoni.RunOptions{
    VCPUs:   &vcpus,
    MemMiB:  &mem,
    Mounts:  []tormoni.Mount{{Guest: "/src", Host: "/home/you/project"}},
    Workdir: "/src",
    Net:     "tsi",
    Env:     []string{"CI=1"},
})
```

`GPU`, `Sound` and `Display` are the least portable part of the surface. A host whose libkrun was
built without a backend refuses them, and `--display` does not currently boot on macOS at all. This
package passes them through and lets the CLI's refusal reach you as an `*Error`; it does not probe
for support.

## Environment values go out, names come back

`Env` entries are sent to the guest whole. Tormoni deliberately never writes an environment *value*
to a record, so `run.Posture.Env` is a list of **names only** — and no field anywhere on a `Run`
carries a value.

```go
run, _ := c.Run(ctx, []string{"env"}, &tormoni.RunOptions{Env: []string{"API_KEY=s3cret"}})
fmt.Println(run.Posture.Env) // [API_KEY]
```

## Truncated output

When the capture cap cuts the output, `run.OutputTruncated` is true and `run.Stdout` is a **prefix**,
not the whole thing. `StdoutBytes` and `StderrBytes` count what the guest actually wrote.

```go
if run.OutputTruncated {
    log.Printf("showing %d of %d bytes", len(run.Stdout), run.StdoutBytes)
}
```

## The rest of the surface

```go
// Settle a posture without booting anything.
plan, err := c.DryRun(ctx, []string{"echo", "hi"}, nil)
fmt.Println(plan.Posture.MemMiB, plan.EndKind == "") // 512 true

// Read a record that already happened, by id or by name.
run, err := c.Show(ctx, "1789085063489-run-81523")

// List sandboxes. With all=false, only live ones.
runs, err := c.Runs(ctx, true)
for _, r := range runs {
    fmt.Println(r.RunID, *r.Live, r.EndKind)
}
```

Rows from `Runs` omit the captured output — `Stdout`, `Stderr`, `Files` and `Dir` are empty, because
reading every run's bytes to list them would be a directory walk per row. Rows carry `Live`. Use
`Show` to read one run in full.

## Finding the binary

A `Client` runs `tormoni` from `PATH`. To point somewhere else, in order of precedence:

```go
c := tormoni.NewWithPath("/opt/tormoni/bin/tormoni") // 1. an explicit path
// 2. the TORMONI_CLI environment variable, which Tormoni's own desktop app uses
// 3. "tormoni" on PATH
```

## Timestamps

`StartedMS` and `EndedMS` are integer epoch milliseconds, as the CLI writes them.

```go
started := time.UnixMilli(run.StartedMS)
if run.EndedMS != nil {
    took := time.UnixMilli(*run.EndedMS).Sub(started)
}
```

## Testing against this SDK

The test suite never boots a VM: it puts a stub `tormoni` script in a temporary directory and
asserts on the argv the client builds and the documents it parses. You can do the same — point
`Client.Path` at your own stub.

```sh
go test ./...                                  # no hypervisor, no network
TORMONI_INTEGRATION=1 go test -run Integration ./...   # a real install
```

## Cancellation

Cancelling the context kills the sandbox process. If the CLI leaves anything behind holding its
stdout — a supervisor process, say — the call stops waiting on that pipe after ten seconds rather
than blocking on a straggler, and returns whatever record was already written.

## What this SDK does not do

No HTTP client, no retries, no caching or local index, and no parsing of human-readable output. A
failure is an answer. Long-lived sandboxes (`tormoni up`) are not covered in v1.

## License

Apache-2.0, with the rest of the repository. See [LICENSE](../../LICENSE).
