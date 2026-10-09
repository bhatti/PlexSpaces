#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "parameter_server_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "parameter_server_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ parameter_server_actor_bundle.mjs");
