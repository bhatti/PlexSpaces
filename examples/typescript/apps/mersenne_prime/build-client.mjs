#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle the browser client (static/client.ts → static/client.js).

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const WIT_STUB = `
const _noop = () => undefined;
export default _noop;
export const send = _noop, ask = _noop, log = _noop, nowMs = _noop,
  selfId = _noop, spawn = _noop, stop = _noop, link = _noop, unlink = _noop,
  monitor = _noop, demonitor = _noop, sendAfter = _noop,
  kvGet = _noop, kvPut = _noop, kvDelete = _noop, kvList = _noop,
  kvPutWithTtl = _noop, kvGetTtl = _noop, kvCas = _noop, kvIncrement = _noop,
  kvMultiGet = _noop, kvMultiPut = _noop,
  alarmSet = _noop, alarmGet = _noop, alarmDelete = _noop,
  tsWrite = _noop, tsRead = _noop, tsTake = _noop, tsReadAll = _noop,
  lockAcquire = _noop, lockRelease = _noop, lockRenew = _noop,
  blobUpload = _noop, blobDownload = _noop, blobDelete = _noop, blobList = _noop,
  pgJoin = _noop, pgLeave = _noop, pgMembers = _noop, pgBroadcast = _noop,
  poolCheckout = _noop, poolCheckin = _noop, poolGetMetrics = _noop,
  createShardGroup = _noop, bulkUpdateShardGroup = _noop, mapShardGroup = _noop,
  scatterGather = _noop, broadcastShardGroup = _noop, reduceShardGroup = _noop,
  allReduceShardGroup = _noop, barrierShardGroup = _noop, spawnActors = _noop,
  applicationMetricsAdd = _noop, applicationGetMetrics = _noop,
  applicationGetStatus = _noop, httpFetch = _noop,
  register = _noop, unregister = _noop, lookup = _noop,
  lookupByAlias = _noop, discover = _noop, heartbeat = _noop;
`;

await esbuild.build({
  entryPoints: [join(__dirname, "static", "client.ts")],
  bundle: true,
  format: "iife",
  outfile: join(__dirname, "static", "client.js"),
  platform: "browser",
  target: "es2022",
  plugins: [{
    name: "stub-wit",
    setup(build) {
      build.onResolve({ filter: /^plexspaces:/ }, args => ({ path: args.path, namespace: "wit-stub" }));
      build.onLoad({ filter: /.*/, namespace: "wit-stub" }, () => ({ contents: WIT_STUB, loader: "js" }));
    },
  }],
});

console.log("  ✓ static/client.js");
