// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni_test

import (
	"context"
	"fmt"
	"log"

	tormoni "github.com/kendricklawton/tormoni/sdk/go"
)

// This example runs a command in a sandbox and reads its captured output. It
// needs a working Tormoni install, so it is compiled but not run by `go test`.
func Example() {
	c := tormoni.New()
	run, err := c.Run(context.Background(), []string{"echo", "hi"}, nil)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Print(run.Stdout) // "hi\n"
	fmt.Println(run.OK()) // true
}

// A guest command exiting non-zero is not an error. It is a record you read.
func ExampleRun_OK() {
	code := 3
	failed := &tormoni.Run{EndKind: tormoni.EndExit, EndCode: &code}
	fmt.Println(failed.OK(), *failed.EndCode)

	// A run that is still going has no end kind at all, and no code.
	running := &tormoni.Run{}
	fmt.Println(running.OK(), running.EndKind == "", running.EndCode == nil)

	// Output:
	// false 3
	// false true true
}

// Options map one-to-one onto the CLI's flags.
func ExampleRunOptions() {
	vcpus, mem := 2, 1024
	opts := &tormoni.RunOptions{
		VCPUs:  &vcpus,
		MemMiB: &mem,
		Mounts: []tormoni.Mount{{Guest: "/mnt", Host: "/home/you/project"}},
		Env:    []string{"API_KEY=s3cret"},
		Net:    "tsi",
	}
	// --vcpus 2 --mem 1024 --mount /mnt=/home/you/project --env API_KEY=s3cret --net tsi
	fmt.Println(len(opts.Mounts), opts.Mounts[0].Guest, opts.Net)
	// Output: 1 /mnt tsi
}
