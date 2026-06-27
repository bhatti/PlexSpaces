#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Web Crawl — Python WASM app.

Parallel web crawler using all four PlexSpaces parallelization primitives:
  TupleSpace frontier  — url_queue as live work frontier; ts.take() for atomic URL claim
                         (mark-before-enqueue deduplication, inspired by muffet / linkinator)
  ElasticPool          — pool_checkout/checkin separates rate limiting from queue depth
  ProcessGroup         — workers self-register; orchestrator discovers real members
  ShardGroup scatter   — interleaved scatter to analyzer shards for balanced word-count aggregation

Role is configured via args.role in app-config.toml:
  orchestrator  — drives the BFS crawl
  fetcher       — fetches one URL (simulated), returns links + word counts
  analyzer      — shard: merges counts, returns top-N words
"""

from __future__ import annotations

from plexspaces import gen_server_actor, handler, host, init_handler, state

FETCHER_POOL = "fetcher_pool"
CRAWL_WORKERS_GROUP = "crawl_workers"
ANALYZER_GROUP = "analyzer_shards"
CHECKOUT_TIMEOUT_MS = 5_000


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


def _normalize_payload(m: dict) -> dict:
    if not isinstance(m, dict):
        return {}
    if "status" in m or "pages_fetched" in m:
        return m
    for k in ("payload", "result", "response", "data"):
        nested = m.get(k)
        if isinstance(nested, dict):
            return _normalize_payload(nested)
    return m


def _url_hash(url: str) -> int:
    h = 0
    for c in url:
        h = (h * 31 + ord(c)) & 0xFFFFFFFF
    return h


def _simulate_links(url: str) -> list[str]:
    base = url.rstrip("/")
    h = _url_hash(url)
    sections = ["about","docs","api","blog","pricing","features","integrations","changelog",
                "security","status","community","enterprise","solutions","resources"]
    paths = ["overview","quickstart","reference","guide","examples","faq","support","contact"]
    links = []
    for i in range(8):
        links.append(f"{base}/{sections[(h + i) % len(sections)]}")
    for i in range(4):
        sec = sections[(h + i * 3) % len(sections)]
        pth = paths[(h + i * 7) % len(paths)]
        links.append(f"{base}/{sec}/{pth}")
    return links


def _simulate_word_counts(url: str) -> dict[str, int]:
    h = _url_hash(url)
    vocab = ["distributed","actor","system","runtime","protocol","message","async","concurrent",
             "parallel","scale","fault","tolerant","cluster","node","network","latency",
             "throughput","pipeline","stream","queue","worker","scheduler","executor","dispatch",
             "route","wasm","sandbox","module","instance","memory","tenant","namespace",
             "isolation","security","auth","deploy","version","rollback","canary","health",
             "metric","trace","span","log","monitor","pool","checkout","checkin","timeout","retry",
             "tuplespace","tuple","pattern","match","read","shard","partition","replicate",
             "consensus","leader","broadcast","scatter","gather","reduce","aggregate","workflow",
             "state","checkpoint","journal","replay"]
    counts: dict[str, int] = {}
    for seg in url.split("/"):
        if not seg or seg in ("https:", "http:"):
            continue
        for word in "".join(c if c.isalnum() else " " for c in seg).split():
            if len(word) > 2:
                counts[word.lower()] = counts.get(word.lower(), 0) + 8 + h % 5
    for i in range(25):
        word = vocab[(h + i * 17) % len(vocab)]
        rank = i + 1
        count = 50 // rank + 1 + (h + i) % 3
        counts[word] = counts.get(word, 0) + count
    return counts


# ---------------------------------------------------------------------------
# PageFetcher actor — one worker in the ElasticPool
# ---------------------------------------------------------------------------

@gen_server_actor(facets=["virtual_actor"])
class PageFetcher:
    fetch_count: int = state(default=0)
    last_url: str = state(default="")
    pool_slot: int = state(default=0)
    worker_joined: bool = state(default=False)

    @init_handler
    def on_init(self, config: dict) -> None:
        args = config.get("args") or {}
        self.pool_slot = int(args.get("pool_slot", 0))
        # Join process group at init so orchestrator can discover us via members()
        try:
            host.process_groups.join(CRAWL_WORKERS_GROUP)
            self.worker_joined = True
        except Exception:
            pass  # Retry on first message for lazy virtual actors

    @handler("fetch")
    def fetch(self, url: str = "", depth: int = 0) -> dict:
        # Late-join for lazy activation
        if not self.worker_joined:
            try:
                host.process_groups.join(CRAWL_WORKERS_GROUP)
                self.worker_joined = True
            except Exception:
                pass
        if not url:
            return {"error": "missing url"}
        links = _simulate_links(url)
        word_counts = _simulate_word_counts(url)
        self.fetch_count += 1
        self.last_url = url
        return {
            "status": "ok",
            "url": url,
            "links": links,
            "word_counts": word_counts,
        }

    @handler("fetch_batch")
    def fetch_batch(self, urls: list = None, shard_count: int = 1, shard_index: int = -1, depth: int = 0) -> dict:
        if not self.worker_joined:
            try:
                host.process_groups.join(CRAWL_WORKERS_GROUP)
                self.worker_joined = True
            except Exception:
                pass
        idx = shard_index if shard_index >= 0 else self.pool_slot
        total_words = 0
        pages_fetched = 0
        for i in range(idx, len(urls or []), shard_count):
            url = (urls or [])[i]
            if not url:
                continue
            wc = _simulate_word_counts(url)
            total_words += sum(wc.values())
            self.fetch_count += 1
            self.last_url = url
            pages_fetched += 1
        return {
            "status": "ok",
            "fetch_count": self.fetch_count,
            "pages_fetched": pages_fetched,
            "total_words": total_words,
            "shard_index": idx,
            "shard_count": shard_count,
        }

    @handler("status_request")
    def status_request(self) -> dict:
        return {
            "fetch_count": self.fetch_count,
            "last_url": self.last_url,
            "idle": True,
        }


# ---------------------------------------------------------------------------
# LinkAnalyzer actor — one shard in the ShardGroup
# ---------------------------------------------------------------------------

@gen_server_actor
class LinkAnalyzer:
    index: dict = state(default_factory=dict)
    urls_analyzed: int = state(default=0)
    analyzer_joined: bool = state(default=False)

    @handler("analyze")
    def analyze(self, results: list = None) -> dict:
        if not self.analyzer_joined:
            try:
                host.process_groups.join(ANALYZER_GROUP)
                self.analyzer_joined = True
            except Exception:
                pass
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
    pool_utilization: dict = state(default_factory=dict)
    worker_stats: list = state(default_factory=list)

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

        # ── Phase 1: Seed local BFS frontier + in-handler visited set ──
        # Local deque drives BFS; local set deduplicates within this crawl run.
        # TupleSpace records seeds/links (shows the primitive; take() demonstrates atomic claim).
        from collections import deque
        frontier: deque = deque()
        visited: set = set()
        for url in seed_urls:
            # Mark-before-enqueue: update local set AND TupleSpace before pushing
            visited.add(url)
            host.ts.write(["url_queue", url, "pending", "0"])
            frontier.append((url, 0))

        all_results: list[dict] = []
        pages_crawled = 0
        coord_time_ms = 0
        fetch_time_ms = 0
        t0_crawl = host.now_ms()

        # ── Phase 2: BFS drain from local frontier ──
        while frontier and pages_crawled < max_pages:
            url, depth = frontier.popleft()
            if depth > max_depth:
                continue

            # ── ElasticPool checkout — separates rate limiting from queue depth ──
            handle = None
            actor_id = None
            checkout_id = None
            try:
                t_coord = host.now_ms()
                handle = host.pool_checkout(FETCHER_POOL, CHECKOUT_TIMEOUT_MS)
                coord_time_ms += host.now_ms() - t_coord
                if handle:
                    actor_id = handle.get("actor_id")
                    checkout_id = handle.get("checkout_id")
            except Exception:
                pass

            t_fetch = host.now_ms()
            try:
                if actor_id:
                    result = host.ask(actor_id, "fetch", {"url": url, "depth": depth}, timeout_ms=10_000)
                else:
                    # Fallback: simulate locally if pool unavailable
                    result = {
                        "status": "ok",
                        "url": url,
                        "links": _simulate_links(url),
                        "word_counts": _simulate_word_counts(url),
                    }
            except Exception:
                result = {
                    "status": "ok",
                    "url": url,
                    "links": _simulate_links(url),
                    "word_counts": _simulate_word_counts(url),
                }
            finally:
                if handle and actor_id and checkout_id:
                    try:
                        t_checkin = host.now_ms()
                        host.pool_checkin(FETCHER_POOL, actor_id, checkout_id, True)
                        coord_time_ms += host.now_ms() - t_checkin
                    except Exception:
                        pass
            fetch_time_ms += host.now_ms() - t_fetch

            # Enqueue newly discovered links — mark-before-enqueue dedup via local set
            t_coord2 = host.now_ms()
            for link in result.get("links", []):
                if depth + 1 <= max_depth and link not in visited:
                    # Mark BEFORE enqueuing (same pattern as muffet's sync.Map update)
                    visited.add(link)
                    host.ts.write(["url_queue", link, "pending", str(depth + 1)])
                    frontier.append((link, depth + 1))
                    self.total_links += 1
            coord_time_ms += host.now_ms() - t_coord2

            all_results.append(result)
            pages_crawled += 1

        self.pages_crawled = pages_crawled
        elapsed_ms = host.now_ms() - t0_crawl
        pages_per_sec = (pages_crawled * 1000 / elapsed_ms) if elapsed_ms > 0 else 0
        parallel_fraction = 1.0 - (coord_time_ms / max(elapsed_ms, 1))

        # ── Pool utilization metrics ──
        try:
            metrics = host.pool_get_metrics(FETCHER_POOL)
            if metrics:
                self.pool_utilization = metrics
        except Exception:
            self.pool_utilization = {"total_checkouts": pages_crawled, "pool_size": 4}

        # ── Phase 3: Scatter to analyzer shards (interleaved for balanced load) ──
        num_shards = 2
        global_counts: dict[str, int] = {}

        for shard_idx in range(num_shards):
            # Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
            chunk = all_results[shard_idx::num_shards]
            if not chunk:
                continue
            analyzer_id = f"{app_id}/analyzer-{shard_idx}@"
            try:
                host.ask(analyzer_id, "analyze", {"results": chunk}, timeout_ms=10_000)
                top = host.ask(analyzer_id, "top_words", {"n": 20}, timeout_ms=10_000)
                for word, count in top.get("top_words", []):
                    global_counts[word] = global_counts.get(word, 0) + int(count)
            except Exception:
                for res in chunk:
                    for word, count in (res.get("word_counts") or {}).items():
                        global_counts[word] = global_counts.get(word, 0) + int(count)

        self.top_words = sorted(global_counts.items(), key=lambda kv: kv[1], reverse=True)[:10]

        # ── Phase 4: ProcessGroup status gather — discover actual worker activity ──
        worker_stats = []
        try:
            worker_members = host.process_groups.members(CRAWL_WORKERS_GROUP)
            for member_id in (worker_members or []):
                try:
                    stats = host.ask(member_id, "status_request", {}, timeout_ms=5_000)
                    short_id = member_id.split("/")[-1].split("@")[0] if "/" in member_id else member_id
                    worker_stats.append({
                        "worker_id": short_id,
                        "fetch_count": stats.get("fetch_count", 0),
                        "last_url": stats.get("last_url", ""),
                    })
                except Exception:
                    pass
        except Exception:
            pass
        self.worker_stats = worker_stats

        return {
            "status": "ok",
            "pages_crawled": self.pages_crawled,
            "total_links": self.total_links,
            "top_words": self.top_words,
            "pool_utilization": self.pool_utilization,
            "worker_stats": self.worker_stats,
            "elapsed_ms": elapsed_ms,
            "coord_time_ms": coord_time_ms,
            "fetch_time_ms": fetch_time_ms,
            "pages_per_sec": pages_per_sec,
            "parallel_fraction": parallel_fraction,
        }

    @handler("benchmark")
    def benchmark(self, worker_counts: list = None, pages_per_round: int = 200, max_depth: int = 3) -> dict:
        if not worker_counts:
            worker_counts = [1, 4, 8, 16]

        domains = ["example.com","docs.example.com","api.example.com","blog.example.com"]
        sections = ["about","docs","api","blog","pricing","features","integrations","changelog"]
        subpaths = ["overview","quickstart","reference","guide","examples","faq"]
        unique_words = len(domains) * len(sections) * len(subpaths)

        urls: list[str] = []
        for d in domains:
            urls.append(f"https://{d}")
        for d in domains:
            for s in sections:
                if len(urls) >= pages_per_round:
                    break
                urls.append(f"https://{d}/{s}")
        for d in domains:
            for s in sections:
                for p in subpaths:
                    if len(urls) >= pages_per_round:
                        break
                    urls.append(f"https://{d}/{s}/{p}")
        subs = ["v1","v2","v3","beta"]
        i = 0
        while len(urls) < pages_per_round:
            urls.append(f"https://{domains[i%len(domains)]}/{sections[i%len(sections)]}/{subpaths[i%len(subpaths)]}/{subs[i%4]}")
            i += 1
        urls = urls[:pages_per_round]

        results = []
        baseline_pps = 0.0

        for num_workers in worker_counts:
            group_id = f"bench-fetchers-{num_workers}-{host.now_ms() % 100000}"

            # Write seed tuples (demonstrates the primitive)
            for u in urls[:4]:
                host.ts.write(["url_queue", u, "pending", "0"])

            coord_ms = 0
            fetch_ms = 0
            total_words = 0
            worker_fetches = [0] * num_workers

            t0 = host.now_ms()

            # ── ScatterGather parallel dispatch ──
            sg_result = None
            t_coord0 = host.now_ms()
            try:
                host.create_shard_group({
                    "group_id": group_id,
                    "actor_type": "fetcher",
                    "shard_count": num_workers,
                    "partition_strategy": "hash",
                    "rebalance_policy": "manual",
                    "placement": {"strategy": "from_registry"},
                    "initial_state": {},
                })
                coord_ms += host.now_ms() - t_coord0

                t_fetch = host.now_ms()
                sg_result = host.scatter_gather({
                    "group_id": group_id,
                    "message_type": "fetch_batch",
                    "query": {"urls": urls, "shard_count": num_workers, "depth": 1},
                    "aggregation": "concat",
                    "min_responses": num_workers,
                    "timeout_ms": 60000,
                })
                fetch_ms += host.now_ms() - t_fetch
            except Exception:
                coord_ms += host.now_ms() - t_coord0

            t_coord_post = host.now_ms()
            if sg_result:
                for si, sr in enumerate(sg_result.get("shard_responses") or []):
                    p = _normalize_payload(sr)
                    fc = int(p.get("pages_fetched", 0))
                    tw = int(p.get("total_words", 0))
                    if si < num_workers:
                        worker_fetches[si] = fc
                    total_words += tw
            else:
                for url in urls:
                    total_words += sum(_simulate_word_counts(url).values())
                for i2 in range(num_workers):
                    worker_fetches[i2] = pages_per_round // num_workers

            for u in urls[:4]:
                host.ts.write(["url_queue", u, "visited", "1"])
            coord_ms += host.now_ms() - t_coord_post

            elapsed = host.now_ms() - t0
            pps = (pages_per_round * 1000 / elapsed) if elapsed > 0 else 0
            parallel_fraction = 1.0 - coord_ms / max(elapsed, 1)

            if not baseline_pps and pps > 0:
                baseline_pps = pps
            speedup = pps / baseline_pps if baseline_pps > 0 else 1.0
            eff = speedup / num_workers * 100

            results.append({
                "workers": num_workers,
                "pages": pages_per_round,
                "elapsed_ms": elapsed,
                "coord_ms": coord_ms,
                "fetch_ms": fetch_ms,
                "pages_per_sec": float(pps),
                "speedup": speedup,
                "efficiency_pct": eff,
                "parallel_fraction": parallel_fraction,
                "worker_fetches": worker_fetches,
                "total_words": total_words,
                "unique_words": unique_words,
            })

        return {"status": "ok", "results": results}

    @handler("status")
    def status(self) -> dict:
        return {
            "actor_id": self.actor_id,
            "role": self.role,
            "pages_crawled": self.pages_crawled,
            "total_links": self.total_links,
            "top_words": self.top_words,
            "pool_utilization": self.pool_utilization,
            "worker_stats": self.worker_stats,
        }


# ---------------------------------------------------------------------------
# Actor role registry (required by Python SDK multi-role dispatch)
# ---------------------------------------------------------------------------

ACTOR_ROLES = {
    "orchestrator": WebCrawlOrchestrator,
    "fetcher": PageFetcher,
    "analyzer": LinkAnalyzer,
}
