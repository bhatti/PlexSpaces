// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle perf_actor.ts into a self-contained ESM module for jco componentize.
// Uses esbuild with packages:"bundle" so the output has zero external imports —
// jco's SpiderMonkey engine cannot resolve relative specifiers like "./actor.js".
import * as esbuild from "esbuild";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [`${__dirname}/perf_actor.ts`],
  bundle: true,
  format: "esm",
  outfile: `${__dirname}/perf_actor_bundle.mjs`,
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ perf_actor_bundle.mjs");
