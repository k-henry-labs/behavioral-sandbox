// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
)

// DefaultBinary is the binary name looked up on PATH when neither Client.Path
// nor $TORMONI_CLI says otherwise.
const DefaultBinary = "tormoni"

// EnvBinary is the environment variable naming the tormoni binary. It is the
// same variable Tormoni's own desktop app uses to find it.
const EnvBinary = "TORMONI_CLI"

// Client runs the tormoni CLI. The zero value is usable and finds the binary
// the same way New does.
//
// A Client holds no state between calls: Tormoni's records directory is the
// state, and this package neither caches runs nor keeps an index.
type Client struct {
	// Path is the tormoni binary. When empty, $TORMONI_CLI is used, and
	// failing that the name "tormoni" is looked up on PATH. It is read at
	// call time, not when the Client is made.
	Path string
}

// New returns a Client that finds the tormoni binary through $TORMONI_CLI, or
// else on PATH.
func New() *Client { return &Client{} }

// NewWithPath returns a Client that runs the binary at path, ignoring
// $TORMONI_CLI and PATH.
func NewWithPath(path string) *Client { return &Client{Path: path} }

// Run boots a fresh sandbox, runs one command in it, waits, and returns the
// record. command is the guest's command; its first word is resolved by the
// guest's PATH, not the host's. opts may be nil.
//
// The guest's own stdout and stderr do not reach this process's streams: they
// come back in Run.Stdout and Run.Stderr.
//
// A guest command exiting non-zero returns a *Run and a nil error — read
// Run.OK, Run.EndKind and Run.EndCode to learn how it finished. A non-nil
// error means Tormoni itself did not work, and is either ErrNotFound or an
// *Error carrying Tormoni's stderr verbatim.
//
// Cancelling ctx kills the sandbox process.
func (c *Client) Run(ctx context.Context, command []string, opts *RunOptions) (*Run, error) {
	return c.record(ctx, runArgs(command, opts, false))
}

// DryRun settles and returns the posture without booting anything. The
// returned Run has no captured output and no end.
func (c *Client) DryRun(ctx context.Context, command []string, opts *RunOptions) (*Run, error) {
	return c.record(ctx, runArgs(command, opts, true))
}

// Show returns the record of a run that already happened, by id or by name.
func (c *Client) Show(ctx context.Context, id string) (*Run, error) {
	return c.record(ctx, []string{"show", "--json", id})
}

// Runs lists sandboxes. With all false only live sandboxes are listed.
//
// Rows omit the captured output — Stdout, Stderr, Files and Dir are empty,
// because reading every run's bytes to list them would be a directory walk per
// row. Rows carry Live. Use Show to read one run in full.
func (c *Client) Runs(ctx context.Context, all bool) ([]Run, error) {
	args := []string{"ls", "--json"}
	if all {
		args = append(args, "--all")
	}
	inv, err := c.invoke(ctx, args)
	if err != nil {
		return nil, err
	}
	var doc struct {
		Runs json.RawMessage `json:"runs"`
	}
	if err := inv.decode(&doc); err != nil {
		return nil, err
	}
	// The verb answers with an object holding a runs array, never a bare
	// array. A document without the key at all is an operational failure, not
	// an empty list; an explicit null is just an empty list.
	if len(doc.Runs) == 0 {
		return nil, inv.fail(errors.New(`document has no "runs" array`))
	}
	var runs []Run
	if err := json.Unmarshal(doc.Runs, &runs); err != nil {
		return nil, inv.fail(err)
	}
	if runs == nil {
		runs = []Run{}
	}
	return runs, nil
}

// record runs a verb that answers with a single record.
func (c *Client) record(ctx context.Context, args []string) (*Run, error) {
	inv, err := c.invoke(ctx, args)
	if err != nil {
		return nil, err
	}
	var run Run
	if err := inv.decode(&run); err != nil {
		return nil, err
	}
	return &run, nil
}

// runArgs builds the argv for a run, in the order the flag table lists.
// Everything after "--" is the guest's command.
func runArgs(command []string, opts *RunOptions, dry bool) []string {
	args := []string{"run", "--json"}
	if dry {
		args = append(args, "--dry-run")
	}
	args = append(args, opts.args()...)
	args = append(args, "--")
	return append(args, command...)
}

// binary resolves the binary to run, or reports ErrNotFound.
func (c *Client) binary() (string, error) {
	name, source := c.Path, "Client.Path"
	if name == "" {
		name, source = os.Getenv(EnvBinary), "$"+EnvBinary
	}
	if name == "" {
		name, source = DefaultBinary, "PATH"
	}
	path, err := exec.LookPath(name)
	if err != nil {
		return "", fmt.Errorf(
			"%w: %q (from %s); install Tormoni, or set %s or Client.Path to the tormoni binary: %w",
			ErrNotFound, name, source, EnvBinary, err)
	}
	return path, nil
}
