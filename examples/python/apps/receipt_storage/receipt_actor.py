"""
Receipt Storage Service - Python WASM Actor

A simple receipt/expense tracking service demonstrating blob storage patterns.
Store receipts from purchases, list them by merchant, and retrieve for expense reports.

Real-world use case: Personal finance app, expense tracker, small business bookkeeping.
"""

import json
from datetime import datetime
from wit_world import exports

# In-memory receipt storage: {receipt_id: {merchant, amount, date, data, ...}}
_receipts = {}


class Actor(exports.Actor):
    """Receipt storage actor implementing simple-actor interface."""
    
    def init(self, config_json: str) -> str:
        """Initialize the receipt storage service."""
        return ""  # Success
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle receipt storage operations.
        
        Operations:
        - store: Save a new receipt
        - get: Retrieve a receipt by ID
        - list: List receipts (optionally filter by merchant)
        - delete: Remove a receipt
        - summary: Get spending summary by merchant
        """
        global _receipts
        
        try:
            payload = json.loads(payload_json) if payload_json else {}
            op = payload.get("op", msg_type)
            
            if op == "store":
                return self._store_receipt(payload)
            elif op == "get":
                return self._get_receipt(payload.get("id"))
            elif op == "list":
                return self._list_receipts(payload.get("merchant"))
            elif op == "delete":
                return self._delete_receipt(payload.get("id"))
            elif op == "summary":
                return self._spending_summary()
            else:
                return json.dumps({"error": f"Unknown operation: {op}"})
                
        except Exception as e:
            return f"ERROR: {str(e)}"
    
    def _store_receipt(self, payload: dict) -> str:
        """Store a new receipt."""
        global _receipts
        
        merchant = payload.get("merchant", "Unknown")
        amount = payload.get("amount", 0.0)
        date = payload.get("date", "2024-01-01")
        description = payload.get("description", "")
        image_data = payload.get("image")  # Optional base64 image
        
        # Generate receipt ID: merchant-date-counter
        count = sum(1 for r in _receipts.values() if r["merchant"] == merchant) + 1
        receipt_id = f"{merchant.lower().replace(' ', '-')}-{date}-{count}"
        
        _receipts[receipt_id] = {
            "id": receipt_id,
            "merchant": merchant,
            "amount": float(amount),
            "date": date,
            "description": description,
            "image": image_data,
        }
        
        return json.dumps({
            "status": "ok",
            "id": receipt_id,
            "merchant": merchant,
            "amount": amount
        })
    
    def _get_receipt(self, receipt_id: str) -> str:
        """Retrieve a receipt by ID."""
        global _receipts
        
        if not receipt_id:
            return json.dumps({"error": "Missing receipt ID"})
        
        receipt = _receipts.get(receipt_id)
        if not receipt:
            return json.dumps({"error": f"Receipt not found: {receipt_id}"})
        
        return json.dumps({"status": "ok", "receipt": receipt})
    
    def _list_receipts(self, merchant_filter: str = None) -> str:
        """List all receipts, optionally filtered by merchant."""
        global _receipts
        results = []
        
        for receipt in _receipts.values():
            if merchant_filter and merchant_filter.lower() not in receipt["merchant"].lower():
                continue
            # Return summary without image data
            results.append({
                "id": receipt["id"],
                "merchant": receipt["merchant"],
                "amount": receipt["amount"],
                "date": receipt["date"],
                "description": receipt["description"]
            })
        
        # Sort by date descending
        results.sort(key=lambda r: r["date"], reverse=True)
        
        return json.dumps({
            "status": "ok",
            "count": len(results),
            "receipts": results
        })
    
    def _delete_receipt(self, receipt_id: str) -> str:
        """Delete a receipt."""
        global _receipts
        
        if not receipt_id:
            return json.dumps({"error": "Missing receipt ID"})
        
        if receipt_id not in _receipts:
            return json.dumps({"error": f"Receipt not found: {receipt_id}"})
        
        del _receipts[receipt_id]
        return json.dumps({"status": "ok", "deleted": receipt_id})
    
    def _spending_summary(self) -> str:
        """Get spending summary grouped by merchant."""
        global _receipts
        summary = {}
        
        for receipt in _receipts.values():
            merchant = receipt["merchant"]
            if merchant not in summary:
                summary[merchant] = {"total": 0.0, "count": 0}
            summary[merchant]["total"] += receipt["amount"]
            summary[merchant]["count"] += 1
        
        # Sort by total spending descending
        sorted_summary = dict(sorted(
            summary.items(), 
            key=lambda x: x[1]["total"], 
            reverse=True
        ))
        
        grand_total = sum(m["total"] for m in summary.values())
        
        return json.dumps({
            "status": "ok",
            "grand_total": round(grand_total, 2),
            "by_merchant": sorted_summary
        })
    
    def get_state(self) -> str:
        """Return current state for durability."""
        global _receipts
        return json.dumps({"receipts": _receipts})
    
    def set_state(self, state_json: str) -> str:
        """Restore state from snapshot."""
        global _receipts
        try:
            state = json.loads(state_json)
            _receipts = state.get("receipts", {})
            return ""  # Success
        except Exception as e:
            return f"ERROR: {str(e)}"
