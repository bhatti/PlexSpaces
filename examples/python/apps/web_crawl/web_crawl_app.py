#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Web Crawl — Python WASM app.

Parallel web crawler using:
  ElasticPool pattern  — 4 PageFetcher actors, round-robin across URLs
  TupleSpace           — url_queue space: pending → done URL tracking
  ShardGroup pattern   — 2 analyzer shards scatter/reduce word counts

Inspired by Ray's web-crawl and map-reduce examples:
  https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html
  https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html

Role is configured via args.role in app-config.toml:
  orchestrator  — drives the BFS crawl
  fetcher       — fetches one URL (simulated), returns links + word counts
  analyzer      — shard: merges counts, returns top-N words
"""

from __future__ import annotations

from plexspaces import gen_server_actor, handler, host, init_handler, state


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------

def _app_id_from_actor_id(actor_id: str) -> str:
    if "//" in actor_id and "::" in actor_id:
        suffix = actor_id.split("//", 1)[1]
        qualified = suffix.split("@", 1)[0]
        parts = qualified.split("::", 1)
        if len(parts) == 2:
            return parts[1]
    return ""


def _simulate_links(url: str) -> list[str]:
    base = url.rstrip("/")
    return [f"{base}/about", f"{base}/docs", f"{base}/api"]


def _simulate_word_counts(url: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for seg in url.split("/"):
        if not seg or seg in ("https:", "http:"):
            continue
        for word in "".join(c if c.isalnum() else " " for c in seg).split():
            if len(word) > 2:
                counts[word.lower()] = counts.get(word.lower(), 0) + 1
    return counts


# ---------------------------------------------------------------------------
# PageFetcher actor — one worker in the ElasticPool
# ---------------------------------------------------------------------------

@gen_server_actor(facets=["virtual_actor"])
class PageFetcher:
    fetch_count: int = state(default=0)

    @handler("fetch")
    def fetch(self, url: str = "") -> dict:
        if not url:
            return {"error": "missing url"}
        links = _simulate_links(url)
        word_counts = _simulate_word_counts(url)
        self.fetch_count += 1
        return {
            "status": "ok",
            "url": url,
            "links": links,
            "word_counts": word_counts,
        }


# ---------------------------------------------------------------------------
# LinkAnalyzer actor — one shard in the ShardGroup
# ---------------------------------------------------------------------------

@gen_server_actor
class LinkAnalyzer:
    index: dict = state(default_factory=dict)
    urls_analyzed: int = state(default=0)

    @handler("analyze")
    def analyze(self, results: list = None) -> dict:
        for result in (results or []):
            for word, count in (result.get("word_counts") or {}).items():
                self.index[word] = self.index.get(word, 0) + int(count)
            self.urls_analyzed += 1
        return {"status": "ok", "urls_analyzed": self.urls_analyzed}

    @handler("top_words")
    def top_words(self, n: int = 10) -> dict:
        sorted_pairs = sorted(self.index.items(), key=lambda kv: kv[1], reverse=True)
        return {"top_words": sorted_pairs[:n]}


# ---------------------------------------------------------------------------
# WebCrawlOrchestrator — coordinates the entire crawl
# ---------------------------------------------------------------------------

@gen_server_actor
class WebCrawlOrchestrator:
    actor_id: str = state(default="")
    application_id: str = state(default="")
    role: str = state(default="orchestrator")
    pages_crawled: int = state(default=0)
    total_links: int = state(default=0)
    top_words: list = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = _app_id_from_actor_id(self.actor_id)
        args = config.get("args") or {}
        self.role = args.get("role", "orchestrator")

    @handler("crawl")
    def crawl(
        self,
        seed_urls: list = None,
        max_pages: int = 20,
        max_depth: int = 2,
    ) -> dict:
        if not seed_urls:
            seed_urls = ["https://example.com"]

        app_id = self.application_id
        visited: set[str] = set()
        queue: list[tuple[str, int]] = [(u, 0) for u in seed_urls]

        # Seed TupleSpace url_queue (pending URLs)
        for url in seed_urls:
            host.ts.write(["url_queue", url, "pending"])

        all_results: list[dict] = []
        fetcher_idx = 0
        pool_size = 4

        while queue and len(visited) < max_pages:
            url, depth = queue.pop(0)
            if url in visited or depth > max_depth:
                continue
            visited.add(url)

            # Checkout fetcher from pool (round-robin — ElasticPool pattern)
            fetcher_id = f"{app_id}/fetcher-{fetcher_idx % pool_size}@"
            fetcher_idx += 1

            try:
                result = host.ask(fetcher_id, "fetch", {"url": url}, timeout_ms=10_000)
            except Exception:
                # Fallback: simulate locally
                result = {
                    "status": "ok",
                    "url": url,
                    "links": _simulate_links(url),
                    "word_counts": _simulate_word_counts(url),
                }

            for link in result.get("links", []):
                if link not in visited:
                    queue.append((link, depth + 1))
                    self.total_links += 1

            # Mark done in TupleSpace
            host.ts.write(["url_queue", url, "done"])
            all_results.append(result)
            self.pages_crawled += 1

        # Scatter to analyzer shards (ShardGroup reduce pattern)
        num_shards = 2
        chunk_size = max(1, (len(all_results) + num_shards - 1) // num_shards)
        global_counts: dict[str, int] = {}

        for shard_idx in range(num_shards):
            chunk = all_results[shard_idx * chunk_size: (shard_idx + 1) * chunk_size]
            if not chunk:
                break
            analyzer_id = f"{app_id}/analyzer-{shard_idx}@"
            try:
                host.ask(analyzer_id, "analyze", {"results": chunk}, timeout_ms=10_000)
                top = host.ask(analyzer_id, "top_words", {"n": 20}, timeout_ms=10_000)
                for word, count in top.get("top_words", []):
                    global_counts[word] = global_counts.get(word, 0) + int(count)
            except Exception:
                # Local fallback
                for res in chunk:
                    for word, count in (res.get("word_counts") or {}).items():
                        global_counts[word] = global_counts.get(word, 0) + int(count)

        self.top_words = sorted(global_counts.items(), key=lambda kv: kv[1], reverse=True)[:10]

        return {
            "status": "ok",
            "pages_crawled": self.pages_crawled,
            "total_links": self.total_links,
            "top_words": self.top_words,
        }

    @handler("status")
    def status(self) -> dict:
        return {
            "actor_id": self.actor_id,
            "role": self.role,
            "pages_crawled": self.pages_crawled,
            "total_links": self.total_links,
            "top_words": self.top_words,
        }


# ---------------------------------------------------------------------------
# Actor role registry (required by Python SDK multi-role dispatch)
# ---------------------------------------------------------------------------

ACTOR_ROLES = {
    "orchestrator": WebCrawlOrchestrator,
    "fetcher": PageFetcher,
    "analyzer": LinkAnalyzer,
}
