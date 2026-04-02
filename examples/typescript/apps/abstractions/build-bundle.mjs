#!/usr/bin/env node
// SPDX-License-Identifier: LGPL-2.1-or-later

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "abstractions_app.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "abstractions_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ abstractions_actor_bundle.mjs");
