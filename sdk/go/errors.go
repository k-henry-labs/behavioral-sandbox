// Copyright 2026 The Boxdesk Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package boxdesk

import (
	"errors"
	"fmt"
	"strings"
)

// ErrNotFound is returned when the boxdesk binary cannot be located. Test for
// it with errors.Is.
var ErrNotFound = errors.New("boxdesk: binary not found")

// Error is an operational failure: Boxdesk itself did not work. The binary
// missing, the guest root being absent, the hypervisor not answering — those
// are Errors.
//
// A guest command exiting non-zero is not an Error. It is a *Run with EndKind
// EndExit and a non-zero EndCode.
//
// Stderr is Boxdesk's own message, written for a person and reproduced
// verbatim. This package never rewords it.
type Error struct {
	// Verb is the CLI verb that failed: "run", "show" or "ls".
	Verb string

	// Args is the argument list passed to the binary, without the binary
	// itself, and with the value of every --env entry replaced by
	// "<redacted>". Boxdesk deliberately never writes an environment value to
	// a record, and an error from this package must not undo that by spilling
	// one into a log. The names are left intact.
	Args []string

	// Stdout is whatever the binary wrote to stdout, unparsed.
	Stdout string

	// Stderr is Boxdesk's message, verbatim.
	Stderr string

	// ExitCode is the binary's exit status, or -1 if it never ran to
	// completion (it could not start, or the context ended first).
	ExitCode int

	// Err is the underlying cause: an *exec.ExitError, a JSON parse error, or
	// a context error. Unwrap returns it.
	Err error
}

// Error returns Boxdesk's own stderr when there is any, so upstream's wording
// reaches the caller untouched.
func (e *Error) Error() string {
	// Only the trailing newline goes; whatever Boxdesk wrote inside the
	// message is its own. Stderr that is nothing but whitespace is no message
	// at all, and falls through to something a reader can act on.
	if strings.TrimSpace(e.Stderr) != "" {
		return strings.TrimRight(e.Stderr, "\n")
	}
	if e.Err != nil {
		return fmt.Sprintf("boxdesk %s: %v", e.Verb, e.Err)
	}
	return fmt.Sprintf("boxdesk %s: exited %d without writing a JSON document", e.Verb, e.ExitCode)
}

// Unwrap returns the underlying cause, so errors.Is reaches context.Canceled,
// context.DeadlineExceeded and *exec.ExitError.
func (e *Error) Unwrap() error { return e.Err }

// redactionMarker replaces the value half of an --env entry in a reported
// argument list.
const redactionMarker = "<redacted>"

// redactEnvValues copies args with the value of every --env entry replaced, so
// that an *Error can be logged or serialised without spilling a secret the
// caller passed in. Boxdesk deliberately never writes an environment value to
// a record; an error from this package must not undo that.
//
// Only flags are considered: everything after the bare "--" is the guest's
// command, where an "--env" is just a word.
func redactEnvValues(args []string) []string {
	out := make([]string, len(args))
	copy(out, args)
	for i := 0; i < len(out); i++ {
		if out[i] == "--" {
			break
		}
		if out[i] != "--env" || i+1 >= len(out) {
			continue
		}
		i++
		if name, _, found := strings.Cut(out[i], "="); found {
			out[i] = name + "=" + redactionMarker
		}
	}
	return out
}
