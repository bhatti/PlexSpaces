"""
Receipt Storage Service - Python WASM Actor (with SDK)

A simple receipt/expense tracking service demonstrating storage patterns.
Real-world use case: Personal finance app, expense tracker, bookkeeping.

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent receipts storage
- @handler(): Routes storage operations
"""

from plexspaces import actor, state, handler, init_handler


@actor
class ReceiptStorageService:
    """Receipt storage actor for expense tracking."""
    
    # Receipts storage: {receipt_id: {merchant, amount, date, ...}}
    receipts: dict = state(default_factory=dict)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize receipt storage."""
        self.receipts = config.get("receipts", {})
    
    @handler("store")
    def store_receipt(self, merchant: str = "Unknown", amount: float = 0.0,
                     date: str = "2024-01-01", description: str = "",
                     image: str = None) -> dict:
        """Store a new receipt."""
        # Generate receipt ID
        count = sum(1 for r in self.receipts.values() if r["merchant"] == merchant) + 1
        receipt_id = f"{merchant.lower().replace(' ', '-')}-{date}-{count}"
        
        self.receipts[receipt_id] = {
            "id": receipt_id,
            "merchant": merchant,
            "amount": float(amount),
            "date": date,
            "description": description,
            "image": image
        }
        
        return {"status": "ok", "id": receipt_id, "merchant": merchant, "amount": amount}
    
    @handler("get")
    def get_receipt(self, id: str = "") -> dict:
        """Retrieve a receipt by ID."""
        if not id:
            return {"error": "Missing receipt ID"}
        
        receipt = self.receipts.get(id)
        if not receipt:
            return {"error": f"Receipt not found: {id}"}
        
        return {"status": "ok", "receipt": receipt}
    
    @handler("list")
    def list_receipts(self, merchant: str = None) -> dict:
        """List all receipts, optionally filtered by merchant."""
        results = []
        
        for receipt in self.receipts.values():
            if merchant and merchant.lower() not in receipt["merchant"].lower():
                continue
            results.append({
                "id": receipt["id"],
                "merchant": receipt["merchant"],
                "amount": receipt["amount"],
                "date": receipt["date"],
                "description": receipt["description"]
            })
        
        results.sort(key=lambda r: r["date"], reverse=True)
        return {"status": "ok", "count": len(results), "receipts": results}
    
    @handler("delete")
    def delete_receipt(self, id: str = "") -> dict:
        """Delete a receipt."""
        if not id:
            return {"error": "Missing receipt ID"}
        
        if id not in self.receipts:
            return {"error": f"Receipt not found: {id}"}
        
        del self.receipts[id]
        return {"status": "ok", "deleted": id}
    
    @handler("summary")
    def spending_summary(self) -> dict:
        """Get spending summary grouped by merchant."""
        summary = {}
        
        for receipt in self.receipts.values():
            merchant = receipt["merchant"]
            if merchant not in summary:
                summary[merchant] = {"total": 0.0, "count": 0}
            summary[merchant]["total"] += receipt["amount"]
            summary[merchant]["count"] += 1
        
        sorted_summary = dict(sorted(
            summary.items(),
            key=lambda x: x[1]["total"],
            reverse=True
        ))
        
        grand_total = sum(m["total"] for m in summary.values())
        return {"status": "ok", "grand_total": round(grand_total, 2), "by_merchant": sorted_summary}
