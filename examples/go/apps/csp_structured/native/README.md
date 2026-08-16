# CSP Structured Concurrency — Native Go

Standalone Go program demonstrating CSP gotchas and proper structured concurrency fixes
for the scatter-gather pattern.

## Gotchas Demonstrated

| # | Gotcha | What Happens | Fix |
|---|--------|-------------|-----|
| 1 | Goroutine leak | Blocked goroutines never terminate | `context.WithCancel` |
| 2 | Nil channel | Recv/send block forever | Always initialize channels |
| 3 | Send on closed | Runtime panic | Only close from sender side |
| 4 | Buffered vs unbuffered | Breaks rendezvous semantics | Choose deliberately |
| 5 | Select non-determinism | Random case chosen when both ready | Unlike CSP external choice |

## Running

```bash
# Run the demo
go run .

# Run gotcha tests
go test -v ./...
```

## Key Patterns

**Before (broken):**
```go
// Goroutines leak when caller returns early
for _, url := range urls {
    go func() { ch <- fetch(url) }()
}
return <-ch  // N-1 goroutines still running
```

**After (structured):**
```go
ctx, cancel := context.WithTimeout(ctx, timeout)
defer cancel()
g, ctx := errgroup.WithContext(ctx)
for _, url := range urls {
    g.Go(func() error {
        resp, err := fetchWithContext(ctx, url)
        // ...
    })
}
g.Wait()  // All goroutines done here — structured guarantee
```

## References

- [Architecture](../../../../../docs/architecture.md)
- [Getting Started](../../../../../docs/getting-started.md)
