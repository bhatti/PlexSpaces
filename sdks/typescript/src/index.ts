// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces TypeScript SDK
//
// Build actors with minimal boilerplate via inheritance (mirrors Python SDK's decorators).

export { PlexSpacesActor, WorkflowActor } from "./actor.js";
export { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from "./wit-payload.js";
export {
  actor,
  gen_server_actor,
  event_actor,
  fsm_actor,
  workflow_actor,
  handler,
  init_handler,
  run_handler,
  signal_handler,
  query_handler,
  getActorDefinition,
} from "./decorators.js";
export { Host, ProcessGroups, TupleSpace, host, ServiceHttpClient } from "./host.js";
export { ActorRouter } from "./router.js";
export {
  defaultRetryConfig,
  withRetry,
  type RetryConfig,
} from "./workflow.js";
export {
  LeaderWorkerClient,
  listWorkerNodeIds,
} from "./leader_worker.js";
export { ActorID } from "./actor_id.js";
