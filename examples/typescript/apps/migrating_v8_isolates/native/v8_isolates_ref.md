# V8 Isolates Reference: High-Throughput Multi-Tenant Log Processing

V8 isolates provide lightweight, isolated JavaScript runtimes (separate heaps, shared code). Used for multi-tenant serverless (e.g. Cloudflare Workers), log processing pipelines, and high-throughput event handling.

## Concepts

- **Isolate**: One V8 heap; run user code in isolation. Fast to create/destroy compared to full V8 instances.
- **Batch processing**: Process many log lines (or events) per isolate request cycle to amortize overhead.
- **Routing**: By log level, tenant, or topic; often done after parse in the application.

## Native-style usage (Node with worker threads or isolate API)

```javascript
// Conceptual: one isolate per batch or per tenant
const { Isolate } = require("isolated-vm"); // or similar

async function processLogBatch(lines) {
  const isolate = new Isolate({ memoryLimit: 128 });
  const context = await isolate.createContext();
  const result = await context.eval(`
    const byLevel = { INFO: 0, WARN: 0, ERROR: 0, DEBUG: 0 };
    lines.forEach(line => {
      const level = line.split(/\\s/)[0];
      if (byLevel[level] !== undefined) byLevel[level]++;
    });
    byLevel;
  `, { lines });
  isolate.dispose();
  return result;
}
```

## PlexSpaces mapping

| V8 Isolates | PlexSpaces |
|-------------|------------|
| One isolate per batch/tenant | One GenServer actor; process_batch(payload) handles many lines per call |
| Batch of lines in, aggregate out | process_batch({ lines: string[] }) → parse level, update state, return counts |
| Throughput via many isolates | Throughput via many process_batch calls (and/or multiple actor instances) |
| Routing by level in app | by_level in state; optional channels for downstream routing |

This example uses a single actor and process_batch to achieve high throughput with minimal boilerplate; scale out by deploying more instances or routing by tenant/level to different actors.
