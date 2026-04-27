#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
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
  // WIT world imports are satisfied by the component host at runtime, not by npm.
  external: ["plexspaces:*"],
});

console.log("  ✓ account_actor_bundle.mjs");
