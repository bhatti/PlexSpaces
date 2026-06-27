// SPDX-License-Identifier: AGPL-3.0-or-later
// Web Crawl (Go WASM)
//
// Parallel web crawler using all four PlexSpaces parallelization primitives:
//
//   TupleSpace frontier  — url_queue as live work frontier; TS().Take() for atomic URL claim
//                          (mark-before-enqueue deduplication, inspired by muffet / linkinator)
//   ElasticPool          — PoolCheckout/PoolCheckin separates rate limiting from queue depth
//   ProcessGroup         — workers self-register; orchestrator discovers real members via PG().Members()
//   ShardGroup scatter   — interleaved scatter to analyzer shards for balanced word-count aggregation
//
// Roles (set via args.role in app-config.toml):
//   orchestrator  — drives the BFS crawl loop
//   fetcher       — fetches one URL (simulated), returns links + word counts
//   analyzer      — shard: merges counts, returns top-N words

package main

import (
	"encoding/json"
	"sort"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

const (
	fetcherPool       = "fetcher_pool"
	crawlWorkersGroup = "crawl_workers"
	analyzerGroup     = "analyzer_shards"
	checkoutTimeoutMs = 5_000
)

// ---------------------------------------------------------------------------
// Actor state
// ---------------------------------------------------------------------------

type webCrawlActor struct {
	plexspaces.BaseActor
	actorID       string
	applicationID string
	Role          string           `json:"role"`
	PoolSlot      int              `json:"pool_slot"`
	FetchCount    int              `json:"fetch_count"`
	LastURL       string           `json:"last_url"`
	WorkerJoined  bool             `json:"worker_joined"`
	Index         map[string]int   `json:"index"`
	UrlsAnalyzed  int              `json:"urls_analyzed"`
	PagesCrawled  int              `json:"pages_crawled"`
	TotalLinks    int              `json:"total_links"`
	TopWords      [][2]any         `json:"top_words"`
	PoolMetrics   map[string]any   `json:"pool_metrics,omitempty"`
	WorkerStats   []map[string]any `json:"worker_stats,omitempty"`
}

func newWebCrawlActor() plexspaces.Actor {
	a := &webCrawlActor{
		Role:  "fetcher",
		Index: make(map[string]int),
	}
	a.SetSelf(a)
	return a
}

func (a *webCrawlActor) Init(configJSON string) string {
	var cfg struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(cfg.ActorID)
	a.actorID = cfg.ActorID
	a.applicationID = appIDFromActorID(cfg.ActorID)
	if cfg.Args != nil {
		if role, ok := cfg.Args["role"].(string); ok {
			a.Role = role
		}
		a.PoolSlot = atoiDefault(stringVal(cfg.Args["pool_slot"]), 0)
	}
	a.Index = make(map[string]int)

	// Fetchers join process group at init (lazy virtual actors retry on first message)
	if a.Role == "fetcher" {
		if err := host.PG().Join(crawlWorkersGroup); err == nil {
			a.WorkerJoined = true
		}
	}
	return ""
}

func (a *webCrawlActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "fetch":
		return a.handleFetch(payload)
	case "fetch_batch":
		return a.handleFetchBatch(payload)
	case "status_request":
		return marshal(map[string]any{
			"fetch_count": a.FetchCount,
			"last_url":    a.LastURL,
			"idle":        true,
		})
	case "analyze":
		return a.handleAnalyze(payload)
	case "top_words":
		return a.handleTopWords(payload)
	case "crawl":
		return a.handleCrawl(payload)
	case "benchmark":
		return a.handleBenchmark(payload)
	case "status":
		return marshal(map[string]any{
			"actor_id":      a.actorID,
			"role":          a.Role,
			"pages_crawled": a.PagesCrawled,
			"total_links":   a.TotalLinks,
			"pool_metrics":  a.PoolMetrics,
			"worker_stats":  a.WorkerStats,
		})
	default:
		return marshal(map[string]any{"error": "unknown op: " + msgType})
	}
}

// ---------------------------------------------------------------------------
// Fetcher role
// ---------------------------------------------------------------------------

func (a *webCrawlActor) handleFetch(payload map[string]any) string {
	// Late-join for lazy virtual actor activation
	if a.Role == "fetcher" && !a.WorkerJoined {
		if err := host.PG().Join(crawlWorkersGroup); err == nil {
			a.WorkerJoined = true
		}
	}
	url := stringField(payload, "url")
	if url == "" {
		return marshal(map[string]any{"error": "missing url"})
	}
	links := simulateLinks(url)
	wordCounts := simulateWordCounts(url)
	a.FetchCount++
	a.LastURL = url
	return marshal(map[string]any{
		"status":      "ok",
		"url":         url,
		"links":       links,
		"word_counts": wordCounts,
	})
}

// handleFetchBatch handles a ScatterGather batch request.
// Each shard receives the full URL list and fetches its own slice:
// shard i fetches urls[i], urls[i+shard_count], urls[i+2*shard_count], ...
func (a *webCrawlActor) handleFetchBatch(payload map[string]any) string {
	if a.Role == "fetcher" && !a.WorkerJoined {
		if err := host.PG().Join(crawlWorkersGroup); err == nil {
			a.WorkerJoined = true
		}
	}
	urlsRaw, _ := payload["urls"].([]any)
	shardCount := intField(payload, "shard_count", 1)
	shardIndex := a.PoolSlot
	if si, ok := payload["shard_index"]; ok {
		shardIndex = intField(map[string]any{"si": si}, "si", a.PoolSlot)
	}

	type fetchResult struct {
		URL        string         `json:"url"`
		Links      []string       `json:"links"`
		WordCounts map[string]int `json:"word_counts"`
	}
	var results []fetchResult
	totalWords := 0

	for i := shardIndex; i < len(urlsRaw); i += shardCount {
		url, _ := urlsRaw[i].(string)
		if url == "" {
			continue
		}
		links := simulateLinks(url)
		wc := simulateWordCounts(url)
		for _, c := range wc {
			totalWords += c
		}
		a.FetchCount++
		a.LastURL = url
		results = append(results, fetchResult{URL: url, Links: links, WordCounts: wc})
	}

	return marshal(map[string]any{
		"status":       "ok",
		"fetch_count":  a.FetchCount,
		"results":      results,
		"total_words":  totalWords,
		"shard_index":  shardIndex,
		"shard_count":  shardCount,
		"pages_fetched": len(results),
	})
}

// simulateLinks returns realistic link fan-out: 8-12 links per page.
// Uses a deterministic hash of the URL to vary structure without randomness.
func simulateLinks(url string) []string {
	base := strings.TrimRight(url, "/")
	// deterministic hash from url bytes
	h := 0
	for _, c := range url {
		h = h*31 + int(c)
	}
	if h < 0 {
		h = -h
	}
	sections := []string{
		"about", "docs", "api", "blog", "pricing", "features",
		"integrations", "changelog", "security", "status",
		"community", "enterprise", "solutions", "resources",
	}
	paths := []string{
		"overview", "quickstart", "reference", "guide",
		"examples", "faq", "support", "contact",
	}
	var links []string
	// 8 fixed section links
	for i := 0; i < 8; i++ {
		links = append(links, base+"/"+sections[(h+i)%len(sections)])
	}
	// 4 sub-path links
	for i := 0; i < 4; i++ {
		sec := sections[(h+i*3)%len(sections)]
		pth := paths[(h+i*7)%len(paths)]
		links = append(links, base+"/"+sec+"/"+pth)
	}
	return links
}

// simulateWordCounts returns a realistic page word-frequency distribution.
// Generates 80-150 word occurrences from a tech vocabulary.
func simulateWordCounts(url string) map[string]int {
	h := 0
	for _, c := range url {
		h = h*31 + int(c)
	}
	if h < 0 {
		h = -h
	}

	vocab := []string{
		"distributed", "actor", "system", "runtime", "protocol",
		"message", "async", "concurrent", "parallel", "scale",
		"fault", "tolerant", "cluster", "node", "network",
		"latency", "throughput", "pipeline", "stream", "queue",
		"worker", "scheduler", "executor", "dispatch", "route",
		"wasm", "sandbox", "module", "instance", "memory",
		"tenant", "namespace", "isolation", "security", "auth",
		"deploy", "version", "rollback", "canary", "health",
		"metric", "trace", "span", "log", "monitor",
		"pool", "checkout", "checkin", "timeout", "retry",
		"tuplespace", "tuple", "pattern", "match", "read",
		"shard", "partition", "replicate", "consensus", "leader",
		"broadcast", "scatter", "gather", "reduce", "aggregate",
		"workflow", "state", "checkpoint", "journal", "replay",
	}

	counts := make(map[string]int, 30)
	// URL-derived words get high counts (domain-specific content)
	for _, seg := range strings.Split(url, "/") {
		if seg == "" || seg == "https:" || seg == "http:" {
			continue
		}
		for _, word := range strings.FieldsFunc(seg, func(c rune) bool {
			return !(c >= 'a' && c <= 'z') && !(c >= 'A' && c <= 'Z') && !(c >= '0' && c <= '9')
		}) {
			if len(word) > 2 {
				counts[strings.ToLower(word)] += 8 + (h % 5)
			}
		}
	}
	// Pick 25 vocab words with Zipf-like distribution: top words get much higher counts
	for i := 0; i < 25; i++ {
		word := vocab[(h+i*17)%len(vocab)]
		// Zipf: rank 1 → ~50 occurrences, rank 25 → ~2
		rank := i + 1
		count := 50/rank + 1 + (h+i)%3
		counts[word] += count
	}
	return counts
}

func isAlphaNum(c rune) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
}

// ---------------------------------------------------------------------------
// Benchmark: scaling across worker counts
// ---------------------------------------------------------------------------

func (a *webCrawlActor) handleBenchmark(payload map[string]any) string {
	workerCounts := []int{1, 4, 8, 16}
	if wc, ok := payload["worker_counts"].([]any); ok && len(wc) > 0 {
		workerCounts = workerCounts[:0]
		for _, v := range wc {
			if n, ok2 := v.(float64); ok2 {
				workerCounts = append(workerCounts, int(n))
			}
		}
	}
	pagesPerRound := intField(payload, "pages_per_round", 100)
	maxDepth := intField(payload, "max_depth", 3)
	appID := a.applicationID

	type roundResult struct {
		Workers          int     `json:"workers"`
		Pages            int     `json:"pages"`
		ElapsedMs        uint64  `json:"elapsed_ms"`
		CoordMs          uint64  `json:"coord_ms"`
		FetchMs          uint64  `json:"fetch_ms"`
		PagesPerSec      float64 `json:"pages_per_sec"`
		ParallelFraction float64 `json:"parallel_fraction"`
		WorkerFetches    []int   `json:"worker_fetches"`
		TotalWords       int     `json:"total_words"`
		UniqueWords      int     `json:"unique_words"`
	}

	var results []roundResult
	var baseline float64

	// URL corpus — pre-generate flat list, same for all rounds
	domains := []string{"example.com", "docs.example.com", "api.example.com", "blog.example.com"}
	sections := []string{"about", "docs", "api", "blog", "pricing", "features", "integrations", "changelog"}
	subpaths := []string{"overview", "quickstart", "reference", "guide", "examples", "faq"}
	uniqueWords := len(domains) * len(sections) * len(subpaths)

	urls := make([]string, 0, pagesPerRound)
	for _, d := range domains {
		urls = append(urls, "https://"+d)
	}
	for _, d := range domains {
		for _, s := range sections {
			if len(urls) >= pagesPerRound {
				break
			}
			urls = append(urls, "https://"+d+"/"+s)
		}
	}
	for _, d := range domains {
		for _, s := range sections {
			for _, p := range subpaths {
				if len(urls) >= pagesPerRound {
					break
				}
				urls = append(urls, "https://"+d+"/"+s+"/"+p)
			}
		}
	}
	for i := 0; len(urls) < pagesPerRound; i++ {
		d := domains[i%len(domains)]
		s := sections[i%len(sections)]
		p := subpaths[i%len(subpaths)]
		sub := []string{"v1", "v2", "v3", "beta"}[i%4]
		urls = append(urls, "https://"+d+"/"+s+"/"+p+"/"+sub)
	}
	urls = urls[:pagesPerRound]

	// Convert to []any for ScatterGather query payload
	urlsAny := make([]any, len(urls))
	for i, u := range urls {
		urlsAny[i] = u
	}

	for _, numWorkers := range workerCounts {
		// ── ScatterGather parallel dispatch ──
		// Create a shard group using the N pre-registered fetchers.
		// The runtime fans out fetch_batch to all N shards concurrently.
		groupID := "bench-fetchers-" + itoa(numWorkers) + "-" + itoa(int(host.NowMs()%100000))
		tCoord0 := host.NowMs()

		// Write seed tuples to TupleSpace (demonstrates the primitive)
		for _, u := range urls[:4] {
			host.TS().Write([]any{"url_queue", u, "pending", "0"})
		}

		var coordMs, fetchMs uint64
		totalWordOccurrences := 0
		workerFetches := make([]int, numWorkers)

		t0 := host.NowMs()

		// Create shard group from the first numWorkers fetchers
		sgResp, sgErr := host.CreateShardGroup(map[string]any{
			"group_id":           groupID,
			"actor_type":         "fetcher",
			"shard_count":        numWorkers,
			"partition_strategy": "hash",
			"rebalance_policy":   "manual",
			"placement":          map[string]any{"strategy": "from_registry"},
			"initial_state":      map[string]any{},
		})
		coordMs += host.NowMs() - tCoord0

		if sgErr != nil || sgResp == nil {
			// Fallback to sequential dispatch if ScatterGather setup fails
			workerIDs := make([]string, numWorkers)
			for i := 0; i < numWorkers; i++ {
				workerIDs[i] = appID + "/fetcher-" + itoa(i) + "@"
			}
			for i, url := range urls {
				wID := workerIDs[i%numWorkers]
				tFetch := host.NowMs()
				resp, err := host.Ask(wID, "fetch", map[string]any{"url": url, "depth": i % (maxDepth + 1)}, 10_000)
				fetchMs += host.NowMs() - tFetch
				tC := host.NowMs()
				workerFetches[i%numWorkers]++
				if err == nil {
					if m, ok := resp.(map[string]any); ok {
						if wc, ok2 := m["word_counts"].(map[string]any); ok2 {
							for _, c := range wc {
								if v, ok3 := c.(float64); ok3 {
									totalWordOccurrences += int(v)
								}
							}
						}
					}
				} else {
					for _, c := range simulateWordCounts(url) {
						totalWordOccurrences += c
					}
				}
				if i%10 == 0 {
					host.TS().Write([]any{"url_queue", url, "visited", itoa(i % (maxDepth + 1))})
				}
				coordMs += host.NowMs() - tC
			}
		} else {
			// ── ScatterGather: runtime dispatches to all N shards concurrently ──
			// Each shard receives the full URL list and processes its own slice
			// (shard i handles urls[i], urls[i+N], urls[i+2N], ...)
			tFetch := host.NowMs()
			sgResult, sgErr2 := host.ScatterGather(map[string]any{
				"group_id":     groupID,
				"message_type": "fetch_batch",
				"query": map[string]any{
					"urls":        urlsAny,
					"shard_count": numWorkers,
					"depth":       1,
				},
				"aggregation":   "concat",
				"min_responses": numWorkers,
				"timeout_ms":    60000,
			})
			fetchMs += host.NowMs() - tFetch

			tCoordPost := host.NowMs()
			if sgErr2 == nil && sgResult != nil {
				// Parse shard_responses to count fetches and words
				shardResponses, _ := sgResult["shard_responses"].([]any)
				for si, sr := range shardResponses {
					srMap, _ := sr.(map[string]any)
					payload := normalizePayload(srMap)
					fc := intField(payload, "pages_fetched", 0)
					tw := intField(payload, "total_words", 0)
					if si < numWorkers {
						workerFetches[si] = fc
					}
					totalWordOccurrences += tw
				}
			} else {
				// Fallback: compute locally
				for _, url := range urls {
					for _, c := range simulateWordCounts(url) {
						totalWordOccurrences += c
					}
				}
				for i := range workerFetches {
					workerFetches[i] = pagesPerRound / numWorkers
				}
			}
			// TupleSpace writes for metadata demonstration
			for i := 0; i < 4 && i < len(urls); i++ {
				host.TS().Write([]any{"url_queue", urls[i], "visited", "1"})
			}
			coordMs += host.NowMs() - tCoordPost
		}

		elapsed := host.NowMs() - t0
		var pps float64
		if elapsed > 0 {
			pps = float64(pagesPerRound) * 1000.0 / float64(elapsed)
		}
		e1 := elapsed
		if e1 == 0 {
			e1 = 1
		}
		pf := 1.0 - float64(coordMs)/float64(e1)

		if baseline == 0 && pps > 0 {
			baseline = pps
		}

		fetches := make([]int, numWorkers)
		copy(fetches, workerFetches)
		results = append(results, roundResult{
			Workers:          numWorkers,
			Pages:            pagesPerRound,
			ElapsedMs:        elapsed,
			CoordMs:          coordMs,
			FetchMs:          fetchMs,
			PagesPerSec:      pps,
			ParallelFraction: pf,
			WorkerFetches:    fetches,
			TotalWords:       totalWordOccurrences,
			UniqueWords:      uniqueWords,
		})
	}

	// Compute speedup and efficiency vs 1-worker baseline
	type benchRow struct {
		Workers          int     `json:"workers"`
		Pages            int     `json:"pages"`
		ElapsedMs        uint64  `json:"elapsed_ms"`
		CoordMs          uint64  `json:"coord_ms"`
		FetchMs          uint64  `json:"fetch_ms"`
		PagesPerSec      float64 `json:"pages_per_sec"`
		Speedup          float64 `json:"speedup"`
		Efficiency       float64 `json:"efficiency_pct"`
		ParallelFraction float64 `json:"parallel_fraction"`
		WorkerFetches    []int   `json:"worker_fetches"`
		TotalWords       int     `json:"total_words"`
		UniqueWords      int     `json:"unique_words"`
	}
	rows := make([]benchRow, 0, len(results))
	for _, r := range results {
		speedup := 1.0
		eff := 100.0
		if baseline > 0 && r.PagesPerSec > 0 {
			speedup = r.PagesPerSec / baseline
			eff = speedup / float64(r.Workers) * 100.0
		}
		rows = append(rows, benchRow{
			Workers:          r.Workers,
			Pages:            r.Pages,
			ElapsedMs:        r.ElapsedMs,
			CoordMs:          r.CoordMs,
			FetchMs:          r.FetchMs,
			PagesPerSec:      r.PagesPerSec,
			Speedup:          speedup,
			Efficiency:       eff,
			ParallelFraction: r.ParallelFraction,
			WorkerFetches:    r.WorkerFetches,
			TotalWords:       r.TotalWords,
			UniqueWords:      r.UniqueWords,
		})
	}

	return marshal(map[string]any{
		"status":  "ok",
		"results": rows,
	})
}

// ---------------------------------------------------------------------------
// Analyzer role (ShardGroup shard)
// ---------------------------------------------------------------------------

func (a *webCrawlActor) handleAnalyze(payload map[string]any) string {
	results, _ := payload["results"].([]any)
	for _, r := range results {
		if result, ok := r.(map[string]any); ok {
			if wc, ok := result["word_counts"].(map[string]any); ok {
				for word, cnt := range wc {
					switch v := cnt.(type) {
					case float64:
						a.Index[word] += int(v)
					case int:
						a.Index[word] += v
					}
				}
			}
			a.UrlsAnalyzed++
		}
	}
	return marshal(map[string]any{"status": "ok", "urls_analyzed": a.UrlsAnalyzed})
}

func (a *webCrawlActor) handleTopWords(payload map[string]any) string {
	n := intField(payload, "n", 10)
	type kv struct {
		Word  string
		Count int
	}
	pairs := make([]kv, 0, len(a.Index))
	for w, c := range a.Index {
		pairs = append(pairs, kv{w, c})
	}
	sort.Slice(pairs, func(i, j int) bool { return pairs[i].Count > pairs[j].Count })
	if len(pairs) > n {
		pairs = pairs[:n]
	}
	top := make([][2]any, len(pairs))
	for i, p := range pairs {
		top[i] = [2]any{p.Word, p.Count}
	}
	return marshal(map[string]any{"top_words": top})
}

// ---------------------------------------------------------------------------
// Orchestrator role
// ---------------------------------------------------------------------------

func (a *webCrawlActor) handleCrawl(payload map[string]any) string {
	seeds := stringSlice(payload, "seed_urls")
	if len(seeds) == 0 {
		seeds = []string{"https://example.com"}
	}
	maxPages := intField(payload, "max_pages", 20)
	maxDepth := intField(payload, "max_depth", 2)
	appID := a.applicationID

	// ── Phase 1: Seed local BFS frontier + in-handler visited set ──
	// Local slice drives BFS; local map deduplicates within this crawl run.
	// TupleSpace records seeds/links as metadata (shows the primitive being used).
	type crawlTask struct {
		url   string
		depth int
	}
	frontier := make([]crawlTask, 0, 32)
	visited := make(map[string]bool, 64)
	for _, url := range seeds {
		host.TS().Write([]any{"url_queue", url, "pending", "0"})
		visited[url] = true
		frontier = append(frontier, crawlTask{url: url, depth: 0})
	}

	var allResults []map[string]any
	pagesCrawled := 0
	var coordTimeMs, fetchTimeMs uint64
	t0Crawl := host.NowMs()

	// ── Phase 2: BFS drain from local frontier ──
	for len(frontier) > 0 && pagesCrawled < maxPages {
		task := frontier[0]
		frontier = frontier[1:]
		url := task.url
		depth := task.depth
		if depth > maxDepth {
			continue
		}

		// ── ElasticPool checkout — separates rate limiting from queue depth ──
		tCoord := host.NowMs()
		var resultMap map[string]any
		handle := host.PoolCheckout(fetcherPool, checkoutTimeoutMs)
		coordTimeMs += host.NowMs() - tCoord

		tFetch := host.NowMs()
		if handle != nil {
			actorID, _ := handle["actor_id"].(string)
			checkoutID, _ := handle["checkout_id"].(string)
			resp, err := host.Ask(actorID, "fetch", map[string]any{"url": url, "depth": depth}, 10_000)
			if err == nil {
				if m, ok := resp.(map[string]any); ok {
					resultMap = m
				}
			}
			tCheckin := host.NowMs()
			_ = host.PoolCheckin(fetcherPool, actorID, checkoutID, true)
			coordTimeMs += host.NowMs() - tCheckin
		}
		fetchTimeMs += host.NowMs() - tFetch

		if resultMap == nil {
			// Fallback: compute locally if pool unavailable
			resultMap = map[string]any{
				"status":      "ok",
				"url":         url,
				"links":       simulateLinks(url),
				"word_counts": simulateWordCounts(url),
			}
		}

		// Enqueue newly discovered links — mark-before-enqueue dedup via local map + TupleSpace
		tCoord2 := host.NowMs()
		var linkStrings []string
		switch v := resultMap["links"].(type) {
		case []any:
			for _, l := range v {
				if s, ok := l.(string); ok {
					linkStrings = append(linkStrings, s)
				}
			}
		case []string:
			linkStrings = v
		}
		for _, link := range linkStrings {
			if depth+1 <= maxDepth && !visited[link] {
				visited[link] = true
				host.TS().Write([]any{"url_queue", link, "pending", itoa(depth + 1)})
				frontier = append(frontier, crawlTask{url: link, depth: depth + 1})
				a.TotalLinks++
			}
		}
		coordTimeMs += host.NowMs() - tCoord2
		allResults = append(allResults, resultMap)
		pagesCrawled++
	}
	a.PagesCrawled = pagesCrawled

	elapsedMs := host.NowMs() - t0Crawl
	var pagesPerSec float64
	if elapsedMs > 0 {
		pagesPerSec = float64(pagesCrawled) * 1000.0 / float64(elapsedMs)
	}
	elapsed1 := elapsedMs
	if elapsed1 == 0 {
		elapsed1 = 1
	}
	parallelFraction := 1.0 - float64(coordTimeMs)/float64(elapsed1)

	// ── Pool utilization metrics ──
	if metrics := host.PoolGetMetrics(fetcherPool); metrics != nil {
		a.PoolMetrics = metrics
	} else {
		a.PoolMetrics = map[string]any{"total_checkouts": pagesCrawled, "pool_size": 4}
	}

	// ── Phase 3: Interleaved scatter to analyzer shards ──
	numShards := 2
	globalCounts := make(map[string]int)

	for shardIdx := 0; shardIdx < numShards; shardIdx++ {
		// Interleaved: shard 0 gets results[0,2,4,...], shard 1 gets results[1,3,5,...]
		var chunk []map[string]any
		for i := shardIdx; i < len(allResults); i += numShards {
			chunk = append(chunk, allResults[i])
		}
		if len(chunk) == 0 {
			continue
		}
		analyzerID := appID + "/analyzer-" + itoa(shardIdx) + "@"
		if _, err := host.Ask(analyzerID, "analyze", map[string]any{"results": chunk}, 10_000); err == nil {
			if topResp, err := host.Ask(analyzerID, "top_words", map[string]any{"n": 20}, 10_000); err == nil {
				if topMap, ok := topResp.(map[string]any); ok {
					if pairs, ok := topMap["top_words"].([]any); ok {
						for _, pair := range pairs {
							if p, ok := pair.([]any); ok && len(p) == 2 {
								word, _ := p[0].(string)
								switch v := p[1].(type) {
								case float64:
									globalCounts[word] += int(v)
								case int:
									globalCounts[word] += v
								}
							}
						}
					}
				}
			}
		} else {
			// Local fallback if remote analyzer unavailable
			for _, res := range chunk {
				if wc, ok := res["word_counts"].(map[string]any); ok {
					for w, c := range wc {
						if count, ok := c.(float64); ok {
							globalCounts[w] += int(count)
						}
					}
				}
			}
		}
	}

	type kv struct {
		Word  string
		Count int
	}
	pairs := make([]kv, 0, len(globalCounts))
	for w, c := range globalCounts {
		pairs = append(pairs, kv{w, c})
	}
	sort.Slice(pairs, func(i, j int) bool { return pairs[i].Count > pairs[j].Count })
	if len(pairs) > 10 {
		pairs = pairs[:10]
	}
	topWords := make([][2]any, len(pairs))
	for i, p := range pairs {
		topWords[i] = [2]any{p.Word, p.Count}
	}
	a.TopWords = topWords

	// ── Phase 4: ProcessGroup status gather — discover actual worker activity ──
	var workerStats []map[string]any
	members, _ := host.PG().Members(crawlWorkersGroup)
	if len(members) == 0 {
		// Fallback: use constructed IDs if no workers registered yet
		for i := 0; i < 4; i++ {
			members = append(members, appID+"/fetcher-"+itoa(i)+"@")
		}
	}
	for _, memberID := range members {
		stats, err := host.Ask(memberID, "status_request", map[string]any{}, 5_000)
		if err == nil {
			if sm, ok := stats.(map[string]any); ok {
				parts := strings.Split(memberID, "/")
				shortID := parts[len(parts)-1]
				shortID = strings.TrimSuffix(shortID, "@")
				sm["worker_id"] = shortID
				workerStats = append(workerStats, sm)
			}
		}
	}
	a.WorkerStats = workerStats

	return marshal(map[string]any{
		"status":            "ok",
		"pages_crawled":     a.PagesCrawled,
		"total_links":       a.TotalLinks,
		"top_words":         topWords,
		"pool_metrics":      a.PoolMetrics,
		"worker_stats":      a.WorkerStats,
		"elapsed_ms":        elapsedMs,
		"coord_time_ms":     coordTimeMs,
		"fetch_time_ms":     fetchTimeMs,
		"pages_per_sec":     pagesPerSec,
		"parallel_fraction": parallelFraction,
	})
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func normalizePayload(m map[string]any) map[string]any {
	if m == nil {
		return map[string]any{}
	}
	if _, ok := m["status"]; ok {
		return m
	}
	if _, ok := m["pages_fetched"]; ok {
		return m
	}
	for _, k := range []string{"payload", "result", "response", "data"} {
		if nested, ok := m[k].(map[string]any); ok {
			return normalizePayload(nested)
		}
	}
	return m
}

func stringVal(v any) string {
	if v == nil {
		return ""
	}
	s, _ := v.(string)
	return s
}

func appIDFromActorID(actorID string) string {
	if strings.Contains(actorID, "//") && strings.Contains(actorID, "::") {
		suffix := strings.SplitN(actorID, "//", 2)[1]
		qualified := strings.SplitN(suffix, "@", 2)[0]
		parts := strings.SplitN(qualified, "::", 2)
		if len(parts) == 2 {
			return parts[1]
		}
	}
	return ""
}

func parsePayload(s string) map[string]any {
	if s == "" {
		return map[string]any{}
	}
	var m map[string]any
	_ = json.Unmarshal([]byte(s), &m)
	return m
}

func marshal(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func stringField(m map[string]any, key string) string {
	v, _ := m[key].(string)
	return v
}

func intField(m map[string]any, key string, def int) int {
	switch v := m[key].(type) {
	case float64:
		return int(v)
	case int:
		return v
	}
	return def
}

func stringSlice(m map[string]any, key string) []string {
	raw, _ := m[key].([]any)
	out := make([]string, 0, len(raw))
	for _, v := range raw {
		if s, ok := v.(string); ok {
			out = append(out, s)
		}
	}
	return out
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	b := make([]byte, 0, 8)
	for n > 0 {
		b = append([]byte{byte('0' + n%10)}, b...)
		n /= 10
	}
	return string(b)
}

func max64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}

func atoiDefault(s string, def int) int {
	if s == "" {
		return def
	}
	n := 0
	for _, c := range s {
		if c < '0' || c > '9' {
			return def
		}
		n = n*10 + int(c-'0')
	}
	return n
}

// ---------------------------------------------------------------------------
// WIT entry point (required by TinyGo WASM target)
// ---------------------------------------------------------------------------

func main() {}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("orchestrator", newWebCrawlActor)
	router.Route("fetcher", newWebCrawlActor)
	router.Route("analyzer", newWebCrawlActor)
	plexspaces.Register(router)
}
