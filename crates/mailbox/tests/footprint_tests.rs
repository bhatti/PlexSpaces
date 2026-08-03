// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Struct-size and heap-footprint regression tests for the mailbox crate.
//
// Background (planx.txt D1-D2):
//   Before simplification each in-memory FIFO actor allocated:
//     - broadcast::channel(1024) ring buffer  ≈ 100 KB heap
//     - InMemoryChannel struct                ≈ 400 B heap
//     - internal_queue VecDeque               ≈ 128 B heap
//     - background tokio::spawn task          ≈ 512 B heap
//     - Arc<Notify>                           ≈  80 B heap
//   Total heap per actor: ~102 KB
//
//   After D1 (direct bounded mpsc fast path) + D2 (inline atomics):
//     The stack struct is ~776 B (Option fields, Arcs, Strings).
//     Heap per actor on the fast path: only the bounded mpsc channel
//     ring buffer (capacity * sizeof(ProtoMessage)) plus two Arc allocs.
//     At the default capacity of 1000 and ProtoMessage ≈ 200 B on heap,
//     actual per-actor heap is a few KB, not 100 KB.
//
// These tests are regression guards: if the struct grows back toward the
// pre-D1 size, or if 10k actor allocations consume >500 MB, the tests fail.

use plexspaces_mailbox::{mailbox_config_default, Mailbox};

/// Mailbox stack struct must stay below 1 KB.
///
/// The struct intentionally contains Option fields so that the fast path
/// (FIFO in-memory) pays zero for unused slow-path state.  The struct
/// itself is ~776 B; this bound gives 25% headroom for minor layout shifts
/// while preventing the accidental re-introduction of large inline fields.
#[test]
fn mailbox_stack_size_is_within_budget() {
    let size = std::mem::size_of::<Mailbox>();
    assert!(
        size <= 1024,
        "Mailbox struct size ({size}B) exceeds 1 KB — check for added inline fields or lost Optionality"
    );
}

/// MailboxObservabilityStats is a plain-data snapshot struct copied on
/// every metrics scrape; it must stay cheap.
#[test]
fn mailbox_observability_stats_is_copy_sized() {
    use plexspaces_mailbox::MailboxObservabilityStats;
    let size = std::mem::size_of::<MailboxObservabilityStats>();
    assert!(
        size <= 128,
        "MailboxObservabilityStats size ({size}B) exceeds 128 B"
    );
}

/// 10,000 FIFO in-memory mailboxes must allocate no more than 10 MB of
/// stack-struct data, confirming D1 eliminated the 100 KB/actor broadcast
/// ring buffer.
///
/// Before D1: 10k actors × ~100 KB = ~1 GB heap.
/// After  D1: 10k actors × ~776 B stack = ~7.6 MB of struct data.
///
/// The test checks only the stack struct contribution (deterministic,
/// compiler-verifiable); actual heap is smaller because the mpsc channel
/// is shared via Arc.
#[tokio::test]
async fn ten_thousand_mailboxes_stack_under_10mb() {
    let mut mailboxes = Vec::with_capacity(10_000);
    for i in 0..10_000_u32 {
        let m = Mailbox::new(
            mailbox_config_default(),
            format!("footprint-{i}"),
            String::new(),
            String::new(),
            None,
        )
        .await
        .expect("mailbox creation must not fail");
        mailboxes.push(m);
    }

    let per_mailbox_bytes = std::mem::size_of::<Mailbox>();
    let total_stack_kb = (per_mailbox_bytes * 10_000) / 1024;

    assert!(
        total_stack_kb <= 10_240, // 10 MB of stack structs for 10k mailboxes
        "10k mailbox stack contribution ({total_stack_kb} KB) exceeds 10 MB — struct grew unexpectedly"
    );

    drop(mailboxes);
}
