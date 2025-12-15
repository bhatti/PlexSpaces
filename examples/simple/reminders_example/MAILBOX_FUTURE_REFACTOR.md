# Mailbox Future-Based Refactor Proposal

## Current Issue

The mailbox's `dequeue()` method returns `Option<Message>` immediately, not a future that yields values. This causes:
1. **Busy-waiting**: Actor loop must poll with sleep to avoid CPU spinning
2. **Latency**: 10ms sleep adds unnecessary delay when messages are available
3. **Inefficiency**: Not truly event-driven, wastes CPU cycles

## Proposed Solution

Refactor mailbox to use `tokio::sync::mpsc::Receiver` internally, allowing `dequeue()` to return a future that can be awaited in `tokio::select!`.

## Architecture Options

### Option 1: Channel-Based Mailbox (Recommended)

**Pros:**
- Truly event-driven (no polling)
- Zero latency when messages available
- Works perfectly with `tokio::select!`
- Follows Rust async best practices
- Efficient (no CPU waste)

**Cons:**
- Requires refactoring mailbox internals
- Need to handle ordering/priority differently
- Selective receive needs separate mechanism

**Implementation:**
```rust
pub struct Mailbox {
    config: MailboxConfig,
    sender: mpsc::UnboundedSender<Message>,  // For enqueue
    receiver: mpsc::UnboundedReceiver<Message>, // For dequeue
    // Priority queue for ordering (feeds into channel)
    priority_queue: Arc<RwLock<BinaryHeap<PriorityMessage>>>,
    stats: RwLock<MailboxStats>,
}

impl Mailbox {
    pub fn new(config: MailboxConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        // ... initialize priority queue based on ordering strategy
    }
    
    pub async fn enqueue(&self, message: Message) -> Result<(), MailboxError> {
        // Apply ordering/priority logic
        // Send to channel
        self.sender.send(message).map_err(|_| MailboxError::Full)
    }
    
    /// Returns a future that yields messages (can be used in tokio::select!)
    pub fn dequeue(&self) -> impl Future<Output = Option<Message>> {
        // Return receiver.recv() future directly
        self.receiver.recv()
    }
}
```

**Actor Loop:**
```rust
loop {
    tokio::select! {
        Some(message) = mailbox.dequeue() => {
            // Process message
        }
        _ = shutdown_rx.recv() => {
            break;
        }
    }
}
```

### Option 2: Hybrid Approach (Queue + Channel Notification)

**Pros:**
- Maintains current ordering/priority logic
- Minimal changes to existing code
- Still event-driven

**Cons:**
- More complex (two mechanisms)
- Still need to handle queue + channel sync

**Implementation:**
```rust
pub struct Mailbox {
    queue: Arc<RwLock<VecDeque<Message>>>,
    notify_tx: mpsc::UnboundedSender<()>,  // Notification channel
    notify_rx: mpsc::UnboundedReceiver<()>,
    // ... rest
}

impl Mailbox {
    pub async fn enqueue(&self, message: Message) -> Result<(), MailboxError> {
        // Add to queue (existing logic)
        // Notify via channel
        let _ = self.notify_tx.send(());
    }
    
    pub async fn dequeue(&self) -> Option<Message> {
        loop {
            // Check queue first
            if let Some(msg) = self.queue.write().await.pop_front() {
                return Some(msg);
            }
            // Wait for notification
            self.notify_rx.recv().await;
        }
    }
}
```

### Option 3: Keep Current + Add `dequeue_future()` Method

**Pros:**
- Backward compatible
- Minimal changes
- Can migrate gradually

**Cons:**
- Still has polling overhead
- Two APIs to maintain

## Recommendation

**Option 1 (Channel-Based)** is the best long-term solution because:
1. **Performance**: Zero latency, truly event-driven
2. **Correctness**: Works properly with `tokio::select!`
3. **Simplicity**: Single mechanism (channel) instead of queue + sleep
4. **Industry Standard**: Matches how other Rust actor frameworks work

## Migration Path

1. **Phase 1**: Implement channel-based mailbox alongside current one
2. **Phase 2**: Update actor loop to use new API
3. **Phase 3**: Remove old polling-based implementation
4. **Phase 4**: Update all examples and tests

## Ordering/Priority Handling

For channel-based mailbox, we can:
- Use a priority queue that feeds into the channel
- Or use multiple channels (one per priority level)
- Or use a single channel with priority wrapper type

## Selective Receive

For `dequeue_matching()`, we can:
- Use a separate mechanism (like a filter on the receiver)
- Or maintain a separate queue for selective receives
- Or use `tokio::select!` with multiple receivers

## Impact Assessment

**Breaking Changes:**
- `dequeue()` signature changes (returns `impl Future` instead of `Option`)
- Need to update all call sites

**Performance:**
- ✅ Eliminates 10ms sleep latency
- ✅ Zero CPU waste from polling
- ✅ Better scalability

**Complexity:**
- ⚠️ More complex internal implementation
- ⚠️ Need to handle ordering/priority differently
- ✅ Simpler actor loop (no manual sleep)

## Conclusion

**Yes, we should refactor the mailbox to return a future.** The current workaround is acceptable short-term, but a channel-based mailbox is the proper long-term solution that aligns with Rust async best practices and eliminates the busy-waiting issue entirely.

