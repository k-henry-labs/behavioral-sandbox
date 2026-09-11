// Copyright 2026 The Boxdesk Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package boxdesk_test

import (
	"context"
	"os"
	"testing"
	"time"

	boxdesk "github.com/kendricklawton/boxdesk/sdk/go"
)

// TestIntegrationRun boots a real sandbox with the real binary. It is skipped
// unless BOXDESK_INTEGRATION=1, because CI has no hypervisor.
//
//	BOXDESK_INTEGRATION=1 go test -run TestIntegration -v ./...
func TestIntegrationRun(t *testing.T) {
	if os.Getenv("BOXDESK_INTEGRATION") != "1" {
		t.Skip("set BOXDESK_INTEGRATION=1 to run against a real Boxdesk install")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	c := boxdesk.New()

	run, err := c.Run(ctx, []string{"echo", "hi"}, nil)
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	if !run.OK() {
		t.Fatalf("run did not finish cleanly: end_kind=%q end_code=%v stderr=%q",
			run.EndKind, run.EndCode, run.Stderr)
	}
	if run.Stdout != "hi\n" {
		t.Errorf("stdout = %q, want %q", run.Stdout, "hi\n")
	}
	if run.RunID == "" {
		t.Error("run_id is empty")
	}

	// The record is readable again by id, and the run is listed.
	shown, err := c.Show(ctx, run.RunID)
	if err != nil {
		t.Fatalf("show: %v", err)
	}
	if shown.RunID != run.RunID {
		t.Errorf("show returned run_id %q, want %q", shown.RunID, run.RunID)
	}
	if _, err := c.Runs(ctx, true); err != nil {
		t.Fatalf("runs: %v", err)
	}

	// A dry run settles a posture without booting.
	plan, err := c.DryRun(ctx, []string{"echo", "hi"}, nil)
	if err != nil {
		t.Fatalf("dry run: %v", err)
	}
	if plan.EndKind != "" {
		t.Errorf("dry run has end_kind %q, want none", plan.EndKind)
	}

	// A non-zero guest exit is a record, not an error.
	failed, err := c.Run(ctx, []string{"sh", "-c", "exit 3"}, nil)
	if err != nil {
		t.Fatalf("non-zero guest exit returned an error: %v", err)
	}
	if failed.OK() || failed.EndCode == nil || *failed.EndCode != 3 {
		t.Errorf("end_kind=%q end_code=%v, want exit/3", failed.EndKind, failed.EndCode)
	}
}
