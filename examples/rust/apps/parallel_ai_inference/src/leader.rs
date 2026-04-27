use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct BenchmarkActor;

#[plexspaces_handlers(wasm)]
impl BenchmarkActor {
    #[handler("run_shard_benchmark")]
    fn run_shard_benchmark(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_run_shard(payload))
    }

    #[handler("run_scaling_benchmark")]
    fn run_scaling_benchmark(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_run_scaling(payload))
    }

    #[handler("run_weak_scaling_benchmark")]
    fn run_weak_scaling_benchmark(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_run_weak_scaling(payload))
    }

    #[handler("run_pool_benchmark")]
    fn run_pool_benchmark(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_run_pool(payload))
    }

    #[handler("run_collective_benchmark")]
    fn run_collective_benchmark(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_run_collective(payload))
    }

    #[handler("get_results")]
    fn get_results(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_benchmark_get_results())
    }
}

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct OrchestratorActor;

#[plexspaces_handlers(wasm)]
impl OrchestratorActor {
    #[handler("workflow_run")]
    fn workflow_run(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_orchestrator_workflow_run(payload))
    }

    #[handler("workflow_signal:scale")]
    fn workflow_signal_scale(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_orchestrator_workflow_signal_scale(payload))
    }

    #[handler("workflow_query:status")]
    fn workflow_query_status(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_orchestrator_status())
    }
}
