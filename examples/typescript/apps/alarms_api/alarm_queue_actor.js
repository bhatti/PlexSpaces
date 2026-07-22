// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Alarms API Example — TypeScript WASM actor
//
// Demonstrates the Cloudflare Durable Objects alarm() pattern: a RequestQueue
// actor that batches incoming requests and processes them 10 seconds after the
// first write, using a durable alarm that survives actor deactivation.
//
// ## Cloudflare DO vs PlexSpaces TypeScript
//
// | Cloudflare DO                             | PlexSpaces TypeScript                  |
// |-------------------------------------------|----------------------------------------|
// | export class RequestQueue extends DO      | RequestQueueActor extends PlexSpacesActor |
// | this.ctx.storage.get('count')             | host.kvGet('count') / getState()       |
// | this.ctx.storage.put('count', n)          | host.kvPut('count', ...) / setState()  |
// | this.ctx.storage.setAlarm(Date.now()+10s) | host.alarm.set(host.nowMs() + 10_000)  |
// | this.ctx.storage.getAlarm()               | host.alarm.get()                       |
// | async alarm() { ... }                     | on__alarm__() handler                  |
// | new Response(JSON.stringify(result))      | return { ...result }                   |
// | wrangler.toml [[durable_objects]]         | app-config.toml [[supervisor.children]] |
import { PlexSpacesActor, ActorRouter, host } from "@plexspaces/sdk";
// ============================================================================
// RequestQueueActor — batches requests and processes them on alarm
// ============================================================================
class RequestQueueActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            items: [],
            count: 0,
            total_processed: 0,
            total_alarm_fires: 0,
        };
    }
    // Enqueue an item for deferred batch processing.
    // Sets a durable alarm 10 seconds from now on the FIRST item only.
    // Equivalent to Cloudflare DO: if (count === 0) this.ctx.storage.setAlarm(Date.now() + 10_000)
    onEnqueue(payload) {
        const state = this.state;
        const item = {
            id: state.count + 1,
            data: payload.item ?? payload,
            enqueued_at: host.nowMs(),
        };
        const wasEmpty = state.count === 0;
        state.items.push(item);
        state.count++;
        if (wasEmpty) {
            // First item: schedule alarm 10 seconds from now.
            // Equivalent to: await this.ctx.storage.setAlarm(Date.now() + 10_000)
            const fireAt = host.nowMs() + 10000;
            host.alarm.set(fireAt);
            host.info(`RequestQueue: first item queued, alarm set to fire in 10s at ${fireAt}`);
        }
        return {
            status: "ok",
            queued: state.count,
            item_id: item.id,
            alarm_set: wasEmpty,
        };
    }
    // Return current queue depth and next alarm timestamp.
    // Equivalent to Cloudflare DO: this.ctx.storage.getAlarm()
    onStatus(_payload) {
        const state = this.state;
        const alarmAt = host.alarm.get();
        return {
            status: "ok",
            count: state.count,
            alarm_at: alarmAt,
            alarm_set: alarmAt > 0,
            total_processed: state.total_processed,
            total_alarm_fires: state.total_alarm_fires,
        };
    }
    // Reset the queue (used for test repeatability).
    onReset(_payload) {
        const state = this.state;
        state.items = [];
        state.count = 0;
        host.alarm.delete();
        host.info("RequestQueue: queue reset");
        return { status: "ok", reset: true };
    }
    // Alarm fires when the scheduled timestamp is reached.
    // Equivalent to Cloudflare DO: async alarm() { ... }
    // The PlexSpaces reminder facet delivers this as "__alarm__" message type.
    on__alarm__(_payload) {
        const state = this.state;
        const processed = state.count;
        const items = [...state.items];
        host.info(`RequestQueue: alarm fired, processing ${processed} queued items`);
        for (const item of items) {
            host.info(`RequestQueue: processing item ${item.id}: ${JSON.stringify(item.data)}`);
        }
        // Clear the queue after processing
        state.items = [];
        state.count = 0;
        state.total_processed += processed;
        state.total_alarm_fires++;
        return {
            status: "ok",
            processed,
            total_processed: state.total_processed,
            total_alarm_fires: state.total_alarm_fires,
        };
    }
}
// ============================================================================
// Main — register actor for WASM export
// ============================================================================
const router = new ActorRouter({
    RequestQueueActor: () => new RequestQueueActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
