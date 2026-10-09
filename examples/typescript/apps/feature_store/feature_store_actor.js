// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Feature Store - TypeScript WASM
//
// Production-grade online feature store: feature ingestion → versioned KV storage
// → consistent-hash routing → low-latency online serving with LRU cache.
//
// Architecture: Leader/worker with shard-group placement, scatter/gather,
// compute vs coordination metrics tracking.
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
// ─── Feature Data Generation ─────────────────────────────────────────────────
const USER_FEATURES = [
    "user.age", "user.income", "user.signup_days",
    "user.lifetime_value", "user.login_frequency",
];
const PRODUCT_FEATURES = [
    "product.price", "product.category_id", "product.popularity_score",
    "product.stock_level", "product.review_avg",
];
const INTERACTION_FEATURES = [
    "interaction.click_count", "interaction.purchase_count",
    "interaction.avg_session_duration", "interaction.cart_abandonment_rate",
    "interaction.page_views_7d",
];
const ALL_FEATURES = [...USER_FEATURES, ...PRODUCT_FEATURES, ...INTERACTION_FEATURES];
function generateFeatures(entityCount, seed) {
    const records = [];
    let rng = seed;
    for (let e = 0; e < entityCount; e++) {
        const entityId = `entity-${e}`;
        for (const featureName of ALL_FEATURES) {
            rng = (rng * 1103515245 + 12345) & 0x7fffffff;
            let value;
            if (featureName.includes("age")) {
                value = 18 + (rng % 62);
            }
            else if (featureName.includes("income")) {
                value = 20000 + (rng % 180000);
            }
            else if (featureName.includes("price")) {
                value = Math.round((1 + (rng % 99900)) / 100);
            }
            else if (featureName.includes("category_id")) {
                value = rng % 50;
            }
            else if (featureName.includes("score") || featureName.includes("rate") || featureName.includes("avg")) {
                value = Math.round((rng % 10000) / 100);
            }
            else if (featureName.includes("count") || featureName.includes("views")) {
                value = rng % 5000;
            }
            else if (featureName.includes("duration")) {
                value = 10 + (rng % 3590);
            }
            else if (featureName.includes("days")) {
                value = rng % 3650;
            }
            else if (featureName.includes("level")) {
                value = rng % 10000;
            }
            else {
                value = rng % 10000;
            }
            records.push({
                entity_id: entityId,
                feature_name: featureName,
                value,
                version: 1,
                timestamp: 1700000000000 + (rng % 86400000),
            });
        }
    }
    return records;
}
function hashEntityToShard(entityId, shardCount) {
    let h = 0;
    for (let i = 0; i < entityId.length; i++) {
        h = (h * 31 + entityId.charCodeAt(i)) & 0x7fffffff;
    }
    return h % shardCount;
}
// ─── Leader Actor ────────────────────────────────────────────────────────────
class LeaderActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "", application_id: "", role: "leader",
            total_compute_ms: 0, total_coord_ms: 0,
        };
    }
    onInit(config) {
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "leader";
    }
    onRun(payload) {
        const entityCount = intValue(payload.entity_count, 500);
        const workerCount = intValue(payload.worker_count, 8);
        const lookupCount = intValue(payload.lookup_count, 2000);
        const cacheSize = intValue(payload.cache_size, 200);
        // Create shard group
        const coordStart = host.nowMs();
        const groupId = `feature-store-ts-${host.nowMs()}`;
        const group = host.createShardGroup({
            groupId,
            actorType: "feature-worker",
            shardCount: workerCount,
            partitionStrategy: "hash",
            rebalancePolicy: "manual",
            placement: { strategy: "from_registry" },
            initialState: { cache_max_size: cacheSize },
        });
        const shardActorIds = group.shardActorIds;
        if (shardActorIds.length === 0) {
            return { status: "error", error: "failed to create worker shard group" };
        }
        const coordCreate = host.nowMs() - coordStart;
        // Generate and ingest features
        const features = generateFeatures(entityCount, host.nowMs() % 100000);
        const batchSize = 500;
        let totalComputeMs = 0;
        let totalCoordMs = coordCreate;
        let totalIngested = 0;
        let errorCount = 0;
        // Partition features by entity shard
        const shardBatches = Array.from({ length: workerCount }, () => []);
        for (const f of features) {
            const shard = hashEntityToShard(f.entity_id, workerCount);
            shardBatches[shard].push(f);
        }
        // Ingest via scatter-gather per batch
        for (let s = 0; s < workerCount; s++) {
            const batch = shardBatches[s];
            for (let i = 0; i < batch.length; i += batchSize) {
                const chunk = batch.slice(i, i + batchSize);
                const sgStart = host.nowMs();
                const sgResult = host.scatterGather({
                    groupId,
                    query: {
                        op: "ingest_batch",
                        features: chunk,
                    },
                    timeoutMs: 30000,
                });
                const sgElapsed = host.nowMs() - sgStart;
                totalCoordMs += sgElapsed;
                for (const resp of sgResult.shardResponses) {
                    const result = recordValue(resp.payload ?? resp);
                    if (result.error) {
                        errorCount++;
                        continue;
                    }
                    totalIngested += intValue(result.ingested_count, 0);
                    totalComputeMs += intValue(result.compute_ms, 0);
                }
            }
        }
        // Perform lookups (point + batch + vector assembly)
        let totalLookupMs = 0;
        let cacheHits = 0;
        let cacheMisses = 0;
        let lookupsServed = 0;
        const lookupLatencies = [];
        // Point lookups
        const pointLookupCount = Math.floor(lookupCount * 0.5);
        for (let i = 0; i < pointLookupCount; i += batchSize) {
            const batchEnd = Math.min(i + batchSize, pointLookupCount);
            const lookupEntities = [];
            for (let j = i; j < batchEnd; j++) {
                lookupEntities.push(`entity-${j % entityCount}`);
            }
            const sgStart = host.nowMs();
            const sgResult = host.scatterGather({
                groupId,
                query: {
                    op: "batch_lookup",
                    entity_ids: lookupEntities,
                    feature_names: ALL_FEATURES.slice(0, 5),
                },
                timeoutMs: 30000,
            });
            const sgElapsed = host.nowMs() - sgStart;
            totalCoordMs += sgElapsed;
            for (const resp of sgResult.shardResponses) {
                const result = recordValue(resp.payload ?? resp);
                cacheHits += intValue(result.cache_hits, 0);
                cacheMisses += intValue(result.cache_misses, 0);
                lookupsServed += intValue(result.lookups_served, 0);
                totalComputeMs += intValue(result.compute_ms, 0);
                const latency = intValue(result.compute_ms, 0);
                if (latency > 0)
                    lookupLatencies.push(latency);
            }
        }
        // Vector assembly lookups
        const vectorLookupCount = lookupCount - pointLookupCount;
        for (let i = 0; i < vectorLookupCount; i += batchSize) {
            const batchEnd = Math.min(i + batchSize, vectorLookupCount);
            const lookupEntities = [];
            for (let j = i; j < batchEnd; j++) {
                lookupEntities.push(`entity-${j % entityCount}`);
            }
            const sgStart = host.nowMs();
            const sgResult = host.scatterGather({
                groupId,
                query: {
                    op: "get_feature_vectors",
                    entity_ids: lookupEntities,
                },
                timeoutMs: 30000,
            });
            const sgElapsed = host.nowMs() - sgStart;
            totalCoordMs += sgElapsed;
            for (const resp of sgResult.shardResponses) {
                const result = recordValue(resp.payload ?? resp);
                cacheHits += intValue(result.cache_hits, 0);
                cacheMisses += intValue(result.cache_misses, 0);
                lookupsServed += intValue(result.lookups_served, 0);
                totalComputeMs += intValue(result.compute_ms, 0);
                const latency = intValue(result.compute_ms, 0);
                if (latency > 0)
                    lookupLatencies.push(latency);
            }
        }
        // Compute latency percentiles
        lookupLatencies.sort((a, b) => a - b);
        const p50 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.5)] : 0;
        const p95 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.95)] : 0;
        const p99 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.99)] : 0;
        const wallTime = totalComputeMs + totalCoordMs;
        const total = totalComputeMs + totalCoordMs || 1;
        const granularity = totalCoordMs > 0 ? Math.round((totalComputeMs / totalCoordMs) * 10) / 10 : 0;
        const lookupsPerSec = wallTime > 0 ? Math.round((lookupsServed / wallTime) * 1000) : 0;
        const cacheHitRate = (cacheHits + cacheMisses) > 0
            ? Math.round((cacheHits / (cacheHits + cacheMisses)) * 1000) / 10
            : 0;
        const leaderNodeId = actorNodeId(this.state.actor_id);
        const nodeActorCounts = computeActorCounts(leaderNodeId, shardActorIds);
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "leader.compute": totalComputeMs, "leader.coordination": totalCoordMs } });
        }
        catch (_e) { }
        return {
            status: "ok",
            entity_count: entityCount,
            feature_count: features.length,
            worker_count: workerCount,
            lookup_count: lookupCount,
            cache_size: cacheSize,
            wall_time_ms: wallTime,
            compute_time_ms: totalComputeMs,
            coordination_time_ms: totalCoordMs,
            granularity_ratio: granularity,
            total_ingested: totalIngested,
            lookups_served: lookupsServed,
            lookups_per_sec: lookupsPerSec,
            cache_hits: cacheHits,
            cache_misses: cacheMisses,
            cache_hit_rate_pct: cacheHitRate,
            latency_p50_ms: p50,
            latency_p95_ms: p95,
            latency_p99_ms: p99,
            node_count: Object.keys(nodeActorCounts).length,
            actor_count: shardActorIds.length + 1,
            leader_node_id: leaderNodeId,
            nodes: nodeActorCounts,
            error_count: errorCount,
        };
    }
    onRun_scaling_benchmark(payload) {
        const entityCount = intValue(payload.entity_count, 500);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const lookupCount = intValue(payload.lookup_count, 2000);
        const cacheSize = intValue(payload.cache_size, 200);
        const warmupRounds = intValue(payload.warmup_rounds, 1);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 2);
        const results = [];
        let baselineWall = 0;
        for (const shardCount of shardCounts) {
            for (let w = 0; w < warmupRounds; w++) {
                this.onRun({
                    entity_count: Math.min(entityCount, 100),
                    worker_count: shardCount,
                    lookup_count: Math.min(lookupCount, 200),
                    cache_size: cacheSize,
                });
            }
            let totalWall = 0;
            let totalCompute = 0;
            let totalCoord = 0;
            let totalLookupsPerSec = 0;
            let totalCacheHitRate = 0;
            let nodeCount = 0;
            let errorCount = 0;
            const runEntityCount = Math.min(entityCount, 100);
            const runLookupCount = Math.min(lookupCount, 200);
            for (let r = 0; r < benchmarkRounds; r++) {
                const result = this.onRun({
                    entity_count: runEntityCount,
                    worker_count: shardCount,
                    lookup_count: runLookupCount,
                    cache_size: cacheSize,
                });
                totalWall += intValue(result.wall_time_ms, 0);
                totalCompute += intValue(result.compute_time_ms, 0);
                totalCoord += intValue(result.coordination_time_ms, 0);
                totalLookupsPerSec += intValue(result.lookups_per_sec, 0);
                totalCacheHitRate += floatValue(result.cache_hit_rate_pct, 0);
                nodeCount = intValue(result.node_count, 0);
                errorCount += intValue(result.error_count, 0);
            }
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            const avgLookupsPerSec = Math.round(totalLookupsPerSec / benchmarkRounds);
            const avgCacheHitRate = Math.round((totalCacheHitRate / benchmarkRounds) * 10) / 10;
            const total = avgCompute + avgCoord || 1;
            if (baselineWall === 0)
                baselineWall = avgWall;
            const speedup = baselineWall > 0 && avgWall > 0 ? baselineWall / avgWall : 1;
            const efficiency = (speedup / (shardCount / shardCounts[0])) * 100;
            results.push({
                shards: shardCount,
                lookups_per_sec: avgLookupsPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                compute_pct: (avgCompute * 100) / total,
                granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
                speedup: Math.round(speedup * 100) / 100,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                cache_hit_rate_pct: avgCacheHitRate,
                node_count: nodeCount,
                error_count: errorCount,
            });
        }
        return {
            status: "ok",
            entity_count: entityCount,
            lookup_count: lookupCount,
            results,
        };
    }
    onRun_weak_scaling_benchmark(payload) {
        const entitiesPerShard = intValue(payload.entities_per_shard, 200);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const numPasses = intValue(payload.num_passes, 4);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 1);
        const results = [];
        let baselineThroughput = 0;
        for (const shardCount of shardCounts) {
            const groupId = `feature-bench-ts-${host.nowMs()}`;
            host.createShardGroup({
                groupId,
                actorType: "feature-worker",
                shardCount,
                partitionStrategy: "hash",
                rebalancePolicy: "manual",
                placement: { strategy: "from_registry" },
                initialState: { cache_max_size: entitiesPerShard },
            });
            let totalComputeMs = 0;
            let totalCoordMs = 0;
            let totalLookups = 0;
            let errorCount = 0;
            for (let r = 0; r < benchmarkRounds; r++) {
                const wallStart = host.nowMs();
                // Single broadcast SG — all workers run in parallel, each generates its own data
                const sgResult = host.scatterGather({
                    groupId,
                    query: {
                        op: "benchmark_lookups",
                        entities_per_shard: entitiesPerShard,
                        num_passes: numPasses,
                        seed: r + shardCount * 100,
                    },
                    timeoutMs: 60000,
                });
                const wallMs = host.nowMs() - wallStart;
                let roundCompute = 0;
                let roundLookups = 0;
                for (const resp of sgResult.shardResponses) {
                    const result = recordValue(resp.payload ?? resp);
                    if (result.error) {
                        errorCount++;
                        continue;
                    }
                    roundCompute += intValue(result.compute_ms, 0);
                    roundLookups += intValue(result.lookups_served, 0);
                }
                totalCoordMs += wallMs;
                totalComputeMs += roundCompute;
                totalLookups += roundLookups;
            }
            const avgWall = Math.round(totalCoordMs / benchmarkRounds);
            const avgCompute = Math.round(totalComputeMs / benchmarkRounds);
            const avgCoord = Math.max(avgWall - Math.round(avgCompute / shardCount), 1);
            const avgLookups = Math.round(totalLookups / benchmarkRounds);
            const lookupsPerSec = avgWall > 0 ? Math.round((avgLookups * 1000) / avgWall) : 0;
            const granularity = avgCoord > 0 ? Math.round((Math.round(avgCompute / shardCount) / avgCoord) * 10) / 10 : 0;
            if (baselineThroughput === 0)
                baselineThroughput = lookupsPerSec;
            const efficiency = baselineThroughput > 0 ? (lookupsPerSec / baselineThroughput) * 100 : 100;
            results.push({
                shards: shardCount,
                total_lookups: avgLookups,
                lookups_per_sec: lookupsPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                granularity_ratio: granularity,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                error_count: errorCount,
            });
        }
        return {
            status: "ok",
            entities_per_shard: entitiesPerShard,
            num_passes: numPasses,
            results,
        };
    }
}
// ─── Worker Actor ────────────────────────────────────────────────────────────
class WorkerActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "", application_id: "", role: "worker",
            features: {},
            cache: {},
            cache_order: [],
            cache_max_size: 200,
            cache_hits: 0,
            cache_misses: 0,
            lookups_served: 0,
        };
    }
    onInit(config) {
        const args = recordValue(config.args);
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "worker";
        this.state.cache_max_size = intValue(args.cache_max_size, 200);
    }
    // Worker-local benchmark: generates entities locally, ingests into state, then does
    // num_passes of full-vector lookups. Leader sends one broadcast SG instead of N
    // sequential per-shard calls — enables true weak scaling.
    onBenchmark_lookups(payload) {
        const entitiesPerShard = intValue(payload.entities_per_shard, 200);
        const numPasses = intValue(payload.num_passes, 4);
        const seed = intValue(payload.seed, 42);
        const compStart = host.nowMs();
        // Generate and ingest entities locally
        const features = generateFeatures(entitiesPerShard, seed);
        for (const f of features) {
            const key = `${f.entity_id}:${f.feature_name}`;
            const existing = this.state.features[key] ?? { versions: [] };
            existing.versions.push({ value: f.value, version: 1, timestamp: compStart });
            if (existing.versions.length > 3)
                existing.versions.shift();
            this.state.features[key] = existing;
        }
        // Do num_passes of full-vector lookup for all entities
        let totalLookups = 0;
        let cacheHits = 0;
        let cacheMisses = 0;
        const entityIds = Array.from({ length: entitiesPerShard }, (_, i) => `entity-${i}`);
        for (let p = 0; p < numPasses; p++) {
            // Evict cache between passes to exercise both hot and cold paths
            if (p % 2 === 0) {
                this.state.cache = {};
                this.state.cache_order = [];
            }
            for (const entityId of entityIds) {
                const cached = this.state.cache[entityId];
                if (cached && Object.keys(cached.features).length >= ALL_FEATURES.length) {
                    cacheHits++;
                    this.state.cache_hits++;
                }
                else {
                    cacheMisses++;
                    this.state.cache_misses++;
                    const vector = {};
                    for (const fname of ALL_FEATURES) {
                        const key = `${entityId}:${fname}`;
                        const fv = this.state.features[key];
                        if (fv && fv.versions.length > 0) {
                            vector[fname] = fv.versions[fv.versions.length - 1].value;
                        }
                    }
                    this.addToCache(entityId, vector);
                }
                totalLookups++;
            }
        }
        const computeMs = host.nowMs() - compStart;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.bench_compute": computeMs } });
        }
        catch (_e) { }
        return {
            lookups_served: totalLookups,
            entities_per_shard: entitiesPerShard,
            passes: numPasses,
            cache_hits: cacheHits,
            cache_misses: cacheMisses,
            compute_ms: computeMs,
        };
    }
    onIngest_batch(payload) {
        const compStart = host.nowMs();
        const rawFeatures = anyArray(payload.features);
        let ingested = 0;
        for (const raw of rawFeatures) {
            const f = recordValue(raw);
            const entityId = stringValue(f.entity_id);
            const featureName = stringValue(f.feature_name);
            const value = floatValue(f.value, 0);
            const version = intValue(f.version, 1);
            const timestamp = intValue(f.timestamp, 0);
            if (!entityId || !featureName)
                continue;
            const key = `${entityId}:${featureName}`;
            if (!this.state.features[key]) {
                this.state.features[key] = { versions: [] };
            }
            const fv = this.state.features[key];
            fv.versions.push({ value, version, timestamp });
            // Keep last 3 versions
            if (fv.versions.length > 3) {
                fv.versions = fv.versions.slice(-3);
            }
            // Invalidate cache for this entity
            if (this.state.cache[entityId]) {
                delete this.state.cache[entityId];
                this.state.cache_order = this.state.cache_order.filter(id => id !== entityId);
            }
            ingested++;
        }
        const computeMs = host.nowMs() - compStart;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.ingest_compute": computeMs } });
        }
        catch (_e) { }
        return {
            ingested_count: ingested,
            compute_ms: computeMs,
        };
    }
    onBatch_lookup(payload) {
        const compStart = host.nowMs();
        const entityIds = stringArray(anyArray(payload.entity_ids));
        const featureNames = stringArray(anyArray(payload.feature_names));
        let hits = 0;
        let misses = 0;
        const results = {};
        for (const entityId of entityIds) {
            // Check cache first
            const cached = this.state.cache[entityId];
            if (cached) {
                hits++;
                this.state.cache_hits++;
                const entityResult = {};
                for (const fname of featureNames) {
                    if (cached.features[fname] !== undefined) {
                        entityResult[fname] = cached.features[fname];
                    }
                }
                results[entityId] = entityResult;
                // Move to front of LRU
                this.state.cache_order = this.state.cache_order.filter(id => id !== entityId);
                this.state.cache_order.push(entityId);
                continue;
            }
            misses++;
            this.state.cache_misses++;
            // Fetch from storage
            const entityResult = {};
            for (const fname of featureNames) {
                const key = `${entityId}:${fname}`;
                const fv = this.state.features[key];
                if (fv && fv.versions.length > 0) {
                    entityResult[fname] = fv.versions[fv.versions.length - 1].value;
                }
            }
            results[entityId] = entityResult;
            // Add to cache
            this.addToCache(entityId, entityResult);
            this.state.lookups_served++;
        }
        const computeMs = host.nowMs() - compStart;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.lookup_compute": computeMs } });
        }
        catch (_e) { }
        return {
            lookups_served: entityIds.length,
            cache_hits: hits,
            cache_misses: misses,
            compute_ms: computeMs,
            result_count: Object.keys(results).length,
        };
    }
    onGet_feature_vectors(payload) {
        const compStart = host.nowMs();
        const entityIds = stringArray(anyArray(payload.entity_ids));
        let hits = 0;
        let misses = 0;
        const vectors = {};
        for (const entityId of entityIds) {
            // Check cache
            const cached = this.state.cache[entityId];
            if (cached && Object.keys(cached.features).length >= ALL_FEATURES.length) {
                hits++;
                this.state.cache_hits++;
                vectors[entityId] = { ...cached.features };
                this.state.cache_order = this.state.cache_order.filter(id => id !== entityId);
                this.state.cache_order.push(entityId);
                continue;
            }
            misses++;
            this.state.cache_misses++;
            // Assemble full feature vector
            const vector = {};
            for (const fname of ALL_FEATURES) {
                const key = `${entityId}:${fname}`;
                const fv = this.state.features[key];
                if (fv && fv.versions.length > 0) {
                    vector[fname] = fv.versions[fv.versions.length - 1].value;
                }
            }
            vectors[entityId] = vector;
            this.addToCache(entityId, vector);
            this.state.lookups_served++;
        }
        const computeMs = host.nowMs() - compStart;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.vector_compute": computeMs } });
        }
        catch (_e) { }
        return {
            lookups_served: entityIds.length,
            cache_hits: hits,
            cache_misses: misses,
            compute_ms: computeMs,
            vector_count: Object.keys(vectors).length,
        };
    }
    addToCache(entityId, features) {
        this.state.cache[entityId] = {
            entity_id: entityId,
            features,
            cached_at: host.nowMs(),
        };
        this.state.cache_order.push(entityId);
        // Evict LRU if over capacity
        while (this.state.cache_order.length > this.state.cache_max_size) {
            const evicted = this.state.cache_order.shift();
            if (evicted)
                delete this.state.cache[evicted];
        }
    }
}
// ─── Helpers ─────────────────────────────────────────────────────────────────
function actorApplicationId(actorId) {
    const doubleSlash = actorId.indexOf("//");
    if (doubleSlash < 0)
        return "";
    const rest = actorId.slice(doubleSlash + 2);
    const colonColon = rest.indexOf("::");
    if (colonColon < 0)
        return "";
    const afterColon = rest.slice(colonColon + 2);
    const atSign = afterColon.indexOf("@");
    return atSign >= 0 ? afterColon.slice(0, atSign) : afterColon;
}
function actorNodeId(actorId) {
    const atSign = actorId.lastIndexOf("@");
    return atSign >= 0 ? actorId.slice(atSign + 1) : "";
}
function intValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value))
        return Math.trunc(value);
    if (typeof value === "string") {
        const parsed = Number.parseInt(value, 10);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
}
function floatValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value))
        return value;
    if (typeof value === "string") {
        const parsed = Number.parseFloat(value);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
}
function intArrayValue(value, fallback) {
    if (!Array.isArray(value))
        return fallback;
    return value.map((v) => intValue(v, 0)).filter((v) => v > 0);
}
function stringValue(value) {
    return typeof value === "string" ? value : "";
}
function recordValue(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value)
        ? value
        : {};
}
function anyArray(value) {
    return Array.isArray(value) ? value : [];
}
function stringArray(value) {
    return value.map((item) => String(item)).filter((item) => item.length > 0);
}
function computeActorCounts(leaderNodeId, shardActorIds) {
    const nodes = {
        [leaderNodeId]: { actors: 1, leader_actors: 1, worker_actors: 0 },
    };
    for (const actorId of shardActorIds) {
        const nodeId = actorNodeId(actorId);
        if (!nodes[nodeId]) {
            nodes[nodeId] = { actors: 0, leader_actors: 0, worker_actors: 0 };
        }
        nodes[nodeId].actors += 1;
        nodes[nodeId].worker_actors += 1;
    }
    return nodes;
}
// ─── Router & Export ─────────────────────────────────────────────────────────
const router = new ActorRouter({
    leader: () => new LeaderActor(),
    worker: () => new WorkerActor(),
    "feature-leader": () => new LeaderActor(),
    "feature-worker": () => new WorkerActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
