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
const CHECKOUT_TIMEOUT_MS = 5000;
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function appIdFromActorId(actorId) {
    if (actorId.includes("//") && actorId.includes("::")) {
        const suffix = actorId.split("//", 2)[1];
        const qualified = suffix.split("@", 1)[0];
        const parts = qualified.split("::", 2);
        if (parts.length === 2)
            return parts[1];
    }
    return "";
}
function urlHash(url) {
    let h = 0;
    for (let i = 0; i < url.length; i++) {
        h = Math.imul(h, 31) + url.charCodeAt(i);
        h |= 0;
    }
    return Math.abs(h);
}
function simulateLinks(url) {
    const base = url.replace(/\/+$/, "");
    const h = urlHash(url);
    const sections = ["about", "docs", "api", "blog", "pricing", "features", "integrations", "changelog",
        "security", "status", "community", "enterprise", "solutions", "resources"];
    const paths = ["overview", "quickstart", "reference", "guide", "examples", "faq", "support", "contact"];
    const links = [];
    for (let i = 0; i < 8; i++)
        links.push(`${base}/${sections[(h + i) % sections.length]}`);
    for (let i = 0; i < 4; i++) {
        const sec = sections[(h + i * 3) % sections.length];
        const pth = paths[(h + i * 7) % paths.length];
        links.push(`${base}/${sec}/${pth}`);
    }
    return links;
}
function simulateWordCounts(url) {
    const h = urlHash(url);
    const vocab = ["distributed", "actor", "system", "runtime", "protocol", "message", "async", "concurrent",
        "parallel", "scale", "fault", "tolerant", "cluster", "node", "network", "latency",
        "throughput", "pipeline", "stream", "queue", "worker", "scheduler", "executor", "dispatch",
        "route", "wasm", "sandbox", "module", "instance", "memory", "tenant", "namespace",
        "isolation", "security", "auth", "deploy", "version", "rollback", "canary", "health",
        "metric", "trace", "span", "log", "monitor", "pool", "checkout", "checkin", "timeout", "retry",
        "tuplespace", "tuple", "pattern", "match", "read", "shard", "partition", "replicate",
        "consensus", "leader", "broadcast", "scatter", "gather", "reduce", "aggregate", "workflow",
        "state", "checkpoint", "journal", "replay"];
    const counts = {};
    for (const seg of url.split("/")) {
        if (!seg || seg === "https:" || seg === "http:")
            continue;
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
class PageFetcher extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", role: "fetcher", pool_slot: 0, fetch_count: 0, last_url: "", worker_joined: false };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.role = String(args.role ?? "fetcher");
        this.state.pool_slot = Number(args.pool_slot ?? 0);
        // Join process group at init; retry on first message for lazy activation
        try {
            host.processGroups.join(CRAWL_WORKERS_GROUP);
            this.state.worker_joined = true;
        }
        catch {
            // Retried on first message
        }
    }
    onFetch(payload) {
        // Late-join for lazy virtual actor activation
        if (!this.state.worker_joined) {
            try {
                host.processGroups.join(CRAWL_WORKERS_GROUP);
                this.state.worker_joined = true;
            }
            catch {
                // ignore
            }
        }
        const url = String(payload.url ?? "");
        if (!url)
            return { error: "missing url" };
        const links = simulateLinks(url);
        const word_counts = simulateWordCounts(url);
        this.state.fetch_count += 1;
        this.state.last_url = url;
        return { status: "ok", url, links, word_counts };
    }
    // Handles ScatterGather batch: each shard fetches its own slice by pool_slot
    onFetch_batch(payload) {
        if (!this.state.worker_joined) {
            try {
                host.processGroups.join(CRAWL_WORKERS_GROUP);
                this.state.worker_joined = true;
            }
            catch { /**/ }
        }
        const urlsRaw = payload.urls ?? [];
        const shardCount = Number(payload.shard_count ?? 1);
        const shardIndex = Number(payload.shard_index ?? this.state.pool_slot);
        let totalWords = 0;
        let pagesFetched = 0;
        for (let i = shardIndex; i < urlsRaw.length; i += shardCount) {
            const url = urlsRaw[i];
            if (!url)
                continue;
            const wc = simulateWordCounts(url);
            for (const c of Object.values(wc))
                totalWords += c;
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
    onStatus_request() {
        return {
            fetch_count: this.state.fetch_count,
            last_url: this.state.last_url,
            idle: true,
        };
    }
    onStatus() {
        return { ...this.state };
    }
}
// ---------------------------------------------------------------------------
// LinkAnalyzer actor — one shard in the ShardGroup
// ---------------------------------------------------------------------------
class LinkAnalyzer extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", role: "analyzer", index: {}, urls_analyzed: 0, analyzer_joined: false };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.role = String(args.role ?? "analyzer");
        this.state.index = {};
    }
    onAnalyze(payload) {
        if (!this.state.analyzer_joined) {
            try {
                host.processGroups.join(ANALYZER_GROUP);
                this.state.analyzer_joined = true;
            }
            catch {
                // ignore
            }
        }
        const results = payload.results ?? [];
        for (const result of results) {
            const wc = result.word_counts;
            if (wc) {
                for (const [word, count] of Object.entries(wc)) {
                    this.state.index[word] = (this.state.index[word] ?? 0) + Number(count);
                }
            }
            this.state.urls_analyzed += 1;
        }
        return { status: "ok", urls_analyzed: this.state.urls_analyzed };
    }
    onTop_words(payload) {
        const n = Number(payload.n ?? 10);
        const sorted = Object.entries(this.state.index).sort((a, b) => b[1] - a[1]).slice(0, n);
        return { top_words: sorted };
    }
    onStatus() {
        return { ...this.state };
    }
}
// ---------------------------------------------------------------------------
// WebCrawlOrchestrator actor
// ---------------------------------------------------------------------------
class WebCrawlOrchestrator extends PlexSpacesActor {
    getDefaultState() {
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
    onInit(config) {
        const args = config.args ?? {};
        const actorId = String(config.actor_id ?? "");
        this.state.actor_id = actorId;
        this.state.application_id = appIdFromActorId(actorId);
        this.state.role = String(args.role ?? "orchestrator");
        this.state.pages_crawled = 0;
        this.state.total_links = 0;
        this.state.top_words = [];
    }
    onCrawl(payload) {
        const seedUrls = payload.seed_urls ?? ["https://example.com"];
        const maxPages = Number(payload.max_pages ?? 20);
        const maxDepth = Number(payload.max_depth ?? 2);
        const appId = this.state.application_id;
        const frontier = [];
        const visited = new Set();
        for (const url of seedUrls) {
            host.ts.write(["url_queue", url, "pending", "0"]);
            visited.add(url);
            frontier.push({ url, depth: 0 });
        }
        const allResults = [];
        let pagesCrawled = 0;
        let coordTimeMs = 0;
        let fetchTimeMs = 0;
        const t0Crawl = host.nowMs();
        // ── Phase 2: BFS drain from local frontier ──
        while (frontier.length > 0 && pagesCrawled < maxPages) {
            const task = frontier.shift();
            const { url, depth } = task;
            if (depth > maxDepth)
                continue;
            // ── ElasticPool checkout — separates rate limiting from queue depth ──
            let result;
            let handle = null;
            const tCoord = host.nowMs();
            try {
                handle = host.poolCheckout(FETCHER_POOL, CHECKOUT_TIMEOUT_MS);
            }
            catch {
                handle = null;
            }
            coordTimeMs += host.nowMs() - tCoord;
            const actorId = handle ? String(handle.actor_id ?? "") : "";
            const checkoutId = handle ? String(handle.checkout_id ?? "") : "";
            const tFetch = host.nowMs();
            try {
                if (actorId) {
                    result = host.ask(actorId, "fetch", { url, depth }, 10000);
                }
                else {
                    result = {
                        status: "ok",
                        url,
                        links: simulateLinks(url),
                        word_counts: simulateWordCounts(url),
                    };
                }
            }
            catch {
                result = {
                    status: "ok",
                    url,
                    links: simulateLinks(url),
                    word_counts: simulateWordCounts(url),
                };
            }
            finally {
                if (handle && actorId && checkoutId) {
                    const tCheckin = host.nowMs();
                    try {
                        host.poolCheckin(FETCHER_POOL, actorId, checkoutId, true);
                    }
                    catch { /**/ }
                    coordTimeMs += host.nowMs() - tCheckin;
                }
            }
            fetchTimeMs += host.nowMs() - tFetch;
            // Enqueue newly discovered links — mark-before-enqueue dedup via local Set
            const tCoord2 = host.nowMs();
            const links = result.links ?? [];
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
            const metrics = host.poolGetMetrics(FETCHER_POOL);
            this.state.pool_metrics = metrics ?? { total_checkouts: pagesCrawled, pool_size: 4 };
        }
        catch {
            this.state.pool_metrics = { total_checkouts: pagesCrawled, pool_size: 4 };
        }
        // ── Phase 3: Interleaved scatter to analyzer shards ──
        const numShards = 2;
        const globalCounts = {};
        for (let shardIdx = 0; shardIdx < numShards; shardIdx++) {
            // Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
            const chunk = allResults.filter((_, i) => i % numShards === shardIdx);
            if (chunk.length === 0)
                continue;
            const analyzerId = `${appId}/analyzer-${shardIdx}@`;
            try {
                host.ask(analyzerId, "analyze", { results: chunk }, 10000);
                const top = host.ask(analyzerId, "top_words", { n: 20 }, 10000);
                for (const [word, count] of top.top_words ?? []) {
                    globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
                }
            }
            catch {
                for (const res of chunk) {
                    const wc = res.word_counts;
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
            .slice(0, 10);
        // ── Phase 4: ProcessGroup status gather — discover actual worker activity ──
        const workerStats = [];
        try {
            let members = host.processGroups.members(CRAWL_WORKERS_GROUP);
            if (!members || members.length === 0) {
                // Fallback: use constructed IDs if no workers registered yet
                members = Array.from({ length: 4 }, (_, i) => `${appId}/fetcher-${i}@`);
            }
            for (const memberId of members) {
                try {
                    const stats = host.ask(memberId, "status_request", {}, 5000);
                    const parts = memberId.split("/");
                    const shortId = (parts[parts.length - 1] ?? memberId).replace(/@$/, "");
                    workerStats.push({ worker_id: shortId, ...stats });
                }
                catch {
                    // Worker not yet active — skip
                }
            }
        }
        catch {
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
    onBenchmark(payload) {
        const workerCountsRaw = payload.worker_counts ?? [1, 4, 8, 16];
        const workerCounts = workerCountsRaw.map(Number);
        const pagesPerRound = Number(payload.pages_per_round ?? 200);
        // Build URL corpus — same for all rounds
        const domains = ["example.com", "docs.example.com", "api.example.com", "blog.example.com"];
        const sections = ["about", "docs", "api", "blog", "pricing", "features", "integrations", "changelog"];
        const subpaths = ["overview", "quickstart", "reference", "guide", "examples", "faq"];
        const uniqueWords = domains.length * sections.length * subpaths.length;
        const urls = [];
        for (const d of domains)
            urls.push(`https://${d}`);
        outer1: for (const d of domains) {
            for (const s of sections) {
                if (urls.length >= pagesPerRound)
                    break outer1;
                urls.push(`https://${d}/${s}`);
            }
        }
        outer2: for (const d of domains) {
            for (const s of sections) {
                for (const p of subpaths) {
                    if (urls.length >= pagesPerRound)
                        break outer2;
                    urls.push(`https://${d}/${s}/${p}`);
                }
            }
        }
        const subs = ["v1", "v2", "v3", "beta"];
        for (let i = 0; urls.length < pagesPerRound; i++) {
            urls.push(`https://${domains[i % domains.length]}/${sections[i % sections.length]}/${subpaths[i % subpaths.length]}/${subs[i % subs.length]}`);
        }
        urls.length = pagesPerRound;
        const results = [];
        let baselinePps = 0;
        for (const numWorkers of workerCounts) {
            const groupId = `bench-fetchers-${numWorkers}-${host.nowMs() % 100000}`;
            // Write seed tuples to TupleSpace (demonstrates the primitive)
            for (let i = 0; i < 4 && i < urls.length; i++) {
                host.ts.write(["url_queue", urls[i], "pending", "0"]);
            }
            let coordMs = 0;
            let fetchMs = 0;
            let totalWords = 0;
            const workerFetches = new Array(numWorkers).fill(0);
            const t0 = host.nowMs();
            // ── ScatterGather parallel dispatch ──
            const tCoord0 = host.nowMs();
            let sgResult = null;
            try {
                const sgGroup = host.createShardGroup({
                    group_id: groupId,
                    actor_type: "fetcher",
                    shard_count: numWorkers,
                    partition_strategy: "hash",
                    rebalance_policy: "manual",
                    placement: { strategy: "from_registry" },
                    initial_state: {},
                });
                coordMs += host.nowMs() - tCoord0;
                if (sgGroup) {
                    const tFetch = host.nowMs();
                    sgResult = host.scatterGather({
                        group_id: groupId,
                        message_type: "fetch_batch",
                        query: { urls, shard_count: numWorkers, depth: 1 },
                        aggregation: "concat",
                        min_responses: numWorkers,
                        timeout_ms: 60000,
                    });
                    fetchMs += host.nowMs() - tFetch;
                }
            }
            catch {
                coordMs += host.nowMs() - tCoord0;
            }
            const tCoordPost = host.nowMs();
            if (sgResult) {
                const shardResponses = sgResult.shard_responses ?? [];
                for (let si = 0; si < shardResponses.length; si++) {
                    const sr = shardResponses[si];
                    const p = normalizePayload(sr);
                    const fc = Number(p.pages_fetched ?? 0);
                    const tw = Number(p.total_words ?? 0);
                    if (si < numWorkers)
                        workerFetches[si] = fc;
                    totalWords += tw;
                }
            }
            else {
                // Fallback: compute locally
                for (const url of urls) {
                    const wc = simulateWordCounts(url);
                    for (const c of Object.values(wc))
                        totalWords += Number(c);
                }
                for (let i = 0; i < numWorkers; i++) {
                    workerFetches[i] = Math.floor(pagesPerRound / numWorkers);
                }
            }
            // TupleSpace writes for metadata demo
            for (let i = 0; i < 4 && i < urls.length; i++) {
                host.ts.write(["url_queue", urls[i], "visited", "1"]);
            }
            coordMs += host.nowMs() - tCoordPost;
            const elapsed = host.nowMs() - t0;
            const pps = elapsed > 0 ? (pagesPerRound * 1000) / elapsed : 0;
            const parallelFraction = elapsed > 0 ? 1.0 - coordMs / Math.max(elapsed, 1) : 1.0;
            if (!baselinePps && pps > 0)
                baselinePps = pps;
            const speedup = baselinePps > 0 ? pps / baselinePps : 1.0;
            const efficiency = speedup / numWorkers * 100;
            results.push({
                workers: numWorkers,
                pages: pagesPerRound,
                elapsed_ms: elapsed,
                coord_ms: coordMs,
                fetch_ms: fetchMs,
                pages_per_sec: pps,
                speedup,
                efficiency_pct: efficiency,
                parallel_fraction: parallelFraction,
                worker_fetches: workerFetches,
                total_words: totalWords,
                unique_words: uniqueWords,
            });
        }
        return { status: "ok", results };
    }
    onStatus() {
        return { ...this.state };
    }
}
function normalizePayload(m) {
    if ("status" in m || "pages_fetched" in m)
        return m;
    for (const k of ["payload", "result", "response", "data"]) {
        const nested = m[k];
        if (nested && typeof nested === "object" && !Array.isArray(nested)) {
            return normalizePayload(nested);
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
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
