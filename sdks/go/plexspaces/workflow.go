// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Workflow retry helpers aligned with proto RetryConfig (plexspaces.workflow.v1.RetryConfig).
// Single run-with-config semantics: use DefaultRetryConfig() for one attempt;
// pass config with MaxAttempts > 1 for retries.

package plexspaces

// RetryConfig aligns with proto RetryConfig. Unset MaxAttempts is treated as 1 at runtime.
type RetryConfig struct {
	MaxAttempts       uint32   // 1 = no retries; 0/unset treated as 1
	InitialIntervalMs uint32   // default 100
	BackoffRate       float64  // default 2.0
	MaxIntervalMs     uint32   // default 30000
	RetryableErrors   []string // empty = all
}

// DefaultRetryConfig returns single-attempt config. Aligns with proto when fields unset.
func DefaultRetryConfig() RetryConfig {
	return RetryConfig{
		MaxAttempts:       1,
		InitialIntervalMs: 100,
		BackoffRate:       2.0,
		MaxIntervalMs:     30000,
		RetryableErrors:   nil,
	}
}

func effectiveMaxAttempts(c *RetryConfig) int {
	if c == nil || c.MaxAttempts == 0 {
		return 1
	}
	return int(c.MaxAttempts)
}

// WithRetry runs fn with retries. When config is nil or MaxAttempts is 0/1, single attempt.
// When config is provided with MaxAttempts > 1, retries up to that many times.
// When config is nil, 3 attempts are used (retry-helper default).
// Aligns with Rust run(name, retry, op) and proto RetryConfig.
func WithRetry[T any](fn func() (T, error), config *RetryConfig) (T, error) {
	var zero T
	fromConfig := 0
	if config != nil {
		fromConfig = effectiveMaxAttempts(config)
	}
	maxAttempts := fromConfig
	if maxAttempts <= 0 {
		maxAttempts = 3
	}
	var lastErr error
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		out, err := fn()
		if err == nil {
			return out, nil
		}
		lastErr = err
		if attempt == maxAttempts {
			return zero, err
		}
	}
	return zero, lastErr
}
