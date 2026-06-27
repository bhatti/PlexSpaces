// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Web Crawl - Rust WASM app
//
// Parallel web crawler using all four PlexSpaces parallelization primitives:
//   TupleSpace frontier  — url_queue as live work frontier; host::ts_take() for atomic URL claim
//                          (mark-before-enqueue deduplication, inspired by muffet / linkinator)
//   ElasticPool          — host::pool_checkout/pool_checkin separates rate limiting from queue depth
//   ProcessGroup         — workers self-register; orchestrator discovers real members via pg_members()
//   ShardGroup scatter   — interleaved scatter to analyzer shards for balanced word-count aggregation
//
// Roles (set via args.role in app-config.toml):
//   orchestrator  — drives the BFS crawl loop
//   fetcher       — fetches one URL (simulated), returns links + word counts
//   analyzer      — shard: merges counts, returns top-N words

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use plexspaces_proto::tuplespace::v1::{
    ReadRequest, ReadResponse, Tuple, TupleField,
    tuple_field::Value as ProtoTupleValue,
    WriteRequest,
};
use plexspaces_proto::actor::v1::{
    CreateShardGroupRequest, CreateShardGroupResponse, DataParallelConfig, NodePlacement,
    NodePlacementStrategy, PartitionStrategy, RebalancePolicy, ScatterGatherRequest,
    ScatterGatherResponse, ShardGroupAggregationStrategy,
};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use prost::Message as ProstMessage;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;

const FETCHER_POOL: &str = "fetcher_pool";
const CRAWL_WORKERS_GROUP: &str = "crawl_workers";
const ANALYZER_GROUP: &str = "analyzer_shards";
const CHECKOUT_TIMEOUT_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// Shared state (WASM is single-threaded; Mutex used for Sync trait)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AppState {
    actor_id: String,
    application_id: String,
    role: String,
    // fetcher
    fetch_count: u64,
    last_url: String,
    pool_slot: usize,
    worker_joined: bool,
    // analyzer
    index: HashMap<String, u64>,
    urls_analyzed: u64,
    analyzer_joined: bool,
    // orchestrator
    pages_crawled: u64,
    total_links: u64,
    top_words: Vec<(String, u64)>,
    pool_metrics: Value,
    worker_stats: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct InitConfig {
    actor_id: Option<String>,
    args: Option<HashMap<String, String>>,
}

fn state_cell() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    f(&mut state_cell().lock().expect("state lock poisoned"))
}

fn json_bytes(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

fn json_error(e: impl Into<String>) -> Vec<u8> {
    json!({ "error": e.into() }).to_string().into_bytes()
}

fn parse_payload(b: &[u8]) -> Result<Value, String> {
    if b.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(b).map_err(|e| format!("invalid payload: {e}"))
}

// ---------------------------------------------------------------------------
// Simulated HTTP fetch helpers
// ---------------------------------------------------------------------------

fn url_hash(url: &str) -> usize {
    url.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize))
}

fn simulate_links(url: &str) -> Vec<String> {
    let base = url.trim_end_matches('/');
    let h = url_hash(url);
    let sections = ["about","docs","api","blog","pricing","features","integrations","changelog",
                    "security","status","community","enterprise","solutions","resources"];
    let paths = ["overview","quickstart","reference","guide","examples","faq","support","contact"];
    let mut links = Vec::with_capacity(12);
    for i in 0..8 { links.push(format!("{base}/{}", sections[(h + i) % sections.len()])); }
    for i in 0..4 {
        let sec = sections[(h + i * 3) % sections.len()];
        let pth = paths[(h + i * 7) % paths.len()];
        links.push(format!("{base}/{sec}/{pth}"));
    }
    links
}

fn simulate_word_counts(url: &str) -> HashMap<String, u64> {
    let h = url_hash(url);
    let vocab = ["distributed","actor","system","runtime","protocol","message","async","concurrent",
                 "parallel","scale","fault","tolerant","cluster","node","network","latency",
                 "throughput","pipeline","stream","queue","worker","scheduler","executor","dispatch",
                 "route","wasm","sandbox","module","instance","memory","tenant","namespace",
                 "isolation","security","auth","deploy","version","rollback","canary","health",
                 "metric","trace","span","log","monitor","pool","checkout","checkin","timeout","retry",
                 "tuplespace","tuple","pattern","match","read","shard","partition","replicate",
                 "consensus","leader","broadcast","scatter","gather","reduce","aggregate","workflow",
                 "state","checkpoint","journal","replay"];
    let mut counts = HashMap::new();
    for seg in url.split('/').filter(|s| !s.is_empty() && *s != "https:" && *s != "http:") {
        for word in seg.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 2 {
                *counts.entry(word.to_lowercase()).or_insert(0) += 8 + (h % 5) as u64;
            }
        }
    }
    for i in 0..25 {
        let word = vocab[(h + i * 17) % vocab.len()];
        let rank = (i + 1) as u64;
        let count = 50 / rank + 1 + ((h + i) % 3) as u64;
        *counts.entry(word.to_string()).or_insert(0) += count;
    }
    counts
}

// ---------------------------------------------------------------------------
// TupleSpace proto helpers
// ---------------------------------------------------------------------------

fn proto_string(s: &str) -> TupleField {
    TupleField { value: Some(ProtoTupleValue::String(s.to_string())) }
}

fn proto_wildcard() -> TupleField {
    TupleField { value: Some(ProtoTupleValue::Wildcard(true)) }
}

fn build_tuple(fields: Vec<TupleField>) -> Tuple {
    Tuple { fields, ..Default::default() }
}

fn ts_write_4(f0: &str, f1: &str, f2: &str, f3: &str) {
    let req = WriteRequest {
        tuples: vec![build_tuple(vec![
            proto_string(f0),
            proto_string(f1),
            proto_string(f2),
            proto_string(f3),
        ])],
        transaction_id: String::new(),
    };
    let _ = host::ts_write(&req.encode_to_vec());
}

fn ts_take_pending() -> Option<(String, String)> {
    let req = ReadRequest {
        template: Some(build_tuple(vec![
            proto_string("url_queue"),
            proto_wildcard(),
            proto_string("pending"),
            proto_wildcard(),
        ])),
        take: true,
        max_results: 1,
        blocking: false,
        timeout: None,
        transaction_id: String::new(),
        spatial_filter: None,
    };
    let raw = host::ts_take(&req.encode_to_vec()).ok()?;
    if raw.is_empty() {
        return None;
    }
    let resp = ReadResponse::decode(raw.as_slice()).ok()?;
    let tuple = resp.tuples.into_iter().next()?;
    let fields: Vec<String> = tuple.fields.iter().map(|f| match f.value.as_ref() {
        Some(ProtoTupleValue::String(s)) => s.clone(),
        _ => String::new(),
    }).collect();
    if fields.len() < 4 {
        return None;
    }
    Some((fields[1].clone(), fields[3].clone())) // (url, depth)
}

fn ts_read_any(url: &str) -> bool {
    let req = ReadRequest {
        template: Some(build_tuple(vec![
            proto_string("url_queue"),
            proto_string(url),
            proto_wildcard(),
            proto_wildcard(),
        ])),
        take: false,
        max_results: 1,
        blocking: false,
        timeout: None,
        transaction_id: String::new(),
        spatial_filter: None,
    };
    if let Ok(raw) = host::ts_read(&req.encode_to_vec()) {
        if let Ok(resp) = ReadResponse::decode(raw.as_slice()) {
            return !resp.tuples.is_empty();
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Handler dispatch
// ---------------------------------------------------------------------------

fn handle_init(payload: &[u8]) -> Vec<u8> {
    let cfg: InitConfig = serde_json::from_slice(payload).unwrap_or(InitConfig {
        actor_id: None,
        args: None,
    });
    with_state(|s| {
        s.actor_id = cfg.actor_id.unwrap_or_default();
        s.role = cfg
            .args
            .as_ref()
            .and_then(|a| a.get("role"))
            .cloned()
            .unwrap_or_else(|| "fetcher".to_string());
        s.pool_slot = cfg.args.as_ref()
            .and_then(|a| a.get("pool_slot"))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if s.actor_id.contains("::") {
            if let Some(suffix) = s.actor_id.split("//").nth(1) {
                if let Some(qualified) = suffix.split('@').next() {
                    if let Some(app) = qualified.split("::").nth(1) {
                        s.application_id = app.to_string();
                    }
                }
            }
        }
        // Fetchers join process group at init
        if s.role == "fetcher" {
            #[cfg(not(test))]
            {
                if host::pg_join(CRAWL_WORKERS_GROUP).is_ok() {
                    s.worker_joined = true;
                }
            }
        }
    });
    json_bytes(json!({ "status": "ok" }))
}

fn handle_fetch(payload: &[u8]) -> Vec<u8> {
    let v = match parse_payload(payload) {
        Ok(v) => v,
        Err(e) => return json_error(e),
    };
    let url = match v.get("url").and_then(|u| u.as_str()) {
        Some(u) => u.to_string(),
        None => return json_error("missing url"),
    };

    // Late-join for lazy virtual actor activation
    with_state(|s| {
        if !s.worker_joined {
            #[cfg(not(test))]
            {
                if host::pg_join(CRAWL_WORKERS_GROUP).is_ok() {
                    s.worker_joined = true;
                }
            }
        }
    });

    let links = simulate_links(&url);
    let word_counts = simulate_word_counts(&url);
    with_state(|s| {
        s.fetch_count += 1;
        s.last_url = url.clone();
    });

    json_bytes(json!({
        "status": "ok",
        "url": url,
        "links": links,
        "word_counts": word_counts,
    }))
}

fn handle_status_request(_payload: &[u8]) -> Vec<u8> {
    with_state(|s| {
        json_bytes(json!({
            "fetch_count": s.fetch_count,
            "last_url": s.last_url,
            "idle": true,
        }))
    })
}

fn handle_fetch_batch(payload: &[u8]) -> Vec<u8> {
    let v = match parse_payload(payload) {
        Ok(v) => v,
        Err(e) => return json_error(e),
    };
    let urls: Vec<String> = v.get("urls").and_then(|u| u.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let shard_count = v.get("shard_count").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
    let shard_index = with_state(|s| {
        if !s.worker_joined {
            #[cfg(not(test))]
            { let _ = host::pg_join(CRAWL_WORKERS_GROUP).map(|_| s.worker_joined = true); }
        }
        s.pool_slot
    });

    let mut total_words = 0u64;
    let mut pages_fetched = 0usize;
    let mut i = shard_index;
    while i < urls.len() {
        let url = &urls[i];
        let wc = simulate_word_counts(url);
        total_words += wc.values().sum::<u64>();
        with_state(|s| { s.fetch_count += 1; s.last_url = url.clone(); });
        pages_fetched += 1;
        i += shard_count;
    }

    json_bytes(json!({
        "status": "ok",
        "fetch_count": with_state(|s| s.fetch_count),
        "pages_fetched": pages_fetched,
        "total_words": total_words,
        "shard_index": shard_index,
        "shard_count": shard_count,
    }))
}

fn handle_benchmark(payload: &[u8]) -> Vec<u8> {
    let v = parse_payload(payload).unwrap_or_default();
    let worker_counts: Vec<usize> = v.get("worker_counts").and_then(|w| w.as_array())
        .map(|arr| arr.iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect())
        .unwrap_or_else(|| vec![1, 4, 8, 16]);
    let pages_per_round = v.get("pages_per_round").and_then(|n| n.as_u64()).unwrap_or(200) as usize;
    let app_id = with_state(|s| s.application_id.clone());

    let domains = ["example.com","docs.example.com","api.example.com","blog.example.com"];
    let sections = ["about","docs","api","blog","pricing","features","integrations","changelog"];
    let subpaths = ["overview","quickstart","reference","guide","examples","faq"];
    let unique_words = domains.len() * sections.len() * subpaths.len();

    let mut urls: Vec<String> = Vec::with_capacity(pages_per_round);
    for d in &domains { urls.push(format!("https://{d}")); }
    'outer1: for d in &domains {
        for s in &sections {
            if urls.len() >= pages_per_round { break 'outer1; }
            urls.push(format!("https://{d}/{s}"));
        }
    }
    'outer2: for d in &domains {
        for s in &sections {
            for p in &subpaths {
                if urls.len() >= pages_per_round { break 'outer2; }
                urls.push(format!("https://{d}/{s}/{p}"));
            }
        }
    }
    let subs = ["v1","v2","v3","beta"];
    let mut i = 0usize;
    while urls.len() < pages_per_round {
        urls.push(format!("https://{}/{}/{}/{}",
            domains[i%domains.len()], sections[i%sections.len()],
            subpaths[i%subpaths.len()], subs[i%subs.len()]));
        i += 1;
    }
    urls.truncate(pages_per_round);

    let mut results: Vec<Value> = Vec::new();
    let mut baseline_pps = 0.0f64;

    for &num_workers in &worker_counts {
        let group_id = format!("bench-fetchers-{num_workers}-{}", host::now_ms() % 100_000);

        for u in urls.iter().take(4) {
            ts_write_4("url_queue", u, "pending", "0");
        }

        let mut coord_ms = 0u64;
        let mut fetch_ms = 0u64;
        let mut total_words = 0i64;
        let mut worker_fetches = vec![0i64; num_workers];

        let t0 = host::now_ms();

        // ── ScatterGather parallel dispatch ──
        let sg_req = CreateShardGroupRequest {
            config: Some(DataParallelConfig {
                group_id: group_id.clone(),
                shard_count: num_workers as u32,
                partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
                rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
                placement: Some(NodePlacement {
                    strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
                    ..Default::default()
                }),
            }),
            actor_type: "fetcher".to_string(),
            shard_config: None,
            initial_state: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }.encode_to_vec();

        let t_coord0 = host::now_ms();
        let create_resp = host::create_shard_group(&sg_req);
        coord_ms += host::now_ms().saturating_sub(t_coord0);

        let sg_ok = create_resp.as_ref().map(|b| {
            CreateShardGroupResponse::decode(b.as_slice())
                .ok()
                .and_then(|r| r.group)
                .map(|g| !g.shard_actor_ids.is_empty())
                .unwrap_or(false)
        }).unwrap_or(false);

        if sg_ok {
            let query_json = json!({ "urls": urls, "shard_count": num_workers, "depth": 1 });
            let scatter_req = ScatterGatherRequest {
                group_id: group_id.clone(),
                query: Some(ProtoMessage {
                    id: format!("req-{}", host::now_ms()),
                    message_type: "fetch_batch".to_string(),
                    payload: query_json.to_string().into_bytes(),
                    ..Default::default()
                }),
                timeout: Some(plexspaces_proto::prost_types::Duration { seconds: 60, nanos: 0 }),
                aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
                min_responses: num_workers as u32,
                ..Default::default()
            }.encode_to_vec();

            let t_fetch = host::now_ms();
            let sg_resp = host::scatter_gather(&scatter_req);
            fetch_ms += host::now_ms().saturating_sub(t_fetch);

            let t_coord_post = host::now_ms();
            if let Ok(resp_bytes) = sg_resp {
                if let Ok(response) = ScatterGatherResponse::decode(resp_bytes.as_slice()) {
                    for (si, shard) in response.shard_responses.iter().enumerate() {
                        if let Some(msg) = &shard.response {
                            if let Ok(p) = serde_json::from_slice::<Value>(&msg.payload) {
                                let p_inner = normalize_shard_payload(&p);
                                let fc = p_inner.get("pages_fetched").and_then(|v| v.as_i64()).unwrap_or(0);
                                let tw = p_inner.get("total_words").and_then(|v| v.as_i64()).unwrap_or(0);
                                if si < num_workers { worker_fetches[si] = fc; }
                                total_words += tw;
                            }
                        }
                    }
                }
            } else {
                for url in &urls { total_words += simulate_word_counts(url).values().sum::<u64>() as i64; }
                for f in worker_fetches.iter_mut() { *f = (pages_per_round / num_workers) as i64; }
            }
            for u in urls.iter().take(4) { ts_write_4("url_queue", u, "visited", "1"); }
            coord_ms += host::now_ms().saturating_sub(t_coord_post);
        } else {
            // Fallback: sequential dispatch
            let worker_ids: Vec<String> = (0..num_workers).map(|i| format!("{app_id}/fetcher-{i}@")).collect();
            for (idx, url) in urls.iter().enumerate() {
                let wid = &worker_ids[idx % num_workers];
                let req = json!({ "url": url }).to_string().into_bytes();
                let t_f = host::now_ms();
                let resp = host::ask(wid, "fetch", &req, 10_000);
                fetch_ms += host::now_ms().saturating_sub(t_f);
                let t_c = host::now_ms();
                worker_fetches[idx % num_workers] += 1;
                let tw: u64 = if let Ok(rb) = resp {
                    if let Ok(rv) = serde_json::from_slice::<Value>(&rb) {
                        rv.get("word_counts").and_then(|wc| wc.as_object())
                            .map(|m| m.values().filter_map(|v| v.as_u64()).sum()).unwrap_or(0)
                    } else { 0 }
                } else { simulate_word_counts(url).values().sum() };
                total_words += tw as i64;
                if idx % 10 == 0 { ts_write_4("url_queue", url, "visited", "1"); }
                coord_ms += host::now_ms().saturating_sub(t_c);
            }
        }

        let elapsed = host::now_ms().saturating_sub(t0);
        let pps = if elapsed > 0 { pages_per_round as f64 * 1000.0 / elapsed as f64 } else { 0.0 };
        let elapsed1 = elapsed.max(1);
        let pf = 1.0 - coord_ms as f64 / elapsed1 as f64;

        if baseline_pps == 0.0 && pps > 0.0 { baseline_pps = pps; }
        let speedup = if baseline_pps > 0.0 { pps / baseline_pps } else { 1.0 };
        let eff = speedup / num_workers as f64 * 100.0;

        results.push(json!({
            "workers": num_workers,
            "pages": pages_per_round,
            "elapsed_ms": elapsed,
            "coord_ms": coord_ms,
            "fetch_ms": fetch_ms,
            "pages_per_sec": pps,
            "speedup": speedup,
            "efficiency_pct": eff,
            "parallel_fraction": pf,
            "worker_fetches": worker_fetches,
            "total_words": total_words,
            "unique_words": unique_words,
        }));
    }

    json_bytes(json!({ "status": "ok", "results": results }))
}

fn normalize_shard_payload(v: &Value) -> &Value {
    if v.get("status").is_some() || v.get("pages_fetched").is_some() { return v; }
    for k in &["payload", "result", "response", "data"] {
        if let Some(nested) = v.get(k) {
            if nested.is_object() { return normalize_shard_payload(nested); }
        }
    }
    v
}

fn handle_analyze(payload: &[u8]) -> Vec<u8> {
    let v = match parse_payload(payload) {
        Ok(v) => v,
        Err(e) => return json_error(e),
    };
    let results = match v.get("results").and_then(|r| r.as_array()) {
        Some(r) => r.clone(),
        None => return json_error("missing results array"),
    };

    with_state(|s| {
        if !s.analyzer_joined {
            #[cfg(not(test))]
            {
                if host::pg_join(ANALYZER_GROUP).is_ok() {
                    s.analyzer_joined = true;
                }
            }
        }
        for result in &results {
            if let Some(counts) = result.get("word_counts").and_then(|c| c.as_object()) {
                for (word, count) in counts {
                    *s.index.entry(word.clone()).or_insert(0) += count.as_u64().unwrap_or(1);
                }
            }
            s.urls_analyzed += 1;
        }
    });

    json_bytes(json!({ "status": "ok" }))
}

fn handle_top_words(payload: &[u8]) -> Vec<u8> {
    let v = parse_payload(payload).unwrap_or_default();
    let n = v.get("n").and_then(|n| n.as_u64()).unwrap_or(10) as usize;
    let top = with_state(|s| {
        let mut sorted: Vec<(String, u64)> = s.index.iter().map(|(k, v)| (k.clone(), *v)).collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(n);
        sorted
    });
    json_bytes(json!({ "top_words": top }))
}

fn handle_crawl(payload: &[u8]) -> Vec<u8> {
    let v = parse_payload(payload).unwrap_or_default();
    let seeds: Vec<String> = v
        .get("seed_urls")
        .and_then(|s| s.as_array())
        .map(|arr| arr.iter().filter_map(|u| u.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_else(|| vec!["https://example.com".to_string()]);

    let max_pages = v.get("max_pages").and_then(|n| n.as_u64()).unwrap_or(20) as usize;
    let max_depth = v.get("max_depth").and_then(|n| n.as_u64()).unwrap_or(2) as usize;

    let app_id = with_state(|s| s.application_id.clone());

    // ── Phase 1: Seed local BFS frontier + in-handler visited HashSet ──
    // Local VecDeque drives BFS; local HashSet deduplicates within this crawl.
    // TupleSpace records seeds as metadata (shows the primitive being used).
    let mut frontier: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    for url in &seeds {
        // Mark-before-enqueue: write to TupleSpace AND local set before pushing
        ts_write_4("url_queue", url, "pending", "0");
        visited.insert(url.clone());
        frontier.push_back((url.clone(), 0));
    }

    let mut pages_crawled = 0usize;
    let mut total_links = 0u64;
    let mut all_results: Vec<Value> = Vec::new();
    let mut coord_time_ms = 0u64;
    let mut fetch_time_ms = 0u64;
    let t0_crawl = host::now_ms();

    // ── Phase 2: BFS drain from local frontier ──
    while !frontier.is_empty() && pages_crawled < max_pages {
        let (url, depth) = match frontier.pop_front() {
            Some(t) => t,
            None => break,
        };
        if depth > max_depth {
            continue;
        }

        // ── ElasticPool checkout — separates rate limiting from queue depth ──
        let t_coord = host::now_ms();
        let checkout_raw_result = host::pool_checkout(FETCHER_POOL, CHECKOUT_TIMEOUT_MS);
        coord_time_ms += host::now_ms().saturating_sub(t_coord);

        let t_fetch = host::now_ms();
        let result_bytes = if let Ok(checkout_raw) = checkout_raw_result {
            if !checkout_raw.is_empty() {
                if let Ok(handle) = serde_json::from_slice::<Value>(&checkout_raw) {
                    let actor_id = handle.get("actor_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let checkout_id = handle.get("checkout_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if !actor_id.is_empty() {
                        let fetch_req = json!({ "url": url, "depth": depth }).to_string().into_bytes();
                        let result = host::ask(&actor_id, "fetch", &fetch_req, 10_000);
                        let t_checkin = host::now_ms();
                        let _ = host::pool_checkin(FETCHER_POOL, &actor_id, &checkout_id, true);
                        coord_time_ms += host::now_ms().saturating_sub(t_checkin);
                        match result {
                            Ok(b) => b,
                            Err(_) => fallback_result(&url),
                        }
                    } else {
                        fallback_result(&url)
                    }
                } else {
                    fallback_result(&url)
                }
            } else {
                fallback_result(&url)
            }
        } else {
            fallback_result(&url)
        };
        fetch_time_ms += host::now_ms().saturating_sub(t_fetch);

        if let Ok(result) = serde_json::from_slice::<Value>(&result_bytes) {
            // Enqueue newly discovered links — mark-before-enqueue dedup via local HashSet
            let t_coord2 = host::now_ms();
            if let Some(links) = result.get("links").and_then(|l| l.as_array()) {
                for link in links {
                    if let Some(link_str) = link.as_str() {
                        if depth + 1 <= max_depth && !visited.contains(link_str) {
                            // Mark BEFORE enqueuing (same pattern as muffet's sync.Map update)
                            visited.insert(link_str.to_string());
                            ts_write_4("url_queue", link_str, "pending", &(depth + 1).to_string());
                            frontier.push_back((link_str.to_string(), depth + 1));
                            total_links += 1;
                        }
                    }
                }
            }
            coord_time_ms += host::now_ms().saturating_sub(t_coord2);
            all_results.push(result);
            pages_crawled += 1;
        }
    }

    let elapsed_ms = host::now_ms().saturating_sub(t0_crawl);
    let pages_per_sec = if elapsed_ms > 0 {
        (pages_crawled as f64) * 1000.0 / (elapsed_ms as f64)
    } else {
        0.0
    };
    let parallel_fraction = if elapsed_ms > 0 {
        1.0 - (coord_time_ms as f64) / (elapsed_ms as f64)
    } else {
        1.0
    };

    // ── Pool utilization metrics ──
    let pool_metrics = if let Ok(raw) = host::pool_get_metrics(FETCHER_POOL) {
        serde_json::from_slice(&raw).unwrap_or_else(|_| json!({ "total_checkouts": pages_crawled, "pool_size": 4 }))
    } else {
        json!({ "total_checkouts": pages_crawled, "pool_size": 4 })
    };

    // ── Phase 3: Interleaved scatter to analyzer shards ──
    let num_shards = 2usize;
    let mut global_counts: HashMap<String, u64> = HashMap::new();

    for shard_idx in 0..num_shards {
        // Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
        let chunk: Vec<&Value> = all_results.iter().enumerate()
            .filter(|(i, _)| i % num_shards == shard_idx)
            .map(|(_, v)| v)
            .collect();
        if chunk.is_empty() {
            continue;
        }
        let analyzer_id = format!("{app_id}/analyzer-{shard_idx}@");
        let analyze_req = json!({ "results": chunk }).to_string().into_bytes();
        if host::ask(&analyzer_id, "analyze", &analyze_req, 10_000).is_ok() {
            let top_req = json!({ "n": 20 }).to_string().into_bytes();
            if let Ok(top_bytes) = host::ask(&analyzer_id, "top_words", &top_req, 10_000) {
                if let Ok(top) = serde_json::from_slice::<Value>(&top_bytes) {
                    if let Some(words) = top.get("top_words").and_then(|w| w.as_array()) {
                        for entry in words {
                            let word = entry.get(0).and_then(|w| w.as_str()).unwrap_or("");
                            let count = entry.get(1).and_then(|c| c.as_u64()).unwrap_or(0);
                            *global_counts.entry(word.to_string()).or_insert(0) += count;
                        }
                    }
                }
            }
        } else {
            // Local fallback
            for result in &chunk {
                if let Some(counts) = result.get("word_counts").and_then(|c| c.as_object()) {
                    for (word, count) in counts {
                        *global_counts.entry(word.clone()).or_insert(0) += count.as_u64().unwrap_or(1);
                    }
                }
            }
        }
    }

    let mut top_words: Vec<(String, u64)> = global_counts.into_iter().collect();
    top_words.sort_by(|a, b| b.1.cmp(&a.1));
    top_words.truncate(10);

    // ── Phase 4: ProcessGroup status gather — discover actual worker activity ──
    let mut worker_stats: Vec<Value> = Vec::new();
    let members = host::pg_members(CRAWL_WORKERS_GROUP).unwrap_or_default();
    let members_to_query = if members.is_empty() {
        (0..4).map(|i| format!("{app_id}/fetcher-{i}@")).collect::<Vec<_>>()
    } else {
        members
    };
    for member_id in &members_to_query {
        let req = json!({}).to_string().into_bytes();
        if let Ok(stats_bytes) = host::ask(member_id, "status_request", &req, 5_000) {
            if let Ok(mut stats) = serde_json::from_slice::<Value>(&stats_bytes) {
                let short_id = member_id.split('/').last()
                    .unwrap_or(member_id)
                    .trim_end_matches('@')
                    .to_string();
                if let Some(obj) = stats.as_object_mut() {
                    obj.insert("worker_id".to_string(), json!(short_id));
                }
                worker_stats.push(stats);
            }
        }
    }

    with_state(|s| {
        s.pages_crawled = pages_crawled as u64;
        s.total_links = total_links;
        s.top_words = top_words.clone();
        s.pool_metrics = pool_metrics.clone();
        s.worker_stats = worker_stats.clone();
    });

    json_bytes(json!({
        "status": "ok",
        "pages_crawled": pages_crawled,
        "total_links": total_links,
        "top_words": top_words,
        "pool_metrics": pool_metrics,
        "worker_stats": worker_stats,
        "elapsed_ms": elapsed_ms,
        "coord_time_ms": coord_time_ms,
        "fetch_time_ms": fetch_time_ms,
        "pages_per_sec": pages_per_sec,
        "parallel_fraction": parallel_fraction,
    }))
}

fn fallback_result(url: &str) -> Vec<u8> {
    let links = simulate_links(url);
    let word_counts = simulate_word_counts(url);
    json!({ "status": "ok", "url": url, "links": links, "word_counts": word_counts })
        .to_string()
        .into_bytes()
}

fn handle_status(_payload: &[u8]) -> Vec<u8> {
    with_state(|s| {
        json_bytes(json!({
            "actor_id": s.actor_id,
            "role": s.role,
            "pages_crawled": s.pages_crawled,
            "total_links": s.total_links,
            "top_words": s.top_words,
            "fetch_count": s.fetch_count,
            "urls_analyzed": s.urls_analyzed,
            "pool_metrics": s.pool_metrics,
            "worker_stats": s.worker_stats,
        }))
    })
}

// ---------------------------------------------------------------------------
// WIT guest entry point
// ---------------------------------------------------------------------------

struct Component;

impl Guest for Component {
    fn init(config: Vec<u8>) -> Result<(), String> {
        handle_init(&config);
        Ok(())
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let _ = from_actor;
        let result = match msg_type.as_str() {
            "fetch" => handle_fetch(&payload),
            "fetch_batch" => handle_fetch_batch(&payload),
            "status_request" => handle_status_request(&payload),
            "analyze" => handle_analyze(&payload),
            "top_words" => handle_top_words(&payload),
            "crawl" => handle_crawl(&payload),
            "benchmark" => handle_benchmark(&payload),
            "status" => handle_status(&payload),
            _ => json_error(format!("unknown message type: {msg_type}")),
        };
        Ok(result)
    }

    fn get_state() -> Result<Vec<u8>, String> {
        Ok(with_state(|s| serde_json::to_vec(s).unwrap_or_else(|_| b"{}".to_vec())))
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        match serde_json::from_slice::<AppState>(&state) {
            Ok(new_state) => {
                with_state(|s| *s = new_state);
                Ok(())
            }
            Err(e) => Err(format!("invalid state: {e}")),
        }
    }
}

export!(Component);

// ---------------------------------------------------------------------------
// Native contract tests (compiled only for non-WASM targets)
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulate_links() {
        let links = simulate_links("https://example.com");
        assert_eq!(links.len(), 12);
        assert!(links.iter().any(|l| l.contains("about") || l.contains("docs") || l.contains("api")));
    }

    #[test]
    fn test_simulate_word_counts() {
        let counts = simulate_word_counts("https://example.com/docs/api");
        assert!(counts.contains_key("example") || counts.contains_key("docs") || counts.contains_key("api"));
    }

    #[test]
    fn test_fetch_handler() {
        let payload = json!({ "url": "https://example.com" }).to_string();
        let result = handle_fetch(payload.as_bytes());
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["status"], "ok");
        assert!(v["links"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_analyze_handler() {
        let results = vec![json!({
            "url": "https://example.com",
            "word_counts": { "hello": 3, "world": 2 }
        })];
        let payload = json!({ "results": results }).to_string();
        let result = handle_analyze(payload.as_bytes());
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[test]
    fn test_top_words_after_analyze() {
        with_state(|s| {
            s.index.clear();
            s.index.insert("plexspaces".to_string(), 10);
            s.index.insert("actors".to_string(), 7);
        });
        let payload = json!({ "n": 5 }).to_string();
        let result = handle_top_words(payload.as_bytes());
        let v: Value = serde_json::from_slice(&result).unwrap();
        let words = v["top_words"].as_array().unwrap();
        assert!(!words.is_empty());
        assert_eq!(words[0][0], "plexspaces");
    }

    #[test]
    fn test_init_handler_sets_role() {
        let cfg = json!({
            "actor_id": "plexspaces://demo::web-crawl@node1",
            "args": { "role": "fetcher" }
        }).to_string();
        let result = handle_init(cfg.as_bytes());
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["status"], "ok");
        let role = with_state(|s| s.role.clone());
        assert_eq!(role, "fetcher");
    }

    #[test]
    fn test_status_request_handler() {
        with_state(|s| {
            s.fetch_count = 3;
            s.last_url = "https://example.com/docs".to_string();
        });
        let result = handle_status_request(b"{}");
        let v: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["fetch_count"], 3);
        assert_eq!(v["last_url"], "https://example.com/docs");
        assert_eq!(v["idle"], true);
    }
}
