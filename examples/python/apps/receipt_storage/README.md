# Receipt Storage Service (Python WASM)

A simple expense tracking service demonstrating blob storage patterns. Store receipts from purchases, list them by merchant, and get spending summaries.

**Real-world use case**: Personal finance app, expense tracker, small business bookkeeping.

## Quick Start

```bash
# Build
./build.sh

# Test (requires running PlexSpaces node)
./test.sh
```

## What It Demonstrates

1. **Storing Structured Data** - Receipts with merchant, amount, date, description
2. **Filtering/Querying** - List receipts, filter by merchant
3. **Aggregations** - Spending summary grouped by merchant
4. **Simple Actor Pattern** - Single actor handling CRUD + queries

## Operations

### Store a Receipt
```json
{
  "op": "store",
  "merchant": "Starbucks",
  "amount": 5.75,
  "date": "2024-01-15",
  "description": "Grande latte"
}
```

### List Receipts
```json
{"op": "list"}
{"op": "list", "merchant": "Starbucks"}  // Filter by merchant
```

### Get Spending Summary
```json
{"op": "summary"}
```
Returns:
```json
{
  "grand_total": 142.57,
  "by_merchant": {
    "Whole Foods": {"total": 87.32, "count": 1},
    "Starbucks": {"total": 10.25, "count": 2},
    "Shell": {"total": 45.00, "count": 1}
  }
}
```

### Get/Delete Receipt
```json
{"op": "get", "id": "starbucks-2024-01-15-1"}
{"op": "delete", "id": "starbucks-2024-01-15-1"}
```

## Example Output

```
=== Receipt Storage Service Test ===
Testing expense tracking with blob storage pattern

1. Deploying receipt storage actor...
2. Storing receipts from a shopping trip...
   - Morning coffee at Starbucks...
   - Weekly groceries at Whole Foods...
   - Fill up at Shell...
   - Afternoon coffee...

5. Get spending summary...
{
    "status": "ok",
    "grand_total": 142.57,
    "by_merchant": {
        "Whole Foods": {"total": 87.32, "count": 1},
        "Shell": {"total": 45.0, "count": 1},
        "Starbucks": {"total": 10.25, "count": 2}
    }
}
```

## Files

| File | Description |
|------|-------------|
| `receipt_actor.py` | Receipt storage implementation |
| `build.sh` | Build with componentize-py |
| `test.sh` | Integration test with sample receipts |

## See Also

- [Python WASM Guide](../../README.md)
- [KeyValue Example](../keyvalue/) - Similar storage pattern
