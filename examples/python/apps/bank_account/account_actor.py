"""
Bank Account Actor - Durable State Example (Python WASM with SDK)

A bank account that demonstrates durability:
- Balance persists across restarts via state() decorator
- Transaction log enables replay after crash
- Multiple accounts deployed via ApplicationSpec

Real-world use case: Banking, wallets, any financial ledger.

## SDK Features Used

1. **@actor(facets=["durability"])** - Marks class as durable PlexSpaces actor
2. **state()** - Defines persistent state fields (auto-saved/restored)
3. **@handler()** - Routes messages to methods
4. **@init_handler** - Custom initialization

## Durability Configuration

WASM actors use checkpoint-based durability (Cloudflare Durable Objects pattern):
- State fields defined with state() are auto-serialized via get_state()/set_state()
- Enable durability via `durability_enabled: true` in release.yaml or WasmConfig
- The `facets=["durability"]` annotation documents this actor expects durability

Note: This differs from Rust actors which use DurabilityFacet. WASM actors use
the simpler checkpoint model for portability across WASM runtimes.
"""

from plexspaces import actor, state, handler, init_handler


@actor(facets=["durability"])
class BankAccount:
    """Bank account actor with durable state."""
    
    # Persistent state fields (auto-saved/restored)
    account_id: str = state(default="")
    balance: int = state(default=0)
    transactions: list = state(default_factory=list)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize account from config."""
        self.account_id = config.get("account_id", "")
        self.balance = 0
        self.transactions = []
    
    @handler("balance", "get")
    def get_balance(self) -> dict:
        """Get current balance."""
        return {"account": self.account_id, "balance": self.balance}
    
    @handler("deposit")
    def deposit(self, amount: int = 0) -> dict:
        """Deposit money into account."""
        if amount <= 0:
            return {"error": "invalid_amount"}
        
        self.balance += amount
        self.transactions.append({
            "type": "deposit",
            "amount": amount,
            "balance_after": self.balance
        })
        return {"status": "ok", "balance": self.balance}
    
    @handler("withdraw")
    def withdraw(self, amount: int = 0) -> dict:
        """Withdraw money from account."""
        if amount <= 0:
            return {"error": "invalid_amount"}
        if amount > self.balance:
            return {"error": "insufficient_funds", "balance": self.balance}
        
        self.balance -= amount
        self.transactions.append({
            "type": "withdraw",
            "amount": amount,
            "balance_after": self.balance
        })
        return {"status": "ok", "balance": self.balance}
    
    @handler("tx_count")
    def transaction_count(self) -> dict:
        """Get number of transactions."""
        return {"count": len(self.transactions)}
    
    @handler("history")
    def get_history(self, count: int = 5) -> dict:
        """Get recent transactions."""
        count = min(count, len(self.transactions))
        recent = self.transactions[-count:] if count > 0 else []
        return {"transactions": recent}
    
    @handler("replay")
    def replay_transactions(self) -> dict:
        """Replay transactions to verify state consistency."""
        replayed = 0
        rebuilt_balance = 0
        for tx in self.transactions:
            if tx["type"] == "deposit":
                rebuilt_balance += tx["amount"]
            elif tx["type"] == "withdraw":
                rebuilt_balance -= tx["amount"]
            replayed += 1
        return {
            "replayed": replayed,
            "rebuilt_balance": rebuilt_balance,
            "current_balance": self.balance
        }
    
    @handler("set_account")
    def set_account_id(self, account_id: str = "") -> dict:
        """Set account ID."""
        self.account_id = account_id
        return {"status": "ok"}
