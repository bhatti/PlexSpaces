# AWS Durable Lambda / Idempotent Webhooks Reference

AWS Lambda with DynamoDB (or similar) is often used for exactly-once webhook processing: store idempotency key and response; on duplicate key return stored response.

## Concepts

- **Idempotency key**: Client-provided key (e.g. Stripe `Idempotency-Key`, or event ID). Same key → same result.
- **Durable store**: DynamoDB table keyed by idempotency key; value = response (or status). Conditional writes to detect first vs duplicate.
- **Webhook delivery**: At-least-once; duplicates are common. Idempotency gives exactly-once semantics.

## Native-style (Lambda + DynamoDB)

```go
// Conceptual: Lambda handler with DDB idempotency
func handler(ctx context.Context, event WebhookEvent) (Response, error) {
    key := event.IdempotencyKey
    if key == "" { key = event.EventID }
    if existing, _ := ddb.GetItem(key); existing != nil {
        return existing.Response, nil  // dedup hit
    }
    resp := process(event)
    ddb.PutItem(key, resp)
    return resp, nil
}
```

## PlexSpaces mapping

| AWS (Lambda + DDB) | PlexSpaces |
|--------------------|------------|
| Lambda handler | GenServer actor; Handle("webhook", payload) |
| DDB idempotency table | In-actor map (idempotency_key → response); GetState/SetState + durability facet |
| Duplicate key → return cached | Lookup Processed[key]; if present return cached JSON |
| Scale | Per partition | Virtual actor instance (e.g. webhook:default); scale with more instances |

This example uses a single virtual actor with a durable map; for very high key cardinality, partition by key prefix and route to multiple actor instances.
