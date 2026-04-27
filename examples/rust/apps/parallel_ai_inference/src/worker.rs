use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct InferenceWorkerActor;

#[plexspaces_handlers(wasm)]
impl InferenceWorkerActor {
    #[handler("infer")]
    fn infer(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_infer(payload))
    }

    #[handler("get_metrics")]
    fn get_metrics(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_get_metrics())
    }

    #[handler("get_numeric_stats")]
    fn get_numeric_stats(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_worker_get_numeric_stats())
    }

    #[handler("reset")]
    fn reset(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_reset())
    }
}
