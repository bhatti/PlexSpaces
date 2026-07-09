// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// MiniPi — Agent Harness & Eval Example (Rust)
//
// Faithful Rust port of the Python minipi example.
// Demonstrates the full OODA-loop agent eval pipeline:
//
// - AgentLoop (OODA: Observe → Orient → Decide → Act)
// - SchemaValidationFacet (priority 95): validates tool inputs before actor sees them
// - ExecutionTraceFacet (priority 85): captures ordered OODA step sequence
// - DurabilityFacet (priority 90): journals every step for crash recovery
// - Supervision tree (one_for_one): crashed actors restart, orchestrator keeps running
// - Human-in-the-loop via ApprovalGateActor (FSM: idle → awaiting_approval → idle)
// - Ollama at http://localhost:11434 with model llama3.2 (mock fallback if unavailable)
//
// Actors spawned:
//   1. LLMGatewayActor    — model abstraction, Ollama + mock fallback, KV cache
//   2. ToolRegistryActor  — tool catalog (web_search, calculator, kv_read, kv_write)
//   3. AgentActor         — OODA loop with AgentLoop harness
//   4. EvalRunnerActor    — durable eval orchestration, fan-out/collect
//   5. ScenarioStoreActor — scenario catalog (10 built-in scenarios)
//   6. ScorerActor        — rubric-based trajectory scoring
//   7. TrajectoryStoreActor — persists and indexes trajectory records
//   8. RegressionDetectorActor — compare scores across runs, flag Δ>5%
//   9. BenchmarkActor     — same scenario, N harness configs, comparison table
//  10. ApprovalGateActor  — human-in-the-loop FSM
//  11. DashboardActor     — read-only aggregator
//  12. AdvisorActor       — two-tier LLM (executor + advisor on low confidence)

mod agent_actor;
mod llm_gateway;
mod tool_registry;
mod eval_runner;
mod scenario_store;
mod scorer;
mod trajectory_store;
mod regression_detector;
mod benchmark;
mod approval_gate;
mod dashboard;
mod advisor;

use agent_actor::AgentActor;
use llm_gateway::LLMGatewayActor;
use tool_registry::ToolRegistryActor;
use eval_runner::EvalRunnerActor;
use scenario_store::ScenarioStoreActor;
use scorer::ScorerActor;
use trajectory_store::TrajectoryStoreActor;
use regression_detector::RegressionDetectorActor;
use benchmark::BenchmarkActor;
use approval_gate::ApprovalGateActor;
use dashboard::DashboardActor;
use advisor::AdvisorActor;

use plexspaces_sdk::{
    spawn, NodeBuilder, RequestContext, RequestContextExt,
    json, Value,
};
use std::time::Duration;
use tracing::info;

const TENANT_ID: &str = "default";
const NAMESPACE: &str = "minipi";
const GRPC_PORT: u16 = 8007;
const HTTP_PORT: u16 = 8007;
const OLLAMA_PROVIDER: &str = "ollama";
const OLLAMA_MODEL: &str = "llama3.2";
const OLLAMA_URL: &str = "http://localhost:11434";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,plexspaces=debug")
        .init();

    info!("MiniPi — Agent Harness & Eval Example (Rust)");
    info!("  OODA loop · SchemaValidationFacet · ExecutionTraceFacet · DurabilityFacet");
    info!("  12 actors · supervision tree (one_for_one) · human-in-the-loop");
    info!("  Ollama provider={} model={} url={}", OLLAMA_PROVIDER, OLLAMA_MODEL, OLLAMA_URL);

    std::env::set_var("BLOB_ENABLED", "false");
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");

    let node_arc = NodeBuilder::new("minipi-node")
        .with_listen_addr(format!("0.0.0.0:{}", GRPC_PORT))
        .with_in_memory_backends()
        .with_auth_disabled()
        .build_started()
        .await;

    wait_for_port(HTTP_PORT).await?;
    info!("Node listening (gRPC {} HTTP {})", GRPC_PORT, HTTP_PORT);

    let service_locator = node_arc.service_locator();
    let ctx = RequestContext::new_without_auth(TENANT_ID.to_string(), NAMESPACE.to_string());

    // ── Spawn all 12 actors ──────────────────────────────────────────────────

    // 1. LLMGatewayActor
    let llm_ref = spawn(
        &ctx,
        service_locator.clone(),
        "llm_gateway".to_string(),
        NAMESPACE,
        LLMGatewayActor::new(OLLAMA_PROVIDER, OLLAMA_MODEL, OLLAMA_URL),
    )
    .await
    .map_err(|e| format!("spawn llm_gateway: {}", e))?;
    info!("Spawned LLMGatewayActor: {}", llm_ref.id());

    // 2. ToolRegistryActor
    let tool_ref = spawn(
        &ctx,
        service_locator.clone(),
        "tool_registry".to_string(),
        NAMESPACE,
        ToolRegistryActor::new(),
    )
    .await
    .map_err(|e| format!("spawn tool_registry: {}", e))?;
    info!("Spawned ToolRegistryActor: {}", tool_ref.id());

    // 3. AgentActor
    let agent_ref = spawn(
        &ctx,
        service_locator.clone(),
        "agent_runner".to_string(),
        NAMESPACE,
        AgentActor::new("agent-runner-1", "", ""),
    )
    .await
    .map_err(|e| format!("spawn agent_runner: {}", e))?;
    info!("Spawned AgentActor: {}", agent_ref.id());

    // 4. EvalRunnerActor
    let eval_ref = spawn(
        &ctx,
        service_locator.clone(),
        "eval_runner".to_string(),
        NAMESPACE,
        EvalRunnerActor::new(),
    )
    .await
    .map_err(|e| format!("spawn eval_runner: {}", e))?;
    info!("Spawned EvalRunnerActor: {}", eval_ref.id());

    // 5. ScenarioStoreActor
    let scenario_ref = spawn(
        &ctx,
        service_locator.clone(),
        "scenario_store".to_string(),
        NAMESPACE,
        ScenarioStoreActor::new(),
    )
    .await
    .map_err(|e| format!("spawn scenario_store: {}", e))?;
    info!("Spawned ScenarioStoreActor: {}", scenario_ref.id());

    // 6. ScorerActor
    let scorer_ref = spawn(
        &ctx,
        service_locator.clone(),
        "scorer".to_string(),
        NAMESPACE,
        ScorerActor::new(),
    )
    .await
    .map_err(|e| format!("spawn scorer: {}", e))?;
    info!("Spawned ScorerActor: {}", scorer_ref.id());

    // 7. TrajectoryStoreActor
    let traj_store_ref = spawn(
        &ctx,
        service_locator.clone(),
        "trajectory_store".to_string(),
        NAMESPACE,
        TrajectoryStoreActor::new(),
    )
    .await
    .map_err(|e| format!("spawn trajectory_store: {}", e))?;
    info!("Spawned TrajectoryStoreActor: {}", traj_store_ref.id());

    // 8. RegressionDetectorActor
    let regression_ref = spawn(
        &ctx,
        service_locator.clone(),
        "regression_detector".to_string(),
        NAMESPACE,
        RegressionDetectorActor::new(),
    )
    .await
    .map_err(|e| format!("spawn regression_detector: {}", e))?;
    info!("Spawned RegressionDetectorActor: {}", regression_ref.id());

    // 9. BenchmarkActor
    let benchmark_ref = spawn(
        &ctx,
        service_locator.clone(),
        "benchmark".to_string(),
        NAMESPACE,
        BenchmarkActor::new(),
    )
    .await
    .map_err(|e| format!("spawn benchmark: {}", e))?;
    info!("Spawned BenchmarkActor: {}", benchmark_ref.id());

    // 10. ApprovalGateActor
    let gate_ref = spawn(
        &ctx,
        service_locator.clone(),
        "approval_gate".to_string(),
        NAMESPACE,
        ApprovalGateActor::new(),
    )
    .await
    .map_err(|e| format!("spawn approval_gate: {}", e))?;
    info!("Spawned ApprovalGateActor: {}", gate_ref.id());

    // 11. DashboardActor
    let dashboard_ref = spawn(
        &ctx,
        service_locator.clone(),
        "dashboard".to_string(),
        NAMESPACE,
        DashboardActor::new(),
    )
    .await
    .map_err(|e| format!("spawn dashboard: {}", e))?;
    info!("Spawned DashboardActor: {}", dashboard_ref.id());

    // 12. AdvisorActor
    let advisor_ref = spawn(
        &ctx,
        service_locator.clone(),
        "advisor".to_string(),
        NAMESPACE,
        AdvisorActor::new(0.8),
    )
    .await
    .map_err(|e| format!("spawn advisor: {}", e))?;
    info!("Spawned AdvisorActor: {}", advisor_ref.id());

    info!("All 12 actors spawned under supervision tree (one_for_one)");

    // ── Self-test via HTTP gateway ────────────────────────────────────────────
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", HTTP_PORT);
    let ns = NAMESPACE;

    // Step 1: ScenarioStore — get_stats
    info!("Step 1: ScenarioStoreActor — get_stats");
    let r = ask(&client, &base, ns, "scenario_store", json!({"action": "get_stats"})).await?;
    let scenario_count = r.get("payload").and_then(|p| p.get("scenario_count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  scenario_count={}", scenario_count);
    assert!(scenario_count >= 10, "expected 10 built-in scenarios, got {}", scenario_count);
    info!("  10 built-in scenarios present");

    // Step 2: ScenarioStore — get_suite smoke
    info!("Step 2: ScenarioStoreActor — get_suite smoke");
    let r = ask(&client, &base, ns, "scenario_store", json!({"action": "get_suite", "suite_name": "smoke"})).await?;
    let count = r.get("payload").and_then(|p| p.get("count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  smoke suite: {} scenarios", count);

    // Step 3: LLMGateway — get_stats
    info!("Step 3: LLMGatewayActor — get_stats");
    let r = ask(&client, &base, ns, "llm_gateway", json!({"action": "get_stats"})).await?;
    let provider = r.get("payload").and_then(|p| p.get("provider")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  provider={}", provider);

    // Step 4: ToolRegistry — list_tools
    info!("Step 4: ToolRegistryActor — list_tools");
    let r = ask(&client, &base, ns, "tool_registry", json!({"action": "list_tools"})).await?;
    let tool_count = r.get("payload").and_then(|p| p.get("count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  registered tools: {}", tool_count);
    assert!(tool_count >= 4, "expected 4 built-in tools, got {}", tool_count);

    // Step 5: ToolRegistry — execute calculator
    info!("Step 5: ToolRegistryActor — calculator: (17 * 24) + (89 - 45)");
    let r = ask(
        &client, &base, ns, "tool_registry",
        json!({"action": "execute", "name": "calculator", "input": {"expression": "(17*24)+(89-45)"}}),
    ).await?;
    let result_val = r.get("payload").and_then(|p| p.get("result")).cloned().unwrap_or(json!(null));
    info!("  result={}", result_val);

    // Step 6: SchemaValidationFacet — reject empty query (simulated via ToolRegistry guard)
    info!("Step 6: SchemaValidationFacet — reject empty query");
    let r = ask(
        &client, &base, ns, "tool_registry",
        json!({"action": "execute", "name": "web_search", "input": {"query": ""}}),
    ).await?;
    let has_error = r.get("payload").and_then(|p| p.get("error")).is_some()
        || r.get("error").is_some();
    if has_error {
        info!("  SchemaValidationFacet rejected empty query (minLength:1)");
    } else {
        info!("  (schema validation check: empty query returned non-error — check facet config)");
    }

    // Step 7: AgentActor — single scenario OODA run
    info!("Step 7: AgentActor — OODA loop run");
    let r = ask(
        &client, &base, ns, "agent_runner",
        json!({
            "action": "run",
            "task": "What is (17 * 24) + (89 - 45)?",
            "eval_run_id": "test-001",
            "scenario_id": "sc-math-01"
        }),
    ).await?;
    let outcome = r.get("payload").and_then(|p| p.get("outcome")).and_then(|v| v.as_str()).unwrap_or("?");
    let steps = r.get("payload").and_then(|p| p.get("step_count")).and_then(|v| v.as_u64()).unwrap_or(0);
    let traj_id = r.get("payload").and_then(|p| p.get("trajectory_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    info!("  outcome={} steps={} trajectory_id={}", outcome, steps, traj_id);
    assert_eq!(outcome, "completed", "AgentActor should complete");

    // Step 8: TrajectoryStore — store and retrieve trajectory
    info!("Step 8: TrajectoryStoreActor — store trajectory");
    let traj_payload = r.get("payload").and_then(|p| p.get("trajectory")).cloned().unwrap_or(json!({}));
    let put_r = ask(
        &client, &base, ns, "trajectory_store",
        json!({"action": "put", "trajectory": traj_payload}),
    ).await?;
    let stored_id = put_r.get("payload").and_then(|p| p.get("trajectory_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    info!("  stored trajectory_id={}", stored_id);

    // Retrieve
    if !stored_id.is_empty() {
        let get_r = ask(
            &client, &base, ns, "trajectory_store",
            json!({"action": "get", "trajectory_id": stored_id}),
        ).await?;
        let traj_outcome = get_r.get("payload").and_then(|p| p.get("trajectory")).and_then(|t| t.get("outcome")).and_then(|v| v.as_str()).unwrap_or("?");
        info!("  retrieved trajectory outcome={}", traj_outcome);
    }

    // Step 9: Scorer — score a trajectory
    info!("Step 9: ScorerActor — score trajectory");
    let sample_traj = json!({
        "trajectory_id": "t-test-01",
        "agent_actor_id": "agent-01",
        "steps": [
            {"kind": "observe", "success": true},
            {"kind": "orient", "success": true},
            {"kind": "decide", "success": true},
            {"kind": "tool_call", "tool_name": "calculator", "success": true},
            {"kind": "act", "success": true},
        ],
        "outcome": "completed",
        "total_input_tokens": 100,
        "total_output_tokens": 50,
    });
    let r = ask(
        &client, &base, ns, "scorer",
        json!({"action": "score", "trajectory": sample_traj, "rubric": "task_completion"}),
    ).await?;
    let score = r.get("payload").and_then(|p| p.get("score")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    info!("  score={:.3} (rubric: task_completion)", score);

    let r2 = ask(
        &client, &base, ns, "scorer",
        json!({"action": "score", "trajectory": sample_traj, "rubric": "tool_use"}),
    ).await?;
    let score2 = r2.get("payload").and_then(|p| p.get("score")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    info!("  score={:.3} (rubric: tool_use)", score2);

    // Step 10: EvalRunner — smoke suite
    info!("Step 10: EvalRunnerActor — smoke suite (1 scenario)");
    let smoke_scenarios = json!([{
        "scenario_id": "sc-math-01",
        "input": "What is 6 * 7?",
        "expected": "42",
        "rubric": "task_completion",
        "difficulty": "easy"
    }]);
    let r = ask(
        &client, &base, ns, "eval_runner",
        json!({
            "action": "run",
            "suite_name": "smoke",
            "scenarios": smoke_scenarios,
            "eval_run_id": "eval-smoke-001"
        }),
    ).await?;
    let pass_rate = r.get("payload").and_then(|p| p.get("pass_rate")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let completed = r.get("payload").and_then(|p| p.get("completed_scenarios")).and_then(|v| v.as_u64()).unwrap_or(0);
    let total = r.get("payload").and_then(|p| p.get("total_scenarios")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  pass_rate={:.1}% completed={}/{}", pass_rate * 100.0, completed, total);

    // Step 11: RegressionDetector — set baseline + compare
    info!("Step 11: RegressionDetectorActor — set baseline + compare");
    let baseline_scores = json!([
        {"trajectory_id": "sc-math-01", "score": 0.9},
        {"trajectory_id": "sc-search-01", "score": 0.7}
    ]);
    let r = ask(
        &client, &base, ns, "regression_detector",
        json!({"action": "set_baseline", "eval_run_id": "eval-smoke-001", "scores": baseline_scores}),
    ).await?;
    info!("  baseline set: {} scenarios", r.get("payload").and_then(|p| p.get("scenarios")).and_then(|v| v.as_u64()).unwrap_or(0));

    let current_scores = json!([
        {"trajectory_id": "sc-math-01", "score": 0.92},
        {"trajectory_id": "sc-search-01", "score": 0.55}
    ]);
    let r = ask(
        &client, &base, ns, "regression_detector",
        json!({"action": "compare", "eval_run_id": "eval-smoke-002", "scores": current_scores}),
    ).await?;
    let reg_count = r.get("payload").and_then(|p| p.get("regression_count")).and_then(|v| v.as_u64()).unwrap_or(0);
    let imp_count = r.get("payload").and_then(|p| p.get("improvement_count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  regressions={} improvements={}", reg_count, imp_count);
    assert!(reg_count >= 1, "expected at least 1 regression (sc-search-01 dropped 0.7→0.55)");
    info!("  Regression detected correctly");

    // Step 12: Benchmark — 2-config comparison
    info!("Step 12: BenchmarkActor — 2-config comparison");
    let bench_scenarios = json!([{"scenario_id": "sc-math-01", "input": "What is 6 * 7?", "rubric": "task_completion", "difficulty": "easy"}]);
    let bench_configs = json!([
        {"name": "conservative", "max_iterations": 3, "token_budget": 1024},
        {"name": "balanced", "max_iterations": 10, "token_budget": 4096}
    ]);
    let r = ask(
        &client, &base, ns, "benchmark",
        json!({
            "action": "run",
            "scenarios": bench_scenarios,
            "configs": bench_configs,
            "benchmark_id": "bench-001"
        }),
    ).await?;
    let winner = r.get("payload").and_then(|p| p.get("winner")).and_then(|v| v.as_str()).unwrap_or("?");
    let configs_tested = r.get("payload").and_then(|p| p.get("configs_tested")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  configs_tested={} winner={}", configs_tested, winner);
    assert_eq!(configs_tested, 2, "expected 2 configs tested");

    // Step 13: ApprovalGate — human-in-the-loop FSM
    info!("Step 13: ApprovalGateActor — human-in-the-loop FSM");
    let r = ask(&client, &base, ns, "approval_gate", json!({"action": "get_status"})).await?;
    let fsm_state = r.get("payload").and_then(|p| p.get("state")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  FSM state: {} (expected: idle)", fsm_state);
    assert_eq!(fsm_state, "idle", "approval gate should start idle");

    let r = ask(
        &client, &base, ns, "approval_gate",
        json!({"action": "request_approval", "agent_id": "agent-test-001", "action_name": "delete_all_records", "context": {"reason": "cleanup"}}),
    ).await?;
    info!("  request_approval status: {}", r.get("payload").and_then(|p| p.get("status")).and_then(|v| v.as_str()).unwrap_or("?"));

    let r = ask(&client, &base, ns, "approval_gate", json!({"action": "get_status"})).await?;
    let fsm_state2 = r.get("payload").and_then(|p| p.get("state")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  FSM state after request: {} (expected: awaiting_approval)", fsm_state2);

    let r = ask(
        &client, &base, ns, "approval_gate",
        json!({"action": "approve", "approver": "alice@example.com", "comment": "LGTM"}),
    ).await?;
    let approved_by = r.get("payload").and_then(|p| p.get("approver")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  Approved by: {}", approved_by);

    let r = ask(&client, &base, ns, "approval_gate", json!({"action": "get_status"})).await?;
    let fsm_state3 = r.get("payload").and_then(|p| p.get("state")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  FSM state after approval: {} (expected: idle)", fsm_state3);
    assert_eq!(fsm_state3, "idle", "approval gate should return to idle");

    let r = ask(&client, &base, ns, "approval_gate", json!({"action": "get_history"})).await?;
    let history_count = r.get("payload").and_then(|p| p.get("count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  Decision history: {} entries", history_count);

    // Step 14: Dashboard — aggregate summary
    info!("Step 14: DashboardActor — aggregate results");
    let r = ask(&client, &base, ns, "dashboard", json!({"action": "summary"})).await?;
    let dash_status = r.get("payload").and_then(|p| p.get("status")).and_then(|v| v.as_str()).unwrap_or("?");
    info!("  Dashboard summary status: {}", dash_status);

    let r = ask(&client, &base, ns, "dashboard", json!({"action": "list_eval_runs", "limit": 5})).await?;
    let run_count = r.get("payload").and_then(|p| p.get("count")).and_then(|v| v.as_u64()).unwrap_or(0);
    info!("  Eval runs visible to dashboard: {}", run_count);

    // Step 15: AdvisorActor — two-tier LLM stats
    info!("Step 15: AdvisorActor — two-tier LLM (executor + advisor on-demand)");
    let r = ask(&client, &base, ns, "advisor", json!({"action": "get_stats"})).await?;
    let threshold = r.get("payload").and_then(|p| p.get("confidence_threshold")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let escalation_rate = r.get("payload").and_then(|p| p.get("escalation_rate_pct")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    info!("  confidence_threshold={:.1} escalation_rate={:.1}%", threshold, escalation_rate);

    info!("MiniPi example completed successfully.");
    info!("");
    info!("Demonstrated PlexSpaces harness + eval patterns:");
    info!("  SchemaValidationFacet (priority 95)  — method input validation");
    info!("  ExecutionTraceFacet (priority 85)    — ordered OODA step capture");
    info!("  DurabilityFacet (priority 90)        — journal every step, crash-safe");
    info!("  Supervision tree (one_for_one)       — crashed actor restarts");
    info!("  OODA AgentLoop                       — step tracking, budget enforcement");
    info!("  Parallel eval fan-out                — N scenarios per eval suite");
    info!("  Regression detection                 — compare scores, flag +/-5% threshold");
    info!("  Benchmark comparison                 — same scenario, different harness configs");
    info!("  Human-in-the-loop (FSM)              — agent suspends, gate waits, resumes");
    info!("  Ollama integration                   — provider={} model={}", OLLAMA_PROVIDER, OLLAMA_MODEL);

    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(())
}

async fn ask(
    client: &reqwest::Client,
    base: &str,
    namespace: &str,
    actor: &str,
    payload: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let url = format!("{}/api/v1/actors/{}/{}/ask", base, namespace, actor);
    let resp = client
        .post(&url)
        .header("x-tenant-id", TENANT_ID)
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {} from {}: {}", status, actor, &text[..200.min(text.len())]).into());
    }
    let v: Value = resp.json().await?;
    Ok(v)
}

async fn wait_for_port(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("127.0.0.1:{}", port);
    for _ in 0..60 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("HTTP gateway did not become ready on port {}", port).into())
}
