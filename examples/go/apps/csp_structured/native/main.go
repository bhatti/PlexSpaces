// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// CSP Structured Concurrency — Native Go
//
// Demonstrates the scatter-gather pattern three ways:
// 1. Naive (goroutine leak)
// 2. context.WithTimeout + errgroup (proper structured concurrency)
// 3. CSP-style select with rendezvous channels

package main

import (
	"context"
	"fmt"
	"sync"
	"time"

	"golang.org/x/sync/errgroup"
)

// ServiceResponse represents a response from a backend service.
type ServiceResponse struct {
	ServiceID int
	Data      string
	Latency   time.Duration
}

// simulateService models a backend call with variable latency.
func simulateService(ctx context.Context, id int, latency time.Duration) (ServiceResponse, error) {
	select {
	case <-time.After(latency):
		return ServiceResponse{
			ServiceID: id,
			Data:      fmt.Sprintf("response-from-service-%d", id),
			Latency:   latency,
		}, nil
	case <-ctx.Done():
		return ServiceResponse{}, ctx.Err()
	}
}

// ---------------------------------------------------------------------------
// Approach 1: NAIVE — goroutine leak
// ---------------------------------------------------------------------------

// ScatterGatherNaive spawns goroutines with no cancellation.
// BUG: goroutines for slow services leak when we return after collecting K results.
func ScatterGatherNaive(services []time.Duration, firstK int) []ServiceResponse {
	ch := make(chan ServiceResponse, len(services))

	// Fire-and-forget goroutines — no context, no cancellation
	for i, latency := range services {
		go func(id int, lat time.Duration) {
			time.Sleep(lat)
			ch <- ServiceResponse{
				ServiceID: id,
				Data:      fmt.Sprintf("response-from-service-%d", id),
				Latency:   lat,
			}
		}(i, latency)
	}

	// Collect first K results — remaining goroutines LEAK
	results := make([]ServiceResponse, 0, firstK)
	for range firstK {
		results = append(results, <-ch)
	}
	// BUG: N-K goroutines still running, channel never drained
	return results
}

// ---------------------------------------------------------------------------
// Approach 2: context.WithTimeout + errgroup (structured)
// ---------------------------------------------------------------------------

// ScatterGatherStructured uses errgroup for structured lifetime management.
// All goroutines are cancelled when the context deadline fires.
func ScatterGatherStructured(services []time.Duration, firstK int, timeout time.Duration) []ServiceResponse {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	var mu sync.Mutex
	results := make([]ServiceResponse, 0, firstK)

	g, ctx := errgroup.WithContext(ctx)
	for i, latency := range services {
		g.Go(func() error {
			resp, err := simulateService(ctx, i, latency)
			if err != nil {
				return nil // cancelled — not an error
			}
			mu.Lock()
			defer mu.Unlock()
			if len(results) < firstK {
				results = append(results, resp)
				if len(results) >= firstK {
					cancel() // Got enough — cancel the rest
				}
			}
			return nil
		})
	}

	_ = g.Wait() // All goroutines done — structured lifetime guarantee
	return results
}

// ---------------------------------------------------------------------------
// Approach 3: CSP-style select over channels
// ---------------------------------------------------------------------------

// ScatterGatherCSP uses select as a guarded command over per-service channels.
// Demonstrates: rendezvous semantics, timeout guard, deterministic priority.
func ScatterGatherCSP(services []time.Duration, firstK int, timeout time.Duration) []ServiceResponse {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	// One channel per service — CSP "alphabet" of events
	channels := make([]chan ServiceResponse, len(services))
	for i, latency := range services {
		channels[i] = make(chan ServiceResponse, 1)
		go func(id int, lat time.Duration, ch chan<- ServiceResponse) {
			resp, err := simulateService(ctx, id, lat)
			if err == nil {
				ch <- resp
			}
			close(ch)
		}(i, latency, channels[i])
	}

	// Select over all channels — guarded command pattern
	results := make([]ServiceResponse, 0, firstK)
	for len(results) < firstK {
		// Build a merged channel since Go select doesn't support dynamic cases
		merged := mergeChannels(channels)
		select {
		case resp, ok := <-merged:
			if !ok {
				return results // All channels closed
			}
			results = append(results, resp)
		case <-ctx.Done():
			return results // Timeout fired
		}
	}
	return results
}

// mergeChannels fans in multiple channels into one (CSP parallel composition).
func mergeChannels(channels []chan ServiceResponse) <-chan ServiceResponse {
	merged := make(chan ServiceResponse, len(channels))
	var wg sync.WaitGroup
	for _, ch := range channels {
		wg.Add(1)
		go func(c <-chan ServiceResponse) {
			defer wg.Done()
			for v := range c {
				merged <- v
			}
		}(ch)
	}
	go func() {
		wg.Wait()
		close(merged)
	}()
	return merged
}

func main() {
	services := []time.Duration{
		10 * time.Millisecond,
		50 * time.Millisecond,
		200 * time.Millisecond,
		500 * time.Millisecond,
		1000 * time.Millisecond,
	}
	firstK := 3
	timeout := 300 * time.Millisecond

	fmt.Printf("=== Scatter-Gather: %d services, want first %d within %v ===\n\n",
		len(services), firstK, timeout)

	// Approach 1: Naive
	fmt.Println("--- Approach 1: Naive (LEAKS goroutines) ---")
	results := ScatterGatherNaive(services, firstK)
	for _, r := range results {
		fmt.Printf("  service-%d: %s (%v)\n", r.ServiceID, r.Data, r.Latency)
	}
	fmt.Printf("  WARNING: %d goroutines leaked\n\n", len(services)-len(results))

	// Approach 2: Structured
	fmt.Println("--- Approach 2: errgroup + context (structured) ---")
	results = ScatterGatherStructured(services, firstK, timeout)
	for _, r := range results {
		fmt.Printf("  service-%d: %s (%v)\n", r.ServiceID, r.Data, r.Latency)
	}
	fmt.Println("  All goroutines cancelled on scope exit")
	fmt.Println()

	// Approach 3: CSP select
	fmt.Println("--- Approach 3: CSP-style select (guarded commands) ---")
	results = ScatterGatherCSP(services, firstK, timeout)
	for _, r := range results {
		fmt.Printf("  service-%d: %s (%v)\n", r.ServiceID, r.Data, r.Latency)
	}
	fmt.Println("  Context cancellation cleans up all goroutines")
}
