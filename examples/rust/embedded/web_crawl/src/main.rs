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
//
// This file uses SDK annotations (#[gen_server_actor], #[handler]) and
// spawn helpers (spawn) from plexspaces-sdk.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::Level;

use plexspaces_sdk::{
    gen_server_actor, handler, json, plexspaces_handlers, spawn, ActorContext, BehaviorError,
    GenServerRef, Message, RequestContext, RequestContextExt,
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
    pub status_code: u16,
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

/// Stateless HTTP page fetcher that lives inside the ElasticPool.
/// In a real crawler this would use reqwest; here we simulate the fetch.
#[gen_server_actor]
struct PageFetcher {
    fetch_count: usize,
}

impl PageFetcher {
    fn new() -> Self {
        Self { fetch_count: 0 }
    }
}

#[plexspaces_handlers(gen_server)]
impl PageFetcher {
    #[handler("fetch")]
    async fn handle_fetch(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct FetchReq {
            url: String,
        }
        let req: FetchReq = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad fetch req: {e}")))?;

        self.fetch_count += 1;

        // Simulated fetch: extract "links" and count "words" from the URL path
        let links = simulate_links(&req.url);
        let word_counts = simulate_word_counts(&req.url);

        Ok(json!({
            "url": req.url,
            "links": links,
            "word_counts": word_counts,
            "status_code": 200,
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

/// One shard in the distributed word-frequency ShardGroup.
/// Maintains a partial word-count index for its assigned URL slice.
#[gen_server_actor]
struct LinkAnalyzer {
    index: HashMap<String, usize>,
    urls_analyzed: usize,
}

impl LinkAnalyzer {
    fn new() -> Self {
        Self {
            index: HashMap::new(),
            urls_analyzed: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl LinkAnalyzer {
    #[handler("analyze")]
    async fn handle_analyze(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct AnalyzeReq {
            results: Vec<CrawlResult>,
        }
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
        struct TopReq {
            n: Option<usize>,
        }
        let req: TopReq = serde_json::from_slice(&msg.payload).unwrap_or(TopReq { n: None });
        let n = req.n.unwrap_or(10);

        let mut sorted: Vec<(String, usize)> = self.index.iter().map(|(k, v)| (k.clone(), *v)).collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(n);

        Ok(json!({
            "top_words": sorted,
            "total_unique_words": self.index.len(),
            "urls_analyzed": self.urls_analyzed,
        }))
    }
}

// =============================================================================
// WebCrawlOrchestrator — coordinates the entire crawl
// =============================================================================

#[gen_server_actor]
struct WebCrawlOrchestrator {
    seed_urls: Vec<String>,
    max_depth: usize,
    max_pages: usize,
}

impl WebCrawlOrchestrator {
    fn new(seed_urls: Vec<String>, max_depth: usize, max_pages: usize) -> Self {
        Self { seed_urls, max_depth, max_pages }
    }
}

#[plexspaces_handlers(gen_server)]
impl WebCrawlOrchestrator {
    /// Start the crawl. Returns CrawlStats when complete.
    #[handler("crawl")]
    async fn handle_crawl(
        &mut self,
        ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let start = Instant::now();

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<(String, usize)> = self
            .seed_urls
            .iter()
            .map(|u| (u.clone(), 0))
            .collect();
        let mut all_results: Vec<CrawlResult> = Vec::new();

        // Build RequestContext from the actor's own tenant/namespace
        let rctx = RequestContext::new_without_auth(
            ctx.tenant_id.clone(),
            ctx.namespace.clone(),
        );
        let sl = ctx.service_locator.clone();

        // Spawn fetcher pool (ElasticPool-style: reuse workers across URLs)
        let pool_size = 4usize;
        let mut fetcher_refs: Vec<GenServerRef> = Vec::new();
        for i in 0..pool_size {
            let actor_ref = spawn(
                &rctx,
                sl.clone(),
                format!("fetcher-{i}"),
                "web_crawl",
                PageFetcher::new(),
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("spawn fetcher: {e}")))?;
            fetcher_refs.push(GenServerRef::new(actor_ref));
        }

        // Spawn analyzer shards (ShardGroup-style: scatter results across shards)
        let num_shards = 2usize;
        let mut analyzer_refs: Vec<GenServerRef> = Vec::new();
        for i in 0..num_shards {
            let actor_ref = spawn(
                &rctx,
                sl.clone(),
                format!("analyzer-{i}"),
                "web_crawl",
                LinkAnalyzer::new(),
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("spawn analyzer: {e}")))?;
            analyzer_refs.push(GenServerRef::new(actor_ref));
        }

        // --- Crawl loop (ElasticPool pattern: checkout fetcher → process → return) ---
        let mut fetcher_idx = 0usize;
        while let Some((url, depth)) = queue.pop() {
            if visited.contains(&url) || visited.len() >= self.max_pages {
                continue;
            }
            if depth > self.max_depth {
                continue;
            }
            visited.insert(url.clone());

            // Checkout fetcher (round-robin across pool)
            let fetcher = &fetcher_refs[fetcher_idx % pool_size];
            fetcher_idx += 1;

            let response: Value = fetcher
                .call(&rctx, "fetch", &json!({ "url": url }))
                .await
                .map_err(|e| BehaviorError::ProcessingError(format!("fetch {url}: {e}")))?;

            let result: CrawlResult = serde_json::from_value(response)
                .map_err(|e| BehaviorError::ProcessingError(format!("parse result: {e}")))?;

            // Enqueue new links
            for link in &result.links {
                if !visited.contains(link) {
                    queue.push((link.clone(), depth + 1));
                }
            }
            all_results.push(result);
        }

        // --- Map-reduce: scatter results to analyzer shards ---
        let chunk_size = (all_results.len() + num_shards - 1) / num_shards;
        for (shard_idx, chunk) in all_results.chunks(chunk_size.max(1)).enumerate() {
            let analyzer = &analyzer_refs[shard_idx % num_shards];
            let _: Value = analyzer
                .call(&rctx, "analyze", &json!({ "results": chunk }))
                .await
                .map_err(|e| BehaviorError::ProcessingError(format!("analyze shard {shard_idx}: {e}")))?;
        }

        // --- Reduce: collect top words from all shards ---
        let mut global_counts: HashMap<String, usize> = HashMap::new();
        for analyzer in &analyzer_refs {
            let top: Value = analyzer
                .call(&rctx, "top_words", &json!({ "n": 20 }))
                .await
                .map_err(|e| BehaviorError::ProcessingError(format!("top_words: {e}")))?;

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

        let mut top_words: Vec<(String, usize)> = global_counts.into_iter().collect();
        top_words.sort_by(|a, b| b.1.cmp(&a.1));
        top_words.truncate(10);

        let stats = CrawlStats {
            pages_crawled: all_results.len(),
            total_links: all_results.iter().map(|r| r.links.len()).sum(),
            top_words: top_words.clone(),
            elapsed_ms: start.elapsed().as_millis() as u64,
        };

        Ok(serde_json::to_value(&stats).unwrap())
    }
}

// =============================================================================
// main — deploys the node + actors, runs the crawl, shuts down
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_env_filter("web_crawl=info,plexspaces=warn")
        .try_init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     Web Crawler — Embedded Single-Binary                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Pattern: ElasticPool (fetchers) + ShardGroup (map-reduce)");
    println!("Inspired by Ray's web-crawl and map-reduce examples.");
    println!();

    // Seed URLs from CLI args or defaults
    let seed_urls: Vec<String> = std::env::args().skip(1).collect();
    let seed_urls = if seed_urls.is_empty() {
        vec![
            "https://example.com".to_string(),
            "https://docs.example.com".to_string(),
        ]
    } else {
        seed_urls
    };

    println!("Seed URLs:");
    for url in &seed_urls {
        println!("  {url}");
    }
    println!();

    // --- Step 1: Create Node ---
    println!("Step 1: Creating PlexSpaces node...");
    use plexspaces_node::NodeBuilder;
    let node = NodeBuilder::new("web-crawl-node")
        .with_clustering_enabled(false)
        .build_started()
        .await;
    let service_locator = node.service_locator();
    println!("  ✓ Node ready");
    println!();

    // Tenant context — mandatory, never use RequestContext::internal()
    let ctx = RequestContext::new_without_auth("demo".to_string(), "web_crawl".to_string());

    // --- Step 2: Spawn orchestrator ---
    println!("Step 2: Spawning WebCrawlOrchestrator...");
    let orchestrator = WebCrawlOrchestrator::new(seed_urls, 2, 20);
    let orchestrator_ref = spawn(&ctx, service_locator, "orchestrator", "web_crawl", orchestrator)
        .await
        .map_err(|e| anyhow::anyhow!("spawn orchestrator: {e}"))?;
    let orch_ref = GenServerRef::new(orchestrator_ref);
    println!("  ✓ Orchestrator spawned");
    println!();

    // --- Step 3: Run the crawl ---
    println!("Step 3: Running crawl...");
    let crawl_start = Instant::now();
    let result: Value = orch_ref
        .call(&ctx, "crawl", &json!({}))
        .await
        .map_err(|e| anyhow::anyhow!("crawl failed: {e}"))?;
    let elapsed = crawl_start.elapsed();
    println!("  ✓ Crawl complete in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
    println!();

    // --- Step 4: Print results ---
    println!("Step 4: Results");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    if let Ok(stats) = serde_json::from_value::<CrawlStats>(result) {
        println!("  Pages crawled : {}", stats.pages_crawled);
        println!("  Total links   : {}", stats.total_links);
        println!("  Elapsed (ms)  : {}", stats.elapsed_ms);
        println!();
        println!("  Top words:");
        for (word, count) in &stats.top_words {
            println!("    {word:<20} {count}");
        }
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // --- Step 5: Shutdown ---
    println!();
    println!("Step 5: Shutting down node...");
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
    fn test_simulate_word_counts_filters_short_words() {
        let counts = simulate_word_counts("https://example.com/a/bb/ccc");
        assert!(!counts.contains_key("a"));
        assert!(!counts.contains_key("bb"));
        assert!(counts.contains_key("ccc"));
    }

    #[test]
    fn test_simulate_links_no_trailing_slash() {
        let links = simulate_links("https://example.com/");
        assert!(links[0].ends_with("/about"), "expected /about suffix");
    }
}
