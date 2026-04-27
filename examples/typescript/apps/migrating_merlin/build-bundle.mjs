#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Bundle sweep_actor.ts + @plexspaces/sdk for jco componentize.

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const out = join(__dirname, "sweep_actor_bundle.mjs");

await esbuild.build({
  entryPoints: [join(__dirname, "sweep_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: out,
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ sweep_actor_bundle.mjs");
