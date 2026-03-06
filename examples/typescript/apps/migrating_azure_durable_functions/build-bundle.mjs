#!/usr/bin/env node
// SPDX-License-Identifier: LGPL-2.1-or-later
// Bundle document_processing_actor.ts + @plexspaces/sdk for jco componentize.

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const out = join(__dirname, "document_processing_actor_bundle.mjs");

await esbuild.build({
  entryPoints: [join(__dirname, "document_processing_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: out,
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ document_processing_actor_bundle.mjs");
