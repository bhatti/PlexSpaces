# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# PlexSpaces Actor Decorators
#
# Provides @actor, @handler, and state() for defining PlexSpaces actors
# with minimal boilerplate (inspired by Ray's @ray.remote).

"""
Actor decorators for PlexSpaces Python SDK.

Usage:
    from plexspaces import actor, handler, state

    @actor
    class Counter:
        count: int = state(default=0)
        
        @handler("increment")
        def increment(self, amount: int = 1) -> dict:
            self.count += amount
            return {"count": self.count}
"""

import inspect
import json
import functools
from typing import Any, Callable, Dict, List, Optional, Type, TypeVar, Union
from dataclasses import dataclass, field

T = TypeVar('T')


@dataclass
class StateField:
    """Descriptor for state fields that should be persisted."""
    name: str = ""
    default: Any = None
    default_factory: Optional[Callable] = None
    
    def __set_name__(self, owner: Type, name: str) -> None:
        self.name = name
    
    def __get__(self, obj: Any, objtype: Type = None) -> Any:
        if obj is None:
            return self
        attr_name = f"_state_{self.name}"
        if not hasattr(obj, attr_name):
            # Initialize with default on first access
            setattr(obj, attr_name, self.get_default())
        return getattr(obj, attr_name)
    
    def __set__(self, obj: Any, value: Any) -> None:
        setattr(obj, f"_state_{self.name}", value)
    
    def get_default(self) -> Any:
        if self.default_factory is not None:
            return self.default_factory()
        return self.default


def state(default: Any = None, default_factory: Callable = None) -> StateField:
    """
    Define a persistent state field for an actor.
    
    State fields are automatically serialized in get_state() and
    restored in set_state(). Use this for any data that should
    survive actor restarts.
    
    Args:
        default: Default value for the field (for immutable types)
        default_factory: Factory function for default (for mutable types like list, dict)
    
    Returns:
        StateField descriptor
    
    Example:
        @actor
        class MyActor:
            count: int = state(default=0)
            items: list = state(default_factory=list)
    """
    return StateField(default=default, default_factory=default_factory)


@dataclass
class HandlerInfo:
    """Metadata for a message handler."""
    method: Callable
    msg_types: List[str]
    invocation: str = "call"  # "call" (request-reply) or "cast" (fire-and-forget)


def handler(*args, invocation: str = "call") -> Callable:
    """
    Decorator to mark a method as a message handler.
    
    The method will be called when a message with any of the specified
    types is received. Arguments are extracted from the JSON payload.
    
    Args:
        *args: Message type(s) to handle. Can also include invocation type as second arg
               for backward compatibility: @handler("op", "call") or @handler("op", "cast")
        invocation: "call" (request-reply, default) or "cast" (fire-and-forget).
                   For GenServer actors, always use "call" since they return responses.
    
    Returns:
        Decorated method
    
    Example:
        @actor
        class PaymentHandler:
            @handler("process", "call")  # request-reply (returns response)
            def process(self, amount: int) -> dict:
                return {"status": "ok", "amount": amount}
            
            @handler("audit", "cast")  # fire-and-forget (no response needed)
            def audit(self, event: str) -> None:
                self.log_event(event)
            
            # Keyword syntax also works:
            @handler("balance", invocation="call")
            def get_balance(self) -> dict:
                return {"balance": self.balance}
    """
    # Parse args: handle both @handler("op", "call") and @handler("op", invocation="call")
    msg_types = []
    effective_invocation = invocation
    
    for arg in args:
        if arg in ("call", "cast"):
            # Second positional arg is invocation type (backward compat)
            effective_invocation = arg
        else:
            msg_types.append(arg)
    
    def decorator(method: Callable) -> Callable:
        # Store handler info as attribute
        method._plexspaces_handler = HandlerInfo(
            method=method,
            msg_types=msg_types,
            invocation=effective_invocation
        )
        return method
    return decorator


def init_handler(method: Callable) -> Callable:
    """
    Decorator to mark a method as the initialization handler.
    
    Called when the actor receives config during init().
    If not specified, config values are set directly on state fields.
    
    Example:
        @actor
        class MyActor:
            @init_handler
            def on_init(self, config: dict):
                self.account_id = config.get("account_id", "")
    """
    method._plexspaces_init_handler = True
    return method


class ActorMeta(type):
    """Metaclass for PlexSpaces actors."""
    
    def __new__(mcs, name: str, bases: tuple, namespace: dict) -> Type:
        # Collect state fields
        state_fields: Dict[str, StateField] = {}
        for attr_name, attr_value in namespace.items():
            if isinstance(attr_value, StateField):
                state_fields[attr_name] = attr_value
        
        # Collect handlers
        handlers: Dict[str, Callable] = {}
        init_handler_method: Optional[Callable] = None
        for attr_name, attr_value in namespace.items():
            if callable(attr_value):
                if hasattr(attr_value, '_plexspaces_handler'):
                    info: HandlerInfo = attr_value._plexspaces_handler
                    for msg_type in info.msg_types:
                        handlers[msg_type] = attr_value
                if hasattr(attr_value, '_plexspaces_init_handler'):
                    init_handler_method = attr_value
        
        # Store metadata in class
        namespace['_plexspaces_state_fields'] = state_fields
        namespace['_plexspaces_handlers'] = handlers
        namespace['_plexspaces_init_handler'] = init_handler_method
        namespace['_plexspaces_is_actor'] = True
        
        return super().__new__(mcs, name, bases, namespace)


# OTP-inspired behavior types (matches crates/behavior and plexspaces_core::BehaviorType)
BEHAVIOR_GEN_SERVER = "GenServer"
BEHAVIOR_GEN_EVENT = "GenEvent"
BEHAVIOR_GEN_STATE_MACHINE = "GenStateMachine"
BEHAVIOR_WORKFLOW = "Workflow"


def _actor_with_behavior(cls: Type[T], behavior_type: str, facets: Optional[List[str]] = None) -> Type[T]:
    """Apply ActorMeta and set __behavior_type__ and __facets__ on the class."""
    if not isinstance(cls, ActorMeta):
        new_cls = ActorMeta(cls.__name__, cls.__bases__, dict(cls.__dict__))
        new_cls.__module__ = cls.__module__
        new_cls.__qualname__ = cls.__qualname__
        cls = new_cls
    setattr(cls, "__behavior_type__", behavior_type)
    setattr(cls, "__facets__", facets or [])
    # Workflow behavior: dispatch_message routes workflow_run / workflow_signal:x / workflow_query:x to run/signal/query
    setattr(cls, "_plexspaces_workflow", behavior_type == BEHAVIOR_WORKFLOW)
    
    # GenServer always uses request-reply (call) - force invocation="call" on all handlers
    if behavior_type == BEHAVIOR_GEN_SERVER:
        for attr_name in dir(cls):
            attr = getattr(cls, attr_name, None)
            if callable(attr) and hasattr(attr, '_plexspaces_handler'):
                handler_info: HandlerInfo = attr._plexspaces_handler
                if handler_info.invocation != "call":
                    # Override to "call" for GenServer pattern
                    handler_info.invocation = "call"
    
    return cls


def actor(
    cls: Type[T] = None,
    *,
    facets: Optional[List[str]] = None
) -> Union[Type[T], Callable[[Type[T]], Type[T]]]:
    """
    Decorator to define a PlexSpaces actor class.
    
    Transforms a regular Python class into a PlexSpaces actor with:
    - Automatic state persistence (via state() fields)
    - Message routing (via @handler decorators)
    - JSON serialization/deserialization
    
    Args:
        cls: The class to transform into an actor
        facets: Optional list of facets this actor expects (e.g., ["durability", "registry"]).
                For WASM actors, "durability" means checkpoint-based persistence is enabled
                via WasmConfig.durability_enabled in the node/release config.
    
    Returns:
        The actor class with PlexSpaces metadata
    
    Example:
        @actor
        class BankAccount:
            balance: int = state(default=0)
            
            @handler("deposit")
            def deposit(self, amount: int) -> dict:
                self.balance += amount
                return {"balance": self.balance}
        
        # With facets (for documentation and config validation):
        @actor(facets=["durability"])
        class DurableAccount:
            balance: int = state(default=0)
    """
    def decorator(cls: Type[T]) -> Type[T]:
        return _actor_with_behavior(cls, BEHAVIOR_GEN_SERVER, facets)
    
    # Handle both @actor and @actor(facets=[...]) syntax
    if cls is not None:
        return decorator(cls)
    return decorator


def event_actor(
    cls: Type[T] = None,
    *,
    facets: Optional[List[str]] = None
) -> Union[Type[T], Callable[[Type[T]], Type[T]]]:
    """
    Decorator for GenEvent-style actors (fire-and-forget event handling).
    
    Like Erlang gen_event: handlers process events asynchronously; no request-reply.
    Use @handler("event_name") for event types. Good for telemetry, logs, side effects.
    
    Args:
        cls: The class to transform into an actor
        facets: Optional list of facets this actor expects (e.g., ["durability"])
    
    Example:
        @event_actor
        class SensorStream:
            @handler("ingest")
            def ingest(self, sensor_id: str, value: str) -> str:
                ...
    """
    def decorator(cls: Type[T]) -> Type[T]:
        return _actor_with_behavior(cls, BEHAVIOR_GEN_EVENT, facets)
    
    if cls is not None:
        return decorator(cls)
    return decorator


def gen_server_actor(
    cls: Type[T] = None,
    *,
    facets: Optional[List[str]] = None
) -> Union[Type[T], Callable[[Type[T]], Type[T]]]:
    """
    Decorator for GenServer-style actors (request-reply).
    
    Like Erlang gen_server: handlers return a reply to the caller.
    Use @handler("call", "call") or GET for read handlers so client gets reply.
    
    Args:
        cls: The class to transform into an actor
        facets: Optional list of facets this actor expects (e.g., ["durability"])
    """
    def decorator(cls: Type[T]) -> Type[T]:
        return _actor_with_behavior(cls, BEHAVIOR_GEN_SERVER, facets)
    
    if cls is not None:
        return decorator(cls)
    return decorator


def fsm_actor(
    cls: Type[T] = None,
    *,
    facets: Optional[List[str]] = None
) -> Union[Type[T], Callable[[Type[T]], Type[T]]]:
    """
    Decorator for FSM-style actors (GenStateMachine).
    
    Like Erlang gen_statem: stateful transitions; handlers can return next state.
    
    Use this decorator for actors with well-defined state transitions. Define
    valid transitions and use @handler for each transition trigger.
    
    Args:
        cls: The class to transform into an actor
        facets: Optional list of facets this actor expects (e.g., ["durability"])
    
    Example:
        @fsm_actor
        class OrderFSM:
            current_state: str = state(default="idle")
            
            @handler("create")
            def create_order(self, order_id: str) -> dict:
                if self.current_state != "idle":
                    return {"error": "must_be_idle"}
                self.current_state = "pending"
                return {"state": "pending"}
    """
    def decorator(cls: Type[T]) -> Type[T]:
        return _actor_with_behavior(cls, BEHAVIOR_GEN_STATE_MACHINE, facets)
    
    if cls is not None:
        return decorator(cls)
    return decorator


def workflow_actor(
    cls: Type[T] = None,
    *,
    facets: Optional[List[str]] = None
) -> Union[Type[T], Callable[[Type[T]], Type[T]]]:
    """
    Decorator for workflow/orchestration actors.
    
    Multi-step flows with durable state. Use this for long-running workflows
    that need checkpointing and recovery.
    
    Args:
        cls: The class to transform into an actor
        facets: Optional list of facets this actor expects (e.g., ["durability"])
    """
    def decorator(cls: Type[T]) -> Type[T]:
        return _actor_with_behavior(cls, BEHAVIOR_WORKFLOW, facets)
    
    if cls is not None:
        return decorator(cls)
    return decorator


# ============================================================================
# Runtime Support (used by generated wrapper)
# ============================================================================

def get_state_dict(instance: Any) -> Dict[str, Any]:
    """Extract state fields from an actor instance as a dict."""
    if not hasattr(instance, '_plexspaces_state_fields'):
        return {}
    
    state_dict = {}
    for name, field in instance._plexspaces_state_fields.items():
        state_dict[name] = getattr(instance, name)
    return state_dict


def set_state_dict(instance: Any, state_dict: Dict[str, Any]) -> None:
    """Restore state fields on an actor instance from a dict."""
    if not hasattr(instance, '_plexspaces_state_fields'):
        return
    
    for name in instance._plexspaces_state_fields:
        if name in state_dict:
            setattr(instance, name, state_dict[name])


def _has_float(obj: Any) -> bool:
    """Return True if obj (or nested dict/list) contains a float. Used to skip sanitize when safe."""
    if isinstance(obj, float):
        return True
    if isinstance(obj, dict):
        return any(_has_float(v) for v in obj.values())
    if isinstance(obj, list):
        return any(_has_float(v) for v in obj)
    return False


def payload_from_request_json(payload_json: str) -> Dict[str, Any]:
    """Parse request body JSON; on 'Extra data' (concatenated JSON), use only the first object.
    Avoids JSONDecodeError when the client or gateway sends multiple JSON values concatenated."""
    if not payload_json or not payload_json.strip():
        return {}
    try:
        return json.loads(payload_json)
    except json.JSONDecodeError as e:
        if "Extra data" in str(e):
            try:
                obj, _ = json.JSONDecoder().raw_decode(payload_json)
                return obj if isinstance(obj, dict) else {}
            except json.JSONDecodeError:
                raise e
        raise e


def _sanitize_payload_for_wasm(obj: Any) -> Any:
    """Recursively convert float to str in dicts/lists so WASM/componentize-py does not trap.
    Note: str(float) can trap in componentize-py WASM; clients should send numbers as strings.
    Creating new dicts/lists in sanitize can trigger WASM frame-cleanup trap on return; use
    _has_float() to skip sanitize when result has no floats."""
    if isinstance(obj, dict):
        return {k: _sanitize_payload_for_wasm(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_sanitize_payload_for_wasm(v) for v in obj]
    if isinstance(obj, float):
        return str(obj)
    return obj


def _desanitize_from_wasm(obj: Any) -> Any:
    """Recursively restore stringified numbers back to numeric types.
    Reverses _sanitize_payload_for_wasm: strings that look like numbers become int or float.
    Applied to state dicts loaded via set_state so arithmetic operations work correctly."""
    if isinstance(obj, dict):
        return {k: _desanitize_from_wasm(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_desanitize_from_wasm(v) for v in obj]
    if isinstance(obj, str):
        # Try int first (stricter), then float
        try:
            int_val = int(obj)
            # Only convert if the string is exactly the int representation
            # (avoids converting "1.0" to int 1)
            if str(int_val) == obj:
                return int_val
        except (ValueError, TypeError):
            pass
        try:
            return float(obj)
        except (ValueError, TypeError):
            pass
    return obj


def dispatch_message(instance: Any, from_actor: str, msg_type: str, payload: Dict[str, Any]) -> Any:
    """
    Dispatch a message to the appropriate handler.

    Ask vs Tell (GET vs POST):
    - GET requests use ask (request-reply): msg_type is "call", payload is query params as JSON.
      Use GET for read handlers (e.g. get_readings, count) so the client receives the reply.
    - POST/PUT/DELETE use tell (fire-and-forget): msg_type is "cast", payload is request body.
      Use POST for write handlers (e.g. ingest, clear); reply is not sent to the client.

    Payload key for operation (consistent across SDKs): canonical key is message_type; aliases op and msg_type.
    Resolved in order: message_type -> op -> msg_type. E.g. {"message_type": "workflow_run"} or {"op": "deposit", "amount": 100}.

    Returns the handler's return value (sent back to client for ask/GET), or an error dict if no handler found.
    """
    handlers = getattr(instance, '_plexspaces_handlers', {})
    effective_type = msg_type
    handler_payload = payload

    # Resolve operation from payload when envelope is cast/call or unknown: message_type (canonical) -> op -> msg_type
    if isinstance(payload, dict):
        if msg_type in ("cast", "call") or msg_type not in handlers:
            for key in ("message_type", "op", "msg_type"):
                val = payload.get(key)
                if not val or (key == "op" and val in ("call", "cast")):
                    continue
                effective_type = val
                handler_payload = payload.get("payload", payload) if isinstance(payload.get("payload"), dict) else payload
                break
        if payload.get("payload") and isinstance(payload.get("payload"), dict) and effective_type == msg_type and msg_type in ("cast", "call"):
            handler_payload = payload.get("payload")

    def _filter_kwargs(method: Callable, data: Dict[str, Any]) -> Dict[str, Any]:
        """Keep only keys that match the method's signature (excluding self)."""
        try:
            sig = inspect.signature(method)
            param_names = {
                n for n in sig.parameters
                if n != "self" and sig.parameters[n].kind
                in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY)
            }
            return {k: v for k, v in data.items() if k in param_names}
        except Exception:
            return data

    # Make from_actor available to handlers that declare a `from_actor` parameter.
    # This lets handlers log/use the sender identity for request/reply debugging.
    def _inject_from_actor(method: Callable, kwargs: Dict[str, Any]) -> Dict[str, Any]:
        if from_actor:
            try:
                sig = inspect.signature(method)
                if "from_actor" in sig.parameters and "from_actor" not in kwargs:
                    kwargs["from_actor"] = from_actor
            except Exception:
                pass
        return kwargs

    # Workflow behavior: route workflow_run / workflow_signal:name / workflow_query:name to run/signal/query (aligned with Rust Workflow trait)
    if getattr(instance, "_plexspaces_workflow", False):
        if effective_type == "workflow_run":
            run_fn = getattr(instance, "run", None)
            if callable(run_fn):
                result = run_fn(handler_payload if isinstance(handler_payload, dict) else payload)
                return result if result is not None else {}
            return {"error": "Workflow actor must implement run(payload)"}
        if effective_type.startswith("workflow_signal:"):
            signal_name = effective_type[len("workflow_signal:"):].strip()
            signal_fn = getattr(instance, "signal", None)
            if callable(signal_fn):
                signal_fn(signal_name, handler_payload if isinstance(handler_payload, dict) else payload)
                return {}
            return {"error": "Workflow actor must implement signal(name, data)"}
        if effective_type.startswith("workflow_query:"):
            query_name = effective_type[len("workflow_query:"):].strip()
            query_fn = getattr(instance, "query", None)
            if callable(query_fn):
                result = query_fn(query_name, handler_payload if isinstance(handler_payload, dict) else payload)
                return result if result is not None else {}
            return {"error": "Workflow actor must implement query(name, params)"}

    # Check for exact match
    if effective_type in handlers:
        handler_method = handlers[effective_type]
        kwargs = _filter_kwargs(handler_method, handler_payload)
        kwargs = _inject_from_actor(handler_method, kwargs)
        return handler_method(instance, **kwargs)
    if msg_type in handlers:
        handler_method = handlers[msg_type]
        kwargs = _filter_kwargs(handler_method, payload)
        kwargs = _inject_from_actor(handler_method, kwargs)
        return handler_method(instance, **kwargs)

    # Check for "call" or "get_state" which might return state
    if effective_type in ("call", "get_state") or msg_type in ("call", "get_state"):
        return get_state_dict(instance)

    return {"error": f"Unknown message type: {effective_type}"}


def init_actor(instance: Any, config: Dict[str, Any]) -> None:
    """
    Initialize an actor instance with config.
    
    If an @init_handler is defined, calls it with config.
    Otherwise, sets config values directly on matching state fields.
    """
    init_handler_method = getattr(instance, '_plexspaces_init_handler', None)
    
    if init_handler_method is not None:
        # The method is already bound to instance via getattr, so just pass config
        init_handler_method(config)
    else:
        # Set config values on state fields
        state_fields = getattr(instance, '_plexspaces_state_fields', {})
        for key, value in config.items():
            if key in state_fields:
                setattr(instance, key, value)
