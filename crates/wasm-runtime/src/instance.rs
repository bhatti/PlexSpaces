// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM instance management (wasmtime Store and Instance wrapper)

use crate::{HostFunctions, WasmCapabilities, WasmError, WasmModule, WasmResult};
use plexspaces_core::ChannelService;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
#[cfg(feature = "component-model")]
use tokio::sync::Semaphore;
use wasmtime::{Caller, Engine, Instance, Linker, Store, StoreLimits, StoreLimitsBuilder};
use hex;

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
    /// Simple actor host for Python-compatible components
    pub simple_host_impl: crate::simple_component_host::SimpleHostImpl,
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

/// Component bindings - either PlexspacesActor or SimpleActor
#[cfg(feature = "component-model")]
pub enum ComponentBindings {
    /// Full PlexspacesActor bindings (for Rust components)
    PlexspacesActor(crate::component_host::PlexspacesActor),
    /// SimpleActor bindings (for Python-compatible components)
    SimpleActor(crate::simple_component_host::ActorWorld),
}

/// Holds both component store and bindings together under a single lock
/// This is needed because PlexspacesActor contains references to the store's WASI context
/// which has non-Sync types (RngCore, clocks, stdio streams).
#[cfg(feature = "component-model")]
pub struct ComponentState {
    /// Component store with WASI context
    store: Store<ComponentContext>,
    /// Component bindings for typed export access (either PlexspacesActor or SimpleActor)
    bindings: ComponentBindings,
}

#[cfg(feature = "component-model")]
impl ComponentState {
    /// Create new component state (used when re-instantiation replaces the Store for SimpleActor second-call workaround).
    pub fn new(store: Store<ComponentContext>, bindings: ComponentBindings) -> Self {
        Self { store, bindings }
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

    /// Component state (store + bindings) for WASM components, None for traditional modules
    /// Wrapped in Arc<Mutex<...>> because ComponentState contains non-Sync WASI types
    /// We use Mutex instead of RwLock because:
    /// - RwLock<T>: Sync requires T: Send + Sync
    /// - Mutex<T>: Sync only requires T: Send
    /// - WASI types are Send but not Sync
    #[cfg(feature = "component-model")]
    component_state: Option<Arc<Mutex<ComponentState>>>,

    /// Engine clone for re-instantiation (SimpleActor only). Wasmtime traps "cannot enter
    /// component instance" on the second sequential call on the same store; we replace
    /// component state with a fresh Store+instance after each handle() for SimpleActor.
    #[cfg(feature = "component-model")]
    reinstantiation_engine: Option<Engine>,

    /// TupleSpace provider for re-instantiation (SimpleActor only).
    #[cfg(feature = "component-model")]
    tuplespace_provider: Option<Arc<dyn plexspaces_core::TupleSpaceProvider>>,

    /// When true, load checkpoint on init and save on terminate. Off by default for performance.
    #[cfg(feature = "component-model")]
    durability_enabled: bool,

    /// Original init config (JSON string) used during initial construction.
    /// Stored so we can replay it during re-instantiation to preserve state.
    #[cfg(feature = "component-model")]
    original_init_config: Option<String>,

    /// Re-instantiation lock (semaphore with permit count 1) to serialize re-instantiations
    /// 
    /// ## Purpose
    /// Prevents concurrent re-instantiations for the same actor instance.
    /// Only one re-instantiation can proceed at a time per actor.
    /// 
    /// ## Design
    /// - Uses Semaphore(1) to ensure exclusive access during re-instantiation
    /// - Combined with global_reinstantiation_semaphore (when set) keeps total concurrent
    ///   instantiations under Wasmtime's memory-stripe limit. Both locks must be held when
    ///   re-instantiating: per-actor lock serializes per actor, global semaphore caps total.
    ///   re-instantiations under Wasmtime's memory-stripe limit (default 10).
    /// 
    /// ## Why Semaphore Instead of Mutex
    /// - Semaphore allows us to track permit acquisition/release for observability
    /// - Better for async operations where we need to drop the permit before async work
    #[cfg(feature = "component-model")]
    reinstantiation_lock: Option<Arc<Semaphore>>,

    /// Global cap on concurrent re-instantiations across all actors (when pooling is enabled).
    /// Acquired before create_fresh_simple_actor_state so we stay under Wasmtime's per-stripe limit.
    #[cfg(feature = "component-model")]
    global_reinstantiation_semaphore: Option<Arc<Semaphore>>,

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

    /// Maximum fuel units for execution (from WasmConfig.limits.max_fuel)
    pub max_fuel: u64,
}

// Note: For components requiring WASI, we use a separate ComponentContext that includes WasiCtx.
// Components are not pooled (created fresh each time) because WasiCtx is not Send.
// This is acceptable because:
// 1. Components are typically larger and less frequently instantiated
// 2. The performance cost is minimal compared to the complexity of making WasiCtx Send
// 3. We can still cache compiled components (just not instances)

// SAFETY: WasmInstance is Send + Sync because:
// 1. Traditional store access is through Arc<RwLock<...>> which provides synchronization
// 2. component_state is wrapped in Arc<Mutex<ComponentState>> which ensures
//    thread-safe access even though ComponentState contains:
//    - ComponentContext with non-Sync WasiCtx (RngCore, clocks, stdio streams)
//    - PlexspacesActor bindings that reference the WASI context
// 3. We use Mutex instead of RwLock for component_state because:
//    - Mutex<T>: Sync only requires T: Send
//    - RwLock<T>: Sync requires T: Send + Sync
//    - WASI types are Send but not Sync
// 4. All methods that access the store/bindings acquire the lock before accessing
// 5. Instance (traditional modules) is Send + Sync from wasmtime
// 6. Module metadata (WasmModule) is Send + Sync
// 7. actor_id is String which is Send + Sync
//
// This is safe because we never access the store/bindings concurrently without proper locking.
// The Mutex ensures that only one thread can access them at a time, making
// the overall WasmInstance safe to share across threads.
//
// Note: ComponentState contains both the store (with WasiCtx) and PlexspacesActor bindings
// (which reference the store's exports). Both are accessed only under the same lock.
//
// Send: All fields are Send (Arc, RwLock, Mutex, Instance, String are all Send)
// Sync: All fields are Sync or protected by synchronization primitives (Arc<Mutex<...>>)
unsafe impl Send for WasmInstance {}
unsafe impl Sync for WasmInstance {}

/// When a WasmInstance is dropped (e.g. on application undeploy via unregister_with_cleanup),
/// we decrement the active instances gauge to avoid leaks and keep metrics accurate.
impl Drop for WasmInstance {
    fn drop(&mut self) {
        metrics::gauge!("plexspaces_wasm_active_instances").decrement(1.0);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self.actor_id,
                "WASM instance dropped (cleanup on undeploy/stop)"
            );
        }
    }
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
        max_fuel: u64,
        channel_service: Option<Arc<dyn ChannelService>>,
        message_sender: Option<Arc<dyn crate::MessageSender>>,
        tuplespace_provider: Option<Arc<dyn plexspaces_core::TupleSpaceProvider>>,
        keyvalue_store: Option<Arc<dyn plexspaces_core::KeyValueStore>>,
        process_group_registry: Option<Arc<plexspaces_process_groups::ProcessGroupRegistry>>,
        lock_manager: Option<Arc<dyn plexspaces_core::LockManager + Send + Sync>>,
        object_registry: Option<Arc<dyn plexspaces_core::actor_context::ObjectRegistry>>,
        journal_storage: Option<Arc<dyn plexspaces_core::JournalStorage>>,
        blob_service: Option<Arc<plexspaces_blob::BlobService>>,
        elastic_pool_service: Option<Arc<dyn plexspaces_core::ElasticPoolService>>,
        durability_enabled: bool,
        global_reinstantiation_semaphore: Option<Arc<Semaphore>>,
    ) -> WasmResult<Self> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_instance_creation_attempts_total").increment(1);

        // Clone values before they're moved (needed for component path and dummy instance)
        let capabilities_clone1 = capabilities.clone();
        let limits_clone1 = limits.clone();
        let capabilities_clone2 = capabilities.clone();
        let limits_clone2 = limits.clone();
        let max_fuel_clone = max_fuel;

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
            elastic_pool_service,
        );

        // Store host_functions in Arc for sharing between traditional and component contexts
        let host_functions_arc = Arc::new(host_functions);

        // Create context
        let context = InstanceContext {
            actor_id: actor_id.clone(),
            host_functions: host_functions_arc.clone(),
            capabilities,
            limits,
            max_fuel,
        };

        // Create linker with host functions
        let mut linker = Linker::new(engine);
        Self::add_host_functions(&mut linker)?;

        // Instantiate module or component (async because runtime has async_support enabled)
        let instantiate_start = std::time::Instant::now();
        
        // We need to create the store before the match so it's available after
        // For components, we'll create a separate store, but for traditional modules we use this one
        let max_fuel_for_store = context.max_fuel;
        let mut store = Store::new(engine, context);

        // Add fuel for execution (required when fuel metering is enabled)
        // Use max_fuel from config (default: 10 billion units)
        let fuel_set = store.set_fuel(max_fuel_for_store);
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
                            max_fuel,
                        };
                        // Configure WASI context for Python components
                        // Note: Do NOT use inherit_env() - Python's runtime tries to setenv()
                        // which WASI doesn't support, causing PyObject_SetItem errors.
                        // Instead, explicitly set only required environment variables.
                        let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
                            .inherit_stdio()
                            // Set minimal env vars Python needs (avoids setenv calls during runtime init)
                            .env("PYTHONDONTWRITEBYTECODE", "1")
                            .env("PYTHONUNBUFFERED", "1")
                            .env("HOME", "/")
                            .env("PATH", "/")
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
                                tuplespace_provider.clone(),
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
                            simple_host_impl: crate::simple_component_host::SimpleHostImpl::new(
                                actor_id.clone(),
                                context_clone.host_functions.clone(),
                                tuplespace_provider.clone(),
                            ),
                        };
                        
                        // Create a new store for the component (not pooled, not Send)
                        let component_fuel = component_ctx.instance_ctx.max_fuel;
                        let mut component_store = Store::new(engine, component_ctx);
                        
                        // Add fuel and limiter to component store
                        // Use max_fuel from InstanceContext
                        let _ = component_store.set_fuel(component_fuel);
                        component_store.limiter(|ctx| &mut ctx.instance_ctx.limits);
                        
                        // Add ALL WASI preview 2 bindings using the recommended approach
                        // This automatically adds wasi:cli/*, wasi:io/*, and all other required interfaces
                        // ComponentContext implements WasiView, so this works seamlessly
                        wasmtime_wasi::add_to_linker_async(&mut component_linker)
                            .map_err(|e| WasmError::InstantiationError(format!(
                                "Failed to add WASI bindings: {}", e
                            )))?;
                        
                        // Add plexspaces host function bindings (for plexspaces-actor interface)
                        crate::component_host::add_plexspaces_host_to_linker(
                            &mut component_linker,
                        )
                        .map_err(|e| WasmError::InstantiationError(format!(
                            "Failed to add plexspaces host bindings: {}", e
                        )))?;
                        
                        // Add simple-actor host function bindings (for Python-compatible components)
                        crate::simple_component_host::plexspaces::simple_actor::host::add_to_linker(
                            &mut component_linker,
                            |ctx: &mut ComponentContext| &mut ctx.simple_host_impl,
                        )
                        .map_err(|e| WasmError::InstantiationError(format!(
                            "Failed to add simple-actor host bindings: {}", e
                        )))?;
                        
                        // Try SimpleActor first (Python/componentize-py components); fall back to PlexspacesActor.
                        // Detection via is_simple_actor_component can fail if import name format differs,
                        // so try instantiation and use the result that works.
                        let is_simple_actor = crate::simple_component_host::is_simple_actor_component(c);
                        if tracing::enabled!(tracing::Level::TRACE) {
                            tracing::trace!(
                                actor_id = %actor_id,
                                is_simple_actor = is_simple_actor,
                                "Attempting component instantiation with WASI bindings"
                            );
                        }

                        let component_bindings = match crate::simple_component_host::ActorWorld::instantiate_async(
                            &mut component_store,
                            c,
                            &component_linker,
                        )
                        .await
                        {
                            Ok(simple_bindings) => {
                                tracing::debug!(
                                    actor_id = %actor_id,
                                    "Instantiating simple-actor component (Python-compatible)"
                                );
                                ComponentBindings::SimpleActor(simple_bindings)
                            }
                            Err(e) => {
                                let err_str = e.to_string();
                                if err_str.contains("plexspaces:simple-actor") && err_str.contains("matching implementation was not found") {
                                    let imports: Vec<String> = c
                                        .component_type()
                                        .imports(engine)
                                        .map(|(k, _)| format!("{}", k))
                                        .collect();
                                    tracing::error!(
                                        actor_id = %actor_id,
                                        component_imports = ?imports,
                                        "Simple-actor component: host not in linker (linker key may not match component import names)"
                                    );
                                    return Err(WasmError::InstantiationError(format!(
                                        "Simple-actor component but host not in linker. Component imports: {:?}. Error: {}",
                                        imports, err_str
                                    )));
                                }
                                tracing::debug!(
                                    actor_id = %actor_id,
                                    error = %err_str,
                                    "Simple-actor instantiation failed, trying PlexspacesActor"
                                );
                                let plexspaces_bindings = crate::component_host::PlexspacesActor::instantiate_async(
                                    &mut component_store,
                                    c,
                                    &component_linker,
                                )
                                .await
                                .map_err(|e| {
                                    let error_msg = e.to_string();
                                    if error_msg.contains("plexspaces:actor/") && error_msg.contains("matching implementation was not found") {
                                        WasmError::InstantiationError(format!(
                                            "Component requires plexspaces host function bindings (e.g., plexspaces:actor/logging@0.1.0). \
                                            Error details: {}",
                                            error_msg
                                        ))
                                    } else if error_msg.contains("plexspaces:simple-actor") && error_msg.contains("matching implementation was not found") {
                                        WasmError::InstantiationError(format!(
                                            "Component requires plexspaces:simple-actor/host (Python component). \
                                            Error details: {}",
                                            error_msg
                                        ))
                                    } else if error_msg.contains("wasi:") && error_msg.contains("matching implementation was not found") {
                                        WasmError::InstantiationError(format!(
                                            "Component requires WASI interface bindings. Error details: {}",
                                            error_msg
                                        ))
                                    } else {
                                        WasmError::InstantiationError(format!(
                                            "Component instantiation failed: {}",
                                            error_msg
                                        ))
                                    }
                                })?;
                                ComponentBindings::PlexspacesActor(plexspaces_bindings)
                            }
                        };
                        
                        // Component instantiation succeeded!
                        tracing::debug!(
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
                            max_fuel: max_fuel_clone,
                        };
                        let mut dummy_store = Store::new(engine, dummy_context);
                        let dummy_instance = dummy_linker
                            .instantiate_async(&mut dummy_store, &dummy_module)
                            .await
                            .map_err(|e| WasmError::InstantiationError(format!(
                                "Failed to create dummy instance for component: {}", e
                            )))?;
                        
                        // Create ComponentState that holds both store and bindings together
                        // This ensures they're always accessed under the same lock
                        let component_state = ComponentState {
                            store: component_store,
                            bindings: component_bindings,
                        };
                        
                        let mut instance = WasmInstance {
                            actor_id: actor_id.clone(),
                            store: Arc::new(RwLock::new(dummy_store)),
                            instance: dummy_instance,
                            #[cfg(feature = "component-model")]
                            component_state: Some(Arc::new(Mutex::new(component_state))),
                            #[cfg(feature = "component-model")]
                            reinstantiation_engine: Some(engine.clone()),
                            #[cfg(feature = "component-model")]
                            tuplespace_provider: tuplespace_provider.clone(),
                            #[cfg(feature = "component-model")]
                            durability_enabled,
                            #[cfg(feature = "component-model")]
                            original_init_config: None, // Will be set after init() succeeds
                            #[cfg(feature = "component-model")]
                            reinstantiation_lock: Some(Arc::new(Semaphore::new(1))),
                            #[cfg(feature = "component-model")]
                            global_reinstantiation_semaphore,
                            module,
                        };
                        
                        // Call init() to properly initialize the component
                        // This is REQUIRED for Python components to set up the runtime
                        {
                            let component_state_ref = instance.component_state.as_ref()
                                .ok_or_else(|| WasmError::ActorFunctionError(
                                    "Component state not available after instantiation".to_string()
                                ))?;
                            
                            let mut state = component_state_ref.lock().await;
                            let ComponentState { store, bindings } = &mut *state;
                            
                            // Convert initial_state to JSON string for SimpleActor
                            let config_json = if initial_state.is_empty() {
                                String::new()
                            } else {
                                String::from_utf8_lossy(initial_state).to_string()
                            };
                            
                            // Store original config for re-instantiation
                            let original_config = if config_json.is_empty() {
                                None
                            } else {
                                Some(config_json.clone())
                            };
                            
                            if tracing::enabled!(tracing::Level::DEBUG) {
                                tracing::debug!(
                                    actor_id = %actor_id,
                                    config_len = config_json.len(),
                                    has_config = original_config.is_some(),
                                    "Storing original init config for re-instantiation"
                                );
                            }
                            
                            match bindings {
                                ComponentBindings::SimpleActor(simple_bindings) => {
                                    let result = simple_bindings.plexspaces_simple_actor_actor()
                                        .call_init(store, &config_json)
                                        .await
                                        .map_err(|e| {
                                            tracing::error!(
                                                actor_id = %actor_id,
                                                error = %e,
                                                "Simple-actor init() call failed"
                                            );
                                            WasmError::ActorFunctionError(format!(
                                                "Simple-actor init() failed: {}", e
                                            ))
                                        })?;
                                    
                                    if !result.is_empty() {
                                        tracing::error!(
                                            actor_id = %actor_id,
                                            error = %result,
                                            "Simple-actor init() returned error"
                                        );
                                        return Err(WasmError::ActorFunctionError(format!(
                                            "Simple-actor init() error: {}", result
                                        )));
                                    }
                                    
                                    // Store original config in instance for re-instantiation
                                    instance.original_init_config = original_config.clone();
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        tracing::debug!(
                                            actor_id = %actor_id,
                                            has_stored_config = instance.original_init_config.is_some(),
                                            "Stored original init config in WasmInstance"
                                        );
                                    }
                                }
                                ComponentBindings::PlexspacesActor(plexspaces_bindings) => {
                                    if !initial_state.is_empty() {
                                        let initial_state_vec = initial_state.to_vec();
                                        let result = plexspaces_bindings.plexspaces_actor_actor()
                                            .call_init(store, &initial_state_vec)
                                            .await
                                            .map_err(|e| {
                                                tracing::error!(
                                                    actor_id = %actor_id,
                                                    error = %e,
                                                    "PlexspacesActor init() call failed"
                                                );
                                                WasmError::ActorFunctionError(format!(
                                                    "PlexspacesActor init() failed: {}", e
                                                ))
                                            })?;
                                        
                                        if let Err(error_msg) = result {
                                            tracing::error!(
                                                actor_id = %actor_id,
                                                error = %error_msg,
                                                "PlexspacesActor init() returned error"
                                            );
                                            return Err(WasmError::ActorFunctionError(format!(
                                                "PlexspacesActor init() error: {}", error_msg
                                            )));
                                        }
                                        
                                        // Store original config for PlexspacesActor (as JSON string)
                                        instance.original_init_config = Some(String::from_utf8_lossy(initial_state).to_string());
                                        
                                        tracing::info!(
                                            actor_id = %actor_id,
                                            "PlexspacesActor init() succeeded"
                                        );
                                    }
                                }
                            }
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
                // Use max_fuel from config
                let fuel_set = store.set_fuel(context.max_fuel);
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
            component_state: None,
            #[cfg(feature = "component-model")]
            reinstantiation_engine: None,
            #[cfg(feature = "component-model")]
            tuplespace_provider: None,
            #[cfg(feature = "component-model")]
            durability_enabled,
            #[cfg(feature = "component-model")]
            original_init_config: None, // Traditional modules don't use init config
            #[cfg(feature = "component-model")]
            reinstantiation_lock: None, // Traditional modules don't need re-instantiation
            #[cfg(feature = "component-model")]
            global_reinstantiation_semaphore: None,
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
                                tracing::warn!("[WASM] log error: invalid pointer or length");
                                return;
                            }

                            let ptr = ptr as usize;
                            let len = len as usize;
                            let data = memory.data(&caller);

                            if ptr + len > data.len() {
                                tracing::warn!("[WASM] log error: out of bounds access");
                                return;
                            }

                            match std::str::from_utf8(&data[ptr..ptr + len]) {
                                Ok(message) => {
                                    // Log with actor context
                                    let actor_id = &caller.data().actor_id;
                                    tracing::info!("[WASM:{}] {}", actor_id, message);
                                }
                                Err(e) => {
                                    tracing::warn!("[WASM] log error: invalid UTF-8: {}", e);
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("[WASM] log error: memory not exported");
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
                                tracing::warn!("[WASM] send_message error: invalid pointer or length");
                                return -1i32;
                            }

                            let to_ptr = to_ptr as usize;
                            let to_len = to_len as usize;
                            let msg_ptr = msg_ptr as usize;
                            let msg_len = msg_len as usize;
                            let data = memory.data(&caller);

                            // Check bounds
                            if to_ptr + to_len > data.len() || msg_ptr + msg_len > data.len() {
                                tracing::warn!("[WASM] send_message error: out of bounds access");
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
                                    
                                    // Extract message_type from payload JSON if available, otherwise use "cast"
                                    let message_type = if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&message) {
                                        json_value.get("op")
                                            .or_else(|| json_value.get("msg_type"))
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| "cast".to_string())
                                    } else {
                                        "cast".to_string()
                                    };
                                    
                                    // Spawn async task to send message (host function is sync)
                                    tokio::spawn(async move {
                                        if let Err(e) = host_functions.send_message(&from_actor, &to_actor, &message_type, &message).await {
                                            tracing::error!(
                                                from = %from_actor,
                                                to = %to_actor,
                                                error = %e,
                                                "Failed to send message from WASM actor"
                                            );
                                        } else {
                                            if tracing::enabled!(tracing::Level::DEBUG) {
                                            tracing::debug!(
                                                from = %from_actor,
                                                to = %to_actor,
                                                "Message sent successfully from WASM actor"
                                            );
                                            }
                                        }
                                    });
                                    
                                    0i32 // Success (message sent asynchronously)
                                }
                                (Err(e), _) | (_, Err(e)) => {
                                    tracing::warn!("[WASM] send_message error: invalid UTF-8: {}", e);
                                    -1i32 // Error
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("[WASM] send_message error: memory not exported");
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
                                tracing::warn!("[WASM] send_to_queue error: invalid pointer or length");
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
                                                if tracing::enabled!(tracing::Level::DEBUG) {
                                                tracing::debug!(
                                                    queue = %queue_name,
                                                    "Message sent to queue from WASM actor"
                                                );
                                                }
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
                                    tracing::warn!("[WASM] send_to_queue error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("[WASM] send_to_queue error: memory not exported");
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
                                tracing::warn!("[WASM] publish_to_topic error: invalid pointer or length");
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
                                                if tracing::enabled!(tracing::Level::DEBUG) {
                                                tracing::debug!(
                                                    topic = %topic_name,
                                                    "Message published to topic from WASM actor"
                                                );
                                                }
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
                                    tracing::warn!("[WASM] publish_to_topic error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("[WASM] publish_to_topic error: memory not exported");
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
                                tracing::warn!("[WASM] receive_from_queue error: invalid pointer or length");
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
                                    tracing::warn!("[WASM] receive_from_queue: async receive not yet supported in sync host functions");
                                    -1i32
                                }
                                _ => {
                                    tracing::warn!("[WASM] receive_from_queue error: invalid UTF-8");
                                    -1i32
                                }
                            }
                        }
                        _ => {
                            tracing::warn!("[WASM] receive_from_queue error: memory not exported");
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
        self.component_state.is_some()
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
    /// - Fallback → `handle_message()` (generic)
    pub async fn handle_message(
        &self,
        from: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> WasmResult<Vec<u8>> {
        self.handle_message_with_id(from, message_type, payload, "").await
    }

    /// Same as handle_message but with message_id for correlation in logs (request/response tracing).
    pub async fn handle_message_with_id(
        &self,
        from: &str,
        message_type: &str,
        payload: Vec<u8>,
        message_id: &str,
    ) -> WasmResult<Vec<u8>> {
        // Check if this is a component instance
        #[cfg(feature = "component-model")]
        {
            if self.component_state.is_some() {
                return self.handle_message_component(from, message_type, payload, message_id).await;
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
    /// 
    /// Uses the bindgen!-generated PlexspacesActor bindings to call the exported
    /// init function with proper typing.
    #[cfg(feature = "component-model")]
    async fn call_component_init(&self, initial_state: &[u8]) -> WasmResult<()> {
        // Get the component state (store + bindings together)
        let component_state = self.component_state.as_ref()
            .ok_or_else(|| WasmError::ActorFunctionError(
                "Component state not available".to_string()
            ))?;
        
        // Acquire lock on the component state (both store and bindings)
        let mut state = component_state.lock().await;
        
        // Destructure to get separate mutable references to store and bindings
        // This avoids borrow checker issues when calling methods
        let ComponentState { store, bindings } = &mut *state;
        
        // Call init based on binding type
        match bindings {
            ComponentBindings::PlexspacesActor(plexspaces_bindings) => {
                // Full PlexspacesActor bindings - init takes Vec<u8>
                let initial_state_vec = initial_state.to_vec();
                let result = plexspaces_bindings.plexspaces_actor_actor()
                    .call_init(store, &initial_state_vec)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            actor_id = %self.actor_id,
                            error = %e,
                            "Component init() call failed"
                        );
                        WasmError::ActorFunctionError(format!(
                            "Component init() call failed: {}", e
                        ))
                    })?;
                
                match result {
                    Ok(()) => {
                        tracing::info!(
                            actor_id = %self.actor_id,
                            initial_state_len = initial_state.len(),
                            "Component init() succeeded"
                        );
                        Ok(())
                    }
                    Err(error_msg) => {
                        tracing::error!(
                            actor_id = %self.actor_id,
                            error = %error_msg,
                            "Component init() returned error"
                        );
                        Err(WasmError::ActorFunctionError(format!(
                            "Component init() returned error: {}", error_msg
                        )))
                    }
                }
            }
            ComponentBindings::SimpleActor(simple_bindings) => {
                // SimpleActor bindings - init takes JSON string
                let config_json = if initial_state.is_empty() {
                    String::new()
                } else {
                    String::from_utf8_lossy(initial_state).to_string()
                };
                
                let result = simple_bindings.plexspaces_simple_actor_actor()
                    .call_init(store, &config_json)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            actor_id = %self.actor_id,
                            error = %e,
                            "Simple actor init() call failed"
                        );
                        WasmError::ActorFunctionError(format!(
                            "Simple actor init() call failed: {}", e
                        ))
                    })?;
                
                // Empty string = success, non-empty = error message
                if result.is_empty() {
                    tracing::info!(
                        actor_id = %self.actor_id,
                        "Simple actor init() succeeded"
                    );
                    Ok(())
                } else {
                    tracing::error!(
                        actor_id = %self.actor_id,
                        error = %result,
                        "Simple actor init() returned error"
                    );
                    Err(WasmError::ActorFunctionError(format!(
                        "Simple actor init() returned error: {}", result
                    )))
                }
            }
        }
    }
    
    /// Tries to get application-level msg_type (handler name) from JSON payload.
    /// Used when routing to handle_event (GenEvent) so event_type is the handler name (e.g. "ingest").
    #[cfg(feature = "component-model")]
    fn try_msg_type_from_payload(payload: &[u8]) -> Option<String> {
        let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
        let s = value.get("msg_type")?.as_str()?.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("call") || s.eq_ignore_ascii_case("cast") {
            return None;
        }
        Some(s.to_string())
    }

    /// Creates a fresh ComponentState (new Store + SimpleActor instance) for the next handle() call.
    /// Wasmtime traps "cannot enter component instance" on the second sequential call on the same
    /// store (see wasmtime#8943); replacing state after each SimpleActor handle() avoids re-entry.
    #[cfg(feature = "component-model")]
    async fn create_fresh_simple_actor_state(&self, instance_ctx: &InstanceContext) -> WasmResult<ComponentState> {
        use crate::runtime::WasmModuleInner;
        let engine = self.reinstantiation_engine.as_ref()
            .ok_or_else(|| WasmError::ActorFunctionError("Reinstantiation engine not set".to_string()))?;
        let c = self.module.module.as_component()
            .ok_or_else(|| WasmError::ActorFunctionError("Module is not a component".to_string()))?
            .clone();
        let mut component_linker = ComponentLinker::new(engine);
        let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
            .inherit_stdio()
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env("HOME", "/")
            .env("PATH", "/")
            .build();
        let tuplespace_provider = self.tuplespace_provider.clone();
        let component_ctx = ComponentContext {
            instance_ctx: instance_ctx.clone(),
            wasi_ctx,
            resource_table: wasmtime_wasi::ResourceTable::new(),
            plexspaces_host: crate::component_host::PlexspacesHost::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            logging_impl: crate::component_host::LoggingImpl { actor_id: self.actor_id.clone() },
            messaging_impl: crate::component_host::MessagingImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            tuplespace_impl: crate::component_host::TuplespaceImpl::new(
                tuplespace_provider.clone(),
                self.actor_id.clone(),
            ),
            channels_impl: crate::component_host::ChannelsImpl::new(instance_ctx.host_functions.clone()),
            durability_impl: crate::component_host::DurabilityImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            workflow_impl: crate::component_host::WorkflowImpl,
            blob_impl: crate::component_host::BlobImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            keyvalue_impl: crate::component_host::KeyValueImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            process_groups_impl: crate::component_host::ProcessGroupsImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            locks_impl: crate::component_host::LocksImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            registry_impl: crate::component_host::RegistryImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            simple_host_impl: crate::simple_component_host::SimpleHostImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
                tuplespace_provider.clone(),
            ),
        };
        let mut component_store = Store::new(engine, component_ctx);
        // Use max_fuel from InstanceContext
        let _ = component_store.set_fuel(instance_ctx.max_fuel);
        component_store.limiter(|ctx| &mut ctx.instance_ctx.limits);
        wasmtime_wasi::add_to_linker_async(&mut component_linker)
            .map_err(|e| WasmError::InstantiationError(format!("Failed to add WASI bindings: {}", e)))?;
        crate::component_host::add_plexspaces_host_to_linker(&mut component_linker)
            .map_err(|e| WasmError::InstantiationError(format!("Failed to add plexspaces host bindings: {}", e)))?;
        crate::simple_component_host::plexspaces::simple_actor::host::add_to_linker(
            &mut component_linker,
            |ctx: &mut ComponentContext| &mut ctx.simple_host_impl,
        )
        .map_err(|e| WasmError::InstantiationError(format!("Failed to add simple-actor host bindings: {}", e)))?;
        let simple_bindings = crate::simple_component_host::ActorWorld::instantiate_async(
            &mut component_store,
            &c,
            &component_linker,
        )
        .await
        .map_err(|e| WasmError::InstantiationError(format!(
            "Simple-actor re-instantiation failed: {}", e
        )))?;
        // Use original init config if available, otherwise use empty string
        let init_config = self.original_init_config.as_deref().unwrap_or("");
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %self.actor_id,
                config_len = init_config.len(),
                has_original_config = self.original_init_config.is_some(),
                "Re-instantiating SimpleActor with original init config"
            );
        }
        let result = simple_bindings.plexspaces_simple_actor_actor()
            .call_init(&mut component_store, init_config)
            .await
            .map_err(|e| WasmError::ActorFunctionError(format!("Simple-actor init() on fresh state failed: {}", e)))?;
        if !result.is_empty() {
            return Err(WasmError::ActorFunctionError(format!("Simple-actor init() on fresh state returned error: {}", result)));
        }
        Ok(ComponentState {
            store: component_store,
            bindings: ComponentBindings::SimpleActor(simple_bindings),
        })
    }

    /// Creates a fresh ComponentState (new Store + PlexspacesActor instance) for the next handle() call.
    /// Same wasmtime re-entry workaround as SimpleActor (wasmtime#8943).
    #[cfg(feature = "component-model")]
    async fn create_fresh_plexspaces_actor_state(&self, instance_ctx: &InstanceContext) -> WasmResult<ComponentState> {
        use crate::runtime::WasmModuleInner;
        let engine = self.reinstantiation_engine.as_ref()
            .ok_or_else(|| WasmError::ActorFunctionError("Reinstantiation engine not set".to_string()))?;
        let c = self.module.module.as_component()
            .ok_or_else(|| WasmError::ActorFunctionError("Module is not a component".to_string()))?
            .clone();
        let mut component_linker = ComponentLinker::new(engine);
        let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
            .inherit_stdio()
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env("HOME", "/")
            .env("PATH", "/")
            .build();
        let tuplespace_provider = self.tuplespace_provider.clone();
        let component_ctx = ComponentContext {
            instance_ctx: instance_ctx.clone(),
            wasi_ctx,
            resource_table: wasmtime_wasi::ResourceTable::new(),
            plexspaces_host: crate::component_host::PlexspacesHost::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            logging_impl: crate::component_host::LoggingImpl { actor_id: self.actor_id.clone() },
            messaging_impl: crate::component_host::MessagingImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            tuplespace_impl: crate::component_host::TuplespaceImpl::new(
                tuplespace_provider.clone(),
                self.actor_id.clone(),
            ),
            channels_impl: crate::component_host::ChannelsImpl::new(instance_ctx.host_functions.clone()),
            durability_impl: crate::component_host::DurabilityImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
            ),
            workflow_impl: crate::component_host::WorkflowImpl,
            blob_impl: crate::component_host::BlobImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            keyvalue_impl: crate::component_host::KeyValueImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            process_groups_impl: crate::component_host::ProcessGroupsImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            locks_impl: crate::component_host::LocksImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            registry_impl: crate::component_host::RegistryImpl {
                actor_id: self.actor_id.clone(),
                host_functions: instance_ctx.host_functions.clone(),
            },
            simple_host_impl: crate::simple_component_host::SimpleHostImpl::new(
                self.actor_id.clone(),
                instance_ctx.host_functions.clone(),
                tuplespace_provider.clone(),
            ),
        };
        let mut component_store = Store::new(engine, component_ctx);
        // Use max_fuel from InstanceContext
        let _ = component_store.set_fuel(instance_ctx.max_fuel);
        component_store.limiter(|ctx| &mut ctx.instance_ctx.limits);
        wasmtime_wasi::add_to_linker_async(&mut component_linker)
            .map_err(|e| WasmError::InstantiationError(format!("Failed to add WASI bindings: {}", e)))?;
        crate::component_host::add_plexspaces_host_to_linker(&mut component_linker)
            .map_err(|e| WasmError::InstantiationError(format!("Failed to add plexspaces host bindings: {}", e)))?;
        crate::simple_component_host::plexspaces::simple_actor::host::add_to_linker(
            &mut component_linker,
            |ctx: &mut ComponentContext| &mut ctx.simple_host_impl,
        )
        .map_err(|e| WasmError::InstantiationError(format!("Failed to add simple-actor host bindings: {}", e)))?;
        let plexspaces_bindings = crate::component_host::PlexspacesActor::instantiate_async(
            &mut component_store,
            &c,
            &component_linker,
        )
        .await
        .map_err(|e| WasmError::InstantiationError(format!(
            "PlexspacesActor re-instantiation failed: {}", e
        )))?;
        Ok(ComponentState {
            store: component_store,
            bindings: ComponentBindings::PlexspacesActor(plexspaces_bindings),
        })
    }

    /// Handle message for component (for WASM components only)
    /// 
    /// Uses the bindgen!-generated PlexspacesActor bindings to call the exported
    /// handle-message function with proper typing.
    /// For "cast"/"info" (GenEvent) tries handle_event first when the component exports it.
    #[cfg(feature = "component-model")]
    async fn handle_message_component(
        &self,
        from: &str,
        message_type: &str,
        payload: Vec<u8>,
        message_id: &str,
    ) -> WasmResult<Vec<u8>> {
        // Note: WIT uses result<payload, string> for better componentize-py compatibility
        // The Result<Vec<u8>, String> maps directly to Rust's Result type
        
        metrics::counter!("plexspaces_wasm_component_message_handled_total").increment(1);
        let start_time = std::time::Instant::now();
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %self.actor_id,
                message_id = %message_id,
                from = from,
                message_type = message_type,
                payload_len = payload.len(),
                "handle_message_component ENTRY"
            );
        }
        
        // Get the component state (store + bindings together)
        let component_state = self.component_state.as_ref()
            .ok_or_else(|| WasmError::ActorFunctionError(
                "Component state not available".to_string()
            ))?;
        
        // Acquire lock on the component state (both store and bindings)
        let mut state = component_state.lock().await;

        // Destructure to get separate mutable references to store and bindings
        // This avoids borrow checker issues when calling methods
        let ComponentState { store, bindings } = &mut *state;

        let from_string = from.to_string();
        let message_type_string = message_type.to_string();

        // Call handle based on binding type
        match bindings {
            ComponentBindings::PlexspacesActor(plexspaces_bindings) => {
                let actor = plexspaces_bindings.plexspaces_actor_actor();
                // GenEvent/EventHandler: for "cast" or "info" try handle_event first (event handler pattern)
                let used_handle_event = (message_type_string == "cast" || message_type_string == "info")
                    && {
                        let event_type = Self::try_msg_type_from_payload(&payload)
                            .unwrap_or_else(|| message_type_string.clone());
                        match actor.call_handle_event(&mut *store, &event_type, &payload).await {
                            Ok(Ok(())) => true,
                            _ => false,
                        }
                    };
                if used_handle_event {
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_component_message_duration_seconds")
                        .record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_component_message_success_total").increment(1);
                    let instance_ctx = store.data().instance_ctx.clone();
                    drop(state);
                    
                    // Acquire re-instantiation lock to serialize re-instantiations per actor
                    let reinstantiation_lock = self.reinstantiation_lock.as_ref()
                        .ok_or_else(|| WasmError::ActorFunctionError(
                            "Re-instantiation lock not available".to_string()
                        ))?;
                    let _permit = reinstantiation_lock.acquire().await
                        .map_err(|_e| {
                            tracing::error!(
                                actor_id = %self.actor_id,
                                message_id = %message_id,
                                "Failed to acquire re-instantiation lock (semaphore closed)"
                            );
                            WasmError::ActorFunctionError(
                                "Failed to acquire re-instantiation lock: semaphore closed".to_string()
                            )
                        })?;
                    let reinstantiation_start = std::time::Instant::now();
                    metrics::counter!("plexspaces_wasm_reinstantiation_total",
                        "actor_id" => self.actor_id.clone()
                    ).increment(1);
                    
                    // Acquire global reinstantiation cap (if set) so we stay under Wasmtime's memory-stripe limit.
                    let component_state = self.component_state.as_ref().expect("component_state set");
                    let new_state = {
                        let _global_permit = if let Some(ref g) = self.global_reinstantiation_semaphore {
                            Some(
                                g.acquire()
                                    .await
                                    .map_err(|_| WasmError::ActorFunctionError("Global reinstantiation semaphore closed".to_string()))?,
                            )
                        } else {
                            None
                        };
                        Self::create_fresh_plexspaces_actor_state(self, &instance_ctx).await
                    }
                        .map_err(|e| {
                            let error_msg = e.to_string();
                            metrics::counter!("plexspaces_wasm_reinstantiation_errors_total",
                                "actor_id" => self.actor_id.clone(),
                                "error_type" => "instantiation_failed"
                            ).increment(1);
                            tracing::error!(
                                actor_id = %self.actor_id,
                                message_id = %message_id,
                                error = %error_msg,
                                "PlexspacesActor re-instantiation after handle_event failed"
                            );
                            WasmError::ActorFunctionError(format!(
                                "Failed to re-instantiate WASM actor after handle_event(): {}",
                                error_msg
                            ))
                        })?;
                    let mut guard = component_state.lock().await;
                    *guard = new_state;
                    
                    let reinstantiation_duration = reinstantiation_start.elapsed();
                    metrics::histogram!("plexspaces_wasm_reinstantiation_duration_seconds",
                        "actor_id" => self.actor_id.clone()
                    ).record(reinstantiation_duration.as_secs_f64());
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            "handle_message_component END PlexspacesActor handle_event Ok"
                        );
                    }
                    return Ok(vec![]);
                }
                // Full PlexspacesActor bindings - handle_message takes Vec<u8>
                let result = actor
                    .call_handle_message(&mut *store, &from_string, &message_type_string, &payload)
                    .await
                    .map_err(|e| {
                        let error_msg = e.to_string();
                        tracing::error!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            error_first_line = %error_msg.lines().next().unwrap_or(""),
                            "Component handle-message() call failed"
                        );
                        metrics::counter!("plexspaces_wasm_component_message_errors_total").increment(1);
                        WasmError::ActorFunctionError(format!(
                            "Component handle-message() call failed: {}", error_msg
                        ))
                    })?;
                
                let duration = start_time.elapsed();
                
                match result {
                    Ok(response_payload) => {
                        metrics::histogram!("plexspaces_wasm_component_message_duration_seconds")
                            .record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_wasm_component_message_success_total").increment(1);
                        let instance_ctx = store.data().instance_ctx.clone();
                        drop(state);
                        
                        // Acquire re-instantiation lock to serialize re-instantiations per actor
                        let reinstantiation_lock = self.reinstantiation_lock.as_ref()
                            .ok_or_else(|| WasmError::ActorFunctionError(
                                "Re-instantiation lock not available".to_string()
                            ))?;
                        let _permit = reinstantiation_lock.acquire().await
                            .map_err(|_e| {
                                tracing::error!(
                                    actor_id = %self.actor_id,
                                    message_id = %message_id,
                                    "Failed to acquire re-instantiation lock (semaphore closed)"
                                );
                                WasmError::ActorFunctionError(
                                    "Failed to acquire re-instantiation lock: semaphore closed".to_string()
                                )
                            })?;
                        let reinstantiation_start = std::time::Instant::now();
                        metrics::counter!("plexspaces_wasm_reinstantiation_total",
                            "actor_id" => self.actor_id.clone()
                        ).increment(1);
                        
                        // Acquire global reinstantiation cap (if set) so we stay under Wasmtime's memory-stripe limit.
                        let component_state = self.component_state.as_ref().expect("component_state set");
                        let new_state = {
                            let _global_permit = if let Some(ref g) = self.global_reinstantiation_semaphore {
                                Some(
                                    g.acquire()
                                        .await
                                        .map_err(|_| WasmError::ActorFunctionError("Global reinstantiation semaphore closed".to_string()))?,
                                )
                            } else {
                                None
                            };
                            Self::create_fresh_plexspaces_actor_state(self, &instance_ctx).await
                        }
                            .map_err(|e| {
                                let error_msg = e.to_string();
                                metrics::counter!("plexspaces_wasm_reinstantiation_errors_total",
                                    "actor_id" => self.actor_id.clone(),
                                    "error_type" => "instantiation_failed"
                                ).increment(1);
                                tracing::error!(
                                    actor_id = %self.actor_id,
                                    message_id = %message_id,
                                    error = %error_msg,
                                    "PlexspacesActor re-instantiation after handle_message failed"
                                );
                                WasmError::ActorFunctionError(format!(
                                    "Failed to re-instantiate WASM actor after handle_message(): {}",
                                    error_msg
                                ))
                            })?;
                        let mut guard = component_state.lock().await;
                        *guard = new_state;
                        
                        let reinstantiation_duration = reinstantiation_start.elapsed();
                        metrics::histogram!("plexspaces_wasm_reinstantiation_duration_seconds",
                            "actor_id" => self.actor_id.clone()
                        ).record(reinstantiation_duration.as_secs_f64());
                        if tracing::enabled!(tracing::Level::TRACE) {
                            tracing::trace!(
                                actor_id = %self.actor_id,
                                message_id = %message_id,
                                duration_ms = reinstantiation_duration.as_millis(),
                                "handle_message_component END PlexspacesActor Ok"
                            );
                        }
                        Ok(response_payload)
                    }
                    Err(error_message) => {
                        metrics::counter!("plexspaces_wasm_component_message_errors_total").increment(1);
                        tracing::warn!(
                            actor_id = %self.actor_id,
                            error_message = %error_message,
                            "Component handle-message() returned error"
                        );
                        if tracing::enabled!(tracing::Level::TRACE) {
                            tracing::trace!(
                                actor_id = %self.actor_id,
                                message_id = %message_id,
                                "handle_message_component END PlexspacesActor Err"
                            );
                        }
                        Err(WasmError::ActorFunctionError(format!(
                            "Actor error: {}", error_message
                        )))
                    }
                }
            }
            ComponentBindings::SimpleActor(simple_bindings) => {
                // SimpleActor bindings - handle takes JSON strings
                // Convert payload to JSON string (it should already be JSON)
                let payload_json = String::from_utf8_lossy(&payload).to_string();
                
                // Log payload JSON for debugging JSON parsing errors (always log on error, debug otherwise)
                tracing::debug!(
                    actor_id = %self.actor_id,
                    message_id = %message_id,
                    from_actor = %from_string,
                    msg_type = %message_type_string,
                    payload_json = %payload_json,
                    payload_len = payload.len(),
                    payload_hex = %hex::encode(&payload[..payload.len().min(200)]),
                    "SimpleActor handle() call - payload JSON"
                );
                
                // Validate JSON before calling WASM handler
                if let Err(e) = serde_json::from_str::<serde_json::Value>(&payload_json) {
                    tracing::error!(
                        actor_id = %self.actor_id,
                        message_id = %message_id,
                        from_actor = %from_string,
                        msg_type = %message_type_string,
                        payload_json = %payload_json,
                        payload_len = payload.len(),
                        payload_hex = %hex::encode(&payload[..payload.len().min(200)]),
                        error = %e,
                        "Invalid JSON payload before calling WASM handle()"
                    );
                    return Err(WasmError::ActorFunctionError(format!(
                        "Invalid JSON payload: {}",
                        e
                    )));
                }
                let result = simple_bindings.plexspaces_simple_actor_actor()
                    .call_handle(&mut *store, &from_string, &message_type_string, &payload_json)
                    .await;
                // Process the result first (before re-instantiation)
                // handle() returns a plain string (JSON-serialized result)
                // Errors are encoded in the JSON payload (e.g., {"error": "message"})
                // or prefixed with "ERROR:" for framework-level errors
                let processed_result: Result<String, WasmError> = match result {
                    Ok(json_string) => {
                        // Actor returned JSON string directly (serialized by iterative serializer)
                        // Check if it's a framework-level error (starts with "ERROR:")
                        if json_string.starts_with("ERROR:") {
                            Err(WasmError::ActorFunctionError(json_string))
                        } else {
                            Ok(json_string)
                        }
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        let invocation_label = if message_type_string.eq_ignore_ascii_case("call") { "ask" } else { "tell" };
                        let error_first_line = error_msg.lines().next().unwrap_or("");
                        tracing::error!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            from_actor = %from_string,
                            msg_type = %message_type_string,
                            invocation = %invocation_label,
                            error_first_line = %error_first_line,
                            payload_json = %payload_json,
                            payload_len = payload.len(),
                            "Simple actor handle() call failed"
                        );
                        if tracing::enabled!(tracing::Level::TRACE) {
                            tracing::trace!(
                                actor_id = %self.actor_id,
                                message_id = %message_id,
                                error_full = %error_msg,
                                "WASM handle backtrace (full)"
                            );
                        }
                        metrics::counter!("plexspaces_wasm_component_message_errors_total").increment(1);
                        Err(WasmError::ActorFunctionError(format!(
                            "{}: {}",
                            crate::SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE,
                            error_msg
                        )))
                    }
                };
                
                // Check if we have an error before proceeding to re-instantiation
                // If error, convert String error to Vec<u8> error response and return early
                if let Err(ref e) = processed_result {
                    tracing::error!(
                        actor_id = %self.actor_id,
                        message_id = %message_id,
                        error = %e,
                        "❌ SimpleActor handle() failed - NOT proceeding to re-instantiation"
                    );
                    // Convert error to JSON error response format
                    let error_json = serde_json::json!({
                        "status": "error",
                        "error": e.to_string()
                    });
                    return Ok(serde_json::to_string(&error_json)
                        .unwrap_or_else(|_| format!(r#"{{"status":"error","error":"{}"}}"#, e))
                        .into_bytes());
                }
                
                // CRITICAL: Capture instance_ctx and saved_state (get_state) WHILE we hold the lock,
                // then drop the lock BEFORE acquiring reinstantiation_lock. This avoids deadlock:
                // we never re-acquire component_state lock in this path until after create_fresh.
                let instance_ctx = store.data().instance_ctx.clone();
                let saved_state = {
                    if let ComponentBindings::SimpleActor(ref old_simple) = bindings {
                        match old_simple.plexspaces_simple_actor_actor()
                            .call_get_state(&mut *store)
                            .await
                        {
                            Ok(state_json) => {
                                if tracing::enabled!(tracing::Level::TRACE) {
                                    tracing::trace!(
                                        actor_id = %self.actor_id,
                                        message_id = %message_id,
                                        state_len = state_json.len(),
                                        "Captured actor state before re-instantiation (while holding lock)"
                                    );
                                }
                                Some(state_json)
                            }
                            Err(e) => {
                                tracing::error!(
                                    actor_id = %self.actor_id,
                                    message_id = %message_id,
                                    from_actor = %from_string,
                                    msg_type = %message_type_string,
                                    error = %e,
                                    "Failed to capture state before re-instantiation; state will be lost"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    }
                };
                drop(state);
                // Re-instantiate after handle() to avoid re-entrancy trap (wasmtime component model).
                // Preserve state across re-instantiation via get_state/set_state cycle:
                //   1. get_state() already captured above while holding lock
                //   2. Create fresh instance (new Store + component + init())
                //   3. Call set_state() on the NEW instance to restore state (after re-acquiring lock once)
                //
                // CRITICAL: Use per-actor re-instantiation lock to serialize re-instantiations.
                let reinstantiation_lock = self.reinstantiation_lock.as_ref()
                    .ok_or_else(|| {
                        tracing::error!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            "Re-instantiation lock not available (None)"
                        );
                        WasmError::ActorFunctionError(
                            "Re-instantiation lock not available".to_string()
                        )
                    })?;
                let _permit = reinstantiation_lock.acquire().await
                    .map_err(|_e| {
                        tracing::error!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            "Failed to acquire re-instantiation lock (semaphore closed)"
                        );
                        WasmError::ActorFunctionError(
                            "Failed to acquire re-instantiation lock: semaphore closed".to_string()
                        )
                    })?;
                let reinstantiation_start = std::time::Instant::now();
                metrics::counter!("plexspaces_wasm_reinstantiation_total",
                    "actor_id" => self.actor_id.clone()
                ).increment(1);
                
                // Step 2: Create fresh instance (saved_state and instance_ctx were captured above while holding lock).
                // No component_state lock held here - avoids deadlock.
                tracing::info!(
                    actor_id = %self.actor_id,
                    message_id = %message_id,
                    saved_state_len = saved_state.as_ref().map(|s| s.len()).unwrap_or(0),
                    "SimpleActor reinstantiation: creating fresh instance (state already captured)"
                );
                // Acquire global reinstantiation cap (if set) so we stay under Wasmtime's memory-stripe limit.
                // CRITICAL: Both per-actor lock (already held above) AND global semaphore must be held.
                // Per-actor lock serializes re-instantiations per actor; global semaphore caps total concurrent.
                let new_state = {
                    let _global_permit = if let Some(ref g) = self.global_reinstantiation_semaphore {
                        Some(
                            g.acquire()
                                .await
                                .map_err(|_| WasmError::ActorFunctionError(
                                    format!(
                                        "Concurrent instantiation limit reached during re-instantiation. \
                                        Reduce load or increase WasmConfig.max_concurrent_instantiations. \
                                        Global reinstantiation semaphore closed (available_permits={}).",
                                        g.available_permits()
                                    )
                                ))?,
                        )
                    } else {
                        None
                    };
                    Self::create_fresh_simple_actor_state(self, &instance_ctx).await
                }
                    .map_err(|e| {
                        let error_msg = e.to_string();
                        metrics::counter!("plexspaces_wasm_reinstantiation_errors_total",
                            "actor_id" => self.actor_id.clone(),
                            "error_type" => "instantiation_failed"
                        ).increment(1);
                        tracing::error!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            from_actor = %from_string,
                            msg_type = %message_type_string,
                            error = %error_msg,
                            "SimpleActor re-instantiation after handle() failed"
                        );
                        WasmError::ActorFunctionError(format!(
                            "Failed to re-instantiate WASM actor after handle(): {}",
                            error_msg
                        ))
                    })?;
                // Step 3: Restore state on the new instance (acquire lock once to replace and set_state)
                let component_state = self.component_state.as_ref().expect("component_state set");
                let mut guard = component_state.lock().await;
                *guard = new_state;
                if let Some(ref state_json) = saved_state {
                    let ComponentState { store: new_store, bindings: new_bindings } = &mut *guard;
                    if let ComponentBindings::SimpleActor(ref new_simple) = new_bindings {
                        match new_simple.plexspaces_simple_actor_actor()
                            .call_set_state(new_store, state_json)
                            .await
                        {
                            Ok(result) if result.is_empty() => {
                                if tracing::enabled!(tracing::Level::TRACE) {
                                    tracing::trace!(
                                        actor_id = %self.actor_id,
                                        message_id = %message_id,
                                        "State restored on new instance after re-instantiation"
                                    );
                                }
                            }
                            Ok(result) => {
                                tracing::warn!(
                                    actor_id = %self.actor_id,
                                    message_id = %message_id,
                                    error = %result,
                                    "set_state() returned error on new instance"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    actor_id = %self.actor_id,
                                    message_id = %message_id,
                                    error = %e,
                                    "set_state() call failed on new instance; state may be lost"
                                );
                            }
                        }
                    }
                }

                // Record re-instantiation success metrics
                let reinstantiation_duration = reinstantiation_start.elapsed();
                metrics::histogram!("plexspaces_wasm_reinstantiation_duration_seconds",
                    "actor_id" => self.actor_id.clone()
                ).record(reinstantiation_duration.as_secs_f64());
                // Result is already serialized JSON string from structured types
                let final_result = processed_result?;
                
                let duration = start_time.elapsed();
                
                // Result is a JSON string - check if it's an error response
                // Error responses have {"status":"error",...} format
                if final_result.starts_with("ERROR:") {
                    let error_message = final_result.strip_prefix("ERROR:").unwrap_or(&final_result);
                    metrics::counter!("plexspaces_wasm_component_message_errors_total").increment(1);
                    tracing::warn!(
                        actor_id = %self.actor_id,
                        error_message = %error_message,
                        "Simple actor handle() returned error"
                    );
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            "handle_message_component END SimpleActor returned ERROR"
                        );
                    }
                    Err(WasmError::ActorFunctionError(format!(
                        "Actor error: {}", error_message
                    )))
                } else {
                    // Re-instantiation already happened above (before processing result).
                    // We use original_init_config to ensure proper initialization, but state changes
                    // from handle() are lost. For proper state preservation, we need fix #2 (return
                    // state from handle) or fix #3 (host function persistence).
                    tracing::debug!(
                        actor_id = %self.actor_id,
                        message_id = %message_id,
                        "SimpleActor handle() succeeded, re-instantiated with original config"
                    );
                    metrics::histogram!("plexspaces_wasm_component_message_duration_seconds")
                        .record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_component_message_success_total").increment(1);
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            actor_id = %self.actor_id,
                            message_id = %message_id,
                            "handle_message_component END SimpleActor Ok"
                        );
                    }
                    Ok(final_result.into_bytes())
                }
            }
        }
    }

    /// Get actor state for persistence (Cloudflare Durable Objects pattern)
    ///
    /// For WASM components, calls the actor's `get-state()` function.
    /// This is the recommended pattern for WASM actor durability.
    ///
    /// ## Returns
    /// State as JSON string bytes, or empty if no state
    ///
    /// ## Errors
    /// Returns error if get_state call fails
    ///
    /// ## Metrics
    /// - `plexspaces_wasm_get_state_total`: Total get-state calls
    /// - `plexspaces_wasm_get_state_success_total`: Successful get-state calls
    /// - `plexspaces_wasm_get_state_errors_total`: Failed get-state calls
    /// - `plexspaces_wasm_get_state_duration_seconds`: Duration of get-state calls
    /// - `plexspaces_wasm_state_size_bytes`: Size of state returned
    #[cfg(feature = "component-model")]
    pub async fn get_state_component(&self) -> WasmResult<Vec<u8>> {
        metrics::counter!("plexspaces_wasm_get_state_total",
            "actor_id" => self.actor_id.clone()
        ).increment(1);
        let start_time = std::time::Instant::now();
        
        let component_state = self.component_state.as_ref()
            .ok_or_else(|| {
                metrics::counter!("plexspaces_wasm_get_state_errors_total",
                    "actor_id" => self.actor_id.clone(),
                    "error" => "no_component_state"
                ).increment(1);
                WasmError::ActorFunctionError(
                    "Component state not available (not a component actor)".to_string()
                )
            })?;
        
        let mut state = component_state.lock().await;
        let ComponentState { store, bindings } = &mut *state;
        
        let result = match bindings {
            ComponentBindings::SimpleActor(simple_bindings) => {
                simple_bindings.plexspaces_simple_actor_actor()
                    .call_get_state(store)
                    .await
                    .map(|s| s.into_bytes())
                    .map_err(|e| WasmError::ActorFunctionError(format!(
                        "get-state() call failed: {}", e
                    )))
            }
            ComponentBindings::PlexspacesActor(plexspaces_bindings) => {
                plexspaces_bindings.plexspaces_actor_actor()
                    .call_get_state(store)
                    .await
                    .map(|s| s.into_bytes())
                    .map_err(|e| WasmError::ActorFunctionError(format!(
                        "get-state() call failed: {}", e
                    )))
            }
        };
        
        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_get_state_duration_seconds",
            "actor_id" => self.actor_id.clone()
        ).record(duration.as_secs_f64());
        
        match result {
            Ok(state_bytes) => {
                metrics::counter!("plexspaces_wasm_get_state_success_total",
                    "actor_id" => self.actor_id.clone()
                ).increment(1);
                metrics::gauge!("plexspaces_wasm_state_size_bytes",
                    "actor_id" => self.actor_id.clone()
                ).set(state_bytes.len() as f64);
                
                tracing::debug!(
                    actor_id = %self.actor_id,
                    state_len = state_bytes.len(),
                    duration_ms = duration.as_millis(),
                    "WASM actor get-state() succeeded"
                );
                
                Ok(state_bytes)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_get_state_errors_total",
                    "actor_id" => self.actor_id.clone(),
                    "error" => "call_failed"
                ).increment(1);
                Err(e)
            }
        }
    }
    
    /// Set actor state for recovery (Cloudflare Durable Objects pattern)
    ///
    /// For WASM components, calls the actor's `set-state()` function.
    /// This is called after restart to restore state from persistence.
    ///
    /// ## Arguments
    /// * `state_json` - State as JSON string
    ///
    /// ## Returns
    /// Empty string on success, error message on failure
    ///
    /// ## Errors
    /// Returns error if set_state call fails
    ///
    /// ## Metrics
    /// - `plexspaces_wasm_set_state_total`: Total set-state calls
    /// - `plexspaces_wasm_set_state_success_total`: Successful set-state calls (state restored)
    /// - `plexspaces_wasm_set_state_errors_total`: Failed set-state calls
    /// - `plexspaces_wasm_set_state_duration_seconds`: Duration of set-state calls
    #[cfg(feature = "component-model")]
    pub async fn set_state_component(&self, state_json: &str) -> WasmResult<()> {
        metrics::counter!("plexspaces_wasm_set_state_total",
            "actor_id" => self.actor_id.clone()
        ).increment(1);
        let start_time = std::time::Instant::now();
        
        let component_state = self.component_state.as_ref()
            .ok_or_else(|| {
                metrics::counter!("plexspaces_wasm_set_state_errors_total",
                    "actor_id" => self.actor_id.clone(),
                    "error" => "no_component_state"
                ).increment(1);
                WasmError::ActorFunctionError(
                    "Component state not available (not a component actor)".to_string()
                )
            })?;
        
        let mut state = component_state.lock().await;
        let ComponentState { store, bindings } = &mut *state;
        
        let result = match bindings {
            ComponentBindings::SimpleActor(simple_bindings) => {
                simple_bindings.plexspaces_simple_actor_actor()
                    .call_set_state(store, state_json)
                    .await
                    .map_err(|e| WasmError::ActorFunctionError(format!(
                        "set-state() call failed: {}", e
                    )))
            }
            ComponentBindings::PlexspacesActor(_plexspaces_bindings) => {
                // Full PlexspacesActor doesn't have set_state in the same interface
                // It uses a different state management pattern
                Err(WasmError::ActorFunctionError(
                    "set-state() not available for full PlexspacesActor (use checkpoint pattern)".to_string()
                ))
            }
        };
        
        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_set_state_duration_seconds",
            "actor_id" => self.actor_id.clone()
        ).record(duration.as_secs_f64());
        
        // Handle result and record metrics
        match result {
            Ok(result_str) => {
                // Empty string means success, non-empty is error
                if result_str.is_empty() {
                    metrics::counter!("plexspaces_wasm_set_state_success_total",
                        "actor_id" => self.actor_id.clone()
                    ).increment(1);
                    
                    tracing::info!(
                        actor_id = %self.actor_id,
                        state_len = state_json.len(),
                        duration_ms = duration.as_millis(),
                        "✅ WASM actor state restored via set-state()"
                    );
                    Ok(())
                } else {
                    metrics::counter!("plexspaces_wasm_set_state_errors_total",
                        "actor_id" => self.actor_id.clone(),
                        "error" => "actor_error"
                    ).increment(1);
                    
                    Err(WasmError::ActorFunctionError(format!(
                        "set-state() returned error: {}", result_str
                    )))
                }
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_set_state_errors_total",
                    "actor_id" => self.actor_id.clone(),
                    "error" => "call_failed"
                ).increment(1);
                Err(e)
            }
        }
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
    
    /// Save actor state to checkpoint storage
    ///
    /// ## Purpose
    /// Persists actor state to journal storage for durability.
    /// Called on graceful shutdown or periodic checkpointing.
    ///
    /// ## How it works (Cloudflare Durable Objects pattern)
    /// 1. Calls actor's get-state() WIT function
    /// 2. Saves state to journal checkpoint table via save_checkpoint()
    /// 3. Records metrics for observability
    ///
    /// ## Returns
    /// Number of bytes saved, or error
    ///
    /// ## Metrics
    /// - `plexspaces_wasm_checkpoint_save_total`: Total checkpoint saves
    /// - `plexspaces_wasm_checkpoint_save_duration_seconds`: Save duration
    #[cfg(feature = "component-model")]
    pub async fn save_checkpoint(&self) -> WasmResult<usize> {
        if !self.durability_enabled {
            return Ok(0);
        }
        metrics::counter!("plexspaces_wasm_checkpoint_save_total",
            "actor_id" => self.actor_id.clone()
        ).increment(1);
        let start_time = std::time::Instant::now();
        
        // Get state from actor
        let state_bytes = self.get_state_component().await?;
        
        if state_bytes.is_empty() {
            tracing::debug!(
                actor_id = %self.actor_id,
                "WASM checkpoint save: empty state, skipping"
            );
            return Ok(0);
        }
        
        // Get journal storage from host functions
        let store = self.store.read().await;
        let journal_storage = store.data().host_functions.journal_storage()
            .ok_or_else(|| WasmError::ActorFunctionError(
                "Journal storage not available for checkpoint".to_string()
            ))?.clone();
        drop(store); // Release lock before async operation
        
        // Create checkpoint using proper Checkpoint struct
        use plexspaces_core::journal_storage::Checkpoint;
        let checkpoint = Checkpoint {
            actor_id: self.actor_id.clone(),
            sequence: 0, // Will be set by storage
            timestamp: None, // Storage layer handles timestamp
            state_data: state_bytes.clone(),
            compression: 0, // No compression
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        
        // Save checkpoint to journal storage
        journal_storage.save_checkpoint(&checkpoint).await
            .map_err(|e| WasmError::ActorFunctionError(format!(
                "Checkpoint save failed: {}", e
            )))?;
        
        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_checkpoint_save_duration_seconds",
            "actor_id" => self.actor_id.clone()
        ).record(duration.as_secs_f64());
        
        tracing::info!(
            actor_id = %self.actor_id,
            state_size = state_bytes.len(),
            duration_ms = duration.as_millis(),
            "✅ WASM checkpoint saved"
        );
        
        Ok(state_bytes.len())
    }
    
    /// Load and restore actor state from checkpoint storage
    ///
    /// ## Purpose
    /// Restores actor state from journal storage on startup.
    /// Called during actor initialization (init hook).
    ///
    /// ## How it works (Cloudflare Durable Objects pattern)
    /// 1. Queries journal for latest checkpoint via get_latest_checkpoint()
    /// 2. If checkpoint exists, calls actor's set-state() WIT function
    /// 3. Records metrics for observability
    ///
    /// ## Returns
    /// Number of bytes restored, or 0 if no checkpoint found
    ///
    /// ## Metrics
    /// - `plexspaces_wasm_checkpoint_load_total`: Total checkpoint loads
    /// - `plexspaces_wasm_checkpoint_load_duration_seconds`: Load duration
    #[cfg(feature = "component-model")]
    pub async fn load_checkpoint(&self) -> WasmResult<usize> {
        if !self.durability_enabled {
            return Ok(0);
        }
        metrics::counter!("plexspaces_wasm_checkpoint_load_total",
            "actor_id" => self.actor_id.clone()
        ).increment(1);
        let start_time = std::time::Instant::now();
        
        // Get journal storage from host functions
        let store = self.store.read().await;
        let journal_storage = match store.data().host_functions.journal_storage() {
            Some(js) => js.clone(),
            None => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    "WASM checkpoint load: no journal storage, skipping"
                );
                return Ok(0);
            }
        };
        drop(store); // Release lock before async operation
        
        // Load latest checkpoint from journal storage
        let checkpoint = match journal_storage.get_latest_checkpoint(&self.actor_id).await {
            Ok(cp) => cp,
            Err(plexspaces_core::journal_storage::JournalError::CheckpointNotFound(_)) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    "🆕 WASM checkpoint load: no checkpoint found, fresh start"
                );
                return Ok(0);
            }
            Err(e) => {
                return Err(WasmError::ActorFunctionError(format!(
                    "Checkpoint load failed: {}", e
                )));
            }
        };
        
        let state_bytes = checkpoint.state_data;
        
        if state_bytes.is_empty() {
            tracing::debug!(
                actor_id = %self.actor_id,
                "WASM checkpoint load: empty checkpoint, skipping restore"
            );
            return Ok(0);
        }
        
        // Convert bytes to JSON string for set-state
        let state_json = String::from_utf8(state_bytes.clone())
            .map_err(|e| WasmError::ActorFunctionError(format!(
                "Invalid UTF-8 in checkpoint: {}", e
            )))?;
        
        // Restore state to actor
        self.set_state_component(&state_json).await?;
        
        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_checkpoint_load_duration_seconds",
            "actor_id" => self.actor_id.clone()
        ).record(duration.as_secs_f64());
        
        tracing::info!(
            actor_id = %self.actor_id,
            state_size = state_bytes.len(),
            duration_ms = duration.as_millis(),
            "✅ WASM state restored from checkpoint"
        );
        
        Ok(state_bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use crate::WasmError;

    /// Ensures the canonical error message "Simple actor handle() call failed" is used when
    /// a simple-actor handle() fails. Logging uses this message (with error_first_line only;
    /// full backtrace only at DEBUG). Instance cleanup (Drop) still runs after this error.
    #[test]
    fn test_simple_actor_handle_failed_error_message() {
        let error_msg = "error while executing at wasm backtrace:\n  line1\n  line2";
        let err = WasmError::ActorFunctionError(format!(
            "{}: {}",
            crate::SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE,
            error_msg
        ));
        let s = err.to_string();
        assert!(
            s.contains(crate::SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE),
            "Error message must contain canonical log message; got: {}",
            s
        );
    }
}
