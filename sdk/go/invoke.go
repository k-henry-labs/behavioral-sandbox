// Copyright 2026 The Boxdesk Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package boxdesk

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os/exec"
	"time"
)

// waitDelay bounds how long waiting on the subprocess may hang after the
// process itself is gone or the caller's context has ended, on stdout and
// stderr pipes that some grandchild of the CLI is still holding open. Without
// it, cancelling a context kills the boxdesk process but the call can still
// block indefinitely, which would make cancellation a promise this package
// does not keep.
//
// It is a var only so the package's own tests can shrink it.
var waitDelay = 10 * time.Second

// invocation is one finished run of the binary.
type invocation struct {
	args     []string
	stdout   []byte
	stderr   []byte
	exitCode int
	// err is the process error, or the context's error if ctx ended first.
	err error
}

// invoke runs the binary to completion. It returns an error only when the
// binary could not be located: a binary that ran and failed is still an
// invocation, because `boxdesk run` exits with the guest command's own status
// and so its exit status says nothing about whether Boxdesk worked.
func (c *Client) invoke(ctx context.Context, args []string) (*invocation, error) {
	bin, err := c.binary()
	if err != nil {
		return nil, err
	}

	cmd := exec.CommandContext(ctx, bin, args...)
	// The environment is inherited on purpose: the CLI reads its own settings
	// from it, BOXDESK_GUEST_ROOT among them. Stdin is left nil, so the guest
	// gets no input and a CLI that asks for some sees EOF rather than hanging.
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	cmd.WaitDelay = waitDelay
	runErr := cmd.Run()

	inv := &invocation{args: args, stdout: stdout.Bytes(), stderr: stderr.Bytes(), exitCode: -1, err: runErr}
	var exitErr *exec.ExitError
	if errors.As(runErr, &exitErr) {
		inv.exitCode = exitErr.ExitCode()
	} else if runErr == nil && cmd.ProcessState != nil {
		inv.exitCode = cmd.ProcessState.ExitCode()
	}
	// A killed process reports "signal: killed"; the reason worth reporting is
	// that the caller's context ended.
	if ctxErr := ctx.Err(); ctxErr != nil {
		inv.err = ctxErr
	}
	return inv, nil
}

// decode parses stdout into v.
//
// This is the whole contract: if stdout parses as one JSON document, the
// sandbox ran, whatever the exit status was. If it does not parse, it is an
// operational failure and stderr holds the message.
func (inv *invocation) decode(v any) error {
	trimmed := bytes.TrimSpace(inv.stdout)
	if len(trimmed) == 0 {
		return inv.fail(errors.New("no output on stdout"))
	}
	if trimmed[0] != '{' {
		return inv.fail(errors.New("stdout is not a JSON object"))
	}
	// json.Unmarshal rejects trailing data, which is what "exactly one JSON
	// document and nothing else" means.
	if err := json.Unmarshal(trimmed, v); err != nil {
		return inv.fail(err)
	}
	return nil
}

// fail builds the *Error for an invocation whose stdout held no usable
// document. fallback describes what was wrong with stdout, and is used only
// when the process itself did not fail: when it did, its own error is why
// there was no document, and it is the more useful cause to unwrap to.
func (inv *invocation) fail(fallback error) *Error {
	cause := fallback
	if inv.err != nil {
		cause = inv.err
	}
	return &Error{
		Verb:     inv.verb(),
		Args:     redactEnvValues(inv.args),
		Stdout:   string(inv.stdout),
		Stderr:   string(inv.stderr),
		ExitCode: inv.exitCode,
		Err:      cause,
	}
}

func (inv *invocation) verb() string {
	if len(inv.args) == 0 {
		return ""
	}
	return inv.args[0]
}
