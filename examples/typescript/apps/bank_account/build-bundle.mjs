#!/usr/bin/env node
// SPDX-License-Identifier: LGPL-2.1-or-later
// Bundle account_actor.ts + @plexspaces/sdk into one ESM file for jco componentize (no Node APIs in output).

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const out = join(__dirname, "account_actor_bundle.mjs");

await esbuild.build({
  entryPoints: [join(__dirname, "account_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: out,
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
});

console.log("  ✓ account_actor_bundle.mjs");
