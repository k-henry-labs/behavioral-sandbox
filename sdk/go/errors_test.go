// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni_test

import (
	"context"
	"errors"
	"os/exec"
	"reflect"
	"strings"
	"testing"

	tormoni "github.com/kendricklawton/tormoni/sdk/go"
)

// TestOperationalFailure covers a stub that writes garbage to stdout, a
// message to stderr, and exits 2.
func TestOperationalFailure(t *testing.T) {
	const message = "tormoni: guest root not found at /home/you/.local/share/tormoni/rootfs\n"
	s := newStub(t, "not json at all\n", message, 2)

	run, err := s.Client().Run(context.Background(), []string{"echo", "hi"}, nil)
	if run != nil {
		t.Errorf("run = %+v, want nil", run)
	}
	var tErr *tormoni.Error
	if !errors.As(err, &tErr) {
		t.Fatalf("err = %#v, want *tormoni.Error", err)
	}
	if tErr.Stderr != message {
		t.Errorf("Stderr = %q, want %q", tErr.Stderr, message)
	}
	// Tormoni's message reaches the caller verbatim, not reworded.
	if got, want := tErr.Error(), strings.TrimRight(message, "\n"); got != want {
		t.Errorf("Error() = %q, want %q", got, want)
	}
	if tErr.ExitCode != 2 {
		t.Errorf("ExitCode = %d, want 2", tErr.ExitCode)
	}
	if tErr.Verb != "run" {
		t.Errorf("Verb = %q, want run", tErr.Verb)
	}
	if tErr.Stdout != "not json at all\n" {
		t.Errorf("Stdout = %q, want the unparsed output", tErr.Stdout)
	}
	var exitErr *exec.ExitError
	if !errors.As(err, &exitErr) {
		t.Error("Unwrap does not reach the *exec.ExitError")
	}
}

// TestUnparsableStdout covers the ways stdout can fail to be one document,
// including a binary that exits cleanly while writing nonsense.
func TestUnparsableStdout(t *testing.T) {
	tests := []struct {
		name     string
		stdout   string
		exitCode int
	}{
		{"nothing at all", "", 0},
		{"human readable output", "Started sandbox run-1234.\n", 0},
		{"a bare array", `[{"run_id":"1"}]`, 0},
		{"a JSON scalar", `"hello"`, 0},
		{"a truncated document", `{"run_id":"1","end_kind":`, 0},
		{"two documents", `{"run_id":"1"}{"run_id":"2"}`, 0},
		{"a document with trailing noise", `{"run_id":"1"} and then some`, 0},
		{"nothing, after failing", "", 70},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := newStub(t, tt.stdout, "something went wrong\n", tt.exitCode)
			_, err := s.Client().Run(context.Background(), []string{"true"}, nil)
			if err == nil {
				t.Fatal("err = nil, want an operational failure")
			}
			var tErr *tormoni.Error
			if !errors.As(err, &tErr) {
				t.Fatalf("err = %#v, want *tormoni.Error", err)
			}
			if tErr.Stderr != "something went wrong\n" {
				t.Errorf("Stderr = %q", tErr.Stderr)
			}
		})
	}
}

// TestErrorWithoutStderr checks the fallback message when Tormoni said nothing
// a person could read.
func TestErrorWithoutStderr(t *testing.T) {
	s := newStub(t, "", "", 0)
	_, err := s.Client().Show(context.Background(), "run-1")
	var tErr *tormoni.Error
	if !errors.As(err, &tErr) {
		t.Fatalf("err = %#v, want *tormoni.Error", err)
	}
	if !strings.Contains(tErr.Error(), "tormoni show") {
		t.Errorf("Error() = %q, want it to name the verb", tErr.Error())
	}
	if tErr.ExitCode != 0 {
		t.Errorf("ExitCode = %d, want 0", tErr.ExitCode)
	}
}

// TestErrorMessage covers each branch of the message an *Error reports,
// including a value a caller built themselves.
func TestErrorMessage(t *testing.T) {
	tests := []struct {
		name string
		err  tormoni.Error
		want string
	}{
		{
			name: "Tormoni's message, verbatim but for the trailing newline",
			err:  tormoni.Error{Verb: "run", Stderr: "tormoni: no /dev/kvm\n"},
			want: "tormoni: no /dev/kvm",
		},
		{
			name: "a multi-line message keeps its shape",
			err:  tormoni.Error{Verb: "run", Stderr: "tormoni: two things went wrong:\n  - one\n  - two\n"},
			want: "tormoni: two things went wrong:\n  - one\n  - two",
		},
		{
			name: "whitespace is not a message",
			err:  tormoni.Error{Verb: "ls", Stderr: " \n\n", ExitCode: 2},
			want: "tormoni ls: exited 2 without writing a JSON document",
		},
		{
			name: "with a cause but nothing on stderr",
			err:  tormoni.Error{Verb: "show", Err: context.Canceled},
			want: "tormoni show: context canceled",
		},
		{
			name: "the zero value still says something",
			err:  tormoni.Error{},
			want: "tormoni : exited 0 without writing a JSON document",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.err.Error(); got != tt.want {
				t.Errorf("Error() = %q, want %q", got, tt.want)
			}
		})
	}
}

// TestErrorArgsRedactEnvValues: an *Error carries the argument list for
// debugging, and that list contains whatever --env values the caller passed.
// Tormoni deliberately never writes an environment value to a record, so an
// error from this package must not undo that by spilling one into a log.
func TestErrorArgsRedactEnvValues(t *testing.T) {
	const secret = "s3cret-value-nobody-should-see"
	s := newStub(t, "not json", "tormoni: the hypervisor did not answer\n", 1)

	// The guest's own command contains a word that looks like the flag, to
	// check that only real flags are touched.
	_, err := s.Client().Run(context.Background(), []string{"printenv", "--env"}, &tormoni.RunOptions{
		Env:  []string{"API_KEY=" + secret, "BARE_NAME", "EMPTY="},
		Name: "builder",
	})
	var tErr *tormoni.Error
	if !errors.As(err, &tErr) {
		t.Fatalf("err = %#v, want *tormoni.Error", err)
	}

	joined := strings.Join(tErr.Args, " ")
	if strings.Contains(joined, secret) {
		t.Errorf("Error.Args leaked an environment value: %q", joined)
	}
	if !strings.Contains(joined, "API_KEY=") {
		t.Errorf("Error.Args dropped the variable name: %q", joined)
	}
	want := []string{"run", "--json",
		"--env", "API_KEY=<redacted>",
		"--env", "BARE_NAME", // no value to redact
		"--env", "EMPTY=<redacted>",
		"--name", "builder",
		"--", "printenv", "--env"} // past "--", the flag is just a word
	if !reflect.DeepEqual(tErr.Args, want) {
		t.Errorf("Error.Args mismatch\n got: %q\nwant: %q", tErr.Args, want)
	}

	// Redacting must not have touched what was actually sent to the guest.
	wantArgv := []string{"run", "--json",
		"--env", "API_KEY=" + secret,
		"--env", "BARE_NAME",
		"--env", "EMPTY=",
		"--name", "builder",
		"--", "printenv", "--env"}
	if got := s.Argv(); !reflect.DeepEqual(got, wantArgv) {
		t.Errorf("the guest did not get the real values\n got: %q\nwant: %q", got, wantArgv)
	}

	// The message itself is still Tormoni's, verbatim.
	if tErr.Error() != "tormoni: the hypervisor did not answer" {
		t.Errorf("Error() = %q", tErr.Error())
	}
}
