// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni_test

// The two rules the whole contract rests on, and the ones most likely to be
// broken by a well-meaning change: a guest command exiting non-zero is a
// record and not an error, and an environment value goes out to the guest but
// never comes back in a record.

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	tormoni "github.com/kendricklawton/tormoni/sdk/go"
)

// TestGuestExitIsNotAnError is the heart of the contract: the CLI exits with
// the guest command's own status, so a non-zero exit is a record to read, not
// an error to raise.
func TestGuestExitIsNotAnError(t *testing.T) {
	tests := []struct {
		name     string
		doc      string
		exitCode int
		wantKind tormoni.EndKind
		wantCode *int
		wantOK   bool
	}{
		{
			name:     "exited zero",
			doc:      `{"end_kind":"exit","end_code":0}`,
			exitCode: 0,
			wantKind: tormoni.EndExit, wantCode: ptr(0), wantOK: true,
		},
		{
			name:     "exited three",
			doc:      `{"end_kind":"exit","end_code":3}`,
			exitCode: 3,
			wantKind: tormoni.EndExit, wantCode: ptr(3), wantOK: false,
		},
		{
			name:     "exited one, the `false` case",
			doc:      `{"end_kind":"exit","end_code":1}`,
			exitCode: 1,
			wantKind: tormoni.EndExit, wantCode: ptr(1), wantOK: false,
		},
		{
			name:     "killed by a signal",
			doc:      `{"end_kind":"signal","end_code":9}`,
			exitCode: 137,
			wantKind: tormoni.EndSignal, wantCode: ptr(9), wantOK: false,
		},
		{
			name:     "stopped, with no code",
			doc:      `{"end_kind":"stopped","end_code":null}`,
			exitCode: 1,
			wantKind: tormoni.EndStopped, wantCode: nil, wantOK: false,
		},
		{
			name:     "gone, with no code",
			doc:      `{"end_kind":"gone","end_code":null}`,
			exitCode: 1,
			wantKind: tormoni.EndGone, wantCode: nil, wantOK: false,
		},
		{
			name:     "failed, with no code",
			doc:      `{"end_kind":"failed","end_code":null}`,
			exitCode: 1,
			wantKind: tormoni.EndFailed, wantCode: nil, wantOK: false,
		},
		{
			name:     "unknown, with no code",
			doc:      `{"end_kind":"unknown","end_code":null}`,
			exitCode: 1,
			wantKind: tormoni.EndUnknown, wantCode: nil, wantOK: false,
		},
		{
			name:     "still going",
			doc:      `{"end_kind":null,"end_code":null}`,
			exitCode: 0,
			wantKind: "", wantCode: nil, wantOK: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := newStub(t, tt.doc, "", tt.exitCode)
			run, err := s.Client().Run(context.Background(), []string{"sh", "-c", "whatever"}, nil)
			if err != nil {
				t.Fatalf("a guest exit of %d was raised as an error: %v", tt.exitCode, err)
			}
			if run.EndKind != tt.wantKind {
				t.Errorf("EndKind = %q, want %q", run.EndKind, tt.wantKind)
			}
			switch {
			case tt.wantCode == nil && run.EndCode != nil:
				t.Errorf("EndCode = %d, want nil", *run.EndCode)
			case tt.wantCode != nil && run.EndCode == nil:
				t.Errorf("EndCode = nil, want %d", *tt.wantCode)
			case tt.wantCode != nil && *run.EndCode != *tt.wantCode:
				t.Errorf("EndCode = %d, want %d", *run.EndCode, *tt.wantCode)
			}
			if run.OK() != tt.wantOK {
				t.Errorf("OK() = %v, want %v", run.OK(), tt.wantOK)
			}
		})
	}
}

// TestEnvValuesGoOutAndNamesComeBack pins the asymmetry: the guest gets the
// whole KEY=VALUE entry, and the record keeps only the name. No field of a Run
// carries a value, so nothing in this package can read one back.
func TestEnvValuesGoOutAndNamesComeBack(t *testing.T) {
	const secret = "s3cret-value-nobody-should-see"
	doc := `{"run_id":"1-run-1","end_kind":"exit","end_code":0,
	         "posture":{"env":["API_KEY","DEBUG"]}}`
	s := newStub(t, doc, "", 0)

	run, err := s.Client().Run(context.Background(), []string{"env"}, &tormoni.RunOptions{
		Env: []string{"API_KEY=" + secret, "DEBUG=1"},
	})
	if err != nil {
		t.Fatalf("run: %v", err)
	}

	// The value went out on the command line, whole.
	wantArgv := []string{"run", "--json",
		"--env", "API_KEY=" + secret,
		"--env", "DEBUG=1",
		"--", "env"}
	if got := s.Argv(); !reflect.DeepEqual(got, wantArgv) {
		t.Errorf("argv mismatch\n got: %q\nwant: %q", got, wantArgv)
	}

	// Only names came back.
	if !reflect.DeepEqual(run.Posture.Env, []string{"API_KEY", "DEBUG"}) {
		t.Errorf("Posture.Env = %q, want names only", run.Posture.Env)
	}

	// And no corner of the decoded record holds the value.
	encoded, err := json.Marshal(run)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if strings.Contains(string(encoded), secret) {
		t.Errorf("an environment value survived into the record: %s", encoded)
	}
}
