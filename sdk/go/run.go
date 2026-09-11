// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni

import (
	"encoding/json"
	"fmt"
)

// EndKind says how a run finished. It is the empty string while a run is
// still going (the CLI writes null in that case).
//
// Never infer how a run ended from a single string: read both EndKind and
// EndCode. EndCode carries a number only for EndExit and EndSignal.
type EndKind string

// The end kinds Tormoni reports.
const (
	// EndExit means the guest command exited on its own; EndCode is its status.
	EndExit EndKind = "exit"
	// EndSignal means the guest command was killed by a signal; EndCode is the signal.
	EndSignal EndKind = "signal"
	// EndStopped means the sandbox was stopped.
	EndStopped EndKind = "stopped"
	// EndGone means the sandbox disappeared without leaving a status.
	EndGone EndKind = "gone"
	// EndFailed means the sandbox failed to run the command.
	EndFailed EndKind = "failed"
	// EndUnknown means Tormoni could not determine how the run ended.
	EndUnknown EndKind = "unknown"
)

// Run is one Tormoni record: the posture a sandbox was given, how its command
// finished, and what it captured.
//
// The same shape is returned by Run, DryRun, Show and Runs. A dry run has no
// end and no captured output. Rows from Runs omit Stdout, Stderr, Files and
// Dir, and carry Live.
type Run struct {
	RunID   string   `json:"run_id"`
	Name    string   `json:"name"`
	Verb    string   `json:"verb"`
	Command []string `json:"command"`
	Posture Posture  `json:"posture"`

	// StartedMS is epoch milliseconds. EndedMS is nil while a run is still
	// going and for a dry run.
	StartedMS int64  `json:"started_ms"`
	EndedMS   *int64 `json:"ended_ms"`

	// EndKind is empty while a run is still going. EndCode is non-nil only
	// for EndExit and EndSignal.
	EndKind EndKind `json:"end_kind"`
	EndCode *int    `json:"end_code"`

	// PID is the host process id of the sandbox, nil when nothing was booted.
	PID *int `json:"pid"`

	// Stdout and Stderr are the guest's captured streams. When
	// OutputTruncated is true they are a prefix, not the whole thing:
	// StdoutBytes and StderrBytes count what the guest actually wrote.
	Stdout          string `json:"stdout"`
	Stderr          string `json:"stderr"`
	StdoutBytes     int64  `json:"stdout_bytes"`
	StderrBytes     int64  `json:"stderr_bytes"`
	OutputTruncated bool   `json:"output_truncated"`

	// Files are what the guest wrote to /results. Dir is the host directory
	// holding this record.
	Files []File `json:"files"`
	Dir   string `json:"dir"`

	// Live is set only on rows returned by Runs; it is nil on a record from
	// Run, DryRun or Show.
	Live *bool `json:"live,omitempty"`
}

// OK reports whether the guest command exited on its own with status 0.
//
// A non-zero status is not an error: it is a Run with EndKind EndExit and a
// non-zero EndCode.
func (r Run) OK() bool {
	return r.EndKind == EndExit && r.EndCode != nil && *r.EndCode == 0
}

// Posture is the settled configuration a sandbox was given.
type Posture struct {
	Root    string  `json:"root"`
	Rootfs  string  `json:"rootfs"`
	Mounts  []Mount `json:"mounts"`
	Shares  []Share `json:"shares"`
	Network string  `json:"network"`

	// Display is nil when no display was requested.
	Display *string `json:"display"`
	Sound   bool    `json:"sound"`
	GPU     bool    `json:"gpu"`
	Results bool    `json:"results"`

	// Env holds environment variable NAMES only. Tormoni deliberately never
	// writes an environment value to a record, so no value is available here
	// or anywhere else in a Run.
	Env []string `json:"env"`

	VCPUs  int `json:"vcpus"`
	MemMiB int `json:"mem_mib"`
}

// Mount is a host directory made available read-write at a guest path. On the
// wire it is the two-element array [guest_path, host_path].
type Mount struct {
	Guest string
	Host  string
}

// MarshalJSON writes the mount as [guest_path, host_path].
func (m Mount) MarshalJSON() ([]byte, error) {
	return json.Marshal([2]string{m.Guest, m.Host})
}

// UnmarshalJSON reads the mount from [guest_path, host_path].
func (m *Mount) UnmarshalJSON(b []byte) error {
	var pair [2]string
	if err := json.Unmarshal(b, &pair); err != nil {
		return fmt.Errorf("tormoni: mount is not a [guest_path, host_path] pair: %w", err)
	}
	m.Guest, m.Host = pair[0], pair[1]
	return nil
}

// Share is an extra virtiofs device the guest mounts by tag. On the wire it is
// the two-element array [tag, host_path].
type Share struct {
	Tag  string
	Host string
}

// MarshalJSON writes the share as [tag, host_path].
func (s Share) MarshalJSON() ([]byte, error) {
	return json.Marshal([2]string{s.Tag, s.Host})
}

// UnmarshalJSON reads the share from [tag, host_path].
func (s *Share) UnmarshalJSON(b []byte) error {
	var pair [2]string
	if err := json.Unmarshal(b, &pair); err != nil {
		return fmt.Errorf("tormoni: share is not a [tag, host_path] pair: %w", err)
	}
	s.Tag, s.Host = pair[0], pair[1]
	return nil
}

// File is one file the guest left in /results.
type File struct {
	Path      string `json:"path"`
	SizeBytes int64  `json:"size_bytes"`
}
