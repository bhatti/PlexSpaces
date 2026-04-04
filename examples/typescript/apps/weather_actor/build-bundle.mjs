#!/usr/bin/env node

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

await esbuild.build({
  entryPoints: [join(__dirname, "weather_actor.ts")],
  bundle: true,
  format: "esm",
  outfile: join(__dirname, "weather_actor_bundle.mjs"),
  platform: "neutral",
  target: "es2020",
  packages: "bundle",
  external: ["plexspaces:simple-actor/host@0.1.0"],
});

console.log("  ✓ weather_actor_bundle.mjs");
