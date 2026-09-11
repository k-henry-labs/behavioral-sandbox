// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni_test

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"

	tormoni "github.com/kendricklawton/tormoni/sdk/go"
)

// TestDecodeRecord checks every field of the contract's own document.
func TestDecodeRecord(t *testing.T) {
	s := newStub(t, sampleDoc, "", 0)
	run, err := s.Client().Run(context.Background(), []string{"sh", "-c", "echo hello"}, nil)
	if err != nil {
		t.Fatalf("run: %v", err)
	}

	if run.RunID != "1789085063489-run-81523" {
		t.Errorf("RunID = %q", run.RunID)
	}
	if run.Name != "run-81523" {
		t.Errorf("Name = %q", run.Name)
	}
	if run.Verb != "run" {
		t.Errorf("Verb = %q", run.Verb)
	}
	wantCommand := []string{"/bin/busybox", "sh", "-c", "echo hello"}
	if !reflect.DeepEqual(run.Command, wantCommand) {
		t.Errorf("Command = %q, want %q", run.Command, wantCommand)
	}
	if run.StartedMS != 1789085063489 {
		t.Errorf("StartedMS = %d", run.StartedMS)
	}
	if run.EndedMS == nil || *run.EndedMS != 1789085063833 {
		t.Errorf("EndedMS = %v", run.EndedMS)
	}
	if run.EndKind != tormoni.EndExit {
		t.Errorf("EndKind = %q", run.EndKind)
	}
	if run.EndCode == nil || *run.EndCode != 0 {
		t.Errorf("EndCode = %v", run.EndCode)
	}
	if run.PID == nil || *run.PID != 81529 {
		t.Errorf("PID = %v", run.PID)
	}
	if run.Stdout != "hello\n" {
		t.Errorf("Stdout = %q", run.Stdout)
	}
	if run.Stderr != "" {
		t.Errorf("Stderr = %q", run.Stderr)
	}
	if run.StdoutBytes != 6 || run.StderrBytes != 0 {
		t.Errorf("bytes = %d/%d, want 6/0", run.StdoutBytes, run.StderrBytes)
	}
	if run.OutputTruncated {
		t.Error("OutputTruncated = true, want false")
	}
	wantFiles := []tormoni.File{{Path: "note.txt", SizeBytes: 5}}
	if !reflect.DeepEqual(run.Files, wantFiles) {
		t.Errorf("Files = %+v, want %+v", run.Files, wantFiles)
	}
	if run.Dir != "/home/you/.local/share/tormoni/runs/1789085063489-run-81523" {
		t.Errorf("Dir = %q", run.Dir)
	}
	if run.Live != nil {
		t.Errorf("Live = %v on a run record, want nil", run.Live)
	}
	if !run.OK() {
		t.Error("OK() = false, want true")
	}

	p := run.Posture
	if p.Root != "/home/you/.local/share/tormoni/rootfs" {
		t.Errorf("Posture.Root = %q", p.Root)
	}
	if p.Rootfs != "read-only" {
		t.Errorf("Posture.Rootfs = %q", p.Rootfs)
	}
	wantMounts := []tormoni.Mount{{Guest: "/mnt", Host: "/home/you/project"}}
	if !reflect.DeepEqual(p.Mounts, wantMounts) {
		t.Errorf("Posture.Mounts = %+v, want %+v", p.Mounts, wantMounts)
	}
	if len(p.Shares) != 0 {
		t.Errorf("Posture.Shares = %+v, want empty", p.Shares)
	}
	if p.Network != "none" {
		t.Errorf("Posture.Network = %q", p.Network)
	}
	if p.Display != nil {
		t.Errorf("Posture.Display = %v, want nil", p.Display)
	}
	if p.Sound || p.GPU {
		t.Errorf("Posture sound=%v gpu=%v, want both false", p.Sound, p.GPU)
	}
	if !p.Results {
		t.Error("Posture.Results = false, want true")
	}
	if !reflect.DeepEqual(p.Env, []string{"API_KEY"}) {
		t.Errorf("Posture.Env = %q, want [API_KEY]", p.Env)
	}
	if p.VCPUs != 1 || p.MemMiB != 512 {
		t.Errorf("Posture vcpus=%d mem_mib=%d, want 1/512", p.VCPUs, p.MemMiB)
	}
}

// TestRecordRoundTrips checks the pair fields marshal back to the wire shape
// the CLI wrote, so a decoded record can be re-encoded without drifting.
func TestRecordRoundTrips(t *testing.T) {
	var run tormoni.Run
	if err := json.Unmarshal([]byte(sampleDoc), &run); err != nil {
		t.Fatalf("decode: %v", err)
	}
	out, err := json.Marshal(run)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	for _, want := range []string{
		`"mounts":[["/mnt","/home/you/project"]]`,
		`"shares":[]`,
		`"env":["API_KEY"]`,
		`"end_kind":"exit"`,
		`"output_truncated":false`,
	} {
		if !strings.Contains(string(out), want) {
			t.Errorf("re-encoded record is missing %s\ngot: %s", want, out)
		}
	}
}

// TestPairsRoundTrip exercises both directions of the two-element array pairs,
// with shares non-empty — the contract's sample record leaves them out, so
// nothing else encodes one.
func TestPairsRoundTrip(t *testing.T) {
	const doc = `{"posture":{"mounts":[["/mnt","/home/you/project"],["/data","/var/data"]],
	              "shares":[["models","/opt/models"]]}}`
	var run tormoni.Run
	if err := json.Unmarshal([]byte(doc), &run); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got := run.Posture.Mounts[1]; got.Guest != "/data" || got.Host != "/var/data" {
		t.Errorf("mount decoded as %+v, want guest first", got)
	}
	if got := run.Posture.Shares[0]; got.Tag != "models" || got.Host != "/opt/models" {
		t.Errorf("share decoded as %+v, want tag first", got)
	}

	out, err := json.Marshal(run.Posture)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	for _, want := range []string{
		`"mounts":[["/mnt","/home/you/project"],["/data","/var/data"]]`,
		`"shares":[["models","/opt/models"]]`,
	} {
		if !strings.Contains(string(out), want) {
			t.Errorf("re-encoded posture is missing %s\ngot: %s", want, out)
		}
	}
}

// TestShareDecoding covers the [tag, host_path] pairs, which the contract's
// sample leaves empty.
func TestShareDecoding(t *testing.T) {
	doc := `{"posture":{"shares":[["models","/opt/models"],["cache","/var/cache"]]}}`
	s := newStub(t, doc, "", 0)
	run, err := s.Client().Show(context.Background(), "whatever")
	if err != nil {
		t.Fatalf("show: %v", err)
	}
	want := []tormoni.Share{{Tag: "models", Host: "/opt/models"}, {Tag: "cache", Host: "/var/cache"}}
	if !reflect.DeepEqual(run.Posture.Shares, want) {
		t.Errorf("Shares = %+v, want %+v", run.Posture.Shares, want)
	}
}

// TestMalformedPairs: a pair that is not two strings is a decode failure with
// a message naming what was expected, not a silently empty Mount.
func TestMalformedPairs(t *testing.T) {
	tests := []struct {
		name string
		doc  string
		want string
	}{
		{"a mount that is a string", `{"posture":{"mounts":["/mnt=/host"]}}`, "guest_path"},
		{"a mount that is an object", `{"posture":{"mounts":[{"guest":"/mnt"}]}}`, "guest_path"},
		{"a share that is a number", `{"posture":{"shares":[7]}}`, "tag"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := newStub(t, tt.doc, "", 0)
			_, err := s.Client().Show(context.Background(), "run-1")
			if err == nil {
				t.Fatal("err = nil, want a decode failure")
			}
			var tErr *tormoni.Error
			if !errors.As(err, &tErr) {
				t.Fatalf("err = %#v, want *tormoni.Error", err)
			}
			if !strings.Contains(tErr.Err.Error(), tt.want) {
				t.Errorf("cause = %q, want it to mention %q", tErr.Err, tt.want)
			}
		})
	}
}

// TestDryRunHasNoEnd checks the fields a dry run leaves out: a null end is not
// an end of zero.
func TestDryRunHasNoEnd(t *testing.T) {
	doc := `{"run_id":"1-run-1","verb":"run","posture":{"vcpus":2,"mem_mib":1024},
	         "started_ms":1789085063489,"ended_ms":null,"end_kind":null,"end_code":null,"pid":null}`
	s := newStub(t, doc, "", 0)
	run, err := s.Client().DryRun(context.Background(), []string{"true"}, nil)
	if err != nil {
		t.Fatalf("dry run: %v", err)
	}
	if run.EndKind != "" {
		t.Errorf("EndKind = %q, want empty for a null end", run.EndKind)
	}
	if run.EndCode != nil {
		t.Errorf("EndCode = %v, want nil", run.EndCode)
	}
	if run.EndedMS != nil {
		t.Errorf("EndedMS = %v, want nil", run.EndedMS)
	}
	if run.PID != nil {
		t.Errorf("PID = %v, want nil", run.PID)
	}
	if run.OK() {
		t.Error("OK() = true for a run with no end")
	}
	if run.Posture.VCPUs != 2 || run.Posture.MemMiB != 1024 {
		t.Errorf("posture did not settle: %+v", run.Posture)
	}
}

// TestOutputTruncated checks the cut flag reaches the caller, so a prefix is
// never mistaken for the whole thing.
func TestOutputTruncated(t *testing.T) {
	doc := `{"end_kind":"exit","end_code":0,"stdout":"the first part of","stdout_bytes":1048576,
	         "stderr":"and of this too","stderr_bytes":4096,"output_truncated":true}`
	s := newStub(t, doc, "", 0)
	run, err := s.Client().Run(context.Background(), []string{"yes"}, nil)
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	if !run.OutputTruncated {
		t.Fatal("OutputTruncated = false, want true")
	}
	if int64(len(run.Stdout)) >= run.StdoutBytes {
		t.Errorf("Stdout is %d bytes but StdoutBytes is %d; the prefix should be shorter",
			len(run.Stdout), run.StdoutBytes)
	}
	if run.StderrBytes != 4096 {
		t.Errorf("StderrBytes = %d, want 4096", run.StderrBytes)
	}
}

// TestOKOnAValue checks OK is callable on a Run that has no address, which a
// pointer receiver would have made a compile error.
func TestOKOnAValue(t *testing.T) {
	if !(tormoni.Run{EndKind: tormoni.EndExit, EndCode: ptr(0)}).OK() {
		t.Error("OK() = false on a clean exit")
	}
	byValue := map[string]tormoni.Run{
		"failed": {EndKind: tormoni.EndExit, EndCode: ptr(1)},
	}
	if byValue["failed"].OK() {
		t.Error("OK() = true on a non-zero exit")
	}
}

// TestJSONTags pins the wire names, so decoding never leans on Go's
// case-insensitive fallback and no field for environment values can be added
// without this failing.
func TestJSONTags(t *testing.T) {
	tests := []struct {
		typ  reflect.Type
		want map[string]string // Go field -> json tag
	}{
		{
			typ: reflect.TypeOf(tormoni.Run{}),
			want: map[string]string{
				"RunID": "run_id", "Name": "name", "Verb": "verb", "Command": "command",
				"Posture": "posture", "StartedMS": "started_ms", "EndedMS": "ended_ms",
				"EndKind": "end_kind", "EndCode": "end_code", "PID": "pid",
				"Stdout": "stdout", "Stderr": "stderr", "StdoutBytes": "stdout_bytes",
				"StderrBytes": "stderr_bytes", "OutputTruncated": "output_truncated",
				"Files": "files", "Dir": "dir", "Live": "live",
			},
		},
		{
			typ: reflect.TypeOf(tormoni.Posture{}),
			want: map[string]string{
				"Root": "root", "Rootfs": "rootfs", "Mounts": "mounts", "Shares": "shares",
				"Network": "network", "Display": "display", "Sound": "sound", "GPU": "gpu",
				"Results": "results", "Env": "env", "VCPUs": "vcpus", "MemMiB": "mem_mib",
			},
		},
		{
			typ:  reflect.TypeOf(tormoni.File{}),
			want: map[string]string{"Path": "path", "SizeBytes": "size_bytes"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.typ.Name(), func(t *testing.T) {
			got := map[string]string{}
			for i := 0; i < tt.typ.NumField(); i++ {
				f := tt.typ.Field(i)
				tag, ok := f.Tag.Lookup("json")
				if !ok {
					t.Errorf("field %s has no json tag", f.Name)
					continue
				}
				got[f.Name] = strings.Split(tag, ",")[0]
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("fields and tags mismatch\n got: %v\nwant: %v", got, tt.want)
			}
		})
	}
}

func TestRuns(t *testing.T) {
	doc := `{"runs":[
	  {"run_id":"1-run-1","name":"run-1","verb":"run","live":true,"end_kind":null,"end_code":null,
	   "posture":{"network":"tsi","vcpus":2,"mem_mib":1024},"started_ms":1789085063489},
	  {"run_id":"2-run-2","name":"run-2","verb":"run","live":false,"end_kind":"exit","end_code":0,
	   "posture":{"network":"none","vcpus":1,"mem_mib":512},"started_ms":1789085063490,
	   "ended_ms":1789085063833}
	]}`
	s := newStub(t, doc, "", 0)
	runs, err := s.Client().Runs(context.Background(), true)
	if err != nil {
		t.Fatalf("runs: %v", err)
	}
	if len(runs) != 2 {
		t.Fatalf("got %d rows, want 2", len(runs))
	}

	live := runs[0]
	if live.Live == nil || !*live.Live {
		t.Errorf("row 0 Live = %v, want true", live.Live)
	}
	if live.EndKind != "" || live.EndCode != nil {
		t.Errorf("a live row has an end: %q/%v", live.EndKind, live.EndCode)
	}
	if live.OK() {
		t.Error("a live row reports OK()")
	}
	if live.Posture.Network != "tsi" {
		t.Errorf("row 0 network = %q", live.Posture.Network)
	}

	done := runs[1]
	if done.Live == nil || *done.Live {
		t.Errorf("row 1 Live = %v, want false", done.Live)
	}
	if !done.OK() {
		t.Error("row 1 OK() = false, want true")
	}
	// Rows carry no captured output; that is what Show is for.
	if done.Stdout != "" || done.Stderr != "" || done.Files != nil || done.Dir != "" {
		t.Errorf("a listing row carried captured output: %+v", done)
	}
}

// TestRunsEmpty: an empty listing is an empty slice, never nil, and a null
// array means the same as an empty one.
func TestRunsEmpty(t *testing.T) {
	for _, doc := range []string{`{"runs":[]}`, `{"runs":null}`} {
		s := newStub(t, doc, "", 0)
		runs, err := s.Client().Runs(context.Background(), false)
		if err != nil {
			t.Fatalf("%s: %v", doc, err)
		}
		if runs == nil || len(runs) != 0 {
			t.Errorf("%s: runs = %v, want an empty slice", doc, runs)
		}
	}
}

// TestRunsRejectsOtherShapes: the verb answers with an object holding a runs
// array, never a bare array, and never a record.
func TestRunsRejectsOtherShapes(t *testing.T) {
	for _, doc := range []string{
		`[{"run_id":"1-run-1"}]`,
		`{"run_id":"1-run-1","end_kind":"exit"}`,
		`{"runs":{"1-run-1":{}}}`,
	} {
		s := newStub(t, doc, "", 0)
		if runs, err := s.Client().Runs(context.Background(), false); err == nil {
			t.Errorf("%s: err = nil and runs = %v, want an operational failure", doc, runs)
		}
	}
}
