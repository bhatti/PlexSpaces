"""
FSM Actor - Finite State Machine (Order Workflow Example)

Demonstrates a state machine pattern for order processing:
  idle -> pending -> processing -> shipped -> delivered

Real-world use case: E-commerce order tracking, payment processing, ticket workflows.

## SDK Features Used

- @fsm_actor: Marks class as FSM-style PlexSpaces actor (GenStateMachine behavior)
- state(): Defines persistent state fields
- @handler(): Routes messages to transition methods

## GenStateMachine Behavior

@fsm_actor sets behavior_type = GenStateMachine, which:
- Routes messages to handle_transition() WIT export (if defined)
- Enables state tracking and transition validation
- Supports event-driven state changes
"""

from plexspaces import fsm_actor, state, handler, init_handler

# Valid state transitions
TRANSITIONS = {
    "idle": ["pending"],
    "pending": ["processing", "cancelled"],
    "processing": ["shipped", "cancelled"],
    "shipped": ["delivered"],
    "delivered": [],
    "cancelled": []
}


@fsm_actor
class OrderFSM:
    """Order FSM actor with state machine pattern."""
    
    # Persistent state fields
    current_state: str = state(default="idle")
    order_id: str = state(default="")
    items: list = state(default_factory=list)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize FSM to idle state."""
        self.current_state = "idle"
        self.order_id = ""
        self.items = []
    
    @handler("get", "status")
    def get_status(self) -> dict:
        """Get current state."""
        return {"state": self.current_state, "order_id": self.order_id}
    
    @handler("create")
    def create_order(self, order_id: str = "order-1", items: list = None) -> dict:
        """Create order (idle -> pending)."""
        if self.current_state != "idle":
            return {"error": "must_be_idle"}
        self.order_id = order_id
        self.items = items or []
        self.current_state = "pending"
        return {"status": "ok", "state": "pending"}
    
    @handler("process")
    def process_order(self) -> dict:
        """Start processing (pending -> processing)."""
        if self.current_state != "pending":
            return {"error": "must_be_pending"}
        self.current_state = "processing"
        return {"status": "ok", "state": "processing"}
    
    @handler("ship")
    def ship_order(self) -> dict:
        """Ship order (processing -> shipped)."""
        if self.current_state != "processing":
            return {"error": "must_be_processing"}
        self.current_state = "shipped"
        return {"status": "ok", "state": "shipped"}
    
    @handler("deliver")
    def deliver_order(self) -> dict:
        """Deliver order (shipped -> delivered)."""
        if self.current_state != "shipped":
            return {"error": "must_be_shipped"}
        self.current_state = "delivered"
        return {"status": "ok", "state": "delivered"}
    
    @handler("cancel")
    def cancel_order(self) -> dict:
        """Cancel order (pending/processing -> cancelled)."""
        if self.current_state not in ["pending", "processing"]:
            return {"error": "cannot_cancel"}
        self.current_state = "cancelled"
        return {"status": "ok", "state": "cancelled"}
    
    @handler("reset")
    def reset(self) -> dict:
        """Reset to idle state."""
        self.current_state = "idle"
        self.order_id = ""
        self.items = []
        return {"status": "ok", "state": "idle"}
    
    @handler("transitions")
    def get_transitions(self) -> dict:
        """Get valid transitions from current state."""
        valid = TRANSITIONS.get(self.current_state, [])
        return {"state": self.current_state, "valid": valid}
