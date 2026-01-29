# Examples Audit - Required Fixes

## Status Summary

| Example | Current Status | Required Fix | Priority |
|---------|---------------|--------------|----------|
| **timers** | ❌ Uses tokio::spawn | Rewrite with TimerFacet | HIGH |
| **reminders** | ❌ Uses custom ReminderStore | Rewrite with ReminderFacet | HIGH |
| supervision_tree | ⚠️ Manual Actor::new | Use node.spawn() | MEDIUM |
| heat_diffusion | ⚠️ Plain structs | Use actual actors + TupleSpace | MEDIUM |
| mpi_collectives | ⚠️ Simulated | Use actual ProcessGroupRegistry | MEDIUM |
| actor_groups_sharding | ✅ ActorBuilder, tell() | Minor cleanup | LOW |
| durable_actor | ✅ DurabilityFacet | None | - |
| chat_room | ✅ ProcessGroupRegistry | None | - |
| feature_flags | ✅ ProcessGroupRegistry | None | - |
| webhook_handler | ✅ ActorBuilder, tell() | None | - |
| matrix_multiply | ✅ ActorBuilder, tell() | None | - |
| byzantine | ⚠️ Needs testing | Build and test | MEDIUM |

---

## HIGH Priority Fixes

### 1. timers - MUST use TimerFacet

**Current (WRONG):**
```rust
// Uses raw tokio - NOT PlexSpaces!
tokio::spawn(async move {
    loop {
        sleep(Duration::from_secs(1)).await;
        // check idle...
    }
});
```

**Required (CORRECT):**
```rust
// Use TimerFacet
let timer_facet = TimerFacet::new(json!({}), 50);
actor.attach_facet(Box::new(timer_facet)).await?;

// Register timers
facet.register_once("idle_timeout", Duration::from_secs(30)).await?;
facet.register_periodic("heartbeat", Duration::from_secs(5)).await?;
```

### 2. reminders - MUST use ReminderFacet

**Current (WRONG):**
```rust
// Custom ReminderStore - NOT PlexSpaces!
struct ReminderStore {
    reminders: HashMap<String, Reminder>,
}
impl ReminderStore {
    fn schedule(&mut self, name: &str, user_id: &str, delay: Duration) { ... }
}
```

**Required (CORRECT):**
```rust
// Use ReminderFacet with JournalStorage
let storage = Arc::new(MemoryJournalStorage::new());
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);
actor.attach_facet(Box::new(reminder_facet)).await?;

// Register reminders (durable!)
facet.register_reminder(ReminderRegistration {
    reminder_name: "billing".to_string(),
    interval: Some(Duration::from_days(30)),
    first_fire_time: Some(now + Duration::from_days(30)),
    persist_across_activations: true,
    ..Default::default()
}).await?;
```

---

## MEDIUM Priority Fixes

### 3. supervision_tree - Use node.spawn()

**Current:**
```rust
// Manual creation
let mailbox = Mailbox::new(...).await?;
let actor = Actor::new(actor_id, behavior, mailbox, tenant, namespace, None);
```

**Required:**
```rust
// Use node.spawn() or ActorBuilder
let actor = node.spawn(&ctx, &actor_id, "WorkerType", vec![], None, HashMap::new(), vec![]).await?;
```

### 4. heat_diffusion - Use actual actors

**Current:**
```rust
// Plain struct, not actor
struct GridRegion { id: usize, data: Vec<f64>, width: usize }
```

**Required:**
```rust
// Actual actor with TupleSpace coordination
let region_actor = ActorBuilder::new(Box::new(RegionActor::new(id)))
    .spawn(&ctx, service_locator).await?;

// Use TupleSpace for ghost cell exchange
tuplespace.write(&ctx, ("boundary", iter, region_id, data)).await?;
tuplespace.read(&ctx, ("boundary", iter, neighbor_id)).await?;
```

### 5. byzantine - Test and verify

- Build: `cargo build`
- Run: `cargo run`
- Verify output

---

## Files to Delete

Remove unnecessary complexity:
- `examples/rust/embedded/*/tests/` (unless critical)
- `examples/rust/embedded/*/scripts/` (unless needed for run)
- `examples/rust/embedded/*/config/` (use release.toml instead)
- `examples/rust/embedded/*/docker*` (not needed)

---

## Action Plan

1. **Fix timers** - Rewrite with TimerFacet
2. **Fix reminders** - Rewrite with ReminderFacet  
3. **Fix supervision_tree** - Use node.spawn()
4. **Test all** - Build and run each
5. **Update READMEs** - Ensure all have proper docs
6. **Clean up** - Remove unnecessary files

---

## Verification Command

```bash
# Test each example
for dir in actor_groups_sharding supervision_tree durable_actor timers reminders chat_room feature_flags webhook_handler heat_diffusion matrix_multiply mpi_collectives byzantine; do
    echo "=== $dir ==="
    cd examples/rust/embedded/$dir
    cargo build
    cd -
done
```
