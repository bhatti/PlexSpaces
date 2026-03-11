// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Leader-worker client for multi-node: same API surface as Rust/Python/Go SDKs.
// Virtual actors are created lazily on first message; use spawnActorOnNode
// only for non-virtual workers.

/**
 * Client for leader-worker multi-node patterns. Connect to the entry (leader) node
 * via HTTP; list worker node IDs and spawn non-virtual actors on specific nodes.
 * Virtual actors are created lazily on first message—no explicit spawn or ensure.
 */
export class LeaderWorkerClient {
  private readonly entryUrl: string;
  private readonly nodeIdToHttp: Map<string, string> = new Map();

  /**
   * @param entryHttpUrl - Base URL of the entry/leader node (e.g. "http://localhost:8092").
   */
  constructor(entryHttpUrl: string) {
    this.entryUrl = entryHttpUrl.replace(/\/$/, "");
  }

  /**
   * Derive HTTP URL from gRPC node_address (convention: HTTP = gRPC port + 1).
   */
  private static grpcPortToHttp(url: string): string {
    if (!url) return url;
    const match = url.trim().match(/:(\d+)\s*$/);
    if (match) {
      const port = parseInt(match[1], 10) + 1;
      return url.trim().replace(/:(\d+)\s*$/, `:${port}`);
    }
    return url;
  }

  /**
   * List node IDs that can run workers (peers + self). Populates internal
   * cache so spawnActorOnNode can resolve nodeId to node HTTP URL.
   *
   * @returns List of node_id strings.
   */
  async listWorkerNodeIds(
    cluster?: string | null,
    pageSize = 100,
    pageToken = ""
  ): Promise<string[]> {
    const params = new URLSearchParams({ pageSize: String(pageSize) });
    if (cluster) params.set("cluster", cluster);
    if (pageToken) params.set("pageToken", pageToken);
    const url = `${this.entryUrl}/api/v1/nodes?${params.toString()}`;
    const res = await fetch(url, {
      method: "GET",
      headers: { Accept: "application/json" },
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`listWorkerNodeIds failed: ${res.status} ${body}`);
    }
    const data = (await res.json()) as {
      nodes?: Array<{ nodeId?: string; node_id?: string; nodeAddress?: string; node_address?: string }>;
      nodeRegistrations?: Array<{ nodeId?: string; node_id?: string; nodeAddress?: string; node_address?: string }>;
    };
    const nodes = data.nodes ?? data.nodeRegistrations ?? [];
    const ids: string[] = [];
    for (const n of nodes) {
      const nodeId = n.nodeId ?? n.node_id ?? "";
      const nodeAddress = n.nodeAddress ?? n.node_address ?? "";
      if (nodeId) {
        ids.push(nodeId);
        if (nodeAddress) {
          this.nodeIdToHttp.set(nodeId, LeaderWorkerClient.grpcPortToHttp(nodeAddress));
        }
      }
    }
    return ids;
  }

  /**
   * Spawn a non-virtual actor on a specific node. The node must be reachable;
   * call listWorkerNodeIds first so the client can resolve nodeId to URL.
   *
   * @returns Actor ref string (e.g. "worker-ulid@node_id").
   */
  async spawnActorOnNode(
    nodeId: string,
    actorType: string,
    actorId = "",
    initialState: Uint8Array = new Uint8Array(0),
    config?: Record<string, unknown> | null,
    labels?: Record<string, string> | null
  ): Promise<string> {
    const nodeHttp = this.nodeIdToHttp.get(nodeId);
    if (!nodeHttp) {
      throw new Error(
        `unknown nodeId "${nodeId}"; call listWorkerNodeIds first`
      );
    }
    const payload: Record<string, unknown> = { actorType };
    if (actorId) payload.actorId = actorId;
    if (initialState.byteLength > 0) {
      let binary = "";
      for (let i = 0; i < initialState.length; i++) {
        binary += String.fromCharCode(initialState[i]);
      }
      payload.initialState = btoa(binary);
    }
    if (config != null) payload.config = config;
    if (labels != null) payload.labels = labels;
    const body = JSON.stringify(payload);
    const url = `${nodeHttp.replace(/\/$/, "")}/api/v1/actors/spawn`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`spawnActorOnNode failed: ${res.status} ${text}`);
    }
    const out = (await res.json()) as { actorRef?: string; actor_ref?: string };
    const actorRef = out.actorRef ?? out.actor_ref ?? "";
    if (!actorRef) throw new Error("spawnActorOnNode returned empty actorRef");
    return actorRef;
  }
}

/**
 * Convenience: list worker node IDs using a one-off client.
 * For multiple calls or spawnActorOnNode, use LeaderWorkerClient.
 */
export async function listWorkerNodeIds(
  entryHttpUrl: string,
  cluster?: string | null,
  pageSize = 100
): Promise<string[]> {
  const client = new LeaderWorkerClient(entryHttpUrl);
  return client.listWorkerNodeIds(cluster, pageSize);
}
