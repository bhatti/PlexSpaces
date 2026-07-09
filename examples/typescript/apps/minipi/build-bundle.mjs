#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "minipi_actors.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "minipi_actors_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ minipi_actors_bundle.mjs");
