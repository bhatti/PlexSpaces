// Rivet Counter Actor (TypeScript)
// Demonstrates Cloudflare Durable Objects pattern with Rivet

import { Actor } from '@rivet-gg/actors';

export class CounterActor extends Actor {
    private count: number = 0;

    async increment(): Promise<number> {
        this.count++;
        // State automatically persisted via Durable Objects
        return this.count;
    }

    async decrement(): Promise<number> {
        this.count = Math.max(0, this.count - 1);
        return this.count;
    }

    async get(): Promise<number> {
        return this.count;
    }
}

// Usage:
// const counter = await rivet.actors.getOrActivate('counter', 'rivet-1');
// await counter.increment();
// const count = await counter.get();
