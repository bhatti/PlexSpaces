"""
Bank Account Actor - Durable State Example (Python WASM)

A bank account that demonstrates durability:
- Balance persists across restarts via get_state/set_state
- Transaction log enables replay after crash
- Multiple accounts deployed via ApplicationSpec

Real-world use case: Banking, wallets, any financial ledger.

## Durability Features Demonstrated

1. **Persistent State** - Balance saved via get_state(), restored via set_state()
2. **Transaction Log** - Every operation logged for replay/audit
3. **Crash Recovery** - After restart, balance is restored from saved state
4. **Replay** - Transaction log can recreate state from scratch

## WASM Memory Workarounds Applied
See examples/python/README.md for documentation.
"""

import json
from wit_world import exports

# Account state (persisted)
_account_id = ""
_balance = 0
_transactions = []  # [{type, amount, timestamp, balance_after}]


class Actor(exports.Actor):
    """Bank account actor with durable state."""
    
    def init(self, config_json: str) -> str:
        """Initialize account with zero balance."""
        global _account_id, _balance, _transactions
        _account_id = ""
        _balance = 0
        _transactions = []
        
        if config_json:
            cfg = json.loads(config_json)
            _account_id = cfg.get("account_id", "")
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """Handle banking operations."""
        global _account_id, _balance, _transactions
        
        payload = {}
        if payload_json:
            payload = json.loads(payload_json)
        
        op = payload.get("op", msg_type)
        
        # Get balance
        if op == "balance" or op == "get":
            return '{"account":"' + _account_id + '","balance":' + str(_balance) + '}'
        
        # Deposit money
        if op == "deposit":
            amount = payload.get("amount", 0)
            if amount <= 0:
                return '{"error":"invalid_amount"}'
            
            _balance += amount
            _transactions.append({
                "type": "deposit",
                "amount": amount,
                "balance_after": _balance
            })
            return '{"status":"ok","balance":' + str(_balance) + '}'
        
        # Withdraw money
        if op == "withdraw":
            amount = payload.get("amount", 0)
            if amount <= 0:
                return '{"error":"invalid_amount"}'
            if amount > _balance:
                return '{"error":"insufficient_funds","balance":' + str(_balance) + '}'
            
            _balance -= amount
            _transactions.append({
                "type": "withdraw",
                "amount": amount,
                "balance_after": _balance
            })
            return '{"status":"ok","balance":' + str(_balance) + '}'
        
        # Get transaction count
        if op == "tx_count":
            return '{"count":' + str(len(_transactions)) + '}'
        
        # Get recent transactions
        if op == "history":
            count = min(payload.get("count", 5), len(_transactions))
            recent = _transactions[-count:] if count > 0 else []
            return json.dumps({"transactions": recent})
        
        # Replay transactions (rebuild state from log)
        if op == "replay":
            replayed = 0
            rebuilt_balance = 0
            for tx in _transactions:
                if tx["type"] == "deposit":
                    rebuilt_balance += tx["amount"]
                elif tx["type"] == "withdraw":
                    rebuilt_balance -= tx["amount"]
                replayed += 1
            return ('{"replayed":' + str(replayed) + 
                    ',"rebuilt_balance":' + str(rebuilt_balance) + 
                    ',"current_balance":' + str(_balance) + '}')
        
        # Set account ID
        if op == "set_account":
            _account_id = payload.get("account_id", "")
            return '{"status":"ok"}'
        
        return '{"error":"unknown_op"}'
    
    def get_state(self) -> str:
        """
        DURABILITY: Save account state before shutdown/passivation.
        
        This is called by PlexSpaces framework:
        - Before actor passivation (idle timeout)
        - Before node shutdown
        - During snapshot creation
        
        The returned JSON is stored in the journal and restored on restart.
        """
        global _account_id, _balance, _transactions
        return json.dumps({
            "account_id": _account_id,
            "balance": _balance,
            "transactions": _transactions
        })
    
    def set_state(self, state_json: str) -> str:
        """
        DURABILITY: Restore account state after restart/recovery.
        
        This is called by PlexSpaces framework:
        - After actor reactivation
        - After node restart
        - After crash recovery
        
        Balance and transaction history are restored - no data loss!
        """
        global _account_id, _balance, _transactions
        data = json.loads(state_json)
        _account_id = data.get("account_id", "")
        _balance = data.get("balance", 0)
        _transactions = data.get("transactions", [])
        return ""
