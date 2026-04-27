#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Generate WIT TypeScript types for SDK (internal use only)

import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SDK_ROOT = join(__dirname, '..');
// SDK is at: <repo>/sdks/typescript
// WIT is at: <repo>/wit/plexspaces-actor
const REPO_ROOT = join(SDK_ROOT, '../..');
const WIT_DIR = join(REPO_ROOT, 'wit/plexspaces-actor');
const OUTPUT_DIR = join(SDK_ROOT, 'src/generated');

// Check if jco is available
let jco;
try {
  execSync('jco --version', { stdio: 'ignore' });
  jco = 'jco';
} catch {
  // Try node_modules
  const localJco = join(SDK_ROOT, 'node_modules/.bin/jco');
  try {
    execSync(`"${localJco}" --version`, { stdio: 'ignore' });
    jco = localJco;
  } catch {
    console.warn('⚠️  jco not found - skipping type generation (types are optional for IDE support)');
    process.exit(0);
  }
}

// Generate types
try {
  execSync(
    `"${jco}" types "${WIT_DIR}" -n actor-world -o "${OUTPUT_DIR}"`,
    { stdio: 'inherit', cwd: SDK_ROOT }
  );
  console.log('✓ Generated WIT types');
} catch (error) {
  console.warn('⚠️  Type generation failed (types are optional):', error.message);
  process.exit(0); // Don't fail build if types can't be generated
}
