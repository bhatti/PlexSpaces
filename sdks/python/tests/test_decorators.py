# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors

"""Tests for PlexSpaces Python SDK decorators."""

import pytest
import sys
from pathlib import Path

# Add SDK to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces import actor, state, handler, init_handler
from plexspaces.decorators import (
    get_state_dict, set_state_dict, dispatch_message, init_actor,
    event_actor, fsm_actor, gen_server_actor, workflow_actor,
    BEHAVIOR_GEN_SERVER, BEHAVIOR_GEN_EVENT, BEHAVIOR_GEN_STATE_MACHINE, BEHAVIOR_WORKFLOW,
    _sanitize_payload_for_wasm, _desanitize_from_wasm
)


class TestStateDecorator:
    """Tests for state() decorator."""
    
    def test_state_default_value(self):
        """State fields should have default values."""
        @actor
        class TestActor:
            count: int = state(default=0)
            name: str = state(default="default")
        
        instance = TestActor()
        assert instance.count == 0
        assert instance.name == "default"
    
    def test_state_default_factory(self):
        """State fields with factory should create new instances."""
        @actor
        class TestActor:
            items: list = state(default_factory=list)
            data: dict = state(default_factory=dict)
        
        instance1 = TestActor()
        instance2 = TestActor()
        
        # Each instance should have its own list
        instance1.items.append("a")
        assert instance2.items == []
    
    def test_state_set_and_get(self):
        """State fields should be settable and gettable."""
        @actor
        class TestActor:
            value: int = state(default=0)
        
        instance = TestActor()
        instance.value = 42
        assert instance.value == 42


class TestHandlerDecorator:
    """Tests for @handler decorator."""
    
    def test_single_handler(self):
        """Handler should register for single message type."""
        @actor
        class TestActor:
            @handler("ping")
            def ping(self) -> dict:
                return {"pong": True}
        
        assert "ping" in TestActor._plexspaces_handlers
    
    def test_multiple_handlers(self):
        """Handler should register for multiple message types."""
        @actor
        class TestActor:
            @handler("add", "plus", "sum")
            def add(self, a: int, b: int) -> dict:
                return {"result": a + b}
        
        assert "add" in TestActor._plexspaces_handlers
        assert "plus" in TestActor._plexspaces_handlers
        assert "sum" in TestActor._plexspaces_handlers
    
    def test_handler_dispatch(self):
        """Handlers should be callable via dispatch."""
        @actor
        class TestActor:
            @handler("greet")
            def greet(self, name: str) -> dict:
                return {"message": f"Hello, {name}!"}
        
        instance = TestActor()
        result = dispatch_message(instance, "caller", "greet", {"name": "World"})
        assert result == {"message": "Hello, World!"}


class TestActorDecorator:
    """Tests for @actor decorator."""
    
    def test_actor_is_marked(self):
        """Actor class should be marked as PlexSpaces actor."""
        @actor
        class TestActor:
            pass
        
        assert hasattr(TestActor, '_plexspaces_is_actor')
        assert TestActor._plexspaces_is_actor is True
    
    def test_actor_collects_state_fields(self):
        """Actor should collect all state fields."""
        @actor
        class TestActor:
            a: int = state(default=1)
            b: str = state(default="x")
            c: list = state(default_factory=list)
        
        assert len(TestActor._plexspaces_state_fields) == 3
        assert "a" in TestActor._plexspaces_state_fields
        assert "b" in TestActor._plexspaces_state_fields
        assert "c" in TestActor._plexspaces_state_fields
    
    def test_actor_collects_handlers(self):
        """Actor should collect all handlers."""
        @actor
        class TestActor:
            @handler("op1")
            def operation_one(self) -> dict:
                return {}
            
            @handler("op2", "op2_alt")
            def operation_two(self) -> dict:
                return {}
        
        assert len(TestActor._plexspaces_handlers) == 3  # op1, op2, op2_alt


class TestStateSerialization:
    """Tests for state serialization/deserialization."""
    
    def test_get_state_dict(self):
        """Should extract state fields as dict."""
        @actor
        class TestActor:
            count: int = state(default=0)
            name: str = state(default="")
        
        instance = TestActor()
        instance.count = 42
        instance.name = "test"
        
        state_dict = get_state_dict(instance)
        assert state_dict == {"count": 42, "name": "test"}
    
    def test_set_state_dict(self):
        """Should restore state fields from dict."""
        @actor
        class TestActor:
            count: int = state(default=0)
            name: str = state(default="")
        
        instance = TestActor()
        set_state_dict(instance, {"count": 100, "name": "restored"})
        
        assert instance.count == 100
        assert instance.name == "restored"
    
    def test_state_roundtrip(self):
        """State should survive get/set roundtrip."""
        @actor
        class TestActor:
            items: list = state(default_factory=list)
            data: dict = state(default_factory=dict)
        
        instance1 = TestActor()
        instance1.items = [1, 2, 3]
        instance1.data = {"key": "value"}
        
        state_dict = get_state_dict(instance1)
        
        instance2 = TestActor()
        set_state_dict(instance2, state_dict)
        
        assert instance2.items == [1, 2, 3]
        assert instance2.data == {"key": "value"}


class TestInitActor:
    """Tests for actor initialization."""
    
    def test_init_sets_state_fields(self):
        """Init should set matching state fields from config."""
        @actor
        class TestActor:
            account_id: str = state(default="")
            balance: int = state(default=0)
        
        instance = TestActor()
        init_actor(instance, {"account_id": "ACC123", "balance": 1000})
        
        assert instance.account_id == "ACC123"
        assert instance.balance == 1000
    
    def test_init_ignores_unknown_fields(self):
        """Init should ignore config fields not in state."""
        @actor
        class TestActor:
            known: str = state(default="")
        
        instance = TestActor()
        init_actor(instance, {"known": "value", "unknown": "ignored"})
        
        assert instance.known == "value"
        assert not hasattr(instance, "unknown")
    
    def test_init_handler_called(self):
        """Custom init handler should be called."""
        @actor
        class TestActor:
            processed: bool = state(default=False)
            config_data: dict = state(default_factory=dict)
            
            @init_handler
            def on_init(self, config: dict):
                self.processed = True
                self.config_data = config
        
        instance = TestActor()
        init_actor(instance, {"foo": "bar"})
        
        assert instance.processed is True
        assert instance.config_data == {"foo": "bar"}


class TestCompleteActor:
    """Integration tests with complete actor examples."""
    
    def test_counter_actor(self):
        """Test a simple counter actor."""
        @actor
        class Counter:
            count: int = state(default=0)
            
            @handler("increment")
            def increment(self, amount: int = 1) -> dict:
                self.count += amount
                return {"count": self.count}
            
            @handler("decrement")
            def decrement(self, amount: int = 1) -> dict:
                self.count -= amount
                return {"count": self.count}
            
            @handler("get")
            def get(self) -> dict:
                return {"count": self.count}
        
        instance = Counter()
        
        # Test increment
        result = dispatch_message(instance, "", "increment", {"amount": 5})
        assert result == {"count": 5}
        
        # Test decrement
        result = dispatch_message(instance, "", "decrement", {"amount": 2})
        assert result == {"count": 3}
        
        # Test get
        result = dispatch_message(instance, "", "get", {})
        assert result == {"count": 3}
        
        # Test state persistence
        state_dict = get_state_dict(instance)
        assert state_dict == {"count": 3}
        
        new_instance = Counter()
        set_state_dict(new_instance, state_dict)
        
        result = dispatch_message(new_instance, "", "get", {})
        assert result == {"count": 3}
    
    def test_bank_account_actor(self):
        """Test a bank account actor."""
        @actor
        class BankAccount:
            balance: int = state(default=0)
            account_id: str = state(default="")
            
            @handler("deposit")
            def deposit(self, amount: int) -> dict:
                self.balance += amount
                return {"balance": self.balance}
            
            @handler("withdraw")
            def withdraw(self, amount: int) -> dict:
                if amount > self.balance:
                    return {"error": "insufficient_funds"}
                self.balance -= amount
                return {"balance": self.balance}
            
            @handler("balance", "get")
            def get_balance(self) -> dict:
                return {"balance": self.balance, "account_id": self.account_id}
        
        instance = BankAccount()
        init_actor(instance, {"account_id": "ACC001"})
        
        # Deposit
        result = dispatch_message(instance, "", "deposit", {"amount": 100})
        assert result == {"balance": 100}
        
        # Withdraw success
        result = dispatch_message(instance, "", "withdraw", {"amount": 30})
        assert result == {"balance": 70}
        
        # Withdraw failure
        result = dispatch_message(instance, "", "withdraw", {"amount": 100})
        assert result == {"error": "insufficient_funds"}
        
        # Balance unchanged after failed withdraw
        result = dispatch_message(instance, "", "balance", {})
        assert result == {"balance": 70, "account_id": "ACC001"}


class TestBehaviorTypes:
    """Tests for behavior type decorators."""
    
    def test_actor_behavior_type(self):
        """@actor should set GenServer behavior."""
        @actor
        class TestActor:
            pass
        
        assert hasattr(TestActor, '__behavior_type__')
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_SERVER
    
    def test_event_actor_behavior_type(self):
        """@event_actor should set GenEvent behavior."""
        @event_actor
        class TestActor:
            pass
        
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_EVENT
    
    def test_fsm_actor_behavior_type(self):
        """@fsm_actor should set GenStateMachine behavior."""
        @fsm_actor
        class TestActor:
            pass
        
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_STATE_MACHINE
    
    def test_gen_server_actor_behavior_type(self):
        """@gen_server_actor should set GenServer behavior."""
        @gen_server_actor
        class TestActor:
            pass
        
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_SERVER
    
    def test_workflow_actor_behavior_type(self):
        """@workflow_actor should set Workflow behavior."""
        @workflow_actor
        class TestActor:
            pass
        
        assert TestActor.__behavior_type__ == BEHAVIOR_WORKFLOW


class TestFacets:
    """Tests for facets parameter."""
    
    def test_actor_default_no_facets(self):
        """@actor without facets should have empty list."""
        @actor
        class TestActor:
            pass
        
        assert hasattr(TestActor, '__facets__')
        assert TestActor.__facets__ == []
    
    def test_actor_with_facets(self):
        """@actor with facets should store them."""
        @actor(facets=["durability"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability"]
    
    def test_actor_with_multiple_facets(self):
        """@actor with multiple facets should store all."""
        @actor(facets=["durability", "registry"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability", "registry"]
    
    def test_event_actor_with_facets(self):
        """@event_actor with facets should store them."""
        @event_actor(facets=["durability"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability"]
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_EVENT
    
    def test_fsm_actor_with_facets(self):
        """@fsm_actor with facets should store them."""
        @fsm_actor(facets=["durability", "registry"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability", "registry"]
        assert TestActor.__behavior_type__ == BEHAVIOR_GEN_STATE_MACHINE
    
    def test_gen_server_actor_with_facets(self):
        """@gen_server_actor with facets should store them."""
        @gen_server_actor(facets=["durability"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability"]
    
    def test_workflow_actor_with_facets(self):
        """@workflow_actor with facets should store them."""
        @workflow_actor(facets=["durability"])
        class TestActor:
            pass
        
        assert TestActor.__facets__ == ["durability"]

    def test_facets_preserved_with_state_and_handlers(self):
        """Facets should work with full actor definition."""
        @actor(facets=["durability"])
        class BankAccount:
            balance: int = state(default=0)

            @handler("deposit")
            def deposit(self, amount: int) -> dict:
                self.balance += amount
                return {"balance": self.balance}

        assert BankAccount.__facets__ == ["durability"]
        assert BankAccount.__behavior_type__ == BEHAVIOR_GEN_SERVER
        assert "deposit" in BankAccount._plexspaces_handlers
        assert "balance" in BankAccount._plexspaces_state_fields


class TestWorkflowDispatch:
    """Integration tests for workflow run/signal/query dispatch (dispatch_message)."""

    def test_workflow_dispatch_run(self):
        """workflow_run effective_type should call run(payload)."""
        @workflow_actor
        class OrderWorkflow:
            def run(self, payload):
                return {"status": "ok", "order_id": payload.get("order_id", "")}

            def signal(self, name, data):
                pass

            def query(self, name, params):
                return {}

        instance = OrderWorkflow()
        init_actor(instance, {})
        result = dispatch_message(instance, "client", "workflow_run", {"order_id": "o1"})
        assert result == {"status": "ok", "order_id": "o1"}

    def test_workflow_dispatch_signal(self):
        """workflow_signal:name effective_type should call signal(name, payload)."""
        received = []

        @workflow_actor
        class OrderWorkflow:
            def run(self, payload):
                return {}

            def signal(self, name, data):
                received.append((name, data))

            def query(self, name, params):
                return {}

        instance = OrderWorkflow()
        init_actor(instance, {})
        dispatch_message(instance, "client", "workflow_signal:cancel", {"reason": "user"})
        assert received == [("cancel", {"reason": "user"})]

    def test_workflow_dispatch_query(self):
        """workflow_query:name effective_type should call query(name, params)."""
        @workflow_actor
        class OrderWorkflow:
            def run(self, payload):
                return {}

            def signal(self, name, data):
                pass

            def query(self, name, params):
                return {"query": name, "order_id": params.get("order_id", "")}

        instance = OrderWorkflow()
        init_actor(instance, {})
        result = dispatch_message(instance, "client", "workflow_query:status", {"order_id": "o1"})
        assert result == {"query": "status", "order_id": "o1"}

    def test_workflow_dispatch_run_from_payload_op(self):
        """When payload has op=workflow_run, effective_type becomes workflow_run."""
        @workflow_actor
        class OrderWorkflow:
            def run(self, payload):
                return {"ran": True}

            def signal(self, name, data):
                pass

            def query(self, name, params):
                return {}

        instance = OrderWorkflow()
        init_actor(instance, {})
        result = dispatch_message(instance, "client", "cast", {"op": "workflow_run", "order_id": "o1"})
        assert result == {"ran": True}


class TestDesanitizeFromWasm:
    """Tests for _desanitize_from_wasm() which reverses WASM float-to-string sanitization."""

    def test_desanitize_int_string(self):
        """Integer strings should be converted back to int."""
        assert _desanitize_from_wasm("42") == 42
        assert isinstance(_desanitize_from_wasm("42"), int)

    def test_desanitize_float_string(self):
        """Float strings should be converted back to float."""
        assert _desanitize_from_wasm("3.14") == 3.14
        assert isinstance(_desanitize_from_wasm("3.14"), float)

    def test_desanitize_negative_float(self):
        """Negative float strings should be converted back to float."""
        assert _desanitize_from_wasm("-1.5") == -1.5
        assert isinstance(_desanitize_from_wasm("-1.5"), float)

    def test_desanitize_non_numeric_string(self):
        """Non-numeric strings should remain unchanged."""
        assert _desanitize_from_wasm("hello") == "hello"
        assert isinstance(_desanitize_from_wasm("hello"), str)

    def test_desanitize_nested_dict(self):
        """Stringified numbers in dicts should be restored to numeric types."""
        result = _desanitize_from_wasm({"a": "1", "b": "2.5"})
        assert result == {"a": 1, "b": 2.5}
        assert isinstance(result["a"], int)
        assert isinstance(result["b"], float)

    def test_desanitize_nested_list(self):
        """Stringified numbers in lists should be restored to numeric types."""
        result = _desanitize_from_wasm(["1.0", "2.0", "3.0"])
        assert result == [1.0, 2.0, 3.0]
        assert all(isinstance(v, float) for v in result)

    def test_desanitize_mixed_types(self):
        """Mixed structures with strings, ints, and floats should be desanitized correctly."""
        input_obj = {"name": "test", "count": "42", "values": ["1.0", "2.0"]}
        expected = {"name": "test", "count": 42, "values": [1.0, 2.0]}
        result = _desanitize_from_wasm(input_obj)
        assert result == expected
        assert isinstance(result["name"], str)
        assert isinstance(result["count"], int)
        assert all(isinstance(v, float) for v in result["values"])

    def test_desanitize_roundtrip_with_sanitize(self):
        """Sanitize then desanitize should produce the original object for various inputs."""
        test_cases = [
            {"count": 42, "name": "test"},
            {"x": 1.5, "y": -2.3},
            {"items": [1.0, 2.0, 3.0]},
            {"nested": {"a": 1, "b": 2.5}},
            {"mixed": [1, 2.0, "hello", True, None]},
        ]
        for obj in test_cases:
            sanitized = _sanitize_payload_for_wasm(obj)
            restored = _desanitize_from_wasm(sanitized)
            assert restored == obj, f"Roundtrip failed for {obj!r}: sanitized={sanitized!r}, restored={restored!r}"

    def test_desanitize_preserves_none_and_bool(self):
        """None and bool values should pass through unchanged."""
        assert _desanitize_from_wasm(None) is None
        assert _desanitize_from_wasm(True) is True
        assert _desanitize_from_wasm(False) is False

    def test_desanitize_empty_string(self):
        """Empty string should remain an empty string (not converted to a number)."""
        assert _desanitize_from_wasm("") == ""
        assert isinstance(_desanitize_from_wasm(""), str)


class TestDesanitizeNbodyRoundtrip:
    """Roundtrip tests for the exact nbody bug scenario: state with lists of floats.

    The nbody simulation stores body positions, velocities, and masses as lists
    of floats. When state is serialized through WASM, floats become strings.
    _desanitize_from_wasm must restore them so arithmetic works correctly.
    """

    def test_nbody_positions_roundtrip(self):
        """Lists of float positions should survive sanitize/desanitize roundtrip."""
        state = {
            "x": [0.0, 4.84143144246472090e+00, 8.34336671824457987e+00],
            "y": [0.0, -1.16032004402742839e+00, 4.12479856412430479e+00],
            "z": [0.0, -1.03622044471123109e-01, -4.03523417114321381e-01],
        }
        sanitized = _sanitize_payload_for_wasm(state)
        # After sanitize, all values should be strings
        for key in ("x", "y", "z"):
            assert all(isinstance(v, str) for v in sanitized[key])
        restored = _desanitize_from_wasm(sanitized)
        assert restored == state

    def test_nbody_velocities_roundtrip(self):
        """Lists of float velocities (small values) should survive roundtrip."""
        state = {
            "vx": [0.0, 1.66007664274403694e-03, -2.76742510726862411e-03],
            "vy": [0.0, 7.69901118419740425e-03, 4.99852801234917238e-03],
            "vz": [0.0, -6.90460016972063023e-05, 2.30417297573763929e-05],
        }
        sanitized = _sanitize_payload_for_wasm(state)
        restored = _desanitize_from_wasm(sanitized)
        assert restored == state

    def test_nbody_masses_roundtrip(self):
        """Float masses should survive roundtrip."""
        state = {
            "mass": [1.0, 9.54791938424326609e-04, 2.85885980666130812e-04],
        }
        sanitized = _sanitize_payload_for_wasm(state)
        restored = _desanitize_from_wasm(sanitized)
        assert restored == state

    def test_nbody_full_state_roundtrip(self):
        """Complete nbody state with int count + float lists should survive roundtrip."""
        state = {
            "n_bodies": 3,
            "dt": 0.01,
            "x": [0.0, 4.84143144246472090e+00, 8.34336671824457987e+00],
            "y": [0.0, -1.16032004402742839e+00, 4.12479856412430479e+00],
            "vx": [0.0, 1.66007664274403694e-03, -2.76742510726862411e-03],
            "vy": [0.0, 7.69901118419740425e-03, 4.99852801234917238e-03],
            "mass": [1.0, 9.54791938424326609e-04, 2.85885980666130812e-04],
        }
        sanitized = _sanitize_payload_for_wasm(state)
        # n_bodies (int) should pass through sanitize unchanged
        assert sanitized["n_bodies"] == 3
        # dt (float) should become a string
        assert isinstance(sanitized["dt"], str)
        restored = _desanitize_from_wasm(sanitized)
        assert restored == state
        assert isinstance(restored["n_bodies"], int)
        assert isinstance(restored["dt"], float)
        assert all(isinstance(v, float) for v in restored["x"])

    def test_nbody_arithmetic_after_roundtrip(self):
        """Restored float values must support arithmetic (the actual nbody bug)."""
        state = {"x": [1.0, 2.5, -3.7], "vx": [0.1, -0.2, 0.3]}
        sanitized = _sanitize_payload_for_wasm(state)
        restored = _desanitize_from_wasm(sanitized)
        # Simulate one Euler step: x[i] += vx[i] * dt
        dt = 0.01
        for i in range(len(restored["x"])):
            restored["x"][i] += restored["vx"][i] * dt
        # Verify arithmetic worked (values should be close to expected)
        expected_x = [1.0 + 0.1 * dt, 2.5 + (-0.2) * dt, -3.7 + 0.3 * dt]
        for actual, expected in zip(restored["x"], expected_x):
            assert abs(actual - expected) < 1e-15


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
