# boxdesk-js

The JavaScript / TypeScript SDK for [Boxdesk](https://github.com/kendricklawton/boxdesk).

Boxdesk runs untrusted code inside a **hardware-isolated virtual machine** on your own machine —
KVM on Linux, Hypervisor.framework on macOS, via libkrun. It is not a container. Every run leaves a
record: the posture it was given, its captured output, and whatever it wrote to `/results`.

This package is a thin, faithful wrapper around the `boxdesk` command line tool. It shells out, it
reads one JSON document, and it hands it to you. No HTTP, no runtime dependencies.

## Install

```bash
npm install boxdesk
```

You also need the `boxdesk` CLI on your `PATH`. Node 20 or newer.

## Use

```ts
import { Boxdesk } from "boxdesk";

const run = await new Boxdesk().run(["node", "-e", "console.log(6*7)"]);
console.log(run.stdout); // "42\n"
console.log(run.ok);     // true
```

CommonJS works too:

```js
const { Boxdesk } = require("boxdesk");
```

## A failed command is not an error

`boxdesk run` exits with the guest command's own status, so the process exit code tells you nothing
about whether Boxdesk itself worked. The rule this SDK follows:

> If stdout parses as one JSON document, **the sandbox ran**. If it does not, **Boxdesk failed**.

So a guest command that exits non-zero comes back as an ordinary `Run` and raises nothing:

```ts
const run = await boxdesk.run(["sh", "-c", "exit 3"]);
run.ok;      // false
run.endKind; // "exit"
run.endCode; // 3
```

Read `endKind` before `endCode`. `endKind` is one of `"exit"`, `"signal"`, `"stopped"`, `"gone"`,
`"failed"`, `"unknown"`, or `null` while a run is still going; `endCode` carries a number only for
`"exit"` and `"signal"`. Never parse a single string to learn how a run ended.

A missing binary, an absent guest root, or a hypervisor that will not answer *is* an error:

```ts
import { BoxdeskError, BoxdeskNotFoundError } from "boxdesk";

try {
  await boxdesk.run(["true"]);
} catch (error) {
  if (error instanceof BoxdeskNotFoundError) {
    // The CLI could not be found or executed.
  } else if (error instanceof BoxdeskError) {
    error.stderr;   // Boxdesk's own words, verbatim. Show them unchanged.
    error.exitCode; // 2
  }
}
```

Both extend `BoxdeskException`. Boxdesk's messages are written for a person and maintained
upstream, so this SDK passes them through rather than re-wording them.

## API

### `new Boxdesk({ path })`

`path` is optional. It defaults to `$BOXDESK_CLI` — the same variable Boxdesk's desktop app uses —
and then to `boxdesk` on `PATH`.

| Method | Returns | What it does |
|---|---|---|
| `run(command, options?)` | `Promise<Run>` | Boots a fresh sandbox, runs one command, waits |
| `runSync(command, options?)` | `Run` | The same, blocking |
| `dryRun(command, options?)` | `Promise<Run>` | Settles the posture without booting anything |
| `show(id)` | `Promise<Run>` | The record of a run that already happened, by id or name |
| `runs(all?)` | `Promise<Run[]>` | Lists sandboxes; live ones only unless `all` is true |

`command` is an array of strings. Everything in it is the guest's command, and its first word is
resolved by the **guest's** `PATH`, not the host's.

`runs()` rows carry `live` and omit `stdout`, `stderr`, `files` and `dir`, because reading every
run's bytes to list them is a directory walk per row.

### Options

Each maps onto exactly one flag of `boxdesk run`. Anything you leave out is left off the command
line, so the CLI's own defaults stand.

| Option | Flag | Meaning |
|---|---|---|
| `root` | `--root DIR` | Guest root tree. Default `$BOXDESK_GUEST_ROOT`, then `~/.local/share/boxdesk/rootfs` |
| `vcpus` | `--vcpus N` | vCPUs. Default 1 |
| `mem` | `--mem MIB` | Guest RAM in MiB. Default 512 |
| `workdir` | `--workdir DIR` | Guest working directory |
| `mounts` | `--mount GUESTDIR=HOSTDIR` | `[guestDir, hostDir]` pairs, read-write. Repeatable |
| `shares` | `--share TAG=HOSTPATH` | `[tag, hostPath]` pairs; an extra virtiofs device the guest mounts by tag. Repeatable |
| `net` | `--net none\|tsi` | `none` is the default and means no network at all. `tsi` lets the guest reach what the host can |
| `rootfs` | `--rootfs read-only\|writable` | Default `read-only` |
| `env` | `--env KEY=VALUE` | A `Record<string, string>`; one flag per entry |
| `name` | `--name NAME` | Names the sandbox |
| `noResults` | `--no-results` | Drops the default `/results` mount |
| `gpu` | `--gpu` | Optional device |
| `sound` | `--sound` | Optional device |
| `display` | `--display WxH[@HZ]` | Optional device |

`gpu`, `sound` and `display` are the least portable part of the surface: a host whose libkrun was
built without a backend refuses them, and `--display` does not currently boot on macOS at all. The
SDK passes them through and lets the CLI's refusal reach you; it does not probe for support.

```ts
const run = await boxdesk.run(["sh", "-c", "cp report.csv /results/"], {
  mounts: [["/mnt", "/home/you/project"]],
  net: "tsi",
  env: { API_KEY: process.env.API_KEY! },
  vcpus: 2,
  mem: 1024,
});

run.files; // [{ path: "report.csv", sizeBytes: 4096 }]
run.dir;   // the record directory on the host
```

### `Run`

```ts
interface Run {
  readonly runId: string;
  readonly name: string;
  readonly verb: string;
  readonly command: readonly string[];
  readonly posture: Posture;
  readonly startedMs: number;          // integer epoch milliseconds
  readonly endedMs: number | null;
  readonly endKind: EndKind | null;
  readonly endCode: number | null;
  readonly pid: number | null;
  readonly stdout?: string;
  readonly stderr?: string;
  readonly stdoutBytes?: number;
  readonly stderrBytes?: number;
  readonly outputTruncated?: boolean;  // true means stdout is a prefix
  readonly files?: readonly RunFile[];
  readonly dir?: string;
  readonly live?: boolean;             // on runs() rows only
  readonly ok: boolean;                // endKind === "exit" && endCode === 0
}
```

A `Run` is a record of something that already happened, so every field is
`readonly` — mutating one would change nothing on disk. `run()`, `runSync()` and
`dryRun()` accept a `readonly string[]`, so you can feed a previous record's
`command` straight back in.

Two things worth knowing:

- **`outputTruncated`** is true when the capture cap cut the output. When it is true, `stdout` is a
  prefix. Check it before treating captured output as complete.
- **`posture.env` holds names, never values.** Boxdesk deliberately never writes an environment
  value to a record, so no value is available here or anywhere else in this SDK.

## Development

```bash
npm install
npm run verify    # lint, typecheck, test, build
```

Or one at a time:

```bash
npm run lint      # biome check
npm run format    # biome check --write
npm run typecheck # tsc --noEmit
npm test          # no hypervisor and no network needed
npm run build     # tsup -> ESM + CJS + .d.ts
```

The suite talks to a stub `boxdesk` binary written to a temp directory, so it never boots a VM.
There is one integration test that uses the real binary, off unless you ask for it:

```bash
BOXDESK_INTEGRATION=1 npm test
```

## License

Apache-2.0
