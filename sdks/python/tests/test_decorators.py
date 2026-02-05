# SPDX-License-Identifier: LGPL-2.1-or-later
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
    BEHAVIOR_GEN_SERVER, BEHAVIOR_GEN_EVENT, BEHAVIOR_GEN_STATE_MACHINE, BEHAVIOR_WORKFLOW
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


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
