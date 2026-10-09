// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Typed request/response types for shard group host operations.

export interface NodePlacementOptions {
  strategy?: 'from_registry' | 'same_node' | 'node_ids';
  nodeIds?: string[];
  cluster?: string;
  requiredLabels?: Record<string, string>;
  avoidNodeIds?: string[];
  affinityLabels?: Record<string, string>;
}

export interface CreateShardGroupOptions {
  groupId: string;
  actorType: string;
  shardCount: number;
  partitionStrategy?: 'hash' | 'range' | 'consistent_hash';
  rebalancePolicy?: 'manual' | 'on_scale' | 'load_based';
  placement?: NodePlacementOptions;
  initialState?: Record<string, unknown>;
  metadata?: Record<string, string>;
}

export interface CreateShardGroupResult {
  groupId: string;
  actorType: string;
  shardActorIds: string[];
  shardCount: number;
}

export interface ScatterGatherOptions {
  groupId: string;
  query: Record<string, unknown>;
  aggregation?: 'concat' | 'merge' | 'first' | 'majority';
  timeoutMs?: number;
  minResponses?: number;
}

export interface ShardQueryResult {
  shardId: number;
  shardActorId: string;
  payload: unknown;
  success: boolean;
  error: string;
}

export interface ScatterGatherResult {
  shardResponses: ShardQueryResult[];
}

export interface BroadcastShardGroupOptions {
  groupId: string;
  message: Record<string, unknown>;
  timeoutMs?: number;
  minAcks?: number;
}

export interface ReduceShardGroupOptions {
  groupId: string;
  mapFunction: Record<string, unknown>;
  reduction: 'sum' | 'min' | 'max' | 'product' | 'concat' | 'bool_and' | 'bool_or';
  target?: string;
  timeoutMs?: number;
  minResponses?: number;
}

export type AllReduceShardGroupOptions = ReduceShardGroupOptions;

export interface BarrierShardGroupOptions {
  groupId: string;
  barrierId: string;
  round?: number;
  timeoutMs?: number;
  minAcks?: number;
}

export interface MapShardGroupOptions {
  groupId: string;
  mapFunction: Record<string, unknown>;
  timeoutMs?: number;
  minResponses?: number;
}

export interface BulkUpdateShardGroupOptions {
  groupId: string;
  updates: Record<string, Record<string, unknown>>;
  consistencyLevel?: 'eventual' | 'strong' | 'sequential';
  timeoutMs?: number;
  waitForResponses?: boolean;
}
