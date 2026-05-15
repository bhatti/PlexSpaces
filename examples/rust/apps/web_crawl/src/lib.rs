// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Web Crawl - Rust WASM app
//
// Parallel web crawler using three coordinating patterns:
//   ElasticPool  — round-robin pool of PageFetcher actors (4 workers)
//   TupleSpace   — url_queue: pending → done URL tracking and visited-set
//   ShardGroup   — 2 analyzer shards: scatter crawl results, reduce word counts
//
// Inspired by Ray's web-crawl and map-reduce examples:
//   https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html
//   https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html
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
    Tuple, TupleField, WriteRequest,
    tuple_field::Value as ProtoTupleValue,
};
use prost::Message as ProstMessage;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;

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
    // analyzer
    index: HashMap<String, u64>,
    urls_analyzed: u64,
    // orchestrator
    pages_crawled: u64,
    total_links: u64,
    top_words: Vec<(String, u64)>,
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

fn simulate_links(url: &str) -> Vec<String> {
    let base = url.trim_end_matches('/');
    vec![
        format!("{base}/about"),
        format!("{base}/docs"),
        format!("{base}/api"),
    ]
}

fn simulate_word_counts(url: &str) -> HashMap<String, u64> {
    let mut counts = HashMap::new();
    for seg in url.split('/').filter(|s| !s.is_empty() && *s != "https:" && *s != "http:") {
        for word in seg.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 2 {
                *counts.entry(word.to_lowercase()).or_insert(0) += 1;
            }
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// TupleSpace helpers — encode proto WriteRequest for host::ts_write
// ---------------------------------------------------------------------------

fn proto_string_field(s: &str) -> TupleField {
    TupleField {
        value: Some(ProtoTupleValue::String(s.to_string())),
    }
}

fn build_tuple(fields: Vec<TupleField>) -> Tuple {
    Tuple {
        fields,
        ..Default::default()
    }
}

/// Write a string tuple ["url_queue", url, status] to TupleSpace.
fn ts_write_url(url: &str, status: &str) {
    let req = WriteRequest {
        tuples: vec![build_tuple(vec![
            proto_string_field("url_queue"),
            proto_string_field(url),
            proto_string_field(status),
        ])],
        transaction_id: String::new(),
    };
    let _ = host::ts_write(&req.encode_to_vec());
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
        // Derive application_id from actor_id (format: scheme://tenant::app@node)
        if s.actor_id.contains("::") {
            if let Some(suffix) = s.actor_id.split("//").nth(1) {
                if let Some(qualified) = suffix.split('@').next() {
                    if let Some(app) = qualified.split("::").nth(1) {
                        s.application_id = app.to_string();
                    }
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

    let links = simulate_links(&url);
    let word_counts = simulate_word_counts(&url);
    with_state(|s| s.fetch_count += 1);

    json_bytes(json!({
        "status": "ok",
        "url": url,
        "links": links,
        "word_counts": word_counts,
    }))
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

    // Seed TupleSpace url_queue with pending URLs
    for url in &seeds {
        ts_write_url(url, "pending");
    }

    let app_id = with_state(|s| s.application_id.clone());

    // BFS crawl: checkout fetcher from pool (round-robin ElasticPool pattern)
    let mut visited: Vec<String> = Vec::new();
    let mut queue: Vec<(String, usize)> = seeds.iter().map(|u| (u.clone(), 0)).collect();
    let mut pages_crawled = 0u64;
    let mut total_links = 0u64;
    let mut all_results: Vec<Value> = Vec::new();
    let mut fetcher_idx = 0usize;
    let pool_size = 4usize;

    while !queue.is_empty() && visited.len() < max_pages {
        let (url, depth) = queue.remove(0);
        if visited.contains(&url) || depth > max_depth {
            continue;
        }
        visited.push(url.clone());

        // Checkout fetcher from pool (round-robin)
        let fetcher_id = format!("{app_id}/fetcher-{0}@", fetcher_idx % pool_size);
        fetcher_idx += 1;

        let fetch_req = json!({ "url": url }).to_string().into_bytes();
        let result_bytes = match host::ask(&fetcher_id, "fetch", &fetch_req, 10_000) {
            Ok(b) => b,
            Err(_) => {
                // Fallback: compute locally if remote ask fails
                let links = simulate_links(&url);
                let word_counts = simulate_word_counts(&url);
                json!({ "status": "ok", "url": url, "links": links, "word_counts": word_counts })
                    .to_string()
                    .into_bytes()
            }
        };

        if let Ok(result) = serde_json::from_slice::<Value>(&result_bytes) {
            if let Some(links) = result.get("links").and_then(|l| l.as_array()) {
                for link in links {
                    if let Some(link_str) = link.as_str() {
                        if !visited.contains(&link_str.to_string()) {
                            queue.push((link_str.to_string(), depth + 1));
                            total_links += 1;
                        }
                    }
                }
            }
            all_results.push(result);
            pages_crawled += 1;
        }

        // Mark done in TupleSpace
        ts_write_url(&url, "done");
    }

    // Scatter results to analyzer shards (ShardGroup reduce pattern)
    let num_shards = 2usize;
    let chunk_size = (all_results.len() + num_shards - 1) / num_shards;
    let mut global_counts: HashMap<String, u64> = HashMap::new();

    for shard_idx in 0..num_shards {
        let start = shard_idx * chunk_size;
        if start >= all_results.len() {
            break;
        }
        let end = (start + chunk_size).min(all_results.len());
        let chunk = &all_results[start..end];

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
            // Local fallback if remote analyzer unavailable
            for result in chunk {
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

    with_state(|s| {
        s.pages_crawled = pages_crawled;
        s.total_links = total_links;
        s.top_words = top_words.clone();
    });

    json_bytes(json!({
        "status": "ok",
        "pages_crawled": pages_crawled,
        "total_links": total_links,
        "top_words": top_words,
    }))
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
            "analyze" => handle_analyze(&payload),
            "top_words" => handle_top_words(&payload),
            "crawl" => handle_crawl(&payload),
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
        assert_eq!(links.len(), 3);
        assert!(links[0].contains("about"));
        assert!(links[1].contains("docs"));
        assert!(links[2].contains("api"));
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
        // Reset state for this test
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
        // First word should be "plexspaces" with count 10
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
}
