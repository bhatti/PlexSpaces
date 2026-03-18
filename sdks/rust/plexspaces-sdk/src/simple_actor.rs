// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Simple-actor WIT support for deployable Rust WASM applications.
//
// This module centralizes the WIT bindings and the boilerplate Guest/export glue
// so Rust WASM examples do not need to hand-write `wit_bindgen::generate!`,
// `impl Guest`, and `export!(...)` in every app.

use serde::de::DeserializeOwned;
use serde::Serialize;

wit_bindgen::generate!({
    path: "../../../wit/plexspaces-simple-actor",
    world: "actor-world",
});

pub use exports::plexspaces::simple_actor::actor::Guest;
pub use plexspaces::simple_actor::host;

/// Trait implemented by simple-actor WASM app roots.
///
/// The trait keeps the WIT boundary in the SDK while allowing app code to focus on
/// initialization, message handling, and serializable state.
pub trait SimpleActorApp: Sized {
    type State: Default + Serialize + DeserializeOwned;

    fn init(config_json: String) -> Result<Self, String>;
    fn handle(&mut self, from_actor: String, msg_type: String, payload_json: String) -> String;
    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
}

/// Trait implemented by annotation-driven leader/worker handlers inside a simple-actor app.
///
/// This keeps deployable WASM examples close to the native SDK style: actor structs declare
/// handlers with annotations, while the outer app only decides which role instance handles
/// the current message.
pub trait SimpleActorHandlers {
    /// Optional initialization hook for handler-local configuration.
    fn init(&mut self, _config_json: &str) -> Result<(), String> {
        Ok(())
    }

    /// Dispatch one operation for the current role.
    fn handle_operation(
        &mut self,
        from_actor: &str,
        op: &str,
        payload_json: &str,
    ) -> Result<String, String>;
}

/// Export a `SimpleActorApp` implementation as the `plexspaces:simple-actor` guest.
///
/// The generated wrapper keeps one singleton app instance per WASM component instance and
/// handles `get-state` / `set-state` through serde.
#[macro_export]
macro_rules! export_simple_actor_app {
    ($app_ty:ty) => {
        struct __PlexspacesSimpleActorComponent;

        static __PLEXSPACES_SIMPLE_ACTOR_APP: ::std::sync::OnceLock<::std::sync::Mutex<$app_ty>> =
            ::std::sync::OnceLock::new();

        impl $crate::simple_actor::Guest for __PlexspacesSimpleActorComponent {
            fn init(config_json: ::std::string::String) -> ::std::string::String {
                match <$app_ty as $crate::simple_actor::SimpleActorApp>::init(config_json) {
                    Ok(app) => {
                        if let Some(cell) = __PLEXSPACES_SIMPLE_ACTOR_APP.get() {
                            let mut guard =
                                cell.lock().expect("simple actor app state lock poisoned");
                            *guard = app;
                        } else {
                            let _ = __PLEXSPACES_SIMPLE_ACTOR_APP.set(::std::sync::Mutex::new(app));
                        }
                        ::std::string::String::new()
                    }
                    Err(err) => err,
                }
            }

            fn handle(
                from_actor: ::std::string::String,
                msg_type: ::std::string::String,
                payload_json: ::std::string::String,
            ) -> ::std::string::String {
                let cell = __PLEXSPACES_SIMPLE_ACTOR_APP
                    .get()
                    .expect("simple actor app used before init");
                let mut guard = cell.lock().expect("simple actor app state lock poisoned");
                <$app_ty as $crate::simple_actor::SimpleActorApp>::handle(
                    &mut *guard,
                    from_actor,
                    msg_type,
                    payload_json,
                )
            }

            fn get_state() -> ::std::string::String {
                let cell = __PLEXSPACES_SIMPLE_ACTOR_APP
                    .get()
                    .expect("simple actor app used before init");
                let guard = cell.lock().expect("simple actor app state lock poisoned");
                ::serde_json::to_string(<$app_ty as $crate::simple_actor::SimpleActorApp>::state(
                    &*guard,
                ))
                .unwrap_or_else(|_| "{}".to_string())
            }

            fn set_state(state_json: ::std::string::String) -> ::std::string::String {
                let next_state = match ::serde_json::from_str::<
                    <$app_ty as $crate::simple_actor::SimpleActorApp>::State,
                >(&state_json)
                {
                    Ok(value) => value,
                    Err(err) => return format!("invalid state: {}", err),
                };
                let cell = __PLEXSPACES_SIMPLE_ACTOR_APP
                    .get()
                    .expect("simple actor app used before init");
                let mut guard = cell.lock().expect("simple actor app state lock poisoned");
                *<$app_ty as $crate::simple_actor::SimpleActorApp>::state_mut(&mut *guard) =
                    next_state;
                ::std::string::String::new()
            }
        }

        $crate::simple_actor::export!(__PlexspacesSimpleActorComponent);
    };
}
