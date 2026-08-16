// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Go CSP Gotchas — Test Suite
//
// Demonstrates common pitfalls with goroutines and channels.

package main

import (
	"context"
	"runtime"
	"sync"
	"testing"
	"time"

	"golang.org/x/sync/errgroup"
)

// ---------------------------------------------------------------------------
// Gotcha 1: Goroutine leak — no cancellation path
// ---------------------------------------------------------------------------

func TestGotcha_GoroutineLeak(t *testing.T) {
	before := runtime.NumGoroutine()

	// Spawn goroutines that block forever on a channel nobody reads
	ch := make(chan int)
	for i := 0; i < 10; i++ {
		go func(id int) {
			ch <- id // blocks forever — no reader
		}(i)
	}

	time.Sleep(50 * time.Millisecond)
	after := runtime.NumGoroutine()

	leaked := after - before
	if leaked < 10 {
		t.Fatalf("expected at least 10 leaked goroutines, got %d", leaked)
	}
	t.Logf("GOTCHA: %d goroutines leaked (blocked on channel send)", leaked)
}

// ---------------------------------------------------------------------------
// Gotcha 2: Nil channel blocks forever
// ---------------------------------------------------------------------------

func TestGotcha_NilChannelRecvBlocks(t *testing.T) {
	var ch chan int // nil channel

	done := make(chan bool)
	go func() {
		select {
		case <-ch: // blocks forever — recv on nil channel
			done <- true
		case <-time.After(100 * time.Millisecond):
			done <- false
		}
	}()

	result := <-done
	if result {
		t.Fatal("nil channel recv should block forever")
	}
	t.Log("GOTCHA: recv on nil channel blocks forever — no panic, just silent hang")
}

func TestGotcha_NilChannelSendBlocks(t *testing.T) {
	var ch chan int // nil channel

	done := make(chan bool)
	go func() {
		select {
		case ch <- 42: // blocks forever — send on nil channel
			done <- true
		case <-time.After(100 * time.Millisecond):
			done <- false
		}
	}()

	result := <-done
	if result {
		t.Fatal("nil channel send should block forever")
	}
	t.Log("GOTCHA: send on nil channel blocks forever — no panic, just silent hang")
}

// ---------------------------------------------------------------------------
// Gotcha 3b: Receive from closed channel returns zero (no panic)
// ---------------------------------------------------------------------------

func TestGotcha_RecvFromClosedReturnsZero(t *testing.T) {
	ch := make(chan int, 1)
	ch <- 99
	close(ch)

	// First recv gets the buffered value
	v1 := <-ch
	if v1 != 99 {
		t.Fatalf("expected 99, got %d", v1)
	}

	// Second recv gets zero value + ok=false (no panic, unlike send)
	v2, ok := <-ch
	if ok {
		t.Fatal("expected ok=false for recv on closed channel")
	}
	if v2 != 0 {
		t.Fatalf("expected zero value, got %d", v2)
	}
	t.Log("GOTCHA: recv from closed channel returns zero+false (asymmetric with send which PANICS)")
}

// ---------------------------------------------------------------------------
// Gotcha 3: Send on closed channel panics
// ---------------------------------------------------------------------------

func TestGotcha_SendOnClosedPanics(t *testing.T) {
	ch := make(chan int, 1)
	close(ch)

	defer func() {
		if r := recover(); r != nil {
			t.Logf("GOTCHA: send on closed channel panics: %v", r)
		} else {
			t.Fatal("expected panic on send to closed channel")
		}
	}()

	ch <- 42 // PANIC: send on closed channel
}

// ---------------------------------------------------------------------------
// Gotcha 4: Buffered vs unbuffered semantics
// ---------------------------------------------------------------------------

func TestGotcha_BufferedVsUnbuffered(t *testing.T) {
	// Unbuffered: sender blocks until receiver ready (rendezvous)
	unbuffered := make(chan int)
	sent := false
	go func() {
		unbuffered <- 1
		sent = true // only happens after receiver takes the value
	}()

	time.Sleep(10 * time.Millisecond)
	if sent {
		t.Fatal("unbuffered send should block until receiver is ready")
	}
	<-unbuffered // release the sender
	time.Sleep(10 * time.Millisecond)
	if !sent {
		t.Fatal("sender should have completed after receiver took value")
	}

	// Buffered: sender doesn't block if buffer has space
	buffered := make(chan int, 5)
	buffered <- 1 // doesn't block — buffer absorbs it
	buffered <- 2
	t.Logf("GOTCHA: buffered channels break rendezvous — sender doesn't wait for receiver")
}

// ---------------------------------------------------------------------------
// Gotcha 5: Select non-determinism
// ---------------------------------------------------------------------------

func TestGotcha_SelectNonDeterminism(t *testing.T) {
	counts := map[string]int{"from-ch1": 0, "from-ch2": 0}
	for range 100 {
		ch1 := make(chan string, 1)
		ch2 := make(chan string, 1)
		ch1 <- "from-ch1"
		ch2 <- "from-ch2"
		select {
		case v := <-ch1:
			counts[v]++
		case v := <-ch2:
			counts[v]++
		}
	}

	// Go's select is pseudo-random when multiple cases are ready
	t.Logf("GOTCHA: select is non-deterministic: ch1=%d, ch2=%d (unlike CSP external choice)",
		counts["from-ch1"], counts["from-ch2"])
	if counts["from-ch1"] == 0 || counts["from-ch2"] == 0 {
		t.Fatal("expected both channels to be selected at least once")
	}
}

// ---------------------------------------------------------------------------
// Fix: Proper structured concurrency with context + errgroup
// ---------------------------------------------------------------------------

func TestFix_StructuredConcurrencyWithErrgroup(t *testing.T) {
	before := runtime.NumGoroutine()

	services := []time.Duration{
		10 * time.Millisecond,
		50 * time.Millisecond,
		200 * time.Millisecond,
		500 * time.Millisecond,
	}
	timeout := 100 * time.Millisecond

	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	var mu sync.Mutex
	var results []ServiceResponse

	g, ctx := errgroup.WithContext(ctx)
	for i, lat := range services {
		g.Go(func() error {
			resp, err := simulateService(ctx, i, lat)
			if err != nil {
				return nil
			}
			mu.Lock()
			results = append(results, resp)
			mu.Unlock()
			return nil
		})
	}

	_ = g.Wait()
	// Structured guarantee: all goroutines done by this point

	time.Sleep(50 * time.Millisecond) // let runtime clean up
	after := runtime.NumGoroutine()

	t.Logf("FIX: got %d results, goroutines before=%d after=%d (no leak)",
		len(results), before, after)

	if len(results) < 2 {
		t.Fatalf("expected at least 2 fast results, got %d", len(results))
	}
}
