// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Web Crawler - Embedded Single-Binary Example
//
// Demonstrates how to build and run a complete PlexSpaces application as a single
// binary: the node, actors, and application logic all start in main().
//
// Architecture:
//   main()
//     └── PlexSpaces Node (NodeBuilder)
//           ├── WebCrawlOrchestrator (GenServer) — controls the crawl
//           │     ├── ElasticPool<PageFetcher>    — round-robin HTTP workers
//           │     └── ShardGroup<LinkAnalyzer>    — map-reduce word frequency
//           └── shutdown
//
// Comparable to Ray's web-crawl example:
//   ElasticPool  ≈ ray.remote actor pool
//   ShardGroup   ≈ ray map-reduce collectives

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::Level;

use plexspaces_actor::{ActorContext, BehaviorError, Message, RequestContext, RequestContextExt};
use plexspaces_sdk::{
    call_message, gen_server_actor, json, plexspaces_handlers, spawn, ActorRef,
};

extern crate plexspaces_behavior;

// =============================================================================
// Domain types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    pub url: String,
    pub links: Vec<String>,
    pub word_counts: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrawlStats {
    pub pages_crawled: usize,
    pub total_links: usize,
    pub top_words: Vec<(String, usize)>,
    pub elapsed_ms: u64,
}

// =============================================================================
// PageFetcher actor — one worker in the ElasticPool
// =============================================================================

#[gen_server_actor]
struct PageFetcher {
    fetch_count: usize,
}

impl PageFetcher {
    fn new() -> Self {
        Self { fetch_count: 0 }
    }
}

#[plexspaces_handlers]
impl PageFetcher {
    #[handler("fetch")]
    async fn handle_fetch(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct FetchReq { url: String }
        let req: FetchReq = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad fetch req: {e}")))?;

        self.fetch_count += 1;
        let links = simulate_links(&req.url);
        let word_counts = simulate_word_counts(&req.url);

        Ok(json!({
            "url": req.url,
            "links": links,
            "word_counts": word_counts,
            "fetch_count": self.fetch_count,
        }))
    }
}

fn simulate_links(url: &str) -> Vec<String> {
    let base = url.trim_end_matches('/');
    vec![
        format!("{base}/about"),
        format!("{base}/docs"),
        format!("{base}/api"),
    ]
}

fn simulate_word_counts(url: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for segment in url.split('/').filter(|s| !s.is_empty() && *s != "https:" && *s != "http:") {
        for word in segment.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 2 {
                *counts.entry(word.to_lowercase()).or_insert(0) += 1;
            }
        }
    }
    counts
}

// =============================================================================
// LinkAnalyzer actor — shard in the ShardGroup
// =============================================================================

#[gen_server_actor]
struct LinkAnalyzer {
    index: HashMap<String, usize>,
    urls_analyzed: usize,
}

impl LinkAnalyzer {
    fn new() -> Self {
        Self { index: HashMap::new(), urls_analyzed: 0 }
    }
}

#[plexspaces_handlers]
impl LinkAnalyzer {
    #[handler("analyze")]
    async fn handle_analyze(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct AnalyzeReq { results: Vec<CrawlResult> }
        let req: AnalyzeReq = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad analyze req: {e}")))?;

        for result in &req.results {
            for (word, count) in &result.word_counts {
                *self.index.entry(word.clone()).or_insert(0) += count;
            }
            self.urls_analyzed += 1;
        }
        Ok(json!({ "status": "ok", "urls_analyzed": self.urls_analyzed }))
    }

    #[handler("top_words")]
    async fn handle_top_words(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct TopReq { n: Option<usize> }
        let req: TopReq = serde_json::from_slice(&msg.payload).unwrap_or(TopReq { n: None });
        let n = req.n.unwrap_or(10);

        let mut sorted: Vec<(String, usize)> =
            self.index.iter().map(|(k, v)| (k.clone(), *v)).collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(n);

        Ok(json!({ "top_words": sorted, "urls_analyzed": self.urls_analyzed }))
    }
}

// =============================================================================
// Helpers — ask an ActorRef with call_message and parse JSON response
// =============================================================================

async fn ask(
    actor_ref: &ActorRef,
    ctx: &RequestContext,
    op: &str,
    payload: Value,
) -> Result<Value> {
    let mut msg = call_message(payload);
    msg.message_type = op.to_string();
    let reply = actor_ref.ask(ctx, msg, Duration::from_secs(30)).await
        .map_err(|e| anyhow::anyhow!("ask {op}: {e}"))?;
    let v: Value = serde_json::from_slice(&reply.payload)
        .map_err(|e| anyhow::anyhow!("parse reply from {op}: {e}"))?;
    Ok(v)
}

// =============================================================================
// main — deploys the node + actors, runs the crawl, shuts down
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .try_init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     Web Crawler — Embedded Single-Binary                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    let seed_urls: Vec<String> = std::env::args().skip(1).collect();
    let seed_urls = if seed_urls.is_empty() {
        vec!["https://example.com".to_string(), "https://docs.example.com".to_string()]
    } else {
        seed_urls
    };

    println!("Seed URLs: {}", seed_urls.join(", "));

    // --- Step 1: Create Node ---
    use plexspaces_node::NodeBuilder;
    let node = NodeBuilder::new("web-crawl-node")
        .with_clustering_enabled(false)
        .build_started()
        .await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("demo".to_string(), "web_crawl".to_string());

    // --- Step 2: Spawn fetcher pool (ElasticPool pattern) ---
    let pool_size = 4usize;
    let mut fetcher_refs: Vec<ActorRef> = Vec::new();
    for i in 0..pool_size {
        let actor_ref = spawn(&ctx, service_locator.clone(), format!("fetcher-{i}"), "web_crawl", PageFetcher::new())
            .await
            .map_err(|e| anyhow::anyhow!("spawn fetcher-{i}: {e}"))?;
        fetcher_refs.push(actor_ref);
    }
    println!("  ✓ {} fetchers spawned (ElasticPool)", pool_size);

    // --- Step 3: Spawn analyzer shards (ShardGroup pattern) ---
    let num_shards = 2usize;
    let mut analyzer_refs: Vec<ActorRef> = Vec::new();
    for i in 0..num_shards {
        let actor_ref = spawn(&ctx, service_locator.clone(), format!("analyzer-{i}"), "web_crawl", LinkAnalyzer::new())
            .await
            .map_err(|e| anyhow::anyhow!("spawn analyzer-{i}: {e}"))?;
        analyzer_refs.push(actor_ref);
    }
    println!("  ✓ {} analyzer shards spawned (ShardGroup)", num_shards);

    // --- Step 4: BFS crawl loop ---
    println!("Running crawl...");
    let start = Instant::now();
    let max_pages = 20usize;
    let max_depth = 2usize;

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: Vec<(String, usize)> = seed_urls.iter().map(|u| (u.clone(), 0)).collect();
    let mut all_results: Vec<CrawlResult> = Vec::new();
    let mut fetcher_idx = 0usize;

    while let Some((url, depth)) = queue.pop() {
        if visited.contains(&url) || visited.len() >= max_pages || depth > max_depth {
            continue;
        }
        visited.insert(url.clone());

        let fetcher = &fetcher_refs[fetcher_idx % pool_size];
        fetcher_idx += 1;

        let response = ask(fetcher, &ctx, "fetch", json!({ "url": url })).await
            .unwrap_or_else(|_| {
                let links = simulate_links(&url);
                let wc = simulate_word_counts(&url);
                json!({ "url": url, "links": links, "word_counts": wc })
            });

        let result: CrawlResult = serde_json::from_value(response)
            .unwrap_or_else(|_| CrawlResult {
                url: url.clone(), links: simulate_links(&url), word_counts: simulate_word_counts(&url),
            });

        for link in &result.links {
            if !visited.contains(link) {
                queue.push((link.clone(), depth + 1));
            }
        }
        all_results.push(result);
    }

    // --- Step 5: Scatter to analyzer shards ---
    let chunk_size = (all_results.len() + num_shards - 1) / num_shards;
    for (shard_idx, chunk) in all_results.chunks(chunk_size.max(1)).enumerate() {
        let analyzer = &analyzer_refs[shard_idx % num_shards];
        let _ = ask(analyzer, &ctx, "analyze", json!({ "results": chunk })).await;
    }

    // --- Step 6: Reduce — collect top words from all shards ---
    let mut global_counts: HashMap<String, usize> = HashMap::new();
    for analyzer in &analyzer_refs {
        if let Ok(top) = ask(analyzer, &ctx, "top_words", json!({ "n": 20 })).await {
            if let Some(words) = top.get("top_words").and_then(|v| v.as_array()) {
                for entry in words {
                    if let (Some(word), Some(count)) = (
                        entry.get(0).and_then(|v| v.as_str()),
                        entry.get(1).and_then(|v| v.as_u64()),
                    ) {
                        *global_counts.entry(word.to_string()).or_insert(0) += count as usize;
                    }
                }
            }
        }
    }

    let mut top_words: Vec<(String, usize)> = global_counts.into_iter().collect();
    top_words.sort_by(|a, b| b.1.cmp(&a.1));
    top_words.truncate(10);

    let elapsed = start.elapsed();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Pages crawled : {}", all_results.len());
    println!("  Total links   : {}", all_results.iter().map(|r| r.links.len()).sum::<usize>());
    println!("  Elapsed (ms)  : {:.2}", elapsed.as_secs_f64() * 1000.0);
    if !top_words.is_empty() {
        println!("  Top words:");
        for (word, count) in &top_words {
            println!("    {word:<20} {count}");
        }
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    node.shutdown(Duration::from_secs(5)).await?;
    println!("  ✓ Done");
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulate_links() {
        let links = simulate_links("https://example.com");
        assert_eq!(links.len(), 3);
        assert!(links[0].contains("/about"));
        assert!(links[1].contains("/docs"));
        assert!(links[2].contains("/api"));
    }

    #[test]
    fn test_simulate_word_counts() {
        let counts = simulate_word_counts("https://example.com/hello/world");
        assert!(counts.contains_key("example"));
        assert!(counts.contains_key("hello"));
        assert!(counts.contains_key("world"));
    }

    #[test]
    fn test_simulate_word_counts_filters_short() {
        let counts = simulate_word_counts("https://example.com/a/bb/ccc");
        assert!(!counts.contains_key("a"));
        assert!(!counts.contains_key("bb"));
        assert!(counts.contains_key("ccc"));
    }
}
