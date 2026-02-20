// Node.js custom ESM loader that stubs out PlexSpaces virtual imports.
// Used for in-process verification (no WASM runtime needed).
// Usage: node --loader ./node-loader.mjs verify.mjs

const PLEXSPACES_PREFIX = "plexspaces:";

// Stub all host functions with no-ops / sensible defaults
const STUBS = `
let _nowMs = Date.now();

export function send() { return ""; }
export function ask() { return "{}"; }
export function log(level, message) { /* console.log(\`[\${level}] \${message}\`); */ }
export function nowMs() { return BigInt(Date.now()); }
export function selfId() { return "stub-actor:test@node"; }
export function spawn() { return "spawned-actor"; }
export function stop() { return ""; }
export function link() { return ""; }
export function unlink() { return ""; }
export function monitor() { return "monitor-ref"; }
export function demonitor() { return ""; }
export function sendAfter() { return "timer-ref"; }
export function kvGet() { return ""; }
export function kvPut() { return ""; }
export function kvDelete() { return ""; }
export function kvList() { return "[]"; }
export function tsWrite() { return ""; }
export function tsRead() { return ""; }
export function tsTake() { return ""; }
export function tsReadAll() { return "[]"; }
export function lockAcquire() { return "lock-id"; }
export function lockRelease() { return ""; }
export function lockRenew() { return ""; }
export function blobUpload() { return ""; }
export function blobDownload() { return ""; }
export function blobDelete() { return ""; }
export function blobList() { return "[]"; }
export function pgJoin() { return ""; }
export function pgLeave() { return ""; }
export function pgMembers() { return "[]"; }
export function pgBroadcast() { return ""; }
`;

export function resolve(specifier, context, nextResolve) {
  if (specifier.startsWith(PLEXSPACES_PREFIX)) {
    return { url: specifier, shortCircuit: true };
  }
  return nextResolve(specifier, context);
}

export function load(url, context, nextLoad) {
  if (url.startsWith(PLEXSPACES_PREFIX)) {
    return {
      format: "module",
      source: STUBS,
      shortCircuit: true,
    };
  }
  return nextLoad(url, context);
}
