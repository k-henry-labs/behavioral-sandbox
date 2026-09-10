# Examples

Eight things to try after `cargo xtask init` (or an install), each one showing a single property.
Every command below was run on macOS 26.6.2 ARM64 on 2026-09-10 against the tree `cargo xtask init`
writes, and the output shown is what it printed. That tree is Alpine's minirootfs plus the guest
agent: busybox applets, and no language runtimes. `cargo xtask build-rootfs` on Linux is the image
that adds those.

## The guest is a virtual machine, whatever the host is

```console
$ tormoni run -- uname -sm
Linux aarch64
$ uname -sm
Darwin arm64
```

The host is macOS and the guest answers Linux, because the guest is a VM under
Hypervisor.framework rather than a process on this machine. On a Linux host the same command
answers from a kernel that is not the one you booted.

## Nothing reaches the network until the posture says so

```console
$ tormoni run -- wget -T3 -qO- http://example.com
wget: bad address 'example.com'
$ echo $?
1
```

`--net none` is the default, so there is no resolver and no route. Grant a network and the same
command works:

```console
$ tormoni run --net tsi -- wget -T5 -qO- http://example.com
<!doctype html><html lang="en"><head><title>Example Domain</title>...
```

`tsi` is libkrun's transparent socket impersonation: the guest reaches what the host can reach. It
is one of two values, and the record keeps whichever was used.

## No host directory is shared until you name one

```console
$ mkdir -p /tmp/demo-project && echo 'notes from the host' > /tmp/demo-project/notes.txt
$ tormoni run -- ls /tmp/demo-project
ls: /tmp/demo-project: No such file or directory
```

The guest's `/tmp` is the guest's. `--mount` puts a host directory at a guest path, read-write:

```console
$ tormoni run --mount /mnt=/tmp/demo-project -- cat /mnt/notes.txt
notes from the host
```

Mount at a directory the image already has. The guest root is read-only, so a mount at `/work`
is refused rather than silently dropped, and the error names `/mnt` as somewhere that exists.

## What the run wrote comes back

`/results` is the one directory a run is expected to write, and it is collected into the record:

```console
$ tormoni run -- tar -cf /results/etc.tar /etc
tar: removing leading '/' from member names
$ tormoni show run-98315
...
output stdout 0 bytes
output stderr 44 bytes
result etc.tar 290304 bytes
```

`tormoni export` writes that record, its captured output and its results as one tar file.

## The record says what the run could touch

```console
$ tormoni show 1789019565376-run-97863
record 1
id 1789019565376-run-97863
name run-97863
verb run
arg /bin/busybox
arg sh
arg -c
arg cat /mnt/notes.txt; echo written-from-the-guest > /mnt/from-guest.txt
root /Users/you/.local/share/tormoni/rootfs read-only
mount /mnt <- /tmp/demo-project
network none
sound off
gpu off
results on
limits 1 512
started 1789019565376
pid 97865
ended 1789019565673
end exit 0
dir /Users/you/.local/share/tormoni/runs/1789019565376-run-97863
output stdout 20 bytes
output stderr 0 bytes
```

The posture is written as it was settled, before the VM booted. There is no in-kernel enforcer
behind it: the virtiofs tags and the network backend in that record are the policy, which is why
the record spells out every one.

## The exit status is the command's

```console
$ tormoni run -- false; echo $?
1
$ tormoni run -- true; echo $?
0
```

So a sandbox drops into a shell pipeline or a `Makefile` without a wrapper reading its output.

## A sandbox that outlives the command

```console
$ tormoni up --name demo
demo
$ tormoni ls
NAME              PID       VCPUS   MEM       NET     ROOTFS      CHANNEL
demo              98051     1       512       none    read-only   present
$ tormoni exec demo -- uptime
 05:53:02 up 0 min,  0 users,  load average: 0.00, 0.00, 0.00
$ tormoni stop demo
demo
```

`up` registers a socket under the runtime directory, and both binaries find live sandboxes by
reading it, so a sandbox started in the terminal shows up in `Tormoni` and the other way round.

## How long a run takes here

```console
$ for i in $(seq 1 15); do /usr/bin/time -p tormoni run -- true; done
n=15  p50=0.16s  p90=0.23s  max=0.24s
```

macOS 26.6.2 ARM64, 2026-09-10, the `cargo xtask init` tree, one vCPU and 512 MiB. libkrun has no
snapshot surface, so every one of those is a cold boot: there is no warm pool and no reuse behind
the number. Measure your own host before repeating it; this one says nothing about yours.

## In the application

`Tormoni` carries these and more under **Cookbook** in the sidebar, on five shelves: first steps,
what a sandbox cannot reach, getting work back, what it is given, and what a failure looks like.
A press fills the start form rather than booting, so the posture sentence is read before anything
runs, and each entry shows the `tormoni` line it is.

That line is built from the entry's posture rather than stored beside it, so the card and the form
cannot say different things: `the_line_an_entry_shows_is_the_posture_it_fills_in` is what holds
them together, and a second rendering of the same table is where an SDK snippet will come from.
Every run started there is the same `tormoni` binary with the same flags, and lands in the same
record this page has been reading.
