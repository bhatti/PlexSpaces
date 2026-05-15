// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Web Crawl — TypeScript WASM app.
//
// Parallel web crawler using:
//   ElasticPool pattern  — 4 PageFetcher actors, round-robin across URLs
//   TupleSpace           — url_queue space: pending → done URL tracking
//   ShardGroup pattern   — 2 analyzer shards scatter/reduce word counts
//
// Inspired by Ray's web-crawl and map-reduce examples:
//   https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html
//   https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
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
function simulateLinks(url) {
    const base = url.replace(/\/+$/, "");
    return [`${base}/about`, `${base}/docs`, `${base}/api`];
}
function simulateWordCounts(url) {
    const counts = {};
    for (const seg of url.split("/")) {
        if (!seg || seg === "https:" || seg === "http:")
            continue;
        for (const word of seg.split(/[^a-zA-Z0-9]/)) {
            if (word.length > 2) {
                const w = word.toLowerCase();
                counts[w] = (counts[w] ?? 0) + 1;
            }
        }
    }
    return counts;
}
// ---------------------------------------------------------------------------
// PageFetcher actor
// ---------------------------------------------------------------------------
class PageFetcher extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", role: "fetcher", fetch_count: 0 };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.role = String(args.role ?? "fetcher");
    }
    onFetch(payload) {
        const url = String(payload.url ?? "");
        if (!url)
            return { error: "missing url" };
        const links = simulateLinks(url);
        const word_counts = simulateWordCounts(url);
        this.state.fetch_count += 1;
        return { status: "ok", url, links, word_counts };
    }
    onStatus() {
        return { ...this.state };
    }
}
// ---------------------------------------------------------------------------
// LinkAnalyzer actor
// ---------------------------------------------------------------------------
class LinkAnalyzer extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", role: "analyzer", index: {}, urls_analyzed: 0 };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.role = String(args.role ?? "analyzer");
        this.state.index = {};
    }
    onAnalyze(payload) {
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
        const visited = new Set();
        const queue = seedUrls.map((url) => ({ url, depth: 0 }));
        // Seed TupleSpace url_queue (pending URLs)
        for (const url of seedUrls) {
            host.ts.write(["url_queue", url, "pending"]);
        }
        const allResults = [];
        let fetcherIdx = 0;
        const poolSize = 4;
        while (queue.length > 0 && visited.size < maxPages) {
            const item = queue.shift();
            if (visited.has(item.url) || item.depth > maxDepth)
                continue;
            visited.add(item.url);
            // Checkout fetcher from pool (round-robin ElasticPool pattern)
            const fetcherId = `${appId}/fetcher-${fetcherIdx % poolSize}@`;
            fetcherIdx += 1;
            let result;
            try {
                result = host.ask(fetcherId, "fetch", { url: item.url }, 10000);
            }
            catch {
                result = {
                    status: "ok",
                    url: item.url,
                    links: simulateLinks(item.url),
                    word_counts: simulateWordCounts(item.url),
                };
            }
            const links = result.links ?? [];
            for (const link of links) {
                if (!visited.has(link)) {
                    queue.push({ url: link, depth: item.depth + 1 });
                    this.state.total_links += 1;
                }
            }
            // Mark done in TupleSpace
            host.ts.write(["url_queue", item.url, "done"]);
            allResults.push(result);
            this.state.pages_crawled += 1;
        }
        // Scatter to analyzer shards (ShardGroup reduce pattern)
        const numShards = 2;
        const chunkSize = Math.max(1, Math.ceil(allResults.length / numShards));
        const globalCounts = {};
        for (let shardIdx = 0; shardIdx < numShards; shardIdx++) {
            const chunk = allResults.slice(shardIdx * chunkSize, (shardIdx + 1) * chunkSize);
            if (chunk.length === 0)
                break;
            const analyzerId = `${appId}/analyzer-${shardIdx}@`;
            try {
                host.ask(analyzerId, "analyze", { results: chunk }, 10000);
                const top = host.ask(analyzerId, "top_words", { n: 20 }, 10000);
                for (const [word, count] of top.top_words ?? []) {
                    globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
                }
            }
            catch {
                // Local fallback if remote analyzer is unavailable
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
        return {
            status: "ok",
            pages_crawled: this.state.pages_crawled,
            total_links: this.state.total_links,
            top_words: this.state.top_words,
        };
    }
    onStatus() {
        return { ...this.state };
    }
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
