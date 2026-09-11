// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

// Package tormoni is the Go SDK for Tormoni.
//
// Tormoni runs untrusted code inside a hardware-isolated virtual machine on
// the caller's own machine — KVM on Linux, Hypervisor.framework on macOS, via
// libkrun. It is not a container. Every run leaves a record: the posture it
// was given, its captured output, and whatever it wrote to /results.
//
// This package is a thin wrapper around the tormoni CLI. It runs the binary as
// a subprocess with --json and parses the single JSON document the binary
// writes to stdout. There is no HTTP API, no retrying, no caching, and no
// dependency outside the standard library.
//
//	c := tormoni.New()
//	run, err := c.Run(context.Background(), []string{"echo", "hi"}, nil)
//	if err != nil {
//		log.Fatal(err)
//	}
//	fmt.Print(run.Stdout) // "hi\n"
//	fmt.Println(run.OK()) // true
//
// # Errors versus failed runs
//
// `tormoni run` exits with the guest command's own status, so the subprocess
// exit code says nothing about whether Tormoni worked. The rule this package
// follows is the CLI's own: if stdout parses as one JSON document, the sandbox
// ran, and Run.EndKind with Run.EndCode say how the command finished. If
// stdout does not parse, it is an operational failure and stderr holds a line
// written for a person.
//
// So a guest command exiting non-zero returns a *Run with a nil error. A
// missing binary, an absent guest root, or a hypervisor that does not answer
// returns an error: ErrNotFound, or an *Error carrying Tormoni's stderr
// verbatim. This package never rewords Tormoni's messages.
//
// # Finding the binary
//
// A Client runs "tormoni" from PATH by default. Set Client.Path, or the
// TORMONI_CLI environment variable that Tormoni's own desktop app uses, to
// point somewhere else.
package tormoni
