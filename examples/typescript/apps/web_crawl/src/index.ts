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
// Domain types
// ---------------------------------------------------------------------------

type FetcherState = {
  actor_id: string;
  role: string;
  fetch_count: number;
};

type AnalyzerState = {
  actor_id: string;
  role: string;
  index: Record<string, number>;
  urls_analyzed: number;
};

type OrchestratorState = {
  actor_id: string;
  application_id: string;
  role: string;
  pages_crawled: number;
  total_links: number;
  top_words: [string, number][];
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

function simulateLinks(url: string): string[] {
  const base = url.replace(/\/+$/, "");
  return [`${base}/about`, `${base}/docs`, `${base}/api`];
}

function simulateWordCounts(url: string): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const seg of url.split("/")) {
    if (!seg || seg === "https:" || seg === "http:") continue;
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

class PageFetcher extends PlexSpacesActor<FetcherState> {
  getDefaultState(): FetcherState {
    return { actor_id: "", role: "fetcher", fetch_count: 0 };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "fetcher");
  }

  onFetch(payload: Record<string, unknown>): Record<string, unknown> {
    const url = String(payload.url ?? "");
    if (!url) return { error: "missing url" };
    const links = simulateLinks(url);
    const word_counts = simulateWordCounts(url);
    this.state.fetch_count += 1;
    return { status: "ok", url, links, word_counts };
  }

  onStatus(): Record<string, unknown> {
    return { ...this.state };
  }
}

// ---------------------------------------------------------------------------
// LinkAnalyzer actor
// ---------------------------------------------------------------------------

class LinkAnalyzer extends PlexSpacesActor<AnalyzerState> {
  getDefaultState(): AnalyzerState {
    return { actor_id: "", role: "analyzer", index: {}, urls_analyzed: 0 };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "analyzer");
    this.state.index = {};
  }

  onAnalyze(payload: Record<string, unknown>): Record<string, unknown> {
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
    const visited = new Set<string>();
    const queue: Array<{ url: string; depth: number }> = seedUrls.map((url) => ({ url, depth: 0 }));

    // Seed TupleSpace url_queue (pending URLs)
    for (const url of seedUrls) {
      host.ts.write(["url_queue", url, "pending"]);
    }

    const allResults: Record<string, unknown>[] = [];
    let fetcherIdx = 0;
    const poolSize = 4;

    while (queue.length > 0 && visited.size < maxPages) {
      const item = queue.shift()!;
      if (visited.has(item.url) || item.depth > maxDepth) continue;
      visited.add(item.url);

      // Checkout fetcher from pool (round-robin ElasticPool pattern)
      const fetcherId = `${appId}/fetcher-${fetcherIdx % poolSize}@`;
      fetcherIdx += 1;

      let result: Record<string, unknown>;
      try {
        result = host.ask(fetcherId, "fetch", { url: item.url }, 10_000) as Record<string, unknown>;
      } catch {
        result = {
          status: "ok",
          url: item.url,
          links: simulateLinks(item.url),
          word_counts: simulateWordCounts(item.url),
        };
      }

      const links = (result.links as string[] | undefined) ?? [];
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
    const globalCounts: Record<string, number> = {};

    for (let shardIdx = 0; shardIdx < numShards; shardIdx++) {
      const chunk = allResults.slice(shardIdx * chunkSize, (shardIdx + 1) * chunkSize);
      if (chunk.length === 0) break;
      const analyzerId = `${appId}/analyzer-${shardIdx}@`;
      try {
        host.ask(analyzerId, "analyze", { results: chunk }, 10_000);
        const top = host.ask(analyzerId, "top_words", { n: 20 }, 10_000) as { top_words: [string, number][] };
        for (const [word, count] of top.top_words ?? []) {
          globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
        }
      } catch {
        // Local fallback if remote analyzer is unavailable
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

    return {
      status: "ok",
      pages_crawled: this.state.pages_crawled,
      total_links: this.state.total_links,
      top_words: this.state.top_words,
    };
  }

  onStatus(): Record<string, unknown> {
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
