import { describe, it } from "node:test";
import assert from "node:assert/strict";

function actorRoleId(actorId) {
  if (!actorId) return "";
  const canonicalSep = actorId.indexOf("//");
  if (canonicalSep >= 0 && canonicalSep + 2 < actorId.length) {
    const rest = actorId.substring(canonicalSep + 2);
    const behaviorSep = rest.indexOf("::");
    if (behaviorSep >= 0) return rest.substring(0, behaviorSep);
    const nodeSep = rest.indexOf("@");
    return nodeSep >= 0 ? rest.substring(0, nodeSep) : rest;
  }
  const childSep = actorId.indexOf(":");
  if (childSep >= 0) return actorId.substring(0, childSep);
  const nodeSep = actorId.indexOf("@");
  return nodeSep >= 0 ? actorId.substring(0, nodeSep) : actorId;
}

function actorNodeId(actorId) {
  const parts = actorId.split("@");
  return parts.length > 1 ? parts[parts.length - 1] : "local";
}

function normalizeWorkerPayload(payload) {
  let current = payload && typeof payload === "object" && !Array.isArray(payload) ? payload : {};
  while (current && !("status" in current)) {
    let progressed = false;
    for (const key of ["payload", "result", "response", "data"]) {
      const nested = current[key];
      if (nested && typeof nested === "object" && !Array.isArray(nested)) {
        current = nested;
        progressed = true;
        break;
      }
    }
    if (!progressed) break;
  }
  return current;
}

function mergeTopStreams(values, topK) {
  const counts = {};
  for (const value of values) {
    counts[value.stream] = (counts[value.stream] ?? 0) + value.count;
  }
  return Object.entries(counts)
    .map(([stream, count]) => ({ stream, count }))
    .sort((a, b) => b.count - a.count)
    .slice(0, topK);
}

function computeActorCounts(leaderNodeId, shardActorIds) {
  const nodes = {
    [leaderNodeId]: {
      actors: 1,
      leader_actors: 1,
      worker_actors: 0,
    },
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

describe("streaming pipeline helpers", () => {
  it("extracts role from canonical actor ids", () => {
    assert.equal(
      actorRoleId("01ABC//worker-3::streaming-pipeline-ts@test-node-8093"),
      "worker-3",
    );
    assert.equal(actorRoleId("leader:streaming-pipeline-ts@test-node-8091"), "leader");
  });

  it("unwraps nested worker payloads", () => {
    const payload = normalizeWorkerPayload({
      payload: {
        response: {
          status: "ok",
          event_count: 1200,
        },
      },
    });
    assert.equal(payload.status, "ok");
    assert.equal(payload.event_count, 1200);
  });

  it("merges stream counts across shards", () => {
    const merged = mergeTopStreams([
      { stream: "auth", count: 10 },
      { stream: "api", count: 8 },
      { stream: "auth", count: 7 },
    ], 2);
    assert.deepEqual(merged, [
      { stream: "auth", count: 17 },
      { stream: "api", count: 8 },
    ]);
  });

  it("counts leader and worker actors per node from shard ids", () => {
    const counts = computeActorCounts("test-node-8091", [
      "run-1//streaming_pipeline_wasm::streaming-pipeline-ts@test-node-8091",
      "run-2//streaming_pipeline_wasm::streaming-pipeline-ts@test-node-8093",
      "run-3//streaming_pipeline_wasm::streaming-pipeline-ts@test-node-8093",
    ]);
    assert.deepEqual(counts, {
      "test-node-8091": { actors: 2, leader_actors: 1, worker_actors: 1 },
      "test-node-8093": { actors: 2, leader_actors: 0, worker_actors: 2 },
    });
  });
});
