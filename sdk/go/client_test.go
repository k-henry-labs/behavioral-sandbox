// Copyright 2026 The Boxdesk Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package boxdesk_test

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	boxdesk "github.com/kendricklawton/boxdesk/sdk/go"
)

// TestArgv pins the exact argument list the client builds, including flag
// order and repeated flags.
func TestArgv(t *testing.T) {
	tests := []struct {
		name string
		call func(context.Context, *boxdesk.Client) error
		want []string
	}{
		{
			name: "run with no options",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"echo", "hi"}, nil)
				return err
			},
			want: []string{"run", "--json", "--", "echo", "hi"},
		},
		{
			name: "run with empty options adds no flags",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"echo", "hi"}, &boxdesk.RunOptions{})
				return err
			},
			want: []string{"run", "--json", "--", "echo", "hi"},
		},
		{
			name: "the command is passed through verbatim after the separator",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"sh", "-c", "echo hello && exit 1"}, nil)
				return err
			},
			want: []string{"run", "--json", "--", "sh", "-c", "echo hello && exit 1"},
		},
		{
			name: "a command that looks like flags is still the guest's command",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"ls", "--all", "--json"}, nil)
				return err
			},
			want: []string{"run", "--json", "--", "ls", "--all", "--json"},
		},
		{
			name: "root",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Root: "/srv/rootfs"})
				return err
			},
			want: []string{"run", "--json", "--root", "/srv/rootfs", "--", "true"},
		},
		{
			name: "vcpus, including a zero the caller meant",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{VCPUs: ptr(0)})
				return err
			},
			want: []string{"run", "--json", "--vcpus", "0", "--", "true"},
		},
		{
			name: "mem",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{MemMiB: ptr(2048)})
				return err
			},
			want: []string{"run", "--json", "--mem", "2048", "--", "true"},
		},
		{
			name: "workdir",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Workdir: "/work"})
				return err
			},
			want: []string{"run", "--json", "--workdir", "/work", "--", "true"},
		},
		{
			name: "mounts repeat in the order given",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Mounts: []boxdesk.Mount{
					{Guest: "/mnt", Host: "/home/you/project"},
					{Guest: "/data", Host: "/var/data"},
				}})
				return err
			},
			want: []string{"run", "--json",
				"--mount", "/mnt=/home/you/project",
				"--mount", "/data=/var/data",
				"--", "true"},
		},
		{
			name: "shares repeat in the order given",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Shares: []boxdesk.Share{
					{Tag: "models", Host: "/opt/models"},
					{Tag: "cache", Host: "/var/cache"},
				}})
				return err
			},
			want: []string{"run", "--json",
				"--share", "models=/opt/models",
				"--share", "cache=/var/cache",
				"--", "true"},
		},
		{
			name: "net",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Net: "tsi"})
				return err
			},
			want: []string{"run", "--json", "--net", "tsi", "--", "true"},
		},
		{
			name: "rootfs",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Rootfs: "writable"})
				return err
			},
			want: []string{"run", "--json", "--rootfs", "writable", "--", "true"},
		},
		{
			name: "env repeats, and carries the whole entry",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{
					Env: []string{"API_KEY=s3cret", "DEBUG=1", "EMPTY="},
				})
				return err
			},
			want: []string{"run", "--json",
				"--env", "API_KEY=s3cret",
				"--env", "DEBUG=1",
				"--env", "EMPTY=",
				"--", "true"},
		},
		{
			name: "an env value containing = is passed through whole",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{
					Env: []string{"TOKEN=abc=def=="},
				})
				return err
			},
			want: []string{"run", "--json", "--env", "TOKEN=abc=def==", "--", "true"},
		},
		{
			name: "name",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Name: "builder"})
				return err
			},
			want: []string{"run", "--json", "--name", "builder", "--", "true"},
		},
		{
			name: "no-results",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{NoResults: true})
				return err
			},
			want: []string{"run", "--json", "--no-results", "--", "true"},
		},
		{
			name: "gpu",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{GPU: true})
				return err
			},
			want: []string{"run", "--json", "--gpu", "--", "true"},
		},
		{
			name: "sound",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Sound: true})
				return err
			},
			want: []string{"run", "--json", "--sound", "--", "true"},
		},
		{
			name: "display",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"true"}, &boxdesk.RunOptions{Display: "1280x800@60"})
				return err
			},
			want: []string{"run", "--json", "--display", "1280x800@60", "--", "true"},
		},
		{
			name: "every flag at once, in table order",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Run(ctx, []string{"sh", "-c", "make"}, &boxdesk.RunOptions{
					Root:      "/srv/rootfs",
					VCPUs:     ptr(4),
					MemMiB:    ptr(2048),
					Workdir:   "/work",
					Mounts:    []boxdesk.Mount{{Guest: "/mnt", Host: "/home/you/project"}},
					Shares:    []boxdesk.Share{{Tag: "models", Host: "/opt/models"}},
					Net:       "tsi",
					Rootfs:    "writable",
					Env:       []string{"API_KEY=s3cret"},
					Name:      "builder",
					NoResults: true,
					GPU:       true,
					Sound:     true,
					Display:   "1280x800@60",
				})
				return err
			},
			want: []string{"run", "--json",
				"--root", "/srv/rootfs",
				"--vcpus", "4",
				"--mem", "2048",
				"--workdir", "/work",
				"--mount", "/mnt=/home/you/project",
				"--share", "models=/opt/models",
				"--net", "tsi",
				"--rootfs", "writable",
				"--env", "API_KEY=s3cret",
				"--name", "builder",
				"--no-results",
				"--gpu",
				"--sound",
				"--display", "1280x800@60",
				"--", "sh", "-c", "make"},
		},
		{
			name: "dry run sits between the verb and the flags",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.DryRun(ctx, []string{"true"}, &boxdesk.RunOptions{Net: "tsi"})
				return err
			},
			want: []string{"run", "--json", "--dry-run", "--net", "tsi", "--", "true"},
		},
		{
			name: "show",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Show(ctx, "1789085063489-run-81523")
				return err
			},
			want: []string{"show", "--json", "1789085063489-run-81523"},
		},
		{
			name: "ls",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Runs(ctx, false)
				return err
			},
			want: []string{"ls", "--json"},
		},
		{
			name: "ls --all",
			call: func(ctx context.Context, c *boxdesk.Client) error {
				_, err := c.Runs(ctx, true)
				return err
			},
			want: []string{"ls", "--json", "--all"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Every verb answers with a document, so no call here errors.
			s := newStub(t, `{"runs":[]}`, "", 0)
			if err := tt.call(context.Background(), s.Client()); err != nil {
				t.Fatalf("call: %v", err)
			}
			if got := s.Argv(); !reflect.DeepEqual(got, tt.want) {
				t.Errorf("argv mismatch\n got: %q\nwant: %q", got, tt.want)
			}
		})
	}
}

func TestBinaryDiscovery(t *testing.T) {
	t.Run("found on PATH", func(t *testing.T) {
		s := newStub(t, okDoc, "", 0)
		t.Setenv("PATH", filepath.Dir(s.Path)+string(os.PathListSeparator)+os.Getenv("PATH"))
		t.Setenv("BOXDESK_CLI", "")
		if _, err := boxdesk.New().Run(context.Background(), []string{"echo", "hi"}, nil); err != nil {
			t.Fatalf("run: %v", err)
		}
		if got := s.Argv(); len(got) == 0 {
			t.Error("the stub on PATH was never called")
		}
	})

	t.Run("BOXDESK_CLI wins over PATH", func(t *testing.T) {
		onPath := newStub(t, okDoc, "", 0)
		named := newStub(t, okDoc, "", 0)
		t.Setenv("PATH", filepath.Dir(onPath.Path)+string(os.PathListSeparator)+os.Getenv("PATH"))
		t.Setenv("BOXDESK_CLI", named.Path)
		if _, err := boxdesk.New().Run(context.Background(), []string{"echo", "hi"}, nil); err != nil {
			t.Fatalf("run: %v", err)
		}
		if len(named.Argv()) == 0 {
			t.Error("the binary named by BOXDESK_CLI was not used")
		}
		if len(onPath.Argv()) != 0 {
			t.Error("the binary on PATH was used even though BOXDESK_CLI was set")
		}
	})

	t.Run("Client.Path wins over BOXDESK_CLI", func(t *testing.T) {
		named := newStub(t, okDoc, "", 0)
		explicit := newStub(t, okDoc, "", 0)
		t.Setenv("BOXDESK_CLI", named.Path)
		if _, err := explicit.Client().Run(context.Background(), []string{"echo", "hi"}, nil); err != nil {
			t.Fatalf("run: %v", err)
		}
		if len(explicit.Argv()) == 0 {
			t.Error("Client.Path was not used")
		}
		if len(named.Argv()) != 0 {
			t.Error("BOXDESK_CLI was used even though Client.Path was set")
		}
	})

	t.Run("the zero Client works", func(t *testing.T) {
		s := newStub(t, okDoc, "", 0)
		t.Setenv("BOXDESK_CLI", s.Path)
		var c boxdesk.Client
		run, err := c.Run(context.Background(), []string{"echo", "hi"}, nil)
		if err != nil {
			t.Fatalf("run: %v", err)
		}
		if !run.OK() {
			t.Error("OK() = false")
		}
	})
}

// TestBinaryNotFound covers each way of naming a binary that is not there.
func TestBinaryNotFound(t *testing.T) {
	empty := t.TempDir()

	t.Run("named path does not exist", func(t *testing.T) {
		c := boxdesk.NewWithPath(filepath.Join(empty, "nowhere", "boxdesk"))
		_, err := c.Run(context.Background(), []string{"echo", "hi"}, nil)
		if !errors.Is(err, boxdesk.ErrNotFound) {
			t.Fatalf("err = %#v, want ErrNotFound", err)
		}
		// The message says what to do about it.
		msg := err.Error()
		for _, want := range []string{"boxdesk", "BOXDESK_CLI", "Client.Path"} {
			if !strings.Contains(msg, want) {
				t.Errorf("message %q does not mention %q", msg, want)
			}
		}
	})

	t.Run("not on PATH", func(t *testing.T) {
		t.Setenv("PATH", empty)
		t.Setenv("BOXDESK_CLI", "")
		_, err := boxdesk.New().Runs(context.Background(), true)
		if !errors.Is(err, boxdesk.ErrNotFound) {
			t.Fatalf("err = %#v, want ErrNotFound", err)
		}
	})

	t.Run("BOXDESK_CLI points nowhere", func(t *testing.T) {
		t.Setenv("BOXDESK_CLI", filepath.Join(empty, "nope"))
		_, err := boxdesk.New().Show(context.Background(), "run-1")
		if !errors.Is(err, boxdesk.ErrNotFound) {
			t.Fatalf("err = %#v, want ErrNotFound", err)
		}
		if !strings.Contains(err.Error(), "$BOXDESK_CLI") {
			t.Errorf("message %q does not say where the name came from", err.Error())
		}
	})
}

// TestContextCancellation checks the subprocess is killed and the context's
// own error survives to the caller.
func TestContextCancellation(t *testing.T) {
	t.Run("deadline", func(t *testing.T) {
		s := writeStub(t, "exec sleep 30\n")
		ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
		defer cancel()

		start := time.Now()
		_, err := s.Client().Run(ctx, []string{"sleep", "30"}, nil)
		if elapsed := time.Since(start); elapsed > 10*time.Second {
			t.Fatalf("waited %v; the subprocess was not killed", elapsed)
		}
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("err = %#v, want context.DeadlineExceeded", err)
		}
		var tErr *boxdesk.Error
		if !errors.As(err, &tErr) {
			t.Fatalf("err = %#v, want *boxdesk.Error", err)
		}
	})

	t.Run("cancel", func(t *testing.T) {
		s := writeStub(t, "exec sleep 30\n")
		ctx, cancel := context.WithCancel(context.Background())
		go func() {
			time.Sleep(50 * time.Millisecond)
			cancel()
		}()
		defer cancel()
		if _, err := s.Client().Run(ctx, []string{"sleep", "30"}, nil); !errors.Is(err, context.Canceled) {
			t.Fatalf("err = %#v, want context.Canceled", err)
		}
	})
}

// TestWaitDelayBoundsAHeldPipe: the CLI exiting does not close stdout if some
// process it left behind inherited the descriptor, and waiting on that pipe
// blocks the call for as long as the straggler lives — with no deadline on the
// context, forever. WaitDelay bounds it, and the document the CLI wrote before
// it exited still arrives.
func TestWaitDelayBoundsAHeldPipe(t *testing.T) {
	defer boxdesk.SetWaitDelay(250 * time.Millisecond)()

	// The background subshell inherits stdout and outlives the CLI, holding
	// the pipe open for far longer than any caller would wait.
	s := writeStub(t, "(sleep 30) &\nprintf '%s' "+shellQuote(okDoc)+"\nexit 0\n")

	start := time.Now()
	run, err := s.Client().Run(context.Background(), []string{"echo", "hi"}, nil)
	elapsed := time.Since(start)

	if elapsed > 10*time.Second {
		t.Fatalf("the call took %v: waiting on the held pipe was not bounded", elapsed)
	}
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	// The document was written before the straggler was left holding the pipe,
	// so cutting the wait short must not cost us the record.
	if !run.OK() || run.Stdout != "hi\n" {
		t.Errorf("record did not survive the bounded wait: %+v", run)
	}
}

// TestLibraryWritesNothingToStdio checks the guest's captured output stays in
// the record: library code prints nothing to the process's own streams, on
// either the happy path or the failing one.
func TestLibraryWritesNothingToStdio(t *testing.T) {
	outR, outW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	errR, errW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	origOut, origErr := os.Stdout, os.Stderr
	os.Stdout, os.Stderr = outW, errW

	ok := newStub(t, sampleDoc, "", 0)
	_, runErr := ok.Client().Run(context.Background(), []string{"echo", "hello"}, nil)
	bad := newStub(t, "not json", "boxdesk: the hypervisor did not answer\n", 2)
	_, badErr := bad.Client().Run(context.Background(), []string{"echo", "hello"}, nil)
	missing := boxdesk.NewWithPath(filepath.Join(t.TempDir(), "absent"))
	_, missingErr := missing.Runs(context.Background(), true)

	os.Stdout, os.Stderr = origOut, origErr
	outW.Close()
	errW.Close()
	gotOut, _ := io.ReadAll(outR)
	gotErr, _ := io.ReadAll(errR)

	if runErr != nil {
		t.Fatalf("run: %v", runErr)
	}
	if badErr == nil || missingErr == nil {
		t.Fatal("the failing calls did not fail")
	}
	if len(gotOut) != 0 {
		t.Errorf("library wrote to stdout: %q", gotOut)
	}
	if len(gotErr) != 0 {
		t.Errorf("library wrote to stderr: %q", gotErr)
	}
}
