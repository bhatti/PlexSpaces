#!/usr/bin/env node

import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "streaming_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "streaming_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:actor/host@0.1.0"],
});

console.log("  ✓ streaming_actor_bundle.mjs");
