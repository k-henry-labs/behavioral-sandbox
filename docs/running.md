# Running a sandbox

A sandbox with no flags gets three things: its root image, read-only; loopback; and one empty
directory at `/results` for what it produces. Nothing else is shared, everything past that is opted
into on the command line, and the posture is printed (`--dry-run` shows it without booting) because
what is shared **is** the policy.

```console
tormoni run -- uname -a
```

The guest root is `--root`, else `$TORMONI_GUEST_ROOT`, else `~/.local/share/tormoni/rootfs`, which
is where `cargo xtask init` puts one. The flag and the variable are for a tree somewhere else.

## Installing

```console
curl -fsSL https://raw.githubusercontent.com/kendricklawton/tormoni/main/install.sh | sh
```

The address redirects to the latest release's `install.sh` on GitHub, as Ollama's does. The script
is one function called on its last line, so a truncated download runs nothing. It refuses any host
but macOS on ARM64 and Linux on x86_64, downloads that host's artifact and `SHA256SUMS`, verifies
the digest, and then:

- **macOS**: puts `Tormoni.app` in `/Applications`, symlinks `/usr/local/bin/tormoni` to the
  `tormoni` inside it (as this user first, then with `sudo`), and opens the app unless
  `TORMONI_NO_START` is set.
- **Linux**: needs `sudo`, untars `bin/tormoni`, `bin/Tormoni`, a desktop entry, the icon and the
  guest tree under the first of `/usr/local`, `/usr` or `/` whose `bin` is on `PATH`, and warns if
  `/dev/kvm` is not readable and writable by you (membership of the `kvm` group, usually).
- **Both**: unpacks the guest tree the release carries to `~/.local/share/tormoni/rootfs`, keeping
  a tree it did not write unless `TORMONI_REPLACE_ROOTFS=1`; asks before running the package
  manager's libkrun line (`--yes` or `TORMONI_INSTALL_YES=1` skips the question; with no terminal
  it prints the line and exits 1); and `TORMONI_VERSION=0.0.1` takes that tag's assets.
  `--dry-run` prints every command that would change the machine and runs none, which is what the
  gate's tests drive it with.

What it cannot claim, each with its mechanism:

- The binaries are signed ad hoc (`codesign --sign -`), not notarized, and carry no Developer ID.
  `curl` and `unzip` set no quarantine attribute, so the app the script installs opens; the same
  zip saved by a browser is quarantined, and Gatekeeper refuses it until a Developer ID and
  notarization exist.
- On macOS the binaries load libkrun by the install name Homebrew gave it and libkrunfw from
  `/opt/homebrew/lib`, baked in at build time (`crates/krun/build.rs`), so Homebrew at that prefix
  is a requirement of the release. On Linux the binary was linked in a Fedora 43 container, so its
  glibc floor is 2.42, and the loader needs `libkrun.so.1`: Arch, Fedora and openSUSE package it,
  Debian and Ubuntu do not.
- The `tormoni` inside the bundle carries the hypervisor entitlement; a byte copy keeps it, a
  relink loses it.
- The prompt, `sudo`, and the real download are outside the gate: they are run by hand on each
  host before a tag.

### Removing

macOS: `/Applications/Tormoni.app`, `/usr/local/bin/tormoni`, and under `~/.local/share/tormoni`
the `rootfs` tree and `rootfs.sha256`. Linux: `/usr/local/bin/tormoni`, `/usr/local/bin/Tormoni`,
`/usr/local/share/tormoni`, `/usr/local/share/applications/ai.tormoni.app.desktop`,
`/usr/local/share/icons/hicolor/512x512/apps/ai.tormoni.app.png`, plus the same two user paths.
Run records under `~/.local/share/tormoni/runs` are yours and are not removed.

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
| `--env KEY=VALUE` | One guest environment entry. Repeatable. The record keeps the name, never the value. | nothing |
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

**An environment entry reaches the guest whole and the record by name.** The VM is given
`KEY=VALUE`, because that is what `--env` is for; what the record, `tormoni show` and the export
carry is `KEY` alone. `an_env_value_reaches_the_guest_and_never_the_record_or_the_posture_print`
drives the pair, and `tormoni_record::env_key` cuts again as the file is written, so a value
cannot reach it even from a caller that skipped the first cut.

`tormoni export` packages that directory as one ustar file a stock `tar` extracts. A symlink a guest
planted inside `results/` is archived as a link entry, never opened:
`a_symlink_is_archived_as_itself_and_never_followed` in `tormoni-record` holds it to that. A file that
grows or shrinks mid-export keeps the size its header pinned, so exporting a live run stays
readable.

## The notebook

`Tormoni` opens on the notebook, with a sidebar reaching four tabs:

- **The list**: every run, newest first, live ones with a thumbnail of their display. `Select`
  turns each ended row into a tick box, `All` takes every one of them, and `Remove` asks about what
  was selected. A live run has no box: it is refused a delete until it stops.
- **One run**: its posture, output and results beside its live display (keyboard and pointer go
  to the guest), with Stop and Shell while it runs, Re-run and Delete after, and Export always.
- **The start form**: every posture flag as a field, summarised in the record's own posture
  sentence ("This sandbox will: ..."), confirmed before anything boots.
- **The cookbook**: runs worth trying, on five shelves, each one press from a filled form. A press
  fills the form and stops there, so its posture is read before it boots, and the `tormoni` line a
  card shows is built from the same fields the form takes
  (`the_line_an_entry_shows_is_the_posture_it_fills_in`). [Examples](./examples.md) has the same
  runs with their output.
- **Settings** (the platform's command with `,`, from any screen): light,
  dark or the desktop's own mode, and the interface scale, both applied live, the screen a plain
  launch opens on, and what this machine has to run a sandbox with (the `tormoni` binary and guest
  root it found). The picks are kept across launches in a file beside the runs directory.
  `--theme` and `$TORMONI_THEME` outrank the saved palette, and `--open` the saved screen, for one
  launch.

Every delete asks first, in a modal naming the run or the count it would remove, with Cancel and
Delete. Escape and a press outside answer it the same way Cancel does, and cancelling a selection's
question gives the selection back rather than dropping it.
`deleting_one_run_asks_first_and_never_asks_about_a_live_one` and
`a_selection_removes_what_was_selected_and_only_behind_the_confirm` hold both paths to it.

`Tormoni NAME` opens straight onto a run; `--open list|new|settings` opens a screen. Starting and
stopping go through the `tormoni` binary beside the app, or under its bundle's `Contents/Resources`
(`$TORMONI_CLI` overrides which one).

## Platform notes

On macOS ARM64 the same verbs work, but this platform's libkrun builds no `--sound` and no guest
input backend, and a display is viewed in `Tormoni` rather than a window of the helper's own; sign
the binary again after any build (`cargo xtask sign`). **A bundle is named by its `Info.plist`,
not by the file inside it**, so the executable stays `tormoni-app`, the name cargo writes, and
`CFBundleName` is what the menu bar and the Dock read (measured on this host, macOS 26.6.2,
2026-09-09: a bundle whose executable was `tormoni-app` and whose `CFBundleName` was `Zephyr` read
`Zephyr` in both). The items under that title are the exception, since the toolkit builds them
from the process name, which the app sets at startup. `cargo xtask bundle` adds what a bare binary
carries none of: that plist, the identifier `ai.tormoni.app`, the icon, and the `tormoni` under
`Contents/Resources`, entitled for the hypervisor before the bundle is sealed over it. A bare
`target/debug/tormoni-app` has no plist, so it is named for the build: `cargo xtask app` builds the
pair, assembles the bundle and starts the copy inside it, with its output still on the terminal. `--gpu` boots here and the
guest sees `card0` and `renderD128`; this host's virglrenderer carries no Venus, so the offer ends
at the device. The [Architecture](./architecture.md) page carries the fuller status and the
measurements.
