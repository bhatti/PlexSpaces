# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Payment Handler Actor - GenServer Microservice Pattern

Demonstrates a GenServer-style microservice for payment processing:
- Request-reply pattern (synchronous API calls)
- Durable state (transaction log)
- Integration with KV store for idempotency
- Distributed lock for critical sections

Real-world use cases:
- Payment gateway integration
- Order payment processing
- Subscription billing
- Refund handling

## APIs Used

- @gen_server_actor: Request-reply pattern
- host.kv_get/kv_put: Idempotency checks
- host.lock_acquire/lock_release: Critical section protection
- state(): Transaction persistence

## Worker Pool Pattern

In production, deploy multiple instances behind ElasticPool:
1. Pool manages worker lifecycle (spawn/shutdown)
2. Load balancer distributes requests
3. Each worker handles one request at a time
4. Failed workers are restarted automatically
"""

import json
from plexspaces import gen_server_actor, state, handler, init_handler, host


@gen_server_actor(facets=["durability"])  # GenServer with durability enabled
class PaymentHandler:
    """GenServer-style payment processor for microservice deployments."""
    
    # Persistent state
    transaction_count: int = state(default=0)
    total_processed: int = state(default=0)  # In cents
    transactions: list = state(default_factory=list)
    
    @init_handler
    def on_init(self, config: dict) -> None:
        """Initialize payment handler with config."""
        self.transaction_count = 0
        self.total_processed = 0
        self.transactions = []
        merchant_id = config.get("merchant_id", "default")
        host.log("info", f"PaymentHandler initialized for merchant: {merchant_id}")
    
    @handler("process_payment")  # GenServer handlers use the call pattern automatically
    def process_payment(
        self,
        payment_id: str = "",
        amount: int = 0,  # Amount in cents
        currency: str = "USD",
        method: str = "card",
        customer_id: str = "",
        metadata: dict = None
    ) -> dict:
        """
        Process a payment (GenServer call - request-reply).
        
        Args:
            payment_id: Unique payment identifier (for idempotency)
            amount: Amount in cents (integer to avoid float issues)
            currency: Currency code (USD, EUR, etc.)
            method: Payment method (card, bank, wallet)
            customer_id: Customer identifier
            metadata: Additional payment metadata
        
        Returns:
            Payment result with transaction ID
        """
        if not payment_id:
            return {"error": "payment_id required", "status": "failed"}
        if amount <= 0:
            return {"error": "invalid amount", "status": "failed"}
        
        # Idempotency check using KV store
        existing = host.kv_get(f"payment:{payment_id}")
        if existing and not existing.startswith("ERROR"):
            try:
                result = json.loads(existing)
                result["idempotent"] = True
                return result
            except json.JSONDecodeError:
                pass
        
        # Process payment
        self.transaction_count += 1
        tx_id = f"tx-{self.transaction_count}"
        
        # Record transaction
        transaction = {
            "tx_id": tx_id,
            "payment_id": payment_id,
            "amount": amount,
            "currency": currency,
            "method": method,
            "customer_id": customer_id,
            "status": "completed",
            "timestamp": host.now_ms()
        }
        self.transactions = list(self.transactions) + [transaction]
        self.total_processed += amount
        
        # Store result for idempotency
        result = {
            "status": "completed",
            "tx_id": tx_id,
            "payment_id": payment_id,
            "amount": amount,
            "currency": currency
        }
        host.kv_put(f"payment:{payment_id}", json.dumps(result))
        
        host.log("info", f"Payment processed: {tx_id} amount={amount} {currency}")
        return result
    
    @handler("refund")  # GenServer handlers use the call pattern automatically
    def process_refund(
        self,
        refund_id: str = "",
        original_tx_id: str = "",
        amount: int = 0,  # Partial refund amount (0 = full refund)
        reason: str = ""
    ) -> dict:
        """
        Process a refund (GenServer call - request-reply).
        
        Uses distributed lock to prevent race conditions on concurrent refunds.
        
        Args:
            refund_id: Unique refund identifier
            original_tx_id: Original transaction ID
            amount: Refund amount (0 = full refund of original)
            reason: Refund reason
        
        Returns:
            Refund result
        """
        if not refund_id or not original_tx_id:
            return {"error": "refund_id and original_tx_id required", "status": "failed"}
        
        # Idempotency check
        existing = host.kv_get(f"refund:{refund_id}")
        if existing and not existing.startswith("ERROR"):
            try:
                result = json.loads(existing)
                result["idempotent"] = True
                return result
            except json.JSONDecodeError:
                pass
        
        # Acquire lock to prevent concurrent refunds on same transaction
        lock_key = f"refund-lock:{original_tx_id}"
        tenant_id = ""
        namespace = "payment"
        holder_id = f"refund:{refund_id}"
        out = host.lock_acquire(tenant_id, namespace, holder_id, lock_key, 30, 5000) or ""  # 5s timeout
        if not out or out.startswith("ERROR"):
            return {"error": "Could not acquire lock", "status": "failed"}
        try:
            lock_data = json.loads(out)
            lock_version = lock_data.get("version", out)
        except json.JSONDecodeError:
            lock_version = out

        try:
            # Find original transaction
            original_tx = None
            for tx in self.transactions:
                if tx.get("tx_id") == original_tx_id:
                    original_tx = tx
                    break
            
            if not original_tx:
                return {"error": "Original transaction not found", "status": "failed"}
            
            # Determine refund amount
            refund_amount = amount if amount > 0 else original_tx["amount"]
            if refund_amount > original_tx["amount"]:
                return {"error": "Refund amount exceeds original", "status": "failed"}
            
            # Process refund
            self.transaction_count += 1
            refund_tx_id = f"refund-{self.transaction_count}"
            
            refund_tx = {
                "tx_id": refund_tx_id,
                "refund_id": refund_id,
                "original_tx_id": original_tx_id,
                "amount": -refund_amount,  # Negative for refund
                "currency": original_tx["currency"],
                "reason": reason,
                "status": "completed",
                "timestamp": host.now_ms()
            }
            self.transactions = list(self.transactions) + [refund_tx]
            self.total_processed -= refund_amount
            
            result = {
                "status": "completed",
                "refund_tx_id": refund_tx_id,
                "refund_id": refund_id,
                "amount": refund_amount,
                "currency": original_tx["currency"]
            }
            host.kv_put(f"refund:{refund_id}", json.dumps(result))
            
            host.log("info", f"Refund processed: {refund_tx_id} amount={refund_amount}")
            return result
            
        finally:
            # Always release lock
            host.lock_release(lock_key, tenant_id, namespace, holder_id, lock_version)
    
    @handler("get_transaction")  # GenServer handlers use the call pattern automatically
    def get_transaction(self, tx_id: str = "") -> dict:
        """
        Get transaction details (GenServer call).
        
        Args:
            tx_id: Transaction ID
        
        Returns:
            Transaction details or error
        """
        if not tx_id:
            return {"error": "tx_id required"}
        
        for tx in self.transactions:
            if tx.get("tx_id") == tx_id:
                return {"transaction": tx, "found": True}
        
        return {"found": False, "tx_id": tx_id}
    
    @handler("get_balance")  # GenServer handlers use the call pattern automatically
    def get_balance(self) -> dict:
        """
        Get current balance (total processed).
        
        Returns:
            Balance summary
        """
        return {
            "total_processed": self.total_processed,
            "transaction_count": self.transaction_count,
            "currency": "USD"
        }
    
    @handler("list_transactions")  # GenServer handlers use the call pattern automatically
    def list_transactions(self, limit: int = 10, customer_id: str = "") -> dict:
        """
        List recent transactions.
        
        Args:
            limit: Maximum number to return
            customer_id: Filter by customer (optional)
        
        Returns:
            List of transactions
        """
        txs = self.transactions
        if customer_id:
            txs = [tx for tx in txs if tx.get("customer_id") == customer_id]
        
        recent = txs[-limit:] if limit > 0 else txs
        return {"transactions": recent, "count": len(recent), "total": len(txs)}
