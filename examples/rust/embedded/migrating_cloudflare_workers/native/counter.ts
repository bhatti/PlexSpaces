// Cloudflare Workers Durable Object Example
// This is sample code showing the native Cloudflare Workers implementation

// counter.ts - Durable Object class
export class Counter {
    private state: DurableObjectState;
    private count: number = 0;

    constructor(state: DurableObjectState, env: Env) {
        this.state = state;
        // Load state from storage
        this.state.storage.get<number>("count").then((value) => {
            this.count = value || 0;
        });
    }

    async fetch(request: Request): Promise<Response> {
        const url = new URL(request.url);
        const action = url.pathname.split("/").pop();

        switch (action) {
            case "increment":
                this.count++;
                // Persist state
                await this.state.storage.put("count", this.count);
                return new Response(JSON.stringify({ count: this.count }));

            case "decrement":
                this.count = Math.max(0, this.count - 1);
                await this.state.storage.put("count", this.count);
                return new Response(JSON.stringify({ count: this.count }));

            case "get":
                return new Response(JSON.stringify({ count: this.count }));

            default:
                return new Response("Not found", { status: 404 });
        }
    }
}

// worker.ts - Cloudflare Worker
export default {
    async fetch(request: Request, env: Env): Promise<Response> {
        // Get Durable Object ID from URL
        const id = env.COUNTER.idFromName("counter-1");
        const stub = env.COUNTER.get(id);
        
        // Forward request to Durable Object
        return stub.fetch(request);
    },
};

// wrangler.toml
// [durable_objects]
// bindings = [
//   { name = "COUNTER", class_name = "Counter" }
// ]
