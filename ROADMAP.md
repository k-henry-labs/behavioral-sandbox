# Roadmap

**Boxdesk is the Docker for sandbox agents, locally.** An agent that writes code eventually wants
to run it. Today the answers are to run it on the host and hope, or to run it in a container and
hope slightly less. Boxdesk is the third answer: the isolation boundary is the CPU's, the sandbox
is on the machine the agent is already running on, and every run leaves a record of what it could
touch and what it did.

This file is a **proposal, not a commitment**. It is ordered by dependency and by leverage, not by
appetite. Phases are numbered for reference, not as a promise about sequence or dates; several are
independent and could run in parallel, and a few are gated on questions nobody has answered yet.
Where something is unproven, it says so.

## What the sentence demands

"Docker for X" is two claims, and the tree keeps one of them today.

**The Docker half is about images.** Docker's verbs are the visible part; the reason it won is
that `name:tag` made a filesystem a thing you could publish, fetch and depend on. Boxdesk has no
image concept at all — `boxdesk run --root <DIR>` takes a directory you built yourself. Part III
covers this, and it is the largest body of work here.

**The "for sandbox agents" half is about who is calling.** A sandbox an agent cannot reach is a
sandbox an agent will not use. Part II covers this, and most of it is already written — it just
lives outside the repository.

## Where the tree already is

Worth stating plainly, because a roadmap that ignores what exists invents work.

- **The verbs already read like Docker's.** `run`, `exec`, `ls`, `stop`, `rm`, `show`, plus `up`
  for a sandbox that outlives the command that started it, and `export`.
- **There is already a daemon-shaped surface.** `boxdesk serve` runs sandboxes for a caller over
  HTTP, and `crates/channel` carries the control socket to a guest.
- **Four SDKs already wrap the same core** — Rust, Python, JavaScript and Go — so a fifth caller
  is a client of something that exists rather than a new integration path.
- **The record is a real artifact.** `crates/record` keeps what a run could touch, what it printed
  and what it wrote. This is the thing Docker does *not* have, and it is the strongest claim the
  product has for the agent case.
- **The posture model is already the security story.** `docs/security.md` describes it, and the
  words a record uses are the words the flags use.

What is missing is images, an agent-facing entry point that works, and anything that says out loud
who this is for.

---

## Part I — Say what it is

Cheap, and nothing downstream reads right until it is done. In this repository today the word
"agent" always means the in-guest helper process; nothing names an AI agent as the caller.

**Phase 1. Name the audience.** Rewrite the README's opening and the book's `docs/src/content/docs/index.md` around the
case: an agent, on this machine, running code it wrote. Keep the hardware-isolation claim as the
reason to believe, not the headline. *Done when* a reader who arrived from an agent framework
knows in one paragraph whether this is for them.

**Phase 2. A sixty-second first run.** One copy-pasteable path from nothing to a sandboxed command,
with no `cargo xtask` step in it. The guest image requirement is the current cliff — a newcomer on
macOS cannot build a rootfs and has nothing to run. *Done when* the quickstart works on a clean
machine. **Depends on Phase 12 or a prebuilt image to download.**

**Phase 3. An honest capability matrix.** One table: what works on Linux, on macOS ARM64, and
nowhere yet. The gate already prints a "not covered on this host" note; this is the same honesty
in the docs. *Done when* nobody has to read a test to find out whether a feature exists on their
machine.

## Part II — The agent-facing surface

**Most of this is written.** There is a complete MCP server outside the repository, on the official
Rust SDK, with a policy model where the operator decides what a sandbox may reach and the model
does not. It is also **broken right now**: it shells out to a binary named `tormoni`, which the
rename removed, and it carries a cloud backend for a cloud that was deleted from this tree.

**Phase 4. Adopt the MCP server.** Bring it into the workspace as a crate, renamed, with the cloud
backend and its tests deleted and the binary name corrected. *Done when* it builds in this
workspace and a client can start a sandbox through it. **This is the single cheapest step toward
the vision, and it fixes something that is currently broken.**

**Phase 5. Put it under the gate.** fmt, clippy at `-D warnings`, its tests, prose-drift and
`cargo deny`, the same as every other crate. *Done when* `cargo xtask ci` covers it.

**Phase 6. One-line install for MCP clients.** Config snippets for the common clients, and a verb
that prints them. An agent runtime nobody can mount is a library. *Done when* installing is copy,
paste, restart.

**Phase 7. Decide the tool surface deliberately.** Which tools a model may call, and which settings
only the operator may set on the command line. The existing server already draws this line; it
should be written down and tested, because it is the whole security argument. *Done when* a test
asserts that no tool call can widen what a sandbox may reach.

**Phase 8. Sessions across tool calls.** An agent's work is a sequence, not one command. `up`,
`exec` and `stop` already exist; expose them so a model keeps one sandbox across a conversation
instead of paying a cold start per step. *Done when* a model can write a file in one call and read
it in the next.

**Phase 9. Streaming output.** A run that prints for ninety seconds should not be silent for
ninety seconds. *Done when* partial output reaches the client while the run is live.

**Phase 10. The record as a resource.** Expose the run record through MCP so the agent — and the
person reading after it — can fetch what it could touch and what it did. *Done when* a finished
run is addressable and readable without leaving the client.

## Part III — Images

**The Docker half, and the largest part of this file.** The design question comes before the code:
a rootfs directory is not an image, and the gap between them is naming, content addressing,
distribution and a build step.

**Phase 11. Write the image design first.** What an image is here, how it maps onto the guest root
the VM boots, and — the decision that shapes everything after it — whether to adopt OCI or invent
a format. *Done when* there is a written design with the tradeoffs argued. **Nothing else in Part
III should start before this.**

**Recommendation, since taken: adopt OCI.** Reusing the existing registry ecosystem means
every image already published is reachable and nobody has to run new infrastructure. The cost is
flattening layers into a bootable root, which is real work but bounded.

**Phase 12. A local image store.** *Largely landed.* Images by `name:tag`, keyed by manifest
digest, resolved on the way into a run through `--image`. What is left is the default: a run with
neither `--root` nor `--image` still wants a tree on disk.

**Phase 13. Pull.** *Landed.* Fetches from an OCI registry and verifies every digest before the
bytes are opened. Pulling by digest rather than by tag is the piece still missing.

**Phase 14. Flatten an OCI image into a guest root.** *Written, not yet demonstrated.* Layers are
flattened in order with whiteouts applied first, and the mount points this runtime mounts are made
in the tree. *Done when* a stock image from a public registry boots and runs a command — which
needs a host this project can boot on at all.

**Phase 15. A build spec.** A declarative file: a base image and a few steps. Not a Dockerfile
clone — the subset that the sandbox case actually needs. *Done when* the spec is documented and
frozen enough to depend on.

**Phase 16. Build.** Execute that spec — inside a sandbox, which is the obvious dogfood. *Done
when* the project's own guest image is produced by it instead of by `cargo xtask build-rootfs`.

**Phase 17. Cache and collect.** A store that grows without bound is a bug report. *Done when*
unreferenced content is reclaimable and the store's size is reportable.

**Phase 18. Push.** Publish to a registry the user already has. *Done when* an image built locally
is pullable elsewhere.

**Phase 19. Signing and verification.** An agent runtime that silently runs unverified images is a
supply-chain hole with extra steps. *Done when* a signature can be required and a failure refuses
the run.

## What has landed since this was written

**Snapshots, in the sense the word usually means in this space.** A snapshot is a named posture —
what a sandbox made from it boots, what it may touch and how much machine it gets — kept in
`crates/record`, written and read by `boxdesk snapshot new|ls|show|rm`, started from with
`--snapshot NAME` on `run`, `shell` and `up`, and listed in the app's own Snapshots page. This is
what a Daytona "snapshot" is, and it is a template rather than a capture.

**Images, pulled from an OCI registry and flattened into a bootable guest root.** `boxdesk pull
alpine:3.20` resolves a reference the way every other client does, follows the registry's
`WWW-Authenticate` challenge for a token, verifies every blob against its digest before `tar`
opens it, and flattens the layers in order with whiteouts applied first. `boxdesk run --image
NAME:TAG` boots the result, `boxdesk image ls|rm` manages them, and `boxdesk registry add|ls|rm`
records where images come from and who this machine is when it asks — a host, a project and a
username, and never a password.

**That lands the argument of phase 11 and the code of phases 12, 13 and 14.** OCI was adopted,
which the design section below recommended: libkrun boots a directory and a flattened OCI image is
a directory, so there is no disk image to build and no format to invent. What is not done is the
rest of Part III — the build spec, the build, collection, push and signing (phases 15 to 19).

**One claim phase 14 makes is not yet demonstrated here.** A pulled tree is produced and verified,
and `--image` carries it to the boot path; whether a stock image *boots and runs a command* has
not been shown on this machine, because no boot works on it — a hand-made root fails at
`krun_start_enter` identically. That is the same "not covered on this host" the gate already
reports, and the claim stays open until a host with a working hypervisor says otherwise.

**Volumes.** `boxdesk volume new|ls|show|rm` keeps named directories that outlive the sandboxes
that write them, and `--volume NAME:GUESTDIR` mounts one. A volume resolves to an ordinary mount
before anything downstream sees it, so the record still says what a run could touch in the words
it always used. Local only: a directory on this machine, no object store.

**All four of the app's sidebar features are now built**, which is what this section is really
recording. The "not built yet" pages, and the machinery behind them, are gone.

**It takes the word, so Part IV's phases have to be read carefully.** Phases 20 and 21 below are
about capturing a *running* sandbox's memory and vCPU state and branching several from it, which
is a different feature that shares a name. Nothing about the template feature makes that one
easier or harder.

## Part IV — What agents need that Docker never did

**This is where the product stops being a port of Docker.** These are the phases that make the
sentence worth saying.

**Phase 20. Capture a running sandbox.** Not the template feature above: the memory and vCPU
state of a sandbox that is running. **Unproven, and the ground is now known to be harder than this
file first said.** `crates/krun` binds 78 libkrun symbols and not one of them is a snapshot, pause
or resume call; upstream describes libkrun as incorporating code from Firecracker, rust-vmm and
Cloud-Hypervisor, and supporting **KVM on Linux and HVF on macOS/ARM64**. Firecracker has snapshot
support and it is KVM-only, so on the macOS half of the supported platforms there is nothing to
borrow and the vCPU save and restore would be written against Hypervisor.framework. *Done when*
the answer is known and written down, feature or not.

**Phase 21. Fork from a snapshot.** Branch several sandboxes from one captured state. **This is the
highest-value agent feature in this file**: agents explore, and exploring from a shared prefix
without repeating the work is the thing no container runtime gives them. **Depends entirely on
Phase 20.**

**Phase 22. Warm starts.** A pool of ready sandboxes so the common run does not pay boot. *Done
when* a trivial run is fast enough that an agent framework does not cache around it.

**Phase 23. Ceilings per run.** Wall-clock, memory and CPU bounds that a runaway agent cannot
exceed. Some exists (`--vcpus`, `--mem`); a timeout that always fires is the gap. *Done when* a
run that will not stop is stopped anyway.

## Part V — Trust and evidence

**The record is the differentiator.** Lean on it.

**Phase 24. Egress policy.** Network today is `none` or `tsi` — all or nothing. An agent that needs
one API should not get the whole internet. *Done when* a run can be given an allowlist and the
record says what it was.

**Phase 25. Make the record the product.** A readable, diffable, exportable account of what an
agent did on this machine. `export` is the seed. *Done when* handing someone a record is a
sufficient answer to "what did it do?".

**Phase 26. Tamper-evident records.** Sign them, so a record is evidence rather than a log file.
*Done when* a modified record fails verification.

## Part VI — Reach

**Phase 27. More than one sandbox as a unit.** The compose case: an agent's task that needs a
database beside it. *Done when* a multi-sandbox unit starts and stops as one.

**Phase 28. Framework adapters.** Thin bindings for the agent frameworks people actually use, on
top of the four SDKs that already exist. *Done when* using boxdesk from a popular framework is an
import, not an integration.

**Phase 29. A shared box.** `boxdesk serve` already runs sandboxes for a caller over HTTP; the step
is making that a team's box rather than a demo. Self-hosted only — the cloud was removed
deliberately. *Done when* a team can point several agents at one machine safely.

**Phase 30. Distribution.** Releases people can install without a Rust toolchain, on both supported
platforms. *Done when* installing does not require building.

---

## The order I would actually take

1. **Phase 4** — it is broken, it is written, and it is the "for agents" half. Days, not weeks.
2. **Phase 1** — so the rest of the work has something to be consistent with.
3. **Phase 11** — the image design, before any image code.
4. **Phase 20** — the capture investigation, early, because Phase 21 is the best idea in this
   file and it may be impossible. Better to learn that now than after Part III.

## What could make this wrong

- **If libkrun cannot capture a running VM**, Part IV loses its best phase and the product is closer to "Docker
  with a stronger boundary" than to something agents cannot get elsewhere.
- **If OCI flattening is worse than it looks**, Phase 14 could dominate Part III, and inventing a
  narrower format may beat adopting a general one.
- **If agents mostly want speed over isolation**, Phase 22 outranks most of this file, and the
  honest comparison is against a container, not against running on the host.
