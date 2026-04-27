#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle guild_chat_actor.ts + @plexspaces/sdk into one ESM file for jco componentize (no Node APIs in output).

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const out = join(__dirname, "guild_chat_actor_bundle.mjs");

await esbuild.build({
  entryPoints: [join(__dirname, "guild_chat_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: out,
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  // Virtual imports are resolved by jco componentize at WASM link time
  external: ["plexspaces:*"],
});

console.log("  ✓ guild_chat_actor_bundle.mjs");
