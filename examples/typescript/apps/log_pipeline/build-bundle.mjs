#!/usr/bin/env node

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "log_pipeline_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "log_pipeline_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:*"],
});

console.log("  ✓ log_pipeline_actor_bundle.mjs");
