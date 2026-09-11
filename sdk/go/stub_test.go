// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni_test

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	tormoni "github.com/kendricklawton/tormoni/sdk/go"
)

// sampleDoc is the record from the CLI contract, verbatim.
const sampleDoc = `{
  "run_id": "1789085063489-run-81523",
  "name": "run-81523",
  "verb": "run",
  "command": ["/bin/busybox", "sh", "-c", "echo hello"],
  "posture": {
    "root": "/home/you/.local/share/tormoni/rootfs",
    "rootfs": "read-only",
    "mounts": [["/mnt", "/home/you/project"]],
    "shares": [],
    "network": "none",
    "display": null,
    "sound": false,
    "gpu": false,
    "results": true,
    "env": ["API_KEY"],
    "vcpus": 1,
    "mem_mib": 512
  },
  "started_ms": 1789085063489,
  "ended_ms": 1789085063833,
  "end_kind": "exit",
  "end_code": 0,
  "pid": 81529,
  "stdout": "hello\n",
  "stderr": "",
  "stdout_bytes": 6,
  "stderr_bytes": 0,
  "output_truncated": false,
  "files": [{ "path": "note.txt", "size_bytes": 5 }],
  "dir": "/home/you/.local/share/tormoni/runs/1789085063489-run-81523"
}`

// okDoc is the smallest record of a command that exited cleanly.
const okDoc = `{"run_id":"1-run-1","end_kind":"exit","end_code":0,"stdout":"hi\n","stdout_bytes":3}`

// stub stands in for the tormoni binary. It records the argv it was called
// with and prints whatever the test canned for it, so the suite never boots a
// VM and never needs a hypervisor or a network.
type stub struct {
	t        *testing.T
	Path     string
	argvPath string
}

// writeStub writes an executable named "tormoni" whose body is body, prefixed
// with the argv recording every stub does.
func writeStub(t *testing.T, body string) *stub {
	t.Helper()
	dir := t.TempDir()
	argvPath := filepath.Join(dir, "argv")
	script := "#!/bin/sh\n" +
		fmt.Sprintf("for a in \"$@\"; do printf '%%s\\n' \"$a\" >> '%s'; done\n", argvPath) +
		body
	path := filepath.Join(dir, "tormoni")
	if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
		t.Fatalf("writing stub: %v", err)
	}
	return &stub{t: t, Path: path, argvPath: argvPath}
}

// newStub returns a stub that prints stdout verbatim, prints stderrText to
// stderr, and exits with exitCode.
func newStub(t *testing.T, stdout, stderrText string, exitCode int) *stub {
	t.Helper()
	dir := t.TempDir()
	outPath := filepath.Join(dir, "stdout")
	errPath := filepath.Join(dir, "stderr")
	if err := os.WriteFile(outPath, []byte(stdout), 0o644); err != nil {
		t.Fatalf("writing stub stdout: %v", err)
	}
	if err := os.WriteFile(errPath, []byte(stderrText), 0o644); err != nil {
		t.Fatalf("writing stub stderr: %v", err)
	}
	return writeStub(t, fmt.Sprintf("cat '%s'\ncat '%s' >&2\nexit %d\n", outPath, errPath, exitCode))
}

// Client returns a Client pointed straight at the stub.
func (s *stub) Client() *tormoni.Client { return tormoni.NewWithPath(s.Path) }

// Argv is the argument list the stub was last called with, without the binary.
func (s *stub) Argv() []string {
	s.t.Helper()
	b, err := os.ReadFile(s.argvPath)
	if errors.Is(err, os.ErrNotExist) {
		return nil // called with no arguments at all
	}
	if err != nil {
		s.t.Fatalf("reading recorded argv: %v", err)
	}
	trimmed := strings.TrimSuffix(string(b), "\n")
	if trimmed == "" {
		return nil
	}
	return strings.Split(trimmed, "\n")
}

func ptr[T any](v T) *T { return &v }

// shellQuote wraps s for use as a single-quoted shell word.
func shellQuote(s string) string {
	return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'"
}
