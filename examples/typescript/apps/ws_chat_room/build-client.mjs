#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle the browser client (static/client.ts → static/client.js).
// Uses esbuild with browser platform so WebSocket global is not polyfilled.

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "static", "client.ts")],
  bundle: true,
  format: "iife",
  outfile: join(__dirname, "static", "client.js"),
  platform: "browser",
  target: "es2020",
  // Remap @plexspaces/sdk sub-paths to the specific dist files that have no
  // WIT/WASM imports — WsThinClient and ActorID are pure browser-safe JS.
  alias: {
    "@plexspaces/sdk": join(__dirname, "node_modules/@plexspaces/sdk/dist/index.js"),
  },
  // Exclude any WIT host imports that only work inside a WASM component
  external: ["plexspaces:*"],
  // Override imports of actor/host modules that pull in WIT
  // Instead of stubbing WIT, import directly from the browser-safe dist files
  // and avoid pulling in the full SDK index (which re-exports WIT-dependent modules).
  // client.ts imports are remapped below via the alias above; the WIT-dependent
  // modules are tree-shaken out since ws_thin_client.js and actor_id.js don't
  // import them.  We still need to handle any transitive require of plexspaces:*.
  plugins: [{
    name: "stub-wit",
    setup(build) {
      build.onResolve({ filter: /^plexspaces:/ }, args => ({
        path: args.path, namespace: "wit-stub",
      }));
      // Return a module that exports every name as a no-op function
      build.onLoad({ filter: /.*/, namespace: "wit-stub" }, () => ({
        contents: `
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
`,
        loader: "js",
      }));
    },
  }],
});

console.log("  ✓ static/client.js");
