// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM instance management (wasmtime Store and Instance wrapper)

use crate::{HostFunctions, WasmCapabilities, WasmError, WasmModule, WasmResult};
use plexspaces_core::ChannelService;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::{Caller, Engine, Instance, Linker, Store, StoreLimits, StoreLimitsBuilder};

/// WASM actor instance with state and execution context
pub struct WasmInstance {
    /// Actor ID
    actor_id: String,

    /// Wasmtime store (holds instance state and limits)
    store: Arc<RwLock<Store<InstanceContext>>>,

    /// Wasmtime instance (compiled module + imports)
    instance: Instance,

    /// Module metadata
    module: WasmModule,
}

/// Context data stored in wasmtime Store
pub struct InstanceContext {
    /// Actor ID
    pub actor_id: String,

    /// Host functions available to WASM
    pub host_functions: Arc<HostFunctions>,

    /// Capabilities (what WASM is allowed to do)
    pub capabilities: WasmCapabilities,

    /// Resource limits tracker
    pub limits: StoreLimits,
}

impl WasmInstance {
    /// Create new WASM instance from module
    ///
    /// ## Arguments
    /// * `engine` - wasmtime Engine (shared)
    /// * `module` - Compiled WASM module
    /// * `actor_id` - Unique actor identifier
    /// * `initial_state` - Initial state bytes (empty for new actors)
    /// * `capabilities` - Capabilities for this instance
    /// * `limits` - Resource limits for this instance
    ///
    /// ## Returns
    /// New WasmInstance ready to execute
    ///
    /// ## Errors
    /// Returns error if instantiation fails
    pub async fn new(
        engine: &Engine,
        module: WasmModule,
        actor_id: String,
        initial_state: &[u8],
        capabilities: WasmCapabilities,
        limits: StoreLimits,
        channel_service: Option<Arc<dyn ChannelService>>,
    ) -> WasmResult<Self> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_instance_creation_attempts_total").increment(1);

        // Create host functions with optional channel service
        let host_functions = if let Some(channel_service) = channel_service {
            HostFunctions::with_channel_service(channel_service)
        } else {
            HostFunctions::new()
        };

        // Create context
        let context = InstanceContext {
            actor_id: actor_id.clone(),
            host_functions: Arc::new(host_functions),
            capabilities,
            limits,
        };

        // Create store with context
        let mut store = Store::new(engine, context);

        // Add fuel for execution (required when fuel metering is enabled)
        // Default to 1 million fuel units (enough for most operations)
        let fuel_set = store.set_fuel(1_000_000);
        if fuel_set.is_ok() {
            metrics::counter!("plexspaces_wasm_fuel_metering_enabled_total").increment(1);
        } else {
            // Fuel metering not enabled, ignore error
            metrics::counter!("plexspaces_wasm_fuel_metering_disabled_total").increment(1);
        }

        // Try to re-enable limiter (wasmtime async support may have improved)
        // This sets memory limits from StoreLimits
        // Note: limiter() sets the limiter and returns nothing (unit type)
        // We attempt to set it - if it works, memory limits are enforced
        store.limiter(|ctx| &mut ctx.limits);
        // Track that we attempted to set limiter (limiter may or may not work with async)
        metrics::counter!("plexspaces_wasm_memory_limiter_attempted_total").increment(1);

        // Create linker with host functions
        let mut linker = Linker::new(engine);
        Self::add_host_functions(&mut linker)?;

        // Instantiate module or component (async because runtime has async_support enabled)
        let instantiate_start = std::time::Instant::now();
        let instance = {
            #[cfg(feature = "component-model")]
            {
                use crate::runtime::WasmModuleInner;
                match &module.module {
                    WasmModuleInner::Module(m) => {
                        linker
                            .instantiate_async(&mut store, m)
                            .await
                            .map_err(|e| {
                                metrics::counter!("plexspaces_wasm_instance_creation_errors_total").increment(1);
                                WasmError::InstantiationError(e.to_string())
                            })?
                    }
                    WasmModuleInner::Component(c) => {
                        // For components, use ComponentLinker
                        use wasmtime::component::Linker as ComponentLinker;
                        let mut component_linker = ComponentLinker::new(engine);
                        
                        // Note: Components from componentize-py typically need WIT bindings
                        // For now, try to instantiate without bindings (may work for simple components)
                        // This is a basic implementation - full support requires WIT interface generation
                        let _component_instance = component_linker
                            .instantiate_async(&mut store, c)
                            .await
                            .map_err(|e| {
                                metrics::counter!("plexspaces_wasm_component_instantiation_errors_total").increment(1);
                                WasmError::InstantiationError(format!(
                                    "Component instantiation failed: {}. \
                                    Components from componentize-py may require WIT interface bindings. \
                                    Error details: {}",
                                    e, e
                                ))
                            })?;
                        
                        // Components use a different instance type - we need to adapt it
                        // For now, return an error explaining the limitation
                        // TODO: Implement full component instance wrapper that works with WasmInstance
                        return Err(WasmError::InstantiationError(
                            "WASM component instantiation is partially implemented. \
                            Components can be loaded but full instantiation requires WIT interface bindings and \
                            a component-specific instance wrapper. Components from componentize-py need the \
                            component's WIT interfaces to be bound. For now, use traditional WASM modules \
                            (Rust, Go, JavaScript) or wait for full component support."
                                .to_string(),
                        ));
                    }
                }
            }
            #[cfg(not(feature = "component-model"))]
            {
                linker
                    .instantiate_async(&mut store, &module.module)
                    .await
                    .map_err(|e| {
                        metrics::counter!("plexspaces_wasm_instance_creation_errors_total").increment(1);
                        WasmError::InstantiationError(e.to_string())
                    })?
            }
        };
        let instantiate_duration = instantiate_start.elapsed();

        // Call init() function with initial state if provided
        if !initial_state.is_empty() {
            use crate::memory::write_bytes;

            // Get memory instance
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

            // Write initial state to WASM memory at offset 0
            write_bytes(&memory, &mut store, 0, initial_state)?;

            // Get init function
            let init_func = instance
                .get_typed_func::<(i32, i32), i32>(&mut store, "init")
                .map_err(|e| WasmError::ActorFunctionError(format!("init not exported: {}", e)))?;

            // Call init(state_ptr=0, state_len=initial_state.len())
            let result = init_func
                .call_async(&mut store, (0, initial_state.len() as i32))
                .await
                .map_err(|e| WasmError::ActorFunctionError(format!("init failed: {}", e)))?;

            if result != 0 {
                return Err(WasmError::ActorFunctionError(format!(
                    "init returned error code: {}",
                    result
                )));
            }
        }

        let total_duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_instance_creation_duration_seconds").record(total_duration.as_secs_f64());
        metrics::histogram!("plexspaces_wasm_instance_instantiate_duration_seconds").record(instantiate_duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_instance_creation_success_total").increment(1);
        
        // Track active instances (increment)
        metrics::gauge!("plexspaces_wasm_active_instances").increment(1.0);
        
        // Track memory limits (StoreLimits enforces limits, we just track that limits are set)
        // Memory limits are enforced by wasmtime via the limiter
        // StoreLimits doesn't expose a method to check if limits are set, so we always increment
        metrics::counter!("plexspaces_wasm_memory_limits_set_total").increment(1);

        Ok(WasmInstance {
            actor_id,
            store: Arc::new(RwLock::new(store)),
            instance,
            module,
        })
    }

    /// Add host functions to linker
    fn add_host_functions(linker: &mut Linker<InstanceContext>) -> WasmResult<()> {
        // Add logging function
        linker
            .func_wrap(
                "plexspaces",
                "log",
                |mut caller: Caller<'_, InstanceContext>, ptr: i32, len: i32| {
                    // Get memory from caller
                    match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(memory)) => {
                            // Read string from WASM memory
                            if ptr < 0 || len < 0 {
                                eprintln!("[WASM] log error: invalid pointer or length");
                                return;
                            }

                            let ptr = ptr as usize;
                            let len = len as usize;
                            let data = memory.data(&caller);

                            if ptr + len > data.len() {
                                eprintln!("[WASM] log error: out of bounds access");
                                return;
                            }

                            match std::str::from_utf8(&data[ptr..ptr + len]) {
                                Ok(message) => {
                                    // Log with actor context
                                    let actor_id = &caller.data().actor_id;
                                    println!("[WASM:{}] {}", actor_id, message);
                                }
                                Err(e) => {
                                    eprintln!("[WASM] log error: invalid UTF-8: {}", e);
                                }
                            }
                        }
                        _ => {
                            eprintln!("[WASM] log error: memory not exported");
                        }
                    }
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        // Add send_message function
        linker
            .func_wrap(
                "plexspaces",
                "send_message",
                |mut caller: Caller<'_, InstanceContext>,
                 to_ptr: i32,
                 to_len: i32,
                 msg_ptr: i32,
                 msg_len: i32| {
                    // Get memory from caller
                    match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(memory)) => {
                            // Validate pointers
                            if to_ptr < 0 || to_len < 0 || msg_ptr < 0 || msg_len < 0 {
                                eprintln!("[WASM] send_message error: invalid pointer or length");
                                return -1i32;
                            }

                            let to_ptr = to_ptr as usize;
                            let to_len = to_len as usize;
                            let msg_ptr = msg_ptr as usize;
                            let msg_len = msg_len as usize;
                            let data = memory.data(&caller);

                            // Check bounds
                            if to_ptr + to_len > data.len() || msg_ptr + msg_len > data.len() {
                                eprintln!("[WASM] send_message error: out of bounds access");
                                return -1i32;
                            }

                            // Read strings
                            match (
                                std::str::from_utf8(&data[to_ptr..to_ptr + to_len]),
                                std::str::from_utf8(&data[msg_ptr..msg_ptr + msg_len]),
                            ) {
                                (Ok(to_actor), Ok(message)) => {
                                    let from_actor = caller.data().actor_id.clone();
                                    let host_functions = Arc::clone(&caller.data().host_functions);
                                    let to_actor = to_actor.to_string();
                                    let message = message.to_string();
                                    
                                    // Spawn async task to send message (host function is sync)
                                    tokio::spawn(async move {
                                        if let Err(e) = host_functions.send_message(&from_actor, &to_actor, &message).await {
                                            tracing::error!(
                                                from = %from_actor,
                                                to = %to_actor,
                                                error = %e,
                                                "Failed to send message from WASM actor"
                                            );
                                        } else {
                                            tracing::debug!(
                                                from = %from_actor,
                                                to = %to_actor,
                                                "Message sent successfully from WASM actor"
                                            );
                                        }
                                    });
                                    
                                    0i32 // Success (message sent asynchronously)
                                }
                                (Err(e), _) | (_, Err(e)) => {
                                    eprintln!("[WASM] send_message error: invalid UTF-8: {}", e);
                                    -1i32 // Error
                                }
                            }
                        }
                        _ => {
                            eprintln!("[WASM] send_message error: memory not exported");
                            -1i32 // Error
                        }
                    }
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        // Add send_to_queue function
        linker
            .func_wrap(
                "plexspaces",
                "send_to_queue",
                |mut caller: Caller<'_, InstanceContext>,
                 queue_name_ptr: i32,
                 queue_name_len: i32,
                 msg_type_ptr: i32,
                 msg_type_len: i32,
                 payload_ptr: i32,
                 payload_len: i32| -> i32 {
                    match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(memory)) => {
                            // Validate pointers
                            if queue_name_ptr < 0 || queue_name_len < 0 || msg_type_ptr < 0 || msg_type_len < 0 || payload_ptr < 0 || payload_len < 0 {
                                eprintln!("[WASM] send_to_queue error: invalid pointer or length");
                                return -1i32;
                            }

                            let data = memory.data(&caller);
                            let queue_name_bytes = &data[queue_name_ptr as usize..(queue_name_ptr + queue_name_len) as usize];
                            let msg_type_bytes = &data[msg_type_ptr as usize..(msg_type_ptr + msg_type_len) as usize];
                            let payload_bytes = &data[payload_ptr as usize..(payload_ptr + payload_len) as usize];

                            match (
                                std::str::from_utf8(queue_name_bytes),
                                std::str::from_utf8(msg_type_bytes),
                            ) {
                                (Ok(queue_name), Ok(msg_type)) => {
                                    let host_functions = Arc::clone(&caller.data().host_functions);
                                    let queue_name = queue_name.to_string();
                                    let msg_type = msg_type.to_string();
                                    let payload = payload_bytes.to_vec();
                                    
                                    // Spawn async task to send to queue
                                    tokio::spawn(async move {
                                        match host_functions.send_to_queue(&queue_name, &msg_type, payload).await {
                                            Ok(_msg_id) => {
                                                tracing::debug!(
                                                    queue = %queue_name,
                                                    "Message sent to queue from WASM actor"
                                                );
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    queue = %queue_name,
                                                    error = %e,
                                                    "Failed to send message to queue from WASM actor"
                                                );
                                            }
                                        }
                                    });
                                    
                                    0i32 // Success
                                }
                                _ => {
                                    eprintln!("[WASM] send_to_queue error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            eprintln!("[WASM] send_to_queue error: memory not exported");
                            -1i32
                        }
                    }
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        // Add publish_to_topic function
        linker
            .func_wrap(
                "plexspaces",
                "publish_to_topic",
                |mut caller: Caller<'_, InstanceContext>,
                 topic_name_ptr: i32,
                 topic_name_len: i32,
                 msg_type_ptr: i32,
                 msg_type_len: i32,
                 payload_ptr: i32,
                 payload_len: i32| -> i32 {
                    match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(memory)) => {
                            // Validate pointers
                            if topic_name_ptr < 0 || topic_name_len < 0 || msg_type_ptr < 0 || msg_type_len < 0 || payload_ptr < 0 || payload_len < 0 {
                                eprintln!("[WASM] publish_to_topic error: invalid pointer or length");
                                return -1i32;
                            }

                            let data = memory.data(&caller);
                            let topic_name_bytes = &data[topic_name_ptr as usize..(topic_name_ptr + topic_name_len) as usize];
                            let msg_type_bytes = &data[msg_type_ptr as usize..(msg_type_ptr + msg_type_len) as usize];
                            let payload_bytes = &data[payload_ptr as usize..(payload_ptr + payload_len) as usize];

                            match (
                                std::str::from_utf8(topic_name_bytes),
                                std::str::from_utf8(msg_type_bytes),
                            ) {
                                (Ok(topic_name), Ok(msg_type)) => {
                                    let host_functions = Arc::clone(&caller.data().host_functions);
                                    let topic_name = topic_name.to_string();
                                    let msg_type = msg_type.to_string();
                                    let payload = payload_bytes.to_vec();
                                    
                                    // Spawn async task to publish to topic
                                    tokio::spawn(async move {
                                        match host_functions.publish_to_topic(&topic_name, &msg_type, payload).await {
                                            Ok(_msg_id) => {
                                                tracing::debug!(
                                                    topic = %topic_name,
                                                    "Message published to topic from WASM actor"
                                                );
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    topic = %topic_name,
                                                    error = %e,
                                                    "Failed to publish message to topic from WASM actor"
                                                );
                                            }
                                        }
                                    });
                                    
                                    0i32 // Success
                                }
                                _ => {
                                    eprintln!("[WASM] publish_to_topic error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            eprintln!("[WASM] publish_to_topic error: memory not exported");
                            -1i32
                        }
                    }
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        // Note: receive_from_queue is more complex as it needs to block and return data
        // For now, we'll implement a simplified version that returns immediately
        // Full implementation would require async host functions (wasmtime async support)
        linker
            .func_wrap(
                "plexspaces",
                "receive_from_queue",
                |_caller: Caller<'_, InstanceContext>,
                 _queue_name_ptr: i32,
                 _queue_name_len: i32,
                 _timeout_ms: u64| -> i32 {
                    // For now, return -1 (not implemented)
                    // Full implementation requires async host functions
                    eprintln!("[WASM] receive_from_queue: async host functions not yet implemented");
                    -1i32
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        Ok(())
    }

    /// Call actor's behavior-specific handler or fallback to handle_message
    ///
    /// ## Arguments
    /// * `from` - Sender actor ID
    /// * `message_type` - Message type (e.g., "call", "cast", "info")
    /// * `payload` - Message payload bytes
    ///
    /// ## Returns
    /// Response bytes from actor
    ///
    /// ## Errors
    /// Returns error if function call fails or exceeds resource limits
    ///
    /// ## Behavior Routing
    /// Routes messages to behavior-specific handlers:
    /// - "call" → `handle_request()` (GenServer, expects response)
    /// - "cast" or "info" → `handle_event()` (GenEvent, no response)
    /// - Any → `handle_transition()` (GenFSM, returns new state)
    /// - Fallback → `handle_message()` (generic, backward compatibility)
    pub async fn handle_message(
        &self,
        from: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> WasmResult<Vec<u8>> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_message_handled_total").increment(1);

        use crate::memory::{read_bytes, write_bytes};

        let mut store = self.store.write().await;

        // Track fuel consumption before execution
        let fuel_before = store.get_fuel().unwrap_or(0);
        
        // Get memory instance
        let memory = self
            .instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

        // Allocate space in WASM memory for our data
        // Memory layout: [from_bytes][message_type_bytes][payload_bytes]
        // Start at offset 0 for simplicity (real implementation would use allocator)
        let from_bytes = from.as_bytes();
        let msg_type_bytes = message_type.as_bytes();

        let from_ptr = 0i32;
        let from_len = from_bytes.len() as i32;

        let msg_type_ptr = from_len;
        let msg_type_len = msg_type_bytes.len() as i32;

        let payload_ptr = from_len + msg_type_len;
        let payload_len = payload.len() as i32;

        // Write data to WASM memory
        write_bytes(&memory, &mut *store, from_ptr, from_bytes)?;
        write_bytes(&memory, &mut *store, msg_type_ptr, msg_type_bytes)?;
        write_bytes(&memory, &mut *store, payload_ptr, &payload)?;

        // Try behavior-specific handlers first
        // 1. Try handle_request for "call" messages (GenServer)
        if message_type == "call" {
            if let Ok(handle_request_func) = self
                .instance
                .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_request")
            {
                let result_ptr = handle_request_func
                    .call_async(
                        &mut *store,
                        (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
                    )
                    .await
                    .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

                // Read response from memory (if result_ptr != 0)
                if result_ptr != 0 {
                    use crate::memory::read_bytes;
                    match read_bytes(&memory, &mut *store, result_ptr, 4) {
                        Ok(bytes) => return Ok(bytes),
                        Err(_) => return Ok(vec![]),
                    }
                } else {
                    return Ok(vec![]);
                }
            }
        }

        // 2. Try handle_event for "cast" or "info" messages (GenEvent)
        if message_type == "cast" || message_type == "info" {
            if let Ok(handle_event_func) = self
                .instance
                .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_event")
            {
                let result = handle_event_func
                    .call_async(
                        &mut *store,
                        (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
                    )
                    .await
                    .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

                // GenEvent doesn't return a response (result is error code: 0 = success, != 0 = error)
                if result == 0 {
                    return Ok(vec![]);
                } else {
                    return Err(WasmError::ActorFunctionError(format!(
                        "handle_event returned error code: {}",
                        result
                    )));
                }
            }
        }

        // 3. Try handle_transition for state machine (GenFSM)
        if let Ok(handle_transition_func) = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_transition")
        {
            let result_ptr = handle_transition_func
                .call_async(
                    &mut *store,
                    (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
                )
                .await
                .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

            // Read new state name from memory (if result_ptr != 0)
            if result_ptr != 0 {
                use crate::memory::read_bytes;
                // Try to read state name (assume max 64 bytes for state name)
                match read_bytes(&memory, &mut *store, result_ptr, 64) {
                    Ok(bytes) => {
                        // Find null terminator or use full length
                        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                        return Ok(bytes[..len].to_vec());
                    }
                    Err(_) => return Ok(vec![]),
                }
            } else {
                return Ok(vec![]);
            }
        }

        // 4. Fallback to generic handle_message (backward compatibility)
        let handle_message_func = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_message")
            .map_err(|e| {
                WasmError::ActorFunctionError(format!("handle_message not exported: {}", e))
            })?;

        // Call WASM function with pointers and lengths
        let result_ptr = handle_message_func
            .call_async(
                &mut *store,
                (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
            )
            .await
            .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

        // Read response from memory if result_ptr != 0
        let result = if result_ptr == 0 {
            Ok(vec![])
        } else {
            // Read 4 bytes (i32) from memory at result_ptr
            // This is the counter value for our demo module
            use crate::memory::read_bytes;
            match read_bytes(&memory, &mut *store, result_ptr, 4) {
                Ok(bytes) => Ok(bytes),
                Err(_) => {
                    // If reading fails, return empty (backward compatibility)
                    Ok(vec![])
                }
            }
        };

        // Track fuel consumption after execution
        let fuel_after = store.get_fuel().unwrap_or(0);
        let fuel_consumed = if fuel_before > fuel_after {
            fuel_before - fuel_after
        } else {
            0
        };

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_message_handle_duration_seconds").record(duration.as_secs_f64());
        if fuel_consumed > 0 {
            metrics::histogram!("plexspaces_wasm_fuel_consumed").record(fuel_consumed as f64);
        }

        // Track memory usage
        // Note: memory.size() requires AsContext, but we have RwLockWriteGuard
        // Memory tracking can be done via periodic snapshots if needed
        // For now, skip memory size tracking in handle_message

        if result.is_ok() {
            metrics::counter!("plexspaces_wasm_message_handled_success_total").increment(1);
        } else {
            metrics::counter!("plexspaces_wasm_message_handled_errors_total").increment(1);
        }

        result
    }

    /// Snapshot current actor state
    ///
    /// ## Returns
    /// State bytes that can be persisted or migrated
    ///
    /// ## Errors
    /// Returns error if snapshot function not exported
    ///
    /// ## Implementation Notes
    /// Tries two approaches:
    /// 1. Multi-value return: snapshot_state() -> (i32, i32)
    /// 2. i64 return: snapshot_state() -> i64 (packed as ptr | (len << 32))
    pub async fn snapshot_state(&self) -> WasmResult<Vec<u8>> {
        use crate::memory::read_bytes;

        let mut store = self.store.write().await;

        // Get memory instance
        let memory = self
            .instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

        // Try multi-value return first (WAT-based modules)
        if let Ok(snapshot_func) = self
            .instance
            .get_typed_func::<(), (i32, i32)>(&mut *store, "snapshot_state")
        {
            // Call snapshot function (returns ptr, len)
            let (ptr, len) = snapshot_func
                .call_async(&mut *store, ())
                .await
                .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

            // If ptr is 0 or len is 0, return empty state
            if ptr == 0 || len == 0 {
                return Ok(vec![]);
            } else {
                // Read state from memory
                return read_bytes(&memory, &mut *store, ptr, len);
            }
        }

        // Fallback: Try i64 return (Rust-compiled WASM modules)
        if let Ok(snapshot_func) = self
            .instance
            .get_typed_func::<(), i64>(&mut *store, "snapshot_state")
        {
            // Call snapshot function (returns packed ptr|len)
            let packed = snapshot_func
                .call_async(&mut *store, ())
                .await
                .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

            // Unpack: lower 32 bits = ptr, upper 32 bits = len
            let ptr = (packed & 0xFFFFFFFF) as i32;
            let len = ((packed >> 32) & 0xFFFFFFFF) as i32;

            // If ptr is 0 or len is 0, return empty state
            if ptr == 0 || len == 0 {
                return Ok(vec![]);
            } else {
                // Read state from memory
                return read_bytes(&memory, &mut *store, ptr, len);
            }
        }

        Err(WasmError::ActorFunctionError(
            "snapshot_state not exported (tried both (i32,i32) and i64 signatures)".to_string(),
        ))
    }

    /// Get actor ID
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Get module info
    pub fn module(&self) -> &WasmModule {
        &self.module
    }

    /// Call get_supervisor_tree() function from WASM module
    ///
    /// ## Purpose
    /// Calls the exported `get_supervisor_tree()` function to retrieve the supervisor
    /// tree definition as protobuf-encoded SupervisorSpec.
    ///
    /// ## Function Signature
    /// `get_supervisor_tree() -> (ptr: i32, len: i32)`
    /// - Returns pointer and length to protobuf-encoded SupervisorSpec in WASM memory
    ///
    /// ## Returns
    /// Protobuf bytes for SupervisorSpec
    ///
    /// ## Errors
    /// Returns error if function is not exported or call fails
    pub async fn get_supervisor_tree(&self) -> WasmResult<Vec<u8>> {
        use crate::memory::read_bytes;

        let mut store = self.store.write().await;

        // Get memory instance
        let memory = self
            .instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

        // Try to get the get_supervisor_tree function
        // Function signature: () -> (i32, i32) (ptr, len)
        if let Ok(get_tree_func) = self
            .instance
            .get_typed_func::<(), (i32, i32)>(&mut *store, "get_supervisor_tree")
        {
            // Call the function
            let (ptr, len) = get_tree_func
                .call_async(&mut *store, ())
                .await
                .map_err(|e| WasmError::ActorFunctionError(format!("Failed to call get_supervisor_tree: {}", e)))?;

            // If ptr is 0 or len is 0, return empty (no supervisor tree)
            if ptr == 0 || len == 0 {
                return Ok(vec![]);
            }

            // Read protobuf bytes from WASM memory
            return read_bytes(&memory, &mut *store, ptr, len);
        }

        // Function not exported
        Err(WasmError::ActorFunctionError(
            "get_supervisor_tree function not exported".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasmRuntime;

    #[tokio::test]
    async fn test_instance_creation() {
        // Create minimal WASM module (WebAssembly Text format)
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                    i32.const 0
                )
                (func (export "snapshot_state") (result i32 i32)
                    i32.const 0
                    i32.const 0
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let module = runtime
            .load_module("test-actor", "1.0.0", &wasm_bytes)
            .await
            .expect("Failed to load module");

        let limits = StoreLimitsBuilder::new()
            .memory_size(16 * 1024 * 1024) // 16MB
            .build();

        let instance = WasmInstance::new(
            runtime.engine(),
            module,
            "actor-001".to_string(),
            &[],
            crate::capabilities::profiles::default(),
            limits,
            None, // ChannelService not available in tests
        )
        .await
        .expect("Failed to create instance");

        assert_eq!(instance.actor_id(), "actor-001");
    }

    #[tokio::test]
    async fn test_simple_function_call() {
        // Test that we can call a simple WASM function that just returns a constant
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "add_one") (param i32) (result i32)
                    local.get 0
                    i32.const 1
                    i32.add
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).unwrap();
        let runtime = WasmRuntime::new().await.unwrap();
        let module = runtime.load_module("test", "1.0.0", &wasm_bytes).await.unwrap();

        let limits = StoreLimitsBuilder::new()
            .memory_size(16 * 1024 * 1024)
            .build();

        let instance = WasmInstance::new(
            runtime.engine(),
            module,
            "test-simple".to_string(),
            &[],
            crate::capabilities::profiles::default(),
            limits,
            None, // ChannelService not available in tests
        )
        .await
        .unwrap();

        // Call the function directly to test basic WASM execution
        let mut store = instance.store.write().await;
        let add_one_func = instance
            .instance
            .get_typed_func::<i32, i32>(&mut *store, "add_one")
            .unwrap();

        let result = add_one_func.call_async(&mut *store, 41).await;
        assert!(result.is_ok(), "Simple function call should work: {:?}", result.err());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_handle_message() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                    i32.const 0  ;; Return 0 = no response
                )
                (func (export "snapshot_state") (result i32 i32)
                    i32.const 0
                    i32.const 0
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).unwrap();
        let runtime = WasmRuntime::new().await.unwrap();
        let module = runtime.load_module("test", "1.0.0", &wasm_bytes).await.unwrap();

        let limits = StoreLimitsBuilder::new()
            .memory_size(16 * 1024 * 1024)
            .build();

        let instance = WasmInstance::new(
            runtime.engine(),
            module,
            "actor-test".to_string(),
            &[],
            crate::capabilities::profiles::default(),
            limits,
            None, // ChannelService not available in tests
        )
        .await
        .unwrap();

        let result = instance
            .handle_message("sender", "test", vec![1, 2, 3])
            .await
            .unwrap();

        // Returns empty vec when result_ptr == 0 (no response)
        assert_eq!(result, Vec::<u8>::new());
    }

    #[tokio::test]
    async fn test_snapshot_state() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                    i32.const 0
                )
                (func (export "snapshot_state") (result i32 i32)
                    i32.const 100  ;; ptr
                    i32.const 50   ;; len
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).unwrap();
        let runtime = WasmRuntime::new().await.unwrap();
        let module = runtime.load_module("test", "1.0.0", &wasm_bytes).await.unwrap();

        let limits = StoreLimitsBuilder::new()
            .memory_size(16 * 1024 * 1024)
            .build();

        let instance = WasmInstance::new(
            runtime.engine(),
            module,
            "actor-state".to_string(),
            &[],
            crate::capabilities::profiles::default(),
            limits,
            None, // ChannelService not available in tests
        )
        .await
        .unwrap();

        let state = instance.snapshot_state().await.unwrap();

        // Now reads actual memory - WASM returns ptr=100, len=50, so we get 50 bytes
        // Memory is zero-initialized, so we get 50 zeros
        assert_eq!(state.len(), 50);
        assert_eq!(state, vec![0u8; 50]);
    }
}
