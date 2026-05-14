// SPDX-License-Identifier: AGPL-3.0-or-later
// Web Crawl (Go WASM)
//
// Parallel web crawler using:
//   - ElasticPool pattern: round-robin pool of PageFetcher actors
//   - TupleSpace: URL queue (pending → done) and visited-set deduplication
//   - ShardGroup pattern: scatter crawl results to analyzer shards, reduce word counts
//
// Inspired by Ray's web-crawl and map-reduce examples:
//   https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html
//   https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html
//
// Roles (set via args.role in app-config.toml):
//   orchestrator  — drives the BFS crawl loop
//   fetcher       — fetches one URL, returns links + word counts
//   analyzer      — shard: merges word counts, returns top-N words

package main

import (
	"encoding/json"
	"sort"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ---------------------------------------------------------------------------
// Actor state
// ---------------------------------------------------------------------------

type webCrawlActor struct {
	plexspaces.BaseActor
	actorID       string
	applicationID string
	role          string
	// fetcher state
	fetchCount int
	// analyzer state
	index        map[string]int
	urlsAnalyzed int
	// orchestrator state
	pagesCrawled int
	totalLinks   int
	topWords     [][2]any
}

func newWebCrawlActor() plexspaces.Actor {
	a := &webCrawlActor{
		role:  "fetcher",
		index: make(map[string]int),
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
			a.role = role
		}
	}
	a.index = make(map[string]int)
	return ""
}

func (a *webCrawlActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "fetch":
		return a.handleFetch(payload)
	case "analyze":
		return a.handleAnalyze(payload)
	case "top_words":
		return a.handleTopWords(payload)
	case "crawl":
		return a.handleCrawl(payload)
	case "status":
		return marshal(map[string]any{
			"actor_id":      a.actorID,
			"role":          a.role,
			"pages_crawled": a.pagesCrawled,
			"total_links":   a.totalLinks,
		})
	default:
		return marshal(map[string]any{"error": "unknown op: " + msgType})
	}
}

// ---------------------------------------------------------------------------
// Fetcher role
// ---------------------------------------------------------------------------

func (a *webCrawlActor) handleFetch(payload map[string]any) string {
	url := stringField(payload, "url")
	if url == "" {
		return marshal(map[string]any{"error": "missing url"})
	}
	links := simulateLinks(url)
	wordCounts := simulateWordCounts(url)
	a.fetchCount++
	return marshal(map[string]any{
		"status":      "ok",
		"url":         url,
		"links":       links,
		"word_counts": wordCounts,
	})
}

func simulateLinks(url string) []string {
	base := strings.TrimRight(url, "/")
	return []string{base + "/about", base + "/docs", base + "/api"}
}

func simulateWordCounts(url string) map[string]int {
	counts := make(map[string]int)
	for _, seg := range strings.Split(url, "/") {
		if seg == "" || seg == "https:" || seg == "http:" {
			continue
		}
		for _, word := range strings.FieldsFunc(seg, func(c rune) bool { return !isAlphaNum(c) }) {
			if len(word) > 2 {
				counts[strings.ToLower(word)]++
			}
		}
	}
	return counts
}

func isAlphaNum(c rune) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
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
						a.index[word] += int(v)
					case int:
						a.index[word] += v
					}
				}
			}
			a.urlsAnalyzed++
		}
	}
	return marshal(map[string]any{"status": "ok", "urls_analyzed": a.urlsAnalyzed})
}

func (a *webCrawlActor) handleTopWords(payload map[string]any) string {
	n := intField(payload, "n", 10)
	type kv struct {
		Word  string
		Count int
	}
	pairs := make([]kv, 0, len(a.index))
	for w, c := range a.index {
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

	// TupleSpace: seed URL queue (ElasticPool + TupleSpace patterns)
	for _, url := range seeds {
		host.TS().Write([]any{"url_queue", url, "pending"})
	}

	visited := make(map[string]bool)
	type urlDepth struct {
		URL   string
		Depth int
	}
	queue := make([]urlDepth, 0, len(seeds))
	for _, u := range seeds {
		queue = append(queue, urlDepth{u, 0})
	}

	var allResults []map[string]any
	fetcherIdx := 0
	poolSize := 4

	for len(queue) > 0 && len(visited) < maxPages {
		item := queue[0]
		queue = queue[1:]
		if visited[item.URL] || item.Depth > maxDepth {
			continue
		}
		visited[item.URL] = true

		// Checkout fetcher from pool (round-robin ElasticPool pattern)
		fetcherID := appID + "/fetcher-" + itoa(fetcherIdx%poolSize) + "@"
		fetcherIdx++

		var resultMap map[string]any
		resp, err := host.Ask(fetcherID, "fetch", map[string]any{"url": item.URL}, 10_000)
		if err == nil {
			if m, ok := resp.(map[string]any); ok {
				resultMap = m
			}
		}
		if resultMap == nil {
			// Fallback: compute locally if remote ask fails
			resultMap = map[string]any{
				"status":      "ok",
				"url":         item.URL,
				"links":       simulateLinks(item.URL),
				"word_counts": simulateWordCounts(item.URL),
			}
		}

		if links, ok := resultMap["links"].([]any); ok {
			for _, l := range links {
				if link, ok := l.(string); ok && !visited[link] {
					queue = append(queue, urlDepth{link, item.Depth + 1})
					a.totalLinks++
				}
			}
		}
		allResults = append(allResults, resultMap)
		a.pagesCrawled++

		// Mark done in TupleSpace
		host.TS().Write([]any{"url_queue", item.URL, "done"})
	}

	// Scatter to analyzer shards (ShardGroup reduce pattern)
	numShards := 2
	chunkSize := (len(allResults) + numShards - 1) / numShards
	if chunkSize == 0 {
		chunkSize = 1
	}
	globalCounts := make(map[string]int)

	for shardIdx := 0; shardIdx < numShards; shardIdx++ {
		start := shardIdx * chunkSize
		if start >= len(allResults) {
			break
		}
		end := start + chunkSize
		if end > len(allResults) {
			end = len(allResults)
		}
		chunk := allResults[start:end]

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
			// Local fallback if remote analyzer is unavailable
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
	a.topWords = topWords

	return marshal(map[string]any{
		"status":        "ok",
		"pages_crawled": a.pagesCrawled,
		"total_links":   a.totalLinks,
		"top_words":     topWords,
	})
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
