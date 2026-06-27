// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Web Crawler - Embedded Single-Binary Example
//
// Demonstrates how to build and run a complete PlexSpaces application as a single
// binary: the node, actors, and application logic all start in main().
//
// All four parallelization primitives are demonstrated with in-process equivalents:
//
//   TupleSpace frontier  ≈ Arc<Mutex<HashSet>> — mark-before-enqueue deduplication
//   ElasticPool          ≈ tokio::mpsc channel free-list — exclusive checkout per actor ref
//   ProcessGroup         ≈ Arc<RwLock<Vec<ActorRef>>> registry — workers self-register
//   ShardGroup scatter   ≈ interleaved i % num_shards assignment for balanced load

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::Level;

use plexspaces_actor::{ActorContext, BehaviorError, Message, RequestContext, RequestContextExt};
use plexspaces_sdk::{
    call_message, gen_server_actor, json, plexspaces_handlers, spawn, ActorRef,
};

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
pub struct WorkerStatus {
    pub fetch_count: usize,
    pub last_url: String,
}

// =============================================================================
// PageFetcher actor — one worker in the ElasticPool free-list
// =============================================================================

#[gen_server_actor]
struct PageFetcher {
    fetch_count: usize,
    last_url: String,
    // In-process registry — all PageFetchers add themselves here on spawn
    registry: Arc<RwLock<Vec<(String, ActorRef)>>>,
}

impl PageFetcher {
    fn new(registry: Arc<RwLock<Vec<(String, ActorRef)>>>) -> Self {
        Self { fetch_count: 0, last_url: String::new(), registry }
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
        self.last_url = req.url.clone();
        let links = simulate_links(&req.url);
        let word_counts = simulate_word_counts(&req.url);

        Ok(json!({
            "url": req.url,
            "links": links,
            "word_counts": word_counts,
            "fetch_count": self.fetch_count,
        }))
    }

    #[handler("status_request")]
    async fn handle_status(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "fetch_count": self.fetch_count,
            "last_url": self.last_url,
            "idle": true,
        }))
    }
}

fn simulate_links(url: &str) -> Vec<String> {
    let base = url.trim_end_matches('/');
    let h = url.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    let sections = ["about","docs","api","blog","pricing","features","integrations","changelog",
                    "security","status","community","enterprise","solutions","resources"];
    let paths = ["overview","quickstart","reference","guide","examples","faq","support","contact"];
    let mut links = Vec::with_capacity(12);
    for i in 0..8 {
        links.push(format!("{base}/{}", sections[(h + i) % sections.len()]));
    }
    for i in 0..4 {
        let sec = sections[(h + i * 3) % sections.len()];
        let pth = paths[(h + i * 7) % paths.len()];
        links.push(format!("{base}/{sec}/{pth}"));
    }
    links
}

fn simulate_word_counts(url: &str) -> HashMap<String, usize> {
    let h = url.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
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
    // URL-derived words
    for segment in url.split('/').filter(|s| !s.is_empty() && *s != "https:" && *s != "http:") {
        for word in segment.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 2 {
                *counts.entry(word.to_lowercase()).or_insert(0) += 8 + h % 5;
            }
        }
    }
    // Zipf-distributed vocab words
    for i in 0..25 {
        let word = vocab[(h + i * 17) % vocab.len()];
        let rank = i + 1;
        let count = 50 / rank + 1 + (h + i) % 3;
        *counts.entry(word.to_string()).or_insert(0) += count;
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

    // --- Step 2: Spawn fetcher pool (ElasticPool — channel free-list pattern) ---
    // Pre-loading a channel with (name, ActorRef) gives exclusive-per-actor checkout:
    // receive = checkout (removes from pool), send back = checkin (returns to pool).
    // Unlike a Semaphore, this prevents the same actor from being dispatched twice.
    let pool_size = 4usize;
    let (pool_tx, pool_rx) = mpsc::channel::<(String, ActorRef)>(pool_size);
    let pool_rx = Arc::new(tokio::sync::Mutex::new(pool_rx));

    // ProcessGroup registry — all fetchers register here; orchestrator collects for status
    let worker_registry: Arc<RwLock<Vec<(String, ActorRef)>>> = Arc::new(RwLock::new(Vec::new()));

    for i in 0..pool_size {
        let name = format!("fetcher-{i}");
        let actor_ref = spawn(
            &ctx,
            service_locator.clone(),
            name.clone(),
            "web_crawl",
            PageFetcher::new(worker_registry.clone()),
        )
        .await
        .map_err(|e| anyhow::anyhow!("spawn {name}: {e}"))?;

        // Register in process-group registry
        worker_registry.write().unwrap().push((name.clone(), actor_ref.clone()));
        // Pre-load into pool free-list (checkout = recv, checkin = send)
        pool_tx.send((name, actor_ref)).await?;
    }
    println!("  ✓ {} fetchers spawned (ElasticPool — channel free-list)", pool_size);

    // --- Step 3: Spawn analyzer shards (ShardGroup — interleaved scatter) ---
    let num_shards = 2usize;
    let mut analyzer_refs: Vec<(String, ActorRef)> = Vec::new();
    for i in 0..num_shards {
        let name = format!("analyzer-{i}");
        let actor_ref = spawn(&ctx, service_locator.clone(), name.clone(), "web_crawl", LinkAnalyzer::new())
            .await
            .map_err(|e| anyhow::anyhow!("spawn {name}: {e}"))?;
        analyzer_refs.push((name, actor_ref));
    }
    println!("  ✓ {} analyzer shards spawned (ShardGroup — interleaved scatter)", num_shards);

    // --- Step 4: BFS crawl loop ---
    // TupleSpace frontier ≈ Arc<Mutex<HashSet>> — mark-before-enqueue deduplication
    println!("Running crawl...");
    let start = Instant::now();
    let max_pages = 20usize;
    let max_depth = 2usize;

    // Mark-before-enqueue: seeds are marked visited before entering the frontier
    let visited: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();

    for url in &seed_urls {
        visited.lock().unwrap().insert(url.clone());
        frontier.push_back((url.clone(), 0));
    }

    let mut all_results: Vec<CrawlResult> = Vec::new();
    let mut pool_fetch_counts: HashMap<String, usize> = HashMap::new();

    while let Some((url, depth)) = frontier.pop_front() {
        if all_results.len() >= max_pages {
            break;
        }

        // ── ElasticPool checkout — receive from free-list channel (exclusive acquisition) ──
        let (fetcher_name, fetcher_ref) = {
            let mut rx = pool_rx.lock().await;
            match rx.recv().await {
                Some(pair) => pair,
                None => break,
            }
        };

        let response = ask(&fetcher_ref, &ctx, "fetch", json!({ "url": url })).await
            .unwrap_or_else(|_| {
                let links = simulate_links(&url);
                let wc = simulate_word_counts(&url);
                json!({ "url": url, "links": links, "word_counts": wc })
            });

        // ── ElasticPool checkin — return to free-list channel ──
        pool_tx.send((fetcher_name.clone(), fetcher_ref)).await?;

        *pool_fetch_counts.entry(fetcher_name).or_insert(0) += 1;

        let result: CrawlResult = serde_json::from_value(response)
            .unwrap_or_else(|_| CrawlResult {
                url: url.clone(), links: simulate_links(&url), word_counts: simulate_word_counts(&url),
            });

        // Enqueue new links — mark BEFORE pushing to frontier (mark-before-enqueue)
        if depth + 1 <= max_depth {
            for link in &result.links {
                let mut vis = visited.lock().unwrap();
                if !vis.contains(link) {
                    vis.insert(link.clone());
                    drop(vis); // release lock before pushing
                    frontier.push_back((link.clone(), depth + 1));
                }
            }
        }

        all_results.push(result);
    }

    // --- Step 5: Scatter to analyzer shards (ShardGroup — interleaved assignment) ---
    // Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
    // This balances load more evenly than contiguous chunks when page sizes vary.
    for (shard_idx, (_, analyzer)) in analyzer_refs.iter().enumerate() {
        let chunk: Vec<&CrawlResult> = all_results.iter().enumerate()
            .filter(|(i, _)| i % num_shards == shard_idx)
            .map(|(_, r)| r)
            .collect();
        if !chunk.is_empty() {
            let _ = ask(analyzer, &ctx, "analyze", json!({ "results": chunk })).await;
        }
    }

    // --- Step 6: Reduce — collect top words from all shards ---
    let mut global_counts: HashMap<String, usize> = HashMap::new();
    for (_, analyzer) in &analyzer_refs {
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
    let total_links: usize = all_results.iter().map(|r| r.links.len()).sum();

    // --- Step 7: ProcessGroup status gather — collect worker stats from registry ---
    let registry = worker_registry.read().unwrap();
    let mut worker_stats: Vec<Value> = Vec::new();
    for (name, actor_ref) in registry.iter() {
        if let Ok(stats) = ask(actor_ref, &ctx, "status_request", json!({})).await {
            worker_stats.push(json!({
                "worker_id": name,
                "fetch_count": stats.get("fetch_count").and_then(|v| v.as_u64()).unwrap_or(0),
                "last_url": stats.get("last_url").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
    }
    drop(registry);

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Pages crawled : {}", all_results.len());
    println!("  Total links   : {total_links}");
    println!("  Elapsed (ms)  : {:.2}", elapsed.as_secs_f64() * 1000.0);
    let total_fetches: usize = pool_fetch_counts.values().sum();
    println!("  Pool fetches  : {total_fetches} total across {pool_size} workers");
    println!("  Worker status :");
    for stat in &worker_stats {
        let wid = stat.get("worker_id").and_then(|v| v.as_str()).unwrap_or("?");
        let fc = stat.get("fetch_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let lu = stat.get("last_url").and_then(|v| v.as_str()).unwrap_or("");
        println!("    {wid:<12} fetch_count={fc}  last_url={lu}");
    }
    if !top_words.is_empty() {
        println!("  Top words:");
        for (word, count) in &top_words {
            println!("    {word:<20} {count}");
        }
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Key patterns demonstrated:");
    println!("    TupleSpace frontier  — mark-before-enqueue HashSet dedup");
    println!("    ElasticPool          — channel free-list checkout/checkin");
    println!("    ProcessGroup         — worker registry broadcast/collect");
    println!("    ShardGroup scatter   — interleaved i%num_shards assignment");

    // --- Step 8: Scaling benchmark (native Rust — no WASM overhead) ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Scaling benchmark: 1 / 4 / 8 / 16 workers × 200 pages");
    println!("  (Native Rust — no WASM sandbox overhead)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let bench_sl: Arc<dyn plexspaces_actor::ServiceLocator> = service_locator.clone();
    let bench_rows = run_benchmark(bench_sl, &ctx, &[1, 4, 8, 16], 200).await;

    println!("  ┌─────────┬────────┬───────────┬──────────┬──────────┬─────────────┬─────────┬──────────┬──────────────┬─────────────┐");
    println!("  │ Workers │ Pages  │ Elapsed   │ Coord ms │ Fetch ms │  Pages/sec  │ Speedup │  Eff %   │ Parallel frac│  Word data  │");
    println!("  ├─────────┼────────┼───────────┼──────────┼──────────┼─────────────┼─────────┼──────────┼──────────────┼─────────────┤");
    for r in &bench_rows {
        let tw = r.total_words;
        let uw = r.unique_words;
        println!("  │ {:>7} │ {:>6} │ {:>7} ms │ {:>8} │ {:>8} │ {:>11.1} │ {:>7.2}x │ {:>7.1}% │ {:>11.1}% │ {:>5}w/{:>3}u │",
            r.workers, r.pages, r.elapsed_ms, r.coord_ms, r.fetch_ms,
            r.pages_per_sec, r.speedup, r.efficiency_pct,
            r.parallel_fraction * 100.0, tw, uw);
    }
    println!("  └─────────┴────────┴───────────┴──────────┴──────────┴─────────────┴─────────┴──────────┴──────────────┴─────────────┘");
    println!();
    println!("  Legend: Speedup = vs 1-worker baseline,  Eff% = speedup/N×100");
    println!("          Parallel frac = 1 - coord/elapsed (Amdahl serial fraction = 1 - pf)");
    println!("          Word data = total occurrences / unique vocab per round");

    if let Some(last) = bench_rows.last() {
        if last.worker_fetches.iter().any(|&f| f > 0) {
            println!();
            println!("  Worker fetch distribution ({} workers, {} pages):", last.workers, last.pages);
            for (i, fc) in last.worker_fetches.iter().enumerate() {
                let bar: String = "█".repeat((fc / 2).min(40));
                println!("    fetcher-{:>2}: {:>4}  {}", i, fc, bar);
            }
        }
    }

    node.shutdown(Duration::from_secs(5)).await?;
    println!();
    println!("  ✓ Done");
    Ok(())
}

// =============================================================================
// Scaling benchmark — native Rust baseline (no WASM overhead)
// =============================================================================

#[derive(Debug)]
struct BenchRow {
    workers: usize,
    pages: usize,
    elapsed_ms: u128,
    coord_ms: u128,
    fetch_ms: u128,
    pages_per_sec: f64,
    speedup: f64,
    efficiency_pct: f64,
    parallel_fraction: f64,
    worker_fetches: Vec<usize>,
    total_words: usize,
    unique_words: usize,
}

async fn run_benchmark(
    service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    ctx: &RequestContext,
    worker_counts: &[usize],
    pages_per_round: usize,
) -> Vec<BenchRow> {
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
    let mut i = 0usize;
    while urls.len() < pages_per_round {
        let subs = ["v1","v2","v3","beta"];
        urls.push(format!("https://{}/{}/{}/{}",
            domains[i%domains.len()], sections[i%sections.len()],
            subpaths[i%subpaths.len()], subs[i%subs.len()]));
        i += 1;
    }
    urls.truncate(pages_per_round);
    let urls = Arc::new(urls);

    let mut rows: Vec<BenchRow> = Vec::new();
    let mut baseline_pps: Option<f64> = None;

    for &num_workers in worker_counts {
        // Spawn N fetchers
        let mut worker_refs: Vec<(String, ActorRef)> = Vec::with_capacity(num_workers);
        for wi in 0..num_workers {
            let name = format!("bench-fetcher-{wi}");
            let actor_ref = spawn(ctx, service_locator.clone(), name.clone(), "web_crawl",
                PageFetcher::new(Arc::new(RwLock::new(Vec::new()))))
                .await
                .expect("spawn bench fetcher");
            worker_refs.push((name, actor_ref));
        }

        let t0 = Instant::now();

        // Parallel dispatch: spawn one tokio task per worker, each processes its slice
        // (worker i handles urls[i], urls[i+N], urls[i+2N], ...) — no synchronization needed
        let worker_refs_arc = Arc::new(worker_refs);
        let mut handles = Vec::with_capacity(num_workers);
        for wi in 0..num_workers {
            let urls_c = urls.clone();
            let ctx_c = ctx.clone();
            let actor_ref = worker_refs_arc[wi].1.clone();
            let n = num_workers;
            handles.push(tokio::spawn(async move {
                let mut fetch_count = 0usize;
                let mut total_words = 0usize;
                let mut coord_ms = 0u128;
                let mut fetch_ms = 0u128;
                for idx in (wi..urls_c.len()).step_by(n) {
                    let url = &urls_c[idx];
                    let tf = Instant::now();
                    let response = ask(&actor_ref, &ctx_c, "fetch",
                        serde_json::json!({ "url": url })).await
                        .unwrap_or_else(|_| {
                            let wc = simulate_word_counts(url);
                            serde_json::json!({ "url": url, "word_counts": wc })
                        });
                    fetch_ms += tf.elapsed().as_millis();
                    let tc = Instant::now();
                    fetch_count += 1;
                    if let Some(wc) = response.get("word_counts").and_then(|v| v.as_object()) {
                        for (_, v) in wc {
                            total_words += v.as_u64().unwrap_or(0) as usize;
                        }
                    }
                    coord_ms += tc.elapsed().as_millis();
                }
                (fetch_count, total_words, fetch_ms, coord_ms)
            }));
        }

        let mut worker_fetches = vec![0usize; num_workers];
        let mut total_words = 0usize;
        let mut total_fetch_ms = 0u128;
        let mut total_coord_ms = 0u128;
        for (wi, h) in handles.into_iter().enumerate() {
            if let Ok((fc, tw, fm, cm)) = h.await {
                worker_fetches[wi] = fc;
                total_words += tw;
                total_fetch_ms += fm;
                total_coord_ms += cm;
            }
        }

        let elapsed_ms = t0.elapsed().as_millis();
        let pages_per_sec = if elapsed_ms > 0 {
            pages_per_round as f64 * 1000.0 / elapsed_ms as f64
        } else {
            0.0
        };
        let elapsed1 = elapsed_ms.max(1);
        let parallel_fraction = 1.0 - total_coord_ms as f64 / elapsed1 as f64;

        if baseline_pps.is_none() && pages_per_sec > 0.0 {
            baseline_pps = Some(pages_per_sec);
        }
        let speedup = pages_per_sec / baseline_pps.unwrap_or(1.0);
        let efficiency_pct = speedup / num_workers as f64 * 100.0;

        rows.push(BenchRow {
            workers: num_workers,
            pages: pages_per_round,
            elapsed_ms,
            coord_ms: total_coord_ms,
            fetch_ms: total_fetch_ms,
            pages_per_sec,
            speedup,
            efficiency_pct,
            parallel_fraction,
            worker_fetches,
            total_words,
            unique_words,
        });
    }

    rows
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
        assert_eq!(links.len(), 12);
        assert!(links.iter().all(|l| l.starts_with("https://example.com/")));
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

    #[test]
    fn test_mark_before_enqueue() {
        // Verify that the visited set prevents duplicate frontier entries
        let visited: HashSet<String> = HashSet::new();
        let visited = Arc::new(Mutex::new(visited));
        let mut frontier: VecDeque<(String, usize)> = VecDeque::new();

        let url = "https://example.com/docs".to_string();

        {
            let mut vis = visited.lock().unwrap();
            if !vis.contains(&url) {
                vis.insert(url.clone());
                frontier.push_back((url.clone(), 1));
            }
        }
        // Second attempt — should not enqueue again
        {
            let mut vis = visited.lock().unwrap();
            if !vis.contains(&url) {
                vis.insert(url.clone());
                frontier.push_back((url.clone(), 1));
            }
        }

        assert_eq!(frontier.len(), 1, "mark-before-enqueue should prevent duplicates");
    }
}
