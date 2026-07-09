// SPDX-License-Identifier: AGPL-3.0-or-later
// MiniPi — Agent Harness & Eval Example (Go WASM)
//
// Demonstrates: OODA loop, SchemaValidationFacet, ExecutionTraceFacet,
// DurabilityFacet, supervision trees, human-in-the-loop, parallel eval.
//
// Registration wires all 12 actors to their role names from app-config.toml.
package main

import (
	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// init registers all actors with the ActorRouter.
// MUST be in init(), not main() — the router must be populated before the framework
// starts dispatching messages.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("llm_gateway", NewLLMGatewayActor)
	router.Route("tool_registry", NewToolRegistryActor)
	router.Route("eval_runner", NewEvalRunnerActor)
	router.Route("scorer", NewScorerActor)
	router.Route("scenario_store", NewScenarioStoreActor)
	router.Route("trajectory_store", NewTrajectoryStoreActor)
	router.Route("regression_detector", NewRegressionDetectorActor)
	router.Route("benchmark", NewBenchmarkActor)
	router.Route("approval_gate", NewApprovalGateActor)
	router.Route("dashboard", NewDashboardActor)
	// agent_runner is the per-eval spawned agent (same actor type as AgentActor)
	router.Route("agent_runner", NewAgentActor)
	// advisor implements the two-tier LLM pattern: cheap executor + expensive advisor on-demand
	router.Route("advisor", NewAdvisorActor)
	plexspaces.Register(router)
}

func main() {}
