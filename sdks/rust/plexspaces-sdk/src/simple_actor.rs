// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Actor-world WIT support for deployable Rust WASM applications.
//
// This module centralizes the WIT bindings and the boilerplate Guest/export glue
// so Rust WASM examples do not need to hand-write `wit_bindgen::generate!`,
// `impl Guest`, and `export!(...)` in every app.

wit_bindgen::generate!({
    path: "../../../wit/plexspaces-actor",
    world: "actor-world",
});

pub use exports::plexspaces::actor::actor::Guest;
pub use plexspaces::actor::host;

/// Decode a protobuf message from actor-world bytes.
pub fn decode_proto<M>(payload: &[u8]) -> Result<M, String>
where
    M: prost::Message + Default,
{
    M::decode(payload).map_err(|err| err.to_string())
}

/// Encode a protobuf message for the actor-world boundary.
pub fn encode_proto<M>(message: &M) -> Vec<u8>
where
    M: prost::Message,
{
    message.encode_to_vec()
}

/// Trait implemented by deployable actor-world WASM app roots.
///
/// The trait keeps the WIT boundary in the SDK while allowing app code to focus on
/// initialization, message handling, and protobuf-encoded state snapshots.
pub trait ActorWorldApp: Sized {
    type State;

    fn init(config: Vec<u8>) -> Result<Self, String>;
    fn handle(
        &mut self,
        from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String>;
    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
    fn encode_state(state: &Self::State) -> Result<Vec<u8>, String>;
    fn decode_state(state: &[u8]) -> Result<Self::State, String>;
}

/// Trait implemented by annotation-driven leader/worker handlers inside an actor-world app.
///
/// This keeps deployable WASM examples close to the native SDK style: actor structs declare
/// handlers with annotations, while the outer app only decides which role instance handles
/// the current message.
pub trait ActorWorldHandlers {
    /// Optional initialization hook for handler-local configuration.
    fn init(&mut self, _config: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// Dispatch one operation for the current role.
    fn handle_operation(
        &mut self,
        from_actor: &str,
        op: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String>;
}

/// Export an `ActorWorldApp` implementation as the `plexspaces:actor` guest.
///
/// The generated wrapper keeps one singleton app instance per WASM component instance and
/// handles `get-state` / `set-state` through the app's protobuf codec.
#[macro_export]
macro_rules! export_actor_world_app {
    ($app_ty:ty) => {
        struct __PlexspacesActorWorldComponent;

        static __PLEXSPACES_ACTOR_WORLD_APP: ::std::sync::OnceLock<::std::sync::Mutex<$app_ty>> =
            ::std::sync::OnceLock::new();

        impl $crate::simple_actor::Guest for __PlexspacesActorWorldComponent {
            fn init(
                config: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<(), ::std::string::String> {
                match <$app_ty as $crate::simple_actor::ActorWorldApp>::init(config) {
                    Ok(app) => {
                        if let Some(cell) = __PLEXSPACES_ACTOR_WORLD_APP.get() {
                            let mut guard =
                                cell.lock().expect("actor-world app state lock poisoned");
                            *guard = app;
                        } else {
                            let _ = __PLEXSPACES_ACTOR_WORLD_APP.set(::std::sync::Mutex::new(app));
                        }
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }

            fn handle(
                from_actor: ::std::string::String,
                msg_type: ::std::string::String,
                payload: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let mut guard = cell.lock().expect("actor-world app state lock poisoned");
                <$app_ty as $crate::simple_actor::ActorWorldApp>::handle(
                    &mut *guard,
                    from_actor,
                    msg_type,
                    payload,
                )
            }

            fn get_state() -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let guard = cell.lock().expect("actor-world app state lock poisoned");
                <$app_ty as $crate::simple_actor::ActorWorldApp>::encode_state(
                    <$app_ty as $crate::simple_actor::ActorWorldApp>::state(&*guard),
                )
            }

            fn set_state(
                state: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<(), ::std::string::String> {
                let next_state =
                    <$app_ty as $crate::simple_actor::ActorWorldApp>::decode_state(&state)?;
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let mut guard = cell.lock().expect("actor-world app state lock poisoned");
                *<$app_ty as $crate::simple_actor::ActorWorldApp>::state_mut(&mut *guard) =
                    next_state;
                Ok(())
            }
        }

        $crate::simple_actor::export!(__PlexspacesActorWorldComponent);
    };
}
