# Receipt Storage Service (Python WASM with SDK)

A simple expense tracking service for storing and querying receipts.

**Real-world use case**: Personal finance app, expense tracker, small business bookkeeping.

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, state, handler

@actor
class ReceiptStorageService:
    receipts: dict = state(default_factory=dict)
    
    @handler("store")
    def store_receipt(self, merchant: str, amount: float, date: str) -> dict:
        receipt_id = f"{merchant.lower()}-{date}-1"
        self.receipts[receipt_id] = {"merchant": merchant, "amount": amount}
        return {"status": "ok", "id": receipt_id}
```

**Before SDK**: 181 lines with manual WIT interface  
**After SDK**: 110 lines with decorators

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| store | `{"merchant":"Starbucks","amount":5.75,"date":"2024-01-15"}` | Store receipt |
| get | `{"id":"starbucks-2024-01-15-1"}` | Get by ID |
| list | `{"merchant":"Starbucks"}` | List (optional filter) |
| delete | `{"id":"starbucks-2024-01-15-1"}` | Delete receipt |
| summary | `{}` | Spending summary |

## Example Output

```json
{
  "status": "ok",
  "grand_total": 142.57,
  "by_merchant": {
    "Whole Foods": {"total": 87.32, "count": 1},
    "Starbucks": {"total": 10.25, "count": 2}
  }
}
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `ReceiptStorageService` as actor |
| `state()` | Defines `receipts` as persistent dict |
| `@handler()` | Routes store, get, list, summary |

## Files

| File | Description |
|------|-------------|
| `receipt_actor.py` | Receipt storage using SDK |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Feature Flags Example](../feature_flags/) - Similar CRUD pattern
