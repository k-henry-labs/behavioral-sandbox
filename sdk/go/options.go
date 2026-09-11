// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni

import "strconv"

// RunOptions maps one-to-one onto the flags `tormoni run` accepts. Every field
// is optional: a zero value means "do not pass the flag", and the CLI's own
// default stands. This package adds no defaults of its own.
//
// Fields are pointers only where zero is a value a caller might genuinely want
// to send (VCPUs, MemMiB); everywhere else the zero value is unambiguous.
type RunOptions struct {
	// Root is --root DIR, the guest root tree. Unset, the CLI uses
	// $TORMONI_GUEST_ROOT and then ~/.local/share/tormoni/rootfs.
	Root string

	// VCPUs is --vcpus N. Nil leaves the CLI default (1).
	VCPUs *int

	// MemMiB is --mem MIB, guest RAM. Nil leaves the CLI default (512).
	MemMiB *int

	// Workdir is --workdir DIR, the guest working directory.
	Workdir string

	// Mounts are --mount GUESTDIR=HOSTDIR, one flag per entry, in order. Each
	// makes a host directory available read-write at a guest path.
	Mounts []Mount

	// Shares are --share TAG=HOSTPATH, one flag per entry, in order. Each adds
	// a virtiofs device the guest mounts by tag.
	Shares []Share

	// Net is --net, "none" (the CLI default, no network at all) or "tsi" (the
	// guest reaches what the host can). Passed through unvalidated.
	Net string

	// Rootfs is --rootfs, "read-only" (the CLI default) or "writable".
	// Passed through unvalidated.
	Rootfs string

	// Env are --env KEY=VALUE, one flag per entry, in order. The guest gets
	// the whole entry; the record keeps only the name, so a value set here is
	// never readable back off a Run.
	Env []string

	// Name is --name NAME, naming the sandbox.
	Name string

	// NoResults is --no-results, dropping the default /results mount.
	NoResults bool

	// Keep is --keep, filing the run's record instead of sweeping it.
	//
	// A run is ephemeral by default: its output comes back in the Run and the
	// directory it worked in goes with it, so Files is empty and Dir is blank.
	// Keep it when you mean to read what the guest wrote to /results, or to
	// reach the run again with Show.
	Keep bool

	// GPU is --gpu. Not every host has a backend for it; a host whose libkrun
	// was built without one refuses the run, and that refusal reaches you as
	// an *Error.
	GPU bool

	// Sound is --sound, with the same portability caveat as GPU.
	Sound bool

	// Display is --display WxH[@HZ], with the same portability caveat as GPU.
	// It does not currently boot on macOS at all.
	Display string
}

// args renders the options as CLI flags, in the order the flag table lists
// them. A nil *RunOptions renders nothing.
func (o *RunOptions) args() []string {
	if o == nil {
		return nil
	}
	var a []string
	if o.Root != "" {
		a = append(a, "--root", o.Root)
	}
	if o.VCPUs != nil {
		a = append(a, "--vcpus", strconv.Itoa(*o.VCPUs))
	}
	if o.MemMiB != nil {
		a = append(a, "--mem", strconv.Itoa(*o.MemMiB))
	}
	if o.Workdir != "" {
		a = append(a, "--workdir", o.Workdir)
	}
	for _, m := range o.Mounts {
		a = append(a, "--mount", m.Guest+"="+m.Host)
	}
	for _, s := range o.Shares {
		a = append(a, "--share", s.Tag+"="+s.Host)
	}
	if o.Net != "" {
		a = append(a, "--net", o.Net)
	}
	if o.Rootfs != "" {
		a = append(a, "--rootfs", o.Rootfs)
	}
	for _, e := range o.Env {
		a = append(a, "--env", e)
	}
	if o.Name != "" {
		a = append(a, "--name", o.Name)
	}
	if o.NoResults {
		a = append(a, "--no-results")
	}
	if o.Keep {
		a = append(a, "--keep")
	}
	if o.GPU {
		a = append(a, "--gpu")
	}
	if o.Sound {
		a = append(a, "--sound")
	}
	if o.Display != "" {
		a = append(a, "--display", o.Display)
	}
	return a
}
