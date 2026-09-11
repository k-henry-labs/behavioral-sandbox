# Serving sandboxes

`tormoni serve` turns an installed copy into a box that runs sandboxes for a caller over HTTP,
instead of only for the person at the keyboard. It is for somebody who has a machine with a
hypervisor and wants their own team to use it, without anything leaving their network.

**One box, one token, one tenant.** It holds no account, no role and no membership, and it decides
nothing about who is asking. Which box a job should land on, and who is paying for it, are
questions for something that knows about more than one machine; this knows about the machine it is
on.

```console
tormoni serve                      # loopback on 8420
tormoni serve --bind 0.0.0.0:8420  # reachable from elsewhere, said out loud
tormoni serve --concurrency 8      # the most sandboxes to run at once
```

## Before it listens

It refuses rather than starting half-working, and each refusal names the thing to fix:

- **No hypervisor.** Linux wants `/dev/kvm` readable and writable by the user running it, which
  usually means membership of the `kvm` group; macOS wants a machine that virtualises at all. This
  is a host permission, and no reinstall changes it.
- **No token.** Write one to `serve.token` in the data directory at mode `0600`, or set
  `$TORMONI_SERVE_TOKEN`. Nothing mints one for you: it is a secret of your choosing, and the box
  accepts exactly that one.
- **A token anybody can read.** A credential at `0644` on a shared machine belongs to every account
  on it, so it is refused rather than used, and rather than quietly fixed.

**Loopback is the default, and a bare port is loopback on that port.** A sandbox host that comes up
on `0.0.0.0` the first time somebody tries it is a bad default with a long tail; reaching past the
machine takes saying so, and is said back to you at startup.

`--bind`, the token and the data directory each have an environment variable
(`$TORMONI_SERVE_BIND`, `$TORMONI_SERVE_TOKEN`, `$TORMONI_SERVE_DATA`), so a container runs this
with no config file.

## The lanes

Every one takes `Authorization: Bearer <token>` except `/health`, which a load balancer has no
token for. A refusal is RFC 9457 `application/problem+json`, and its `detail` is written to be read
by a person.

| Lane | What it does |
|---|---|
| `POST /v1/runs` | One ephemeral sandbox. The response streams while it runs. |
| `GET /v1/runs/{id}/archive` | The ustar `tormoni export` writes, unchanged. |
| `POST /v1/sandboxes` | One that outlives its command, reached afterwards by name. |
| `POST /v1/sandboxes/{name}/exec` | Another command in one already up. |
| `DELETE /v1/sandboxes/{name}` | Stops one. |
| `GET /health` | Whether this box is up. No bearer. |

A job is the posture a person would have typed, as a body: `command`, and any of `vcpus`,
`mem_mib`, `mounts`, `shares`, `net`, `rootfs`, `env`, `workdir`, `root`, `no_results`. Every field
becomes the flag of the same name, and a field left out is left to the CLI's own default. **A
posture is never relaxed because it is a server**: `net` absent means no network here exactly as it
does at a keyboard.

`POST /v1/runs` answers with newline-delimited JSON, one object per event:

```json
{"event":"started","name":"job-1789085063489"}
{"event":"stdout","text":"hello"}
{"event":"ended","exit_status":0,"run":{ "...": "the record" },"units":{ "...": "what it held" }}
```

## The archive is the same archive

A served run leaves **byte for byte** the record a local run leaves. Not equivalent: the same
bytes, because a job re-enters the same `tormoni run` rather than driving the supervisor a second
time. There is one run path, not two that agree today.
`a_served_archive_is_byte_for_byte_the_one_a_local_export_writes` boots a sandbox through a
listening server and compares the digests.

## What it measures

The box is the only thing that knows what actually ran, so it is the only thing that can measure
it. It writes **units, and never money**: no rate, no currency, no plan and no tier is anywhere in
this tree, because what a unit costs is a question for somewhere that knows who is paying.

| Unit | What accrues |
|---|---|
| `vcpu_seconds` | vCPUs allocated × wall-clock seconds running |
| `memory_gib_seconds` | GiB of memory allocated × wall-clock seconds running |
| `disk_gib_seconds` | GiB of writable disk allocated × wall-clock seconds held |

Two things about how, each because the obvious version is wrong:

- **Accrued while it runs, not at the end.** One line per interval, appended as the interval
  passes, so a sandbox killed by a crash, an OOM or a power cut has already been charged for the
  time it held the machine. A meter that only wrote on a clean exit would under-bill every failure,
  and could be made to under-bill on purpose.
  `a_run_that_was_killed_still_accrued_what_it_held` is what holds it to that.
- **Allocated, not used.** A sandbox given four vCPUs that sits idle still held four and stopped
  them being sold twice. The allocation is the record's own `limits` line, which is what makes a
  count reproducible from the archive alone.

The ledger is one JSON object to a line at `usage.jsonl` in the data directory, appended to and
never rewritten. A reader sums the lines for a run.

## What it is not

- **Not a fleet.** No scheduling, no placement, no failover. One box.
- **Not multi-tenant.** One token. Everybody who holds it is the same caller.
- **Not priced.** Units only.
- **Not a way to relax a posture.** A served run that was quietly less isolated than a local one
  would be the worst defect this product could have.

## Where this is mid-flight

Persistent sandboxes are plumbed rather than finished: `POST /v1/sandboxes` and its exec and stop
reach `up`, `exec` and `stop`, but nothing meters a sandbox held across requests, and what happens
to one when the server restarts is unanswered. `disk_gib_seconds` is therefore always zero, since
the default posture holds no writable disk and nothing yet measures one that does.
