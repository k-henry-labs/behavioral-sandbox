# Running a sandbox

A sandbox with no flags gets three things: its root image, read-only; loopback; and one empty
directory at `/results` for what it produces. Nothing else is shared, everything past that is opted
into on the command line, and the posture is printed (`--dry-run` shows it without booting) because
what is shared **is** the policy.

```console
tormoni run --root ~/.local/share/tormoni/rootfs -- uname -a
```

The guest root falls back to `$TORMONI_GUEST_ROOT`, then `~/.local/share/tormoni/rootfs`, so after one
`export` the `--root` flag can be dropped.

## The verbs

| Verb | What it does |
|---|---|
| `tormoni run -- CMD` | Runs one command in a fresh sandbox and exits with its status. |
| `tormoni shell` | Opens an interactive session (or any command) on a pty in a fresh sandbox. |
| `tormoni up --name NAME` | Starts a sandbox that outlives the command, reachable afterwards by name. |
| `tormoni ls` | Lists the sandboxes running on this machine; `--all` adds the ended runs. |
| `tormoni exec NAME -- CMD` | Runs a command in a sandbox that is already up; `--tty` attaches a terminal. |
| `tormoni stop NAME` | Stops a running sandbox. |
| `tormoni show ID\|NAME` | Prints one run's record: what it could touch, what it printed, what it wrote. |
| `tormoni rm ID\|NAME` | Removes one run's record and everything it captured. |
| `tormoni export ID\|NAME` | Writes one run as a ustar `.tar` (`--to` picks a directory or exact path). |

There is no daemon: a VM is a helper process listening on a control socket in the runtime directory,
so a sandbox started by the CLI is visible to the app and the other way round.

## The posture flags

`run`, `shell` and `up` share these; the record keeps what was granted.

| Flag | Grants | Default |
|---|---|---|
| `--rootfs writable` | The guest writes through to the shared image tree. | `read-only` |
| `--net tsi` | libkrun's socket impersonation: the guest reaches what the host can. | `none` |
| `--mount GUESTDIR=HOSTDIR` | A host directory read-write at a guest path. Repeatable. | nothing |
| `--share TAG=HOSTPATH` | An extra virtiofs device for a guest that mounts by tag. Repeatable. | nothing |
| `--display WIDTHxHEIGHT[@HZ]` | A virtio-gpu display in a window; closing the window stops the sandbox. | none |
| `--sound` | A virtio-snd card on the host's audio server: playback **and** capture. | off |
| `--gpu` | A 3D virtio-gpu into the host renderer (virgl + Venus offered); the guest brings its own driver. | off |
| `--env KEY=VALUE` | One guest environment entry. Repeatable. | nothing |
| `--vcpus N`, `--mem MIB` | Sizing; also `$TORMONI_VCPUS` and `$TORMONI_MEM_MIB`. | 1 vCPU, 512 MiB |
| `--no-results` | Drops the default `/results` mount. | mounted |

## Configuration layering

Configuration is resolved in precedence order: 1. Command line flags. 2. Environment variables
(`$TORMONI_VCPUS`, `$TORMONI_MEM_MIB`, `$TORMONI_GUEST_ROOT`, `$TORMONI_RUNS_DIR`, `$TORMONI_RUNS_KEEP`,
`$TORMONI_OUTPUT_CAP_KIB`, `$TORMONI_LOG`, `$TORMONI_CLI`, `$TORMONI_THEME`). 3. The nearest `.tormoni.toml` in or above
the current working directory. 4. User defaults in `~/.tormoni.toml`. 5. Built-in defaults.

## What a run leaves

Every run leaves one directory under `$TORMONI_RUNS_DIR`, else `$XDG_DATA_HOME/tormoni/runs`, else
`~/.local/share/tormoni/runs`: the `record` file (the posture as settled, the timings, the end), the
captured output (capped at `$TORMONI_OUTPUT_CAP_KIB`, 4 MiB by default, with a `.truncated` sidecar when
cut), and `results/`, the directory the guest saw as `/results`. Ended runs beyond `$TORMONI_RUNS_KEEP`
(200 by default) are pruned oldest-first when a new run starts.

`tormoni export` packages that directory as one ustar file a stock `tar` extracts. A symlink a guest
planted inside `results/` is archived as a link entry, never opened:
`a_symlink_is_archived_as_itself_and_never_followed` in `tormoni-record` holds it to that. A file that
grows or shrinks mid-export keeps the size its header pinned, so exporting a live run stays
readable.

## The notebook

`tormoni-app` opens on the notebook, with a sidebar reaching its three screens:

- **The list**: every run, newest first, live ones with a thumbnail of their display. `Clear
  history` removes the ended runs behind an inline confirm; live runs stay.
- **One run**: its posture, output and results beside its live display (keyboard and pointer go
  to the guest), with Stop and Shell while it runs, Re-run and Delete after, and Export always.
- **The start form**: every posture flag as a field, summarised in the record's own posture
  sentence ("This sandbox will: ..."), confirmed before anything boots.
- **Settings** (the platform's command with `,`, from any screen): light, dark or the desktop's own
  mode, and the interface scale, both applied live, the screen a plain launch opens on, and what
  this machine has to run a sandbox with (the `tormoni` binary and guest root it found). The picks are
  kept across launches in a file beside the runs directory. `--theme` and `$TORMONI_THEME` outrank the
  saved palette, and `--open` the saved screen, for one launch.

`tormoni-app NAME` opens straight onto a run; `--open list|new|settings` opens a screen. Starting and
stopping go through the `tormoni` binary beside the app (`$TORMONI_CLI` overrides which one).

## Platform notes

On macOS ARM64 the same verbs work, but this platform's libkrun builds no `--sound` and no guest
input backend, and a display is viewed in `tormoni-app` rather than a window of the helper's own; sign
the binary again after any build (`cargo xtask sign`). `cargo xtask bundle` assembles
`artifacts/Tormoni.app` from the built pair, which is what makes the window's menu bar
say `Tormoni` rather than the file name `tormoni-app`; the `tormoni` copied inside it is signed there. `--gpu` boots here and the
guest sees `card0` and `renderD128`; this host's virglrenderer carries no Venus, so the offer ends
at the device. The [Architecture](./architecture.md) page carries the fuller status and the
measurements.
