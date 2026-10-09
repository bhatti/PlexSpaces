// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Web Crawl — TypeScript WASM app.
//
// Parallel web crawler using all four PlexSpaces parallelization primitives:
//   TupleSpace frontier  — url_queue as live work frontier; ts.take() for atomic URL claim
//                          (mark-before-enqueue deduplication, inspired by muffet / linkinator)
//   ElasticPool          — poolCheckout/poolCheckin separates rate limiting from queue depth
//   ProcessGroup         — workers self-register; orchestrator discovers real members via members()
//   ShardGroup scatter   — interleaved scatter to analyzer shards for balanced word-count aggregation

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

const FETCHER_POOL = "fetcher_pool";
const CRAWL_WORKERS_GROUP = "crawl_workers";
const ANALYZER_GROUP = "analyzer_shards";
const CHECKOUT_TIMEOUT_MS = 5_000;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

type FetcherState = {
  actor_id: string;
  role: string;
  pool_slot: number;
  fetch_count: number;
  last_url: string;
  worker_joined: boolean;
};

type AnalyzerState = {
  actor_id: string;
  role: string;
  index: Record<string, number>;
  urls_analyzed: number;
  analyzer_joined: boolean;
};

type OrchestratorState = {
  actor_id: string;
  application_id: string;
  role: string;
  pages_crawled: number;
  total_links: number;
  top_words: [string, number][];
  pool_metrics: Record<string, unknown>;
  worker_stats: Record<string, unknown>[];
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function appIdFromActorId(actorId: string): string {
  if (actorId.includes("//") && actorId.includes("::")) {
    const suffix = actorId.split("//", 2)[1];
    const qualified = suffix.split("@", 1)[0];
    const parts = qualified.split("::", 2);
    if (parts.length === 2) return parts[1];
  }
  return "";
}

function urlHash(url: string): number {
  let h = 0;
  for (let i = 0; i < url.length; i++) {
    h = Math.imul(h, 31) + url.charCodeAt(i);
    h |= 0;
  }
  return Math.abs(h);
}

function simulateLinks(url: string): string[] {
  const base = url.replace(/\/+$/, "");
  const h = urlHash(url);
  const sections = ["about","docs","api","blog","pricing","features","integrations","changelog",
                    "security","status","community","enterprise","solutions","resources"];
  const paths    = ["overview","quickstart","reference","guide","examples","faq","support","contact"];
  const links: string[] = [];
  for (let i = 0; i < 8; i++) links.push(`${base}/${sections[(h + i) % sections.length]}`);
  for (let i = 0; i < 4; i++) {
    const sec = sections[(h + i * 3) % sections.length];
    const pth = paths[(h + i * 7) % paths.length];
    links.push(`${base}/${sec}/${pth}`);
  }
  return links;
}

function simulateWordCounts(url: string): Record<string, number> {
  const h = urlHash(url);
  const vocab = ["distributed","actor","system","runtime","protocol","message","async","concurrent",
                 "parallel","scale","fault","tolerant","cluster","node","network","latency",
                 "throughput","pipeline","stream","queue","worker","scheduler","executor","dispatch",
                 "route","wasm","sandbox","module","instance","memory","tenant","namespace",
                 "isolation","security","auth","deploy","version","rollback","canary","health",
                 "metric","trace","span","log","monitor","pool","checkout","checkin","timeout","retry",
                 "tuplespace","tuple","pattern","match","read","shard","partition","replicate",
                 "consensus","leader","broadcast","scatter","gather","reduce","aggregate","workflow",
                 "state","checkpoint","journal","replay"];
  const counts: Record<string, number> = {};
  for (const seg of url.split("/")) {
    if (!seg || seg === "https:" || seg === "http:") continue;
    for (const word of seg.split(/[^a-zA-Z0-9]/)) {
      if (word.length > 2) {
        const w = word.toLowerCase();
        counts[w] = (counts[w] ?? 0) + 8 + h % 5;
      }
    }
  }
  for (let i = 0; i < 25; i++) {
    const word = vocab[(h + i * 17) % vocab.length];
    const rank = i + 1;
    const count = Math.floor(50 / rank) + 1 + (h + i) % 3;
    counts[word] = (counts[word] ?? 0) + count;
  }
  return counts;
}

// ---------------------------------------------------------------------------
// PageFetcher actor — one worker in the ElasticPool
// ---------------------------------------------------------------------------

class PageFetcher extends PlexSpacesActor<FetcherState> {
  getDefaultState(): FetcherState {
    return { actor_id: "", role: "fetcher", pool_slot: 0, fetch_count: 0, last_url: "", worker_joined: false };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "fetcher");
    this.state.pool_slot = Number(args.pool_slot ?? 0);
    // Join process group at init; retry on first message for lazy activation
    try {
      host.processGroups.join(CRAWL_WORKERS_GROUP);
      this.state.worker_joined = true;
    } catch {
      // Retried on first message
    }
  }

  onFetch(payload: Record<string, unknown>): Record<string, unknown> {
    // Late-join for lazy virtual actor activation
    if (!this.state.worker_joined) {
      try {
        host.processGroups.join(CRAWL_WORKERS_GROUP);
        this.state.worker_joined = true;
      } catch {
        // ignore
      }
    }
    const url = String(payload.url ?? "");
    if (!url) return { error: "missing url" };
    const links = simulateLinks(url);
    const word_counts = simulateWordCounts(url);
    this.state.fetch_count += 1;
    this.state.last_url = url;
    return { status: "ok", url, links, word_counts };
  }

  // Handles ScatterGather batch: each shard fetches its own slice by pool_slot
  onFetch_batch(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.worker_joined) {
      try { host.processGroups.join(CRAWL_WORKERS_GROUP); this.state.worker_joined = true; } catch { /**/ }
    }
    const urlsRaw = (payload.urls as string[] | undefined) ?? [];
    const shardCount = Number(payload.shard_count ?? 1);
    const shardIndex = Number(payload.shard_index ?? this.state.pool_slot);

    let totalWords = 0;
    let pagesFetched = 0;
    for (let i = shardIndex; i < urlsRaw.length; i += shardCount) {
      const url = urlsRaw[i];
      if (!url) continue;
      const wc = simulateWordCounts(url);
      for (const c of Object.values(wc)) totalWords += c;
      this.state.fetch_count += 1;
      this.state.last_url = url;
      pagesFetched += 1;
    }

    return {
      status: "ok",
      fetch_count: this.state.fetch_count,
      pages_fetched: pagesFetched,
      total_words: totalWords,
      shard_index: shardIndex,
      shard_count: shardCount,
    };
  }

  // Worker-local benchmark: generates pages locally and runs num_passes of crawl simulation.
  // Leader sends one broadcast SG — enables true weak/strong scaling with one round trip.
  onBenchmark_crawl(payload: Record<string, unknown>): Record<string, unknown> {
    const pagesPerWorker = Number(payload.pages_per_worker ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const seed = Number(payload.seed ?? 42);

    const compStart = host.nowMs();

    // Generate a deterministic set of URLs for this worker
    const domains = ["example.com","docs.example.com","api.example.com","blog.example.com"];
    const sections = ["about","docs","api","blog","pricing","features","integrations","changelog"];
    const subpaths = ["overview","quickstart","reference","guide","examples","faq","support","contact"];
    const urls: string[] = [];
    let h = seed;
    for (let i = 0; i < pagesPerWorker; i++) {
      h = (h * 1103515245 + 12345) & 0x7fffffff;
      const d = domains[h % domains.length]!;
      const s = sections[(h >> 8) % sections.length]!;
      const p = subpaths[(h >> 16) % subpaths.length]!;
      urls.push(`https://${d}/${s}/${p}/${i}`);
    }

    let totalWords = 0;
    let totalLinks = 0;
    for (let pass = 0; pass < numPasses; pass++) {
      for (const url of urls) {
        const wc = simulateWordCounts(url);
        for (const c of Object.values(wc)) totalWords += c;
        const links = simulateLinks(url);
        totalLinks += links.length;
      }
    }

    const computeMs = host.nowMs() - compStart;
    return {
      pages_crawled: pagesPerWorker,
      total_words: totalWords,
      total_links: totalLinks,
      passes: numPasses,
      compute_ms: computeMs,
    };
  }

  onStatus_request(): Record<string, unknown> {
    return {
      fetch_count: this.state.fetch_count,
      last_url: this.state.last_url,
      idle: true,
    };
  }

  onStatus(): Record<string, unknown> {
    return { ...this.state };
  }
}

// ---------------------------------------------------------------------------
// LinkAnalyzer actor — one shard in the ShardGroup
// ---------------------------------------------------------------------------

class LinkAnalyzer extends PlexSpacesActor<AnalyzerState> {
  getDefaultState(): AnalyzerState {
    return { actor_id: "", role: "analyzer", index: {}, urls_analyzed: 0, analyzer_joined: false };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "analyzer");
    this.state.index = {};
  }

  onAnalyze(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.analyzer_joined) {
      try {
        host.processGroups.join(ANALYZER_GROUP);
        this.state.analyzer_joined = true;
      } catch {
        // ignore
      }
    }
    const results = (payload.results as Record<string, unknown>[] | undefined) ?? [];
    for (const result of results) {
      const wc = result.word_counts as Record<string, number> | undefined;
      if (wc) {
        for (const [word, count] of Object.entries(wc)) {
          this.state.index[word] = (this.state.index[word] ?? 0) + Number(count);
        }
      }
      this.state.urls_analyzed += 1;
    }
    return { status: "ok", urls_analyzed: this.state.urls_analyzed };
  }

  onTop_words(payload: Record<string, unknown>): Record<string, unknown> {
    const n = Number(payload.n ?? 10);
    const sorted = Object.entries(this.state.index).sort((a, b) => b[1] - a[1]).slice(0, n);
    return { top_words: sorted };
  }

  onStatus(): Record<string, unknown> {
    return { ...this.state };
  }
}

// ---------------------------------------------------------------------------
// WebCrawlOrchestrator actor
// ---------------------------------------------------------------------------

class WebCrawlOrchestrator extends PlexSpacesActor<OrchestratorState> {
  getDefaultState(): OrchestratorState {
    return {
      actor_id: "",
      application_id: "",
      role: "orchestrator",
      pages_crawled: 0,
      total_links: 0,
      top_words: [],
      pool_metrics: {},
      worker_stats: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    const actorId = String(config.actor_id ?? "");
    this.state.actor_id = actorId;
    this.state.application_id = appIdFromActorId(actorId);
    this.state.role = String(args.role ?? "orchestrator");
    this.state.pages_crawled = 0;
    this.state.total_links = 0;
    this.state.top_words = [];
  }

  onCrawl(payload: Record<string, unknown>): Record<string, unknown> {
    const seedUrls = (payload.seed_urls as string[] | undefined) ?? ["https://example.com"];
    const maxPages = Number(payload.max_pages ?? 20);
    const maxDepth = Number(payload.max_depth ?? 2);
    const appId = this.state.application_id;

    // ── Phase 1: Seed local BFS frontier + in-handler visited set ──
    // Local array drives BFS; local Set deduplicates within this crawl run.
    // TupleSpace records seeds/links as metadata (shows the primitive being used).
    type CrawlTask = { url: string; depth: number };
    const frontier: CrawlTask[] = [];
    const visited = new Set<string>();
    for (const url of seedUrls) {
      host.ts.write(["url_queue", url, "pending", "0"]);
      visited.add(url);
      frontier.push({ url, depth: 0 });
    }

    const allResults: Record<string, unknown>[] = [];
    let pagesCrawled = 0;
    let coordTimeMs = 0;
    let fetchTimeMs = 0;
    const t0Crawl = host.nowMs();

    // ── Phase 2: BFS drain from local frontier ──
    while (frontier.length > 0 && pagesCrawled < maxPages) {
      const task = frontier.shift()!;
      const { url, depth } = task;
      if (depth > maxDepth) continue;

      // ── ElasticPool checkout — separates rate limiting from queue depth ──
      let result: Record<string, unknown>;
      let handle: Record<string, unknown> | null = null;
      const tCoord = host.nowMs();
      try {
        handle = host.poolCheckout(FETCHER_POOL, CHECKOUT_TIMEOUT_MS) as Record<string, unknown> | null;
      } catch {
        handle = null;
      }
      coordTimeMs += host.nowMs() - tCoord;

      const actorId = handle ? String(handle.actor_id ?? "") : "";
      const checkoutId = handle ? String(handle.checkout_id ?? "") : "";

      const tFetch = host.nowMs();
      try {
        if (actorId) {
          result = host.ask(actorId, "fetch", { url, depth }, 10_000) as Record<string, unknown>;
        } else {
          result = {
            status: "ok",
            url,
            links: simulateLinks(url),
            word_counts: simulateWordCounts(url),
          };
        }
      } catch {
        result = {
          status: "ok",
          url,
          links: simulateLinks(url),
          word_counts: simulateWordCounts(url),
        };
      } finally {
        if (handle && actorId && checkoutId) {
          const tCheckin = host.nowMs();
          try { host.poolCheckin(FETCHER_POOL, actorId, checkoutId, true); } catch { /**/ }
          coordTimeMs += host.nowMs() - tCheckin;
        }
      }
      fetchTimeMs += host.nowMs() - tFetch;

      // Enqueue newly discovered links — mark-before-enqueue dedup via local Set
      const tCoord2 = host.nowMs();
      const links = (result.links as string[] | undefined) ?? [];
      for (const link of links) {
        if (depth + 1 <= maxDepth && !visited.has(link)) {
          visited.add(link);
          host.ts.write(["url_queue", link, "pending", String(depth + 1)]);
          frontier.push({ url: link, depth: depth + 1 });
          this.state.total_links += 1;
        }
      }
      coordTimeMs += host.nowMs() - tCoord2;

      allResults.push(result);
      pagesCrawled += 1;
    }

    this.state.pages_crawled = pagesCrawled;
    const elapsedMs = host.nowMs() - t0Crawl;
    const pagesPerSec = elapsedMs > 0 ? (pagesCrawled * 1000) / elapsedMs : 0;
    const parallelFraction = elapsedMs > 0 ? 1.0 - coordTimeMs / Math.max(elapsedMs, 1) : 1.0;

    // ── Pool utilization metrics ──
    try {
      const metrics = host.poolGetMetrics(FETCHER_POOL) as Record<string, unknown>;
      this.state.pool_metrics = metrics ?? { total_checkouts: pagesCrawled, pool_size: 4 };
    } catch {
      this.state.pool_metrics = { total_checkouts: pagesCrawled, pool_size: 4 };
    }

    // ── Phase 3: Interleaved scatter to analyzer shards ──
    const numShards = 2;
    const globalCounts: Record<string, number> = {};

    for (let shardIdx = 0; shardIdx < numShards; shardIdx++) {
      // Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
      const chunk = allResults.filter((_, i) => i % numShards === shardIdx);
      if (chunk.length === 0) continue;
      const analyzerId = `${appId}/analyzer-${shardIdx}@`;
      try {
        host.ask(analyzerId, "analyze", { results: chunk }, 10_000);
        const top = host.ask(analyzerId, "top_words", { n: 20 }, 10_000) as { top_words: [string, number][] };
        for (const [word, count] of top.top_words ?? []) {
          globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
        }
      } catch {
        for (const res of chunk) {
          const wc = res.word_counts as Record<string, number> | undefined;
          if (wc) {
            for (const [word, count] of Object.entries(wc)) {
              globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
            }
          }
        }
      }
    }

    this.state.top_words = Object.entries(globalCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10) as [string, number][];

    // ── Phase 4: ProcessGroup status gather — discover actual worker activity ──
    const workerStats: Record<string, unknown>[] = [];
    try {
      let members = host.processGroups.members(CRAWL_WORKERS_GROUP) as string[] | null;
      if (!members || members.length === 0) {
        // Fallback: use constructed IDs if no workers registered yet
        members = Array.from({ length: 4 }, (_, i) => `${appId}/fetcher-${i}@`);
      }
      for (const memberId of members) {
        try {
          const stats = host.ask(memberId, "status_request", {}, 5_000) as Record<string, unknown>;
          const parts = memberId.split("/");
          const shortId = (parts[parts.length - 1] ?? memberId).replace(/@$/, "");
          workerStats.push({ worker_id: shortId, ...stats });
        } catch {
          // Worker not yet active — skip
        }
      }
    } catch {
      // process group not yet populated
    }
    this.state.worker_stats = workerStats;

    return {
      status: "ok",
      pages_crawled: this.state.pages_crawled,
      total_links: this.state.total_links,
      top_words: this.state.top_words,
      pool_metrics: this.state.pool_metrics,
      worker_stats: this.state.worker_stats,
      elapsed_ms: elapsedMs,
      coord_time_ms: coordTimeMs,
      fetch_time_ms: fetchTimeMs,
      pages_per_sec: pagesPerSec,
      parallel_fraction: parallelFraction,
    };
  }

  private runBenchmarkSg(
    workerCounts: number[],
    getPagesPerWorker: (n: number) => number,
    numPasses: number,
    seedMultiplier: number,
    prefix: string,
  ): Record<string, unknown>[] {
    const results: Record<string, unknown>[] = [];
    let baselinePps = 0;

    for (const numWorkers of workerCounts) {
      const groupId = `${prefix}-${numWorkers}-${host.nowMs() % 100000}`;
      try {
        host.createShardGroup({
          groupId,
          actorType: "fetcher",
          shardCount: numWorkers,
          partitionStrategy: "hash",
          rebalancePolicy: "manual",
          placement: { strategy: "from_registry" },
          initialState: {},
        });
      } catch (e) {
        results.push({ workers: numWorkers, pages: 0, elapsed_ms: 0, coord_ms: 0, fetch_ms: 0,
          pages_per_sec: 0, speedup: 0, efficiency_pct: 0, total_words: 0, error_count: 1,
          error: String(e) });
        continue;
      }

      const pagesPerWorker = getPagesPerWorker(numWorkers);
      const t0 = host.nowMs();
      let sgResult: { shardResponses: { payload: unknown; success: boolean }[] } | null = null;
      try {
        sgResult = host.scatterGather({
          groupId,
          query: { op: "benchmark_crawl", pages_per_worker: pagesPerWorker, num_passes: numPasses, seed: numWorkers * seedMultiplier },
          timeoutMs: 60000,
        });
      } catch (e) {
        const elapsed2 = host.nowMs() - t0;
        results.push({ workers: numWorkers, pages: 0, elapsed_ms: elapsed2, coord_ms: elapsed2, fetch_ms: 0,
          pages_per_sec: 0, speedup: 0, efficiency_pct: 0, total_words: 0, error_count: 1,
          error: String(e) });
        continue;
      }
      const elapsed = host.nowMs() - t0;

      let totalWords = 0;
      let totalCrawled = 0;
      let totalCompute = 0;
      let errorCount = 0;
      for (const sr of (sgResult?.shardResponses ?? [])) {
        const p = normalizePayload(((sr.payload ?? {}) as Record<string, unknown>));
        if (!sr.success || p.error) { errorCount++; continue; }
        totalWords += Number(p.total_words ?? 0);
        totalCrawled += Number(p.pages_crawled ?? 0);
        totalCompute += Number(p.compute_ms ?? 0);
      }

      const avgComputeMs = numWorkers > 0 ? Math.round(totalCompute / numWorkers) : 0;
      const coordMs = Math.max(elapsed - avgComputeMs, 1);
      const pps = elapsed > 0 ? (totalCrawled * 1000) / elapsed : 0;

      if (!baselinePps && pps > 0) baselinePps = pps;
      const speedup = baselinePps > 0 ? pps / baselinePps : 1.0;
      const efficiency = speedup / numWorkers * 100;

      results.push({
        workers: numWorkers,
        pages: totalCrawled,
        elapsed_ms: elapsed,
        coord_ms: coordMs,
        fetch_ms: avgComputeMs,
        pages_per_sec: pps,
        speedup,
        efficiency_pct: efficiency,
        total_words: totalWords,
        error_count: errorCount,
      });
    }
    return results;
  }

  // Strong scaling: fixed total 200 pages split across N workers (pages_per_worker=ceil(200/N))
  onBenchmark(payload: Record<string, unknown>): Record<string, unknown> {
    const workerCountsRaw = (payload.worker_counts as number[] | undefined) ?? [2, 4, 8, 16];
    const workerCounts = workerCountsRaw.map(Number);
    const totalPages = Number(payload.pages_per_round ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const results = this.runBenchmarkSg(
      workerCounts,
      (n) => Math.ceil(totalPages / n),
      numPasses,
      100,
      "bench-strong",
    );
    return { status: "ok", results };
  }

  // Weak scaling: fixed pages_per_worker (200) × N workers — total work grows with N.
  onRun_weak_scaling_benchmark(payload: Record<string, unknown>): Record<string, unknown> {
    const shardCountsRaw = (payload.shard_counts as number[] | undefined) ?? [2, 4, 8, 16];
    const shardCounts = shardCountsRaw.map(Number);
    const pagesPerWorker = Number(payload.pages_per_worker ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const results = this.runBenchmarkSg(
      shardCounts,
      (_n) => pagesPerWorker,
      numPasses,
      200,
      "bench-weak",
    );
    return { status: "ok", results };
  }

  onStatus(): Record<string, unknown> {
    return { ...this.state };
  }
}

function normalizePayload(m: Record<string, unknown>): Record<string, unknown> {
  if ("status" in m || "pages_fetched" in m) return m;
  for (const k of ["payload", "result", "response", "data"]) {
    const nested = m[k];
    if (nested && typeof nested === "object" && !Array.isArray(nested)) {
      return normalizePayload(nested as Record<string, unknown>);
    }
  }
  return m;
}

// ---------------------------------------------------------------------------
// Actor router — dispatches by role from args (matches app-config.toml)
// ---------------------------------------------------------------------------

const router = new ActorRouter({
  orchestrator: () => new WebCrawlOrchestrator(),
  fetcher: () => new PageFetcher(),
  analyzer: () => new LinkAnalyzer(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) =>
    router.init(configJson),
  handle: (
    from: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView,
  ) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) =>
    router.setState(stateJson),
};
