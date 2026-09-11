// Copyright 2026 The Tormoni Authors. All rights reserved.
// Use of this source code is governed by the Apache-2.0 license that can be
// found in the LICENSE file.

package tormoni

import "time"

// SetWaitDelay shrinks the bound on waiting for pipes a grandchild is holding
// open, so a test can exercise it without waiting the production ten seconds.
// It returns a function that restores the old value.
//
// This file is only compiled into the package's own tests.
func SetWaitDelay(d time.Duration) func() {
	previous := waitDelay
	waitDelay = d
	return func() { waitDelay = previous }
}
