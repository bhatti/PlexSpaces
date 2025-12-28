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

#[cfg(feature = "component-model")]
use wasmtime::component::Linker as ComponentLinker;

/// Component context for WASM components (includes WASI and PlexSpaces host implementations)
/// This context is not Send, so components cannot be pooled
#[cfg(feature = "component-model")]
pub struct ComponentContext {
    pub instance_ctx: InstanceContext,
    pub wasi_ctx: wasmtime_wasi::WasiCtx,
    pub resource_table: wasmtime_wasi::ResourceTable,
    pub plexspaces_host: crate::component_host::PlexspacesHost,
    pub logging_impl: crate::component_host::LoggingImpl,
    pub messaging_impl: crate::component_host::MessagingImpl,
    pub tuplespace_impl: crate::component_host::TuplespaceImpl,
    pub channels_impl: crate::component_host::ChannelsImpl,
    pub durability_impl: crate::component_host::DurabilityImpl,
    pub workflow_impl: crate::component_host::WorkflowImpl,
    pub blob_impl: crate::component_host::BlobImpl,
    pub keyvalue_impl: crate::component_host::KeyValueImpl,
    pub process_groups_impl: crate::component_host::ProcessGroupsImpl,
    pub locks_impl: crate::component_host::LocksImpl,
    pub registry_impl: crate::component_host::RegistryImpl,
}

#[cfg(feature = "component-model")]
impl wasmtime_wasi::WasiView for ComponentContext {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.resource_table
    }

    fn ctx(&mut self) -> &mut wasmtime_wasi::WasiCtx {
        &mut self.wasi_ctx
    }
}

/// WASM actor instance with state and execution context
pub struct WasmInstance {
    /// Actor ID
    pub(crate) actor_id: String,

    /// Wasmtime store (holds instance state and limits)
    store: Arc<RwLock<Store<InstanceContext>>>,

    /// Wasmtime instance (for traditional modules)
    instance: Instance,

    /// Component instance (for WASM components, None for traditional modules)
    #[cfg(feature = "component-model")]
    pub(crate) component_instance: Option<wasmtime::component::Instance>,

    /// Component store (for WASM components, None for traditional modules)
    #[cfg(feature = "component-model")]
    component_store: Option<Arc<RwLock<Store<ComponentContext>>>>,

    /// Module metadata
    module: WasmModule,
}

/// Context data stored in wasmtime Store
#[derive(Clone)]
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

// Note: For components requiring WASI, we use a separate ComponentContext that includes WasiCtx.
// Components are not pooled (created fresh each time) because WasiCtx is not Send.
// This is acceptable because:
// 1. Components are typically larger and less frequently instantiated
// 2. The performance cost is minimal compared to the complexity of making WasiCtx Send
// 3. We can still cache compiled components (just not instances)

// SAFETY: WasmInstance is Send + Sync because:
// 1. All Store access is through Arc<RwLock<...>> which provides synchronization
// 2. component_store is wrapped in Arc<RwLock<Store<ComponentContext>>> which ensures
//    thread-safe access even though ComponentContext contains non-Sync WasiCtx
// 3. All methods that access the store acquire the RwLock before accessing
// 4. Instance and component_instance are Send + Sync types from wasmtime
// 5. Module metadata (WasmModule) is Send + Sync
// 6. actor_id is String which is Send + Sync
//
// This is safe because we never access the store concurrently without proper locking.
// The RwLock ensures that only one thread can access the store at a time, making
// the overall WasmInstance safe to share across threads.
//
// Note: Even though ComponentContext contains WasiCtx which is not Sync, the Arc<RwLock<...>>
// wrapper provides the necessary synchronization. The RwLock serializes all access to the
// store, ensuring thread safety.
//
// Send: All fields are Send (Arc, RwLock, Instance, String are all Send)
// Sync: All fields are Sync or protected by synchronization primitives (Arc<RwLock<...>>)
unsafe impl Send for WasmInstance {}
unsafe impl Sync for WasmInstance {}

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
        message_sender: Option<Arc<dyn crate::MessageSender>>,
        tuplespace_provider: Option<Arc<dyn plexspaces_core::TupleSpaceProvider>>,
        keyvalue_store: Option<Arc<dyn plexspaces_keyvalue::KeyValueStore>>,
        process_group_registry: Option<Arc<plexspaces_process_groups::ProcessGroupRegistry>>,
        lock_manager: Option<Arc<dyn plexspaces_locks::LockManager>>,
        object_registry: Option<Arc<plexspaces_object_registry::ObjectRegistry>>,
        journal_storage: Option<Arc<dyn plexspaces_journaling::JournalStorage>>,
        
        blob_service: Option<Arc<plexspaces_blob::BlobService>>,
    ) -> WasmResult<Self> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_instance_creation_attempts_total").increment(1);

        // Clone values before they're moved (needed for component path and dummy instance)
        let capabilities_clone1 = capabilities.clone();
        let limits_clone1 = limits.clone();
        let capabilities_clone2 = capabilities.clone();
        let limits_clone2 = limits.clone();

        // Create host functions with all available services
        let host_functions = HostFunctions::with_all_services(
            message_sender,
            channel_service,
            keyvalue_store,
            process_group_registry,
            lock_manager,
            object_registry,
            journal_storage,
            blob_service,
        );

        // Store host_functions in Arc for sharing between traditional and component contexts
        let host_functions_arc = Arc::new(host_functions);

        // Create context
        let context = InstanceContext {
            actor_id: actor_id.clone(),
            host_functions: host_functions_arc.clone(),
            capabilities,
            limits,
        };

        // Create linker with host functions
        let mut linker = Linker::new(engine);
        Self::add_host_functions(&mut linker)?;

        // Instantiate module or component (async because runtime has async_support enabled)
        let instantiate_start = std::time::Instant::now();
        
        // We need to create the store before the match so it's available after
        // For components, we'll create a separate store, but for traditional modules we use this one
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
                        // For components, use ComponentLinker with WASI preview 2 bindings
                        // Components are not pooled (created fresh each time) because WasiCtx is not Send
                        use wasmtime::component::Linker as ComponentLinker;
                        let mut component_linker = ComponentLinker::new(engine);
                        
                        // Use the module-level ComponentContext struct
                        // Note: WasiView is already implemented for ComponentContext above (line 39)
                        // All WASI bindings are added automatically via add_to_linker_async below.
                        
                        // Create component context with WASI
                        // We need to reconstruct InstanceContext from the cloned values
                        // since context was moved into store
                        // Reuse the same host_functions Arc for component context
                        // This ensures message_sender and channel_service are available
                        let context_clone = InstanceContext {
                            actor_id: actor_id.clone(),
                            host_functions: host_functions_arc.clone(),
                            capabilities: capabilities_clone1,
                            limits: limits_clone1,
                        };
                        let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
                            .inherit_stdio()
                            .inherit_env()
                            .build();
                        
                        let plexspaces_host = crate::component_host::PlexspacesHost::new(
                            actor_id.clone(),
                            context_clone.host_functions.clone(),
                        );
                        // Use provided TupleSpaceProvider or None
                        let tuplespace_provider = tuplespace_provider.clone();
                        
                        let component_ctx = ComponentContext {
                            instance_ctx: context_clone.clone(),
                            wasi_ctx,
                            resource_table: wasmtime_wasi::ResourceTable::new(),
                            plexspaces_host,
                            logging_impl: crate::component_host::LoggingImpl {
                                actor_id: actor_id.clone(),
                            },
                            messaging_impl: crate::component_host::MessagingImpl::new(
                                actor_id.clone(),
                                context_clone.host_functions.clone(),
                            ),
                            tuplespace_impl: crate::component_host::TuplespaceImpl::new(
                                tuplespace_provider,
                                actor_id.clone(),
                            ),
                    channels_impl: crate::component_host::ChannelsImpl::new(context_clone.host_functions.clone()),
                    durability_impl: crate::component_host::DurabilityImpl::new(actor_id.clone(), context_clone.host_functions.clone()),
                            workflow_impl: crate::component_host::WorkflowImpl,
                            blob_impl: crate::component_host::BlobImpl {
                                actor_id: actor_id.clone(),
                                host_functions: context_clone.host_functions.clone(),
                            },
                            keyvalue_impl: crate::component_host::KeyValueImpl {
                                actor_id: actor_id.clone(),
                                host_functions: context_clone.host_functions.clone(),
                            },
                            process_groups_impl: crate::component_host::ProcessGroupsImpl {
                                actor_id: actor_id.clone(),
                                host_functions: context_clone.host_functions.clone(),
                            },
                            locks_impl: crate::component_host::LocksImpl {
                                actor_id: actor_id.clone(),
                                host_functions: context_clone.host_functions.clone(),
                            },
                            registry_impl: crate::component_host::RegistryImpl {
                                actor_id: actor_id.clone(),
                                host_functions: context_clone.host_functions.clone(),
                            },
                        };
                        
                        // Create a new store for the component (not pooled, not Send)
                        let mut component_store = Store::new(engine, component_ctx);
                        
                        // Add fuel and limiter to component store
                        let _ = component_store.set_fuel(1_000_000);
                        component_store.limiter(|ctx| &mut ctx.instance_ctx.limits);
                        
                        // Add ALL WASI preview 2 bindings using the recommended approach
                        // This automatically adds wasi:cli/*, wasi:io/*, and all other required interfaces
                        // ComponentContext implements WasiView, so this works seamlessly
                        wasmtime_wasi::add_to_linker_async(&mut component_linker)
                            .map_err(|e| WasmError::InstantiationError(format!(
                                "Failed to add WASI bindings: {}", e
                            )))?;
                        
                        // Add plexspaces host function bindings
                        crate::component_host::add_plexspaces_host_to_linker(
                            &mut component_linker,
                        )
                        .map_err(|e| WasmError::InstantiationError(format!(
                            "Failed to add plexspaces host bindings: {}", e
                        )))?;
                        
                        tracing::debug!(
                            actor_id = %actor_id,
                            "Attempting component instantiation with WASI bindings"
                        );
                        
                        // Try to instantiate with WASI bindings
                        let component_instance = component_linker
                            .instantiate_async(&mut component_store, c)
                            .await
                            .map_err(|e| {
                                let error_msg = e.to_string();
                                if error_msg.contains("plexspaces:actor/") && error_msg.contains("matching implementation was not found") {
                                WasmError::InstantiationError(format!(
                                        "Component requires plexspaces host function bindings (e.g., plexspaces:actor/logging@0.1.0, plexspaces:actor/messaging@0.1.0). \
                                        These require WIT bindings to be generated using wasmtime::component::bindgen! macro. \
                                        For now, use traditional WASM modules (wasm32-unknown-unknown) instead of components (wasm32-wasip2) \
                                        if you need plexspaces host functions. \
                                    Error details: {}",
                                        error_msg
                                    ))
                                } else if error_msg.contains("wasi:") && error_msg.contains("matching implementation was not found") {
                                    WasmError::InstantiationError(format!(
                                        "Component requires WASI interface bindings that are not available. \
                                        Error details: {}",
                                        error_msg
                                    ))
                                } else {
                                    WasmError::InstantiationError(format!(
                                        "Component instantiation failed: {}",
                                        error_msg
                                    ))
                                }
                            })?;
                        
                        // Component instantiation succeeded!
                        tracing::info!(
                            actor_id = %actor_id,
                            "WASM component instantiated successfully with WASI bindings"
                        );
                        
                        // Call init() function with initial state if provided
                        // For components, we'll call init after storing the instance
                        // (handled in handle_message method for components)
                        
                        let instantiate_duration = instantiate_start.elapsed();
                        let total_duration = start_time.elapsed();
                        metrics::histogram!("plexspaces_wasm_instance_creation_duration_seconds").record(total_duration.as_secs_f64());
                        metrics::histogram!("plexspaces_wasm_instance_instantiate_duration_seconds").record(instantiate_duration.as_secs_f64());
                        metrics::counter!("plexspaces_wasm_instance_creation_success_total").increment(1);
                        metrics::gauge!("plexspaces_wasm_active_instances").increment(1.0);
                        metrics::counter!("plexspaces_wasm_memory_limits_set_total").increment(1);
                        
                        // Create a dummy traditional instance for compatibility
                        // (components don't use traditional instances, but we need to satisfy the struct)
                        // Create a minimal empty WASM module for the dummy instance
                        // This is a valid minimal WASM module (empty module)
                        let minimal_wasm = vec![
                            0x00, 0x61, 0x73, 0x6d, // WASM magic
                            0x01, 0x00, 0x00, 0x00, // Version 1
                        ];
                        let dummy_module = wasmtime::Module::new(engine, &minimal_wasm)
                            .map_err(|e| WasmError::InstantiationError(format!(
                                "Failed to create dummy module for component: {}", e
                            )))?;
                        let dummy_linker = Linker::new(engine);
                        // Use the second clone for dummy context
                        let dummy_capabilities = capabilities_clone2;
                        let dummy_limits = limits_clone2;
                        let dummy_context = InstanceContext {
                            actor_id: actor_id.clone(),
                            host_functions: context_clone.host_functions.clone(),
                            capabilities: dummy_capabilities,
                            limits: dummy_limits,
                        };
                        let mut dummy_store = Store::new(engine, dummy_context);
                        let dummy_instance = dummy_linker
                            .instantiate_async(&mut dummy_store, &dummy_module)
                            .await
                            .map_err(|e| WasmError::InstantiationError(format!(
                                "Failed to create dummy instance for component: {}", e
                            )))?;
                        
                        let instance = WasmInstance {
                            actor_id: actor_id.clone(),
                            store: Arc::new(RwLock::new(dummy_store)),
                            instance: dummy_instance,
                            #[cfg(feature = "component-model")]
                            component_instance: Some(component_instance),
                            #[cfg(feature = "component-model")]
                            component_store: Some(Arc::new(RwLock::new(component_store))),
                            module,
                        };
                        
                        // Call init if initial state provided
                        // Note: Component init is currently not implemented (wasmtime v25 API changes)
                        // This is logged as a warning but doesn't block instantiation
                        // We skip the call to avoid Send issues with non-Send WasmInstance
                        if !initial_state.is_empty() {
                            tracing::warn!(
                                actor_id = %actor_id,
                                "Component init() call not yet implemented for wasmtime v25 API changes - initial state will be ignored"
                            );
                        }
                        
                        return Ok(instance);
                    }
                }
            }
            #[cfg(not(feature = "component-model"))]
            {
                // For traditional modules, create store with context
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
            #[cfg(feature = "component-model")]
            component_instance: None,
            #[cfg(feature = "component-model")]
            component_store: None,
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

        // Add receive_from_queue function
        linker
            .func_wrap(
                "plexspaces",
                "receive_from_queue",
                |mut caller: Caller<'_, InstanceContext>,
                 queue_name_ptr: i32,
                 queue_name_len: i32,
                 timeout_ms: i32| -> i32 {
                    match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(memory)) => {
                            // Validate pointers
                            if queue_name_ptr < 0 || queue_name_len < 0 {
                                eprintln!("[WASM] receive_from_queue error: invalid pointer or length");
                                return -1i32;
                            }

                            let data = memory.data(&caller);
                            let queue_name_bytes = &data[queue_name_ptr as usize..(queue_name_ptr + queue_name_len) as usize];

                            match std::str::from_utf8(queue_name_bytes) {
                                Ok(_queue_name) => {
                                    let _host_functions = Arc::clone(&caller.data().host_functions);
                                    let _timeout_ms = timeout_ms as u64;
                                    
                                    // NOTE: This is a synchronous host function, but receive_from_queue is async.
                                    // Traditional WASM modules use synchronous host functions. For async operations
                                    // like receive_from_queue, use WASM components instead which support async host functions.
                                    // This is an acceptable limitation - traditional modules should use blocking operations
                                    // or migrate to components for async support.
                                    eprintln!("[WASM] receive_from_queue: async receive not yet supported in sync host functions");
                    -1i32
                                }
                                _ => {
                                    eprintln!("[WASM] receive_from_queue error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            eprintln!("[WASM] receive_from_queue error: memory not exported");
                            -1i32
                        }
                    }
                },
            )
            .map_err(|e| WasmError::HostFunctionError(e.to_string()))?;

        Ok(())
    }

    /// Get actor ID
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Check if this is a component instance (components are not Send and cannot be pooled)
    #[cfg(feature = "component-model")]
    pub fn is_component_instance(&self) -> bool {
        self.component_instance.is_some()
    }

    #[cfg(not(feature = "component-model"))]
    pub fn is_component_instance(&self) -> bool {
        false
    }

    /// Get module metadata
    pub fn module(&self) -> &WasmModule {
        &self.module
    }

    /// Call get_supervisor_tree() function from WASM module
    ///
    /// ## Purpose
    /// Calls the exported `get_supervisor_tree()` function to retrieve
    /// the supervisor tree definition as protobuf bytes.
    ///
    /// ## Function Signature
    /// `get_supervisor_tree() -> (ptr: i32, len: i32)`
    /// - Returns pointer and length to protobuf-encoded SupervisorSpec in WASM memory
    ///
    /// ## Returns
    /// Protobuf bytes from WASM module, or empty vec if function doesn't exist
    ///
    /// ## Errors
    /// Returns error if function call fails
    pub async fn get_supervisor_tree(&self) -> WasmResult<Vec<u8>> {
        use crate::memory::read_bytes;

        let mut store = self.store.write().await;

        // Get memory instance
        let memory = self
            .instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

        // Try to get the function
        let func = match self
            .instance
            .get_typed_func::<(), (i32, i32)>(&mut *store, "get_supervisor_tree")
        {
            Ok(f) => f,
            Err(_) => {
                // Function doesn't exist - return empty vec (not an error)
                return Ok(vec![]);
            }
        };

        // Call the function
        let (ptr, len) = func
            .call_async(&mut *store, ())
            .await
            .map_err(|e| WasmError::ActorFunctionError(format!("get_supervisor_tree failed: {}", e)))?;

        // If ptr is 0 or len is 0, return empty vec
        if ptr == 0 || len == 0 {
            return Ok(vec![]);
        }

        // Read bytes from WASM memory
        read_bytes(&memory, &mut *store, ptr, len)
            .map_err(|e| WasmError::ActorFunctionError(format!("Failed to read supervisor tree bytes: {}", e)))
    }

    /// Handle incoming message and return response
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
        // Check if this is a component instance
        #[cfg(feature = "component-model")]
        {
            if self.component_instance.is_some() {
                return self.handle_message_component(from, message_type, payload).await;
            }
        }
        
        metrics::counter!("plexspaces_wasm_message_handled_total").increment(1);

        use crate::memory::{read_bytes, write_bytes};

        let mut store = self.store.write().await;

        // Track fuel consumption before execution (for metrics)
        let _fuel_before = store.get_fuel().unwrap_or(0);
        
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
                let _result = handle_event_func
                    .call_async(
                        &mut *store,
                        (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
                    )
                    .await
                    .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

                // GenEvent doesn't return a response
                    return Ok(vec![]);
            }
        }

        // 3. Try handle_transition for state machine (GenFSM)
        if let Ok(handle_transition_func) = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_transition")
        {
            let new_state_ptr = handle_transition_func
                .call_async(
                    &mut *store,
                    (from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len),
                )
                .await
                .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;

            // Read new state name from memory (if new_state_ptr != 0)
            if new_state_ptr != 0 {
                use crate::memory::read_bytes;
                match read_bytes(&memory, &mut *store, new_state_ptr, 256) {
                    Ok(bytes) => {
                        // Try to parse as string (new state name)
                        if let Ok(state_name) = std::str::from_utf8(&bytes) {
                            // Return state name as response
                            return Ok(state_name.trim_end_matches('\0').as_bytes().to_vec());
                        }
                    }
                    Err(_) => {}
                }
            }
            return Ok(vec![]);
        }

        // 4. Fallback to generic handle_message
        if let Ok(handle_message_func) = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut *store, "handle_message")
        {
        let result_ptr = handle_message_func
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

        // No handler found
        Err(WasmError::ActorFunctionError(format!(
            "No message handler found for message type: {}",
            message_type
        )))
    }

    /// Call component init function (for WASM components only)
    #[cfg(feature = "component-model")]
    async fn call_component_init(&self, initial_state: &[u8]) -> WasmResult<()> {
        // NOTE: Component init() function calling requires wasmtime v25 API updates.
        // The bindgen! macro generates code, but the API for calling exported functions
        // from a component instance has changed in wasmtime v25. This is an acceptable
        // limitation - component init will be updated when the wasmtime v25 API is fully integrated.
        // For now, we skip calling init if initial_state is empty, otherwise log a warning.
        if !initial_state.is_empty() {
            tracing::warn!(
                actor_id = %self.actor_id,
                "Component init() call not yet implemented for wasmtime v25 API changes"
            );
        }
        tracing::debug!(
            actor_id = %self.actor_id,
            "Skipping component init (API needs to be updated for wasmtime v25)"
        );
        Ok(())
    }
    
    /// Handle message for component (for WASM components only)
    #[cfg(feature = "component-model")]
    async fn handle_message_component(
        &self,
        from: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> WasmResult<Vec<u8>> {
        // NOTE: Component handle_message() requires wasmtime v25 API updates for calling
        // exported component functions. The bindgen! macro generates code, but the API for
        // calling exported functions from a component instance has changed in wasmtime v25.
        // This is an acceptable limitation - component message handling will be updated
        // when the wasmtime v25 API is fully integrated.
        tracing::warn!(
            actor_id = %self.actor_id,
            from = from,
            message_type = message_type,
            "Component handle-message() call not yet implemented for wasmtime v25 API changes"
        );
        metrics::counter!("plexspaces_wasm_component_message_handled_total").increment(1);
        metrics::counter!("plexspaces_wasm_component_message_errors_total").increment(1);
        Err(WasmError::ActorFunctionError(
            "Component handle-message() call not yet implemented for wasmtime v25 API changes".to_string()
        ))
    }

    /// Snapshot actor state for persistence
    ///
    /// ## Returns
    /// Serialized state bytes
    ///
    /// ## Errors
    /// Returns error if snapshot fails
    pub async fn snapshot_state(&self) -> WasmResult<Vec<u8>> {
        let mut store = self.store.write().await;

        // Get memory instance
        let memory = self
            .instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not exported".to_string()))?;

        // Get snapshot_state function
        let snapshot_func = self
            .instance
            .get_typed_func::<(), i32>(&mut *store, "snapshot_state")
            .map_err(|e| WasmError::ActorFunctionError(format!("snapshot_state not exported: {}", e)))?;

        // Call snapshot_state()
        let result_ptr = snapshot_func
                .call_async(&mut *store, ())
                .await
            .map_err(|e| WasmError::ActorFunctionError(format!("snapshot_state failed: {}", e)))?;

        // Read state bytes from memory (if result_ptr != 0)
        if result_ptr != 0 {
            use crate::memory::read_bytes;
            // Read length first (assuming first 4 bytes are length)
            match read_bytes(&memory, &mut *store, result_ptr, 4) {
                Ok(len_bytes) => {
                    let len = i32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
                    if len > 0 {
                        read_bytes(&memory, &mut *store, result_ptr + 4, len as i32)
            } else {
                        Ok(vec![])
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(vec![])
        }
    }

    /// Graceful shutdown
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// Returns error if shutdown fails
    pub async fn shutdown(&self) -> WasmResult<()> {
        let mut store = self.store.write().await;

        // Get shutdown function (optional)
        if let Ok(shutdown_func) = self
            .instance
            .get_typed_func::<(), i32>(&mut *store, "shutdown")
        {
            let result = shutdown_func
                .call_async(&mut *store, ())
                .await
                .map_err(|e| WasmError::ActorFunctionError(format!("shutdown failed: {}", e)))?;

            if result != 0 {
                return Err(WasmError::ActorFunctionError(format!(
                    "shutdown returned error code: {}",
                    result
                )));
            }
        }

        Ok(())
    }
}
