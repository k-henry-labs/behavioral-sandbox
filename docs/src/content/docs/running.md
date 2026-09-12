---
title: "Running a sandbox"
description: "What a sandbox gets with no flags, and what each flag opens."
---

A sandbox with no flags gets three things: its root image, read-only; loopback; and one empty
directory at `/results` for what it produces. Nothing else is shared, everything past that is opted
into on the command line, and the posture is printed (`--dry-run` shows it without booting) because
what is shared **is** the policy.

```console
boxdesk run -- uname -a
```

The guest root is `--root`, else `$BOXDESK_GUEST_ROOT`, else `~/.local/share/boxdesk/rootfs`, which
is where `cargo xtask init` puts one. The flag and the variable are for a tree somewhere else.

## Installing

```console
curl -fsSL https://raw.githubusercontent.com/boxdesk/boxdesk/main/install.sh | sh
```

The address redirects to the latest release's `install.sh` on GitHub, as Ollama's does. The script
is one function called on its last line, so a truncated download runs nothing. It refuses any host
but macOS on ARM64 and Linux on x86_64, downloads that host's artifact and `SHA256SUMS`, verifies
the digest, and then:

- **macOS**: puts `Boxdesk.app` in `/Applications`, symlinks `/usr/local/bin/boxdesk` to the
  `boxdesk` inside it (as this user first, then with `sudo`), and opens the app unless
  `BOXDESK_NO_START` is set.
- **Linux**: needs `sudo`, untars `bin/boxdesk`, `bin/Boxdesk`, a desktop entry, the icon and the
  guest tree under the first of `/usr/local`, `/usr` or `/` whose `bin` is on `PATH`, and warns if
  `/dev/kvm` is not readable and writable by you (membership of the `kvm` group, usually).
- **Both**: unpacks the guest tree the release carries to `~/.local/share/boxdesk/rootfs`, keeping
  a tree it did not write unless `BOXDESK_REPLACE_ROOTFS=1`; asks before running the package
  manager's libkrun line (`--yes` or `BOXDESK_INSTALL_YES=1` skips the question; with no terminal
  it prints the line and exits 1); and `BOXDESK_VERSION=0.0.1` takes that tag's assets.
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
- The `boxdesk` inside the bundle carries the hypervisor entitlement; a byte copy keeps it, a
  relink loses it.
- The prompt, `sudo`, and the real download are outside the gate: they are run by hand on each
  host before a tag.

### Removing

macOS: `/Applications/Boxdesk.app`, `/usr/local/bin/boxdesk`, and under `~/.local/share/boxdesk`
the `rootfs` tree and `rootfs.sha256`. Linux: `/usr/local/bin/boxdesk`, `/usr/local/bin/Boxdesk`,
`/usr/local/share/boxdesk`, `/usr/local/share/applications/ai.boxdesk.app.desktop`,
`/usr/local/share/icons/hicolor/512x512/apps/ai.boxdesk.app.png`, plus the same two user paths.
Run records under `~/.local/share/boxdesk/runs` are yours and are not removed.

## The verbs

| Verb | What it does |
|---|---|
| `boxdesk run -- CMD` | Runs one command in a fresh sandbox and exits with its status. |
| `boxdesk shell` | Opens an interactive session (or any command) on a pty in a fresh sandbox. |
| `boxdesk up --name NAME` | Starts a sandbox that outlives the command, reachable afterwards by name. |
| `boxdesk ls` | Lists the sandboxes running on this machine; `--all` adds the ended runs. |
| `boxdesk exec NAME -- CMD` | Runs a command in a sandbox that is already up; `--tty` attaches a terminal. |
| `boxdesk stop NAME` | Stops a running sandbox. |
| `boxdesk show ID\|NAME` | Prints one run's record: what it could touch, what it printed, what it wrote. |
| `boxdesk rm ID\|NAME` | Removes one run's record and everything it captured. |
| `boxdesk export ID\|NAME` | Writes one run as a ustar `.tar` (`--to` picks a directory or exact path). |
| `boxdesk snapshot new NAME` | Writes a snapshot: a named posture to make sandboxes from. |
| `boxdesk snapshot ls` | Lists the snapshots on this machine; `--json` for a client. |
| `boxdesk snapshot show NAME` | Prints what a sandbox made from one would boot and could touch. |
| `boxdesk snapshot rm NAME` | Removes a snapshot. The sandboxes already made from it are untouched. |
| `boxdesk pull REF` | Fetches an OCI image and flattens it into a guest root this machine can boot. |
| `boxdesk image ls\|rm` | The images pulled onto this machine, and removing one. |
| `boxdesk registry add\|ls\|rm` | Where images come from, and who this machine is when it asks. |
| `boxdesk volume new\|ls\|show\|rm` | Directories with lives of their own, mounted into sandboxes by name. |

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
| `--vcpus N`, `--mem MIB` | Sizing; also `$BOXDESK_VCPUS` and `$BOXDESK_MEM_MIB`. | 1 vCPU, 512 MiB |
| `--no-results` | Drops the default `/results` mount. | mounted |
| `--snapshot NAME` | Starts from a snapshot's posture; every flag beside it speaks over it. | none |
| `--image REF` | Boots an image pulled by `boxdesk pull` instead of a tree on disk. Refuses `--root`. | none |
| `--volume NAME:GUESTDIR` | Mounts a volume at a guest path. Repeatable. | nothing |

## Snapshots

A **snapshot is a posture with a name on it**: what a sandbox made from it boots, what it may
touch, and how much machine it gets. It is a template, not a captured VM — nothing pauses or
copies a running sandbox, and `ROADMAP.md` keeps that separate feature apart from this one.

```console
boxdesk snapshot new devbox --net tsi --vcpus 4 --mem 4096 -- sleep infinity
boxdesk run --snapshot devbox -- cargo test
```

`snapshot new` takes the posture flags above, minus `--env`: a posture keeps the *names* of
environment entries and never what they are set to, so a snapshot has nowhere to put a value and
does not pretend otherwise. It refuses to replace a snapshot of the same name unless given
`--force`.

**A flag beside `--snapshot` always speaks over it, including the flag that closes a posture.**
`--net none` against a snapshot asking for `tsi` gives no network. This is why `--net` and
`--rootfs` carry no default value in the parser: with one, typing the default would have been
indistinguishable from typing nothing, and a snapshot would have opened what the caller had just
shut. `a_posture_flag_speaks_over_the_snapshot_even_when_it_closes_one` is the check.

Snapshots live in `$BOXDESK_SNAPSHOTS_DIR`, else `$XDG_DATA_HOME/boxdesk/snapshots`, else
`~/.local/share/boxdesk/snapshots`, one file per snapshot, in the same `key value` lines a record
uses — written by the same writer, so a posture grown in one reaches the other.

## Images

**An image here is a directory, because that is what libkrun boots.** There is no disk image and
no kernel to unpack: the VM is handed a tree over virtiofs, and an OCI image flattened is a tree.
That is why adopting OCI costs so little — every image anyone has already published is reachable,
and no new infrastructure has to run.

```console
boxdesk pull alpine:3.20
boxdesk run --image alpine:3.20 -- /bin/busybox echo hello
```

A reference reads the way every other client reads one: `alpine` is `docker.io/library/alpine`,
`org/img` is a Docker Hub repository, and what makes the first element a registry is a dot, a
colon, or its being `localhost`. Images live in `$BOXDESK_IMAGES_DIR`, else
`$XDG_DATA_HOME/boxdesk/images`, else `~/.local/share/boxdesk/images`, one flattened tree per
manifest digest with a file of references beside them.

**Every blob is verified against its digest before `tar` opens it.** A layer that does not hash to
what the manifest named is refused and removed, so nothing unverified reaches the extractor.
Layers are flattened in the manifest's order with whiteouts applied first: a `.wh.<name>` entry
deletes what a lower layer put there and `.wh..wh..opq` empties the directory it sits in, both
against the tree as it stands *before* this layer lands on it, and the markers never survive into
the root.

**A pulled image carries no boxdesk guest agent**, so `run` works from one and `shell` and `up` do
not — they dial an agent on a vsock a stock image has never heard of. `pull` says so when it
finishes rather than leaving it to be discovered by watching a boot hang.

`pull` uses `curl`, `tar` and `shasum` rather than an HTTP client, a TLS stack, a gzip decoder and
a tar reader as four new dependencies. `xtask` already fetches and verifies its pinned inputs that
way and the installer is a shell script; this is the same bargain.

## Registries

A registry record says where images come from and who this machine is when it asks: a host, a
project, and a username.

```console
boxdesk registry add work ghcr.io --project acme --username buildbot
BOXDESK_REGISTRY_PASSWORD=... boxdesk pull ghcr.io/acme/img:v2
```

**No password is ever written to a file.** A registry record holds the address and the username
and nothing else; the secret is read from `$BOXDESK_REGISTRY_PASSWORD` at the moment a pull needs
it. This is the rule a posture keeps about environment values, for the same reason: a file this
tool writes is a file that gets copied, backed up and shared.
`a_password_neither_reaches_the_file_nor_comes_back_out_of_one` checks it from both sides — a
`password` line somebody planted in a registry file is read past and dropped.

Public images need no registry at all. `boxdesk pull alpine:3.20` works as it stands: an
unauthenticated request is made first, and the `WWW-Authenticate` challenge that comes back is
what names the token service to ask. That is the flow every registry implements, which is why
Docker Hub, ghcr.io and quay.io work without any of them being special-cased.

Registries live in `$BOXDESK_REGISTRIES_DIR`, else `$XDG_DATA_HOME/boxdesk/registries`, else
`~/.local/share/boxdesk/registries`.

## Volumes

**A run's writes end with the run.** What it keeps is `results/` inside its own record, tied to
that one run; a directory given with `--mount` outlives it but is a path you have to remember and
nothing manages. A volume is the third thing: a named directory boxdesk makes, keeps and can list,
mounted into any sandbox that asks for it by name.

```console
boxdesk volume new cache --about "the package cache, shared between runs"
boxdesk run --image alpine:3.20 --volume cache:/mnt -- /bin/busybox ls /mnt
```

**Local only.** A volume is a directory on this machine — not an object store, not shared between
machines. The cloud was removed from this tree deliberately, and nothing here puts it back.

A volume becomes an ordinary `--mount` before anything else sees it, so the record a run leaves
says what it could touch in the same words as ever. That also means the guest path has to be a
directory the image already has, which is the rule `--mount` already keeps, and the refusal is the
same one.

`boxdesk volume rm` refuses a volume that holds something unless given `--force`, and the window
asks the same question behind the same card. A volume exists because its contents were worth
keeping past the run that made them; removing one is the only act in either interface that takes
that away.

Volumes live in `$BOXDESK_VOLUMES_DIR`, else `$XDG_DATA_HOME/boxdesk/volumes`, else
`~/.local/share/boxdesk/volumes`, one directory each: the record, and a `data/` beside it that is
what a guest mounts. The record is never inside what a guest can write.

## Configuration layering

Configuration is resolved in precedence order: 1. Command line flags. 2. The snapshot named by
`--snapshot`, if there is one: being asked for by name outranks the machine's ambient settings.
3. Environment variables
(`$BOXDESK_VCPUS`, `$BOXDESK_MEM_MIB`, `$BOXDESK_GUEST_ROOT`, `$BOXDESK_RUNS_DIR`, `$BOXDESK_RUNS_KEEP`,
`$BOXDESK_OUTPUT_CAP_KIB`, `$BOXDESK_SNAPSHOTS_DIR`, `$BOXDESK_IMAGES_DIR`, `$BOXDESK_REGISTRIES_DIR`,
`$BOXDESK_REGISTRY_PASSWORD`, `$BOXDESK_VOLUMES_DIR`, `$BOXDESK_LOG`, `$BOXDESK_CLI`, `$BOXDESK_THEME`). 4. The nearest
`.boxdesk.toml` in or above the current working directory. 5. User defaults in `~/.boxdesk.toml`.
6. Built-in defaults.

## What a run leaves

Every run leaves one directory under `$BOXDESK_RUNS_DIR`, else `$XDG_DATA_HOME/boxdesk/runs`, else
`~/.local/share/boxdesk/runs`: the `record` file (the posture as settled, the timings, the end), the
captured output (capped at `$BOXDESK_OUTPUT_CAP_KIB`, 4 MiB by default, with a `.truncated` sidecar when
cut), and `results/`, the directory the guest saw as `/results`. Ended runs beyond `$BOXDESK_RUNS_KEEP`
(200 by default) are pruned oldest-first when a new run starts.

**An environment entry reaches the guest whole and the record by name.** The VM is given
`KEY=VALUE`, because that is what `--env` is for; what the record, `boxdesk show` and the export
carry is `KEY` alone. `an_env_value_reaches_the_guest_and_never_the_record_or_the_posture_print`
drives the pair, and `boxdesk_record::env_key` cuts again as the file is written, so a value
cannot reach it even from a caller that skipped the first cut.

`boxdesk export` packages that directory as one ustar file a stock `tar` extracts. A symlink a guest
planted inside `results/` is archived as a link entry, never opened:
`a_symlink_is_archived_as_itself_and_never_followed` in `boxdesk-record` holds it to that. A file that
grows or shrinks mid-export keeps the size its header pinned, so exporting a live run stays
readable.

## The notebook

`Boxdesk` opens on the notebook, with a sidebar reaching four tabs:

- **The list**: every run, newest first, live ones with a thumbnail of their display. `Select`
  turns each ended row into a tick box, `All` takes every one of them, and `Remove` asks about what
  was selected. A live run has no box: it is refused a delete until it stops.
- **One run**: its posture, output and results beside its live display (keyboard and pointer go
  to the guest), with Stop and Shell while it runs, Re-run and Delete after, and Export always.
- **The start form**: every posture flag as a field, summarised in the record's own posture
  sentence ("This sandbox will: ..."), confirmed before anything boots.
- **The cookbook**: runs worth trying, on five shelves, each one press from a filled form. A press
  fills the form and stops there, so its posture is read before it boots, and the `boxdesk` line a
  card shows is built from the same fields the form takes
  (`the_line_an_entry_shows_is_the_posture_it_fills_in`). [Examples](/examples/) has the same
  runs with their output.
- **Settings** (the platform's command with `,`, from any screen): light,
  dark or the desktop's own mode, and the interface scale, both applied live, the screen a plain
  launch opens on, and what this machine has to run a sandbox with (the `boxdesk` binary and guest
  root it found). The picks are kept across launches in a file beside the runs directory.
  `--theme` and `$BOXDESK_THEME` outrank the saved palette, and `--open` the saved screen, for one
  launch.

Every delete asks first, in a modal naming the run or the count it would remove, with Cancel and
Delete. Escape and a press outside answer it the same way Cancel does, and cancelling a selection's
question gives the selection back rather than dropping it.
`deleting_one_run_asks_first_and_never_asks_about_a_live_one` and
`a_selection_removes_what_was_selected_and_only_behind_the_confirm` hold both paths to it.

`Boxdesk NAME` opens straight onto a run; `--open list|new|settings` opens a screen. Starting and
stopping go through the `boxdesk` binary beside the app, or under its bundle's `Contents/Resources`
(`$BOXDESK_CLI` overrides which one).

## Platform notes

On macOS ARM64 the same verbs work, but this platform's libkrun builds no `--sound` and no guest
input backend, and a display is viewed in `Boxdesk` rather than a window of the helper's own; sign
the binary again after any build (`cargo xtask sign`). **A bundle is named by its `Info.plist`,
not by the file inside it**, so the executable stays `boxdesk-app`, the name cargo writes, and
`CFBundleName` is what the menu bar and the Dock read (measured on this host, macOS 26.6.2,
2026-09-09: a bundle whose executable was `boxdesk-app` and whose `CFBundleName` was `Zephyr` read
`Zephyr` in both). The items under that title are the exception, since the toolkit builds them
from the process name, which the app sets at startup. `cargo xtask bundle` adds what a bare binary
carries none of: that plist, the identifier `ai.boxdesk.app`, the icon, and the `boxdesk` under
`Contents/Resources`, entitled for the hypervisor before the bundle is sealed over it. A bare
`target/debug/boxdesk-app` has no plist, so it is named for the build: `cargo xtask app` builds the
pair, assembles the bundle and starts the copy inside it, with its output still on the terminal. `--gpu` boots here and the
guest sees `card0` and `renderD128`; this host's virglrenderer carries no Venus, so the offer ends
at the device. The [Architecture](/architecture/) page carries the fuller status and the
measurements.
