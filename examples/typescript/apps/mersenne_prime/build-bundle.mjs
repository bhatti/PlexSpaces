#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle the WASM actor for jco componentize.

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "mersenne_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "mersenne_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2022",
  external: ["plexspaces:*"],
});

console.log("  ✓ mersenne_actor_bundle.mjs");
