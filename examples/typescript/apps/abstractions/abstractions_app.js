// SPDX-License-Identifier: LGPL-2.1-or-later
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
const DEFAULT_GROUP = "abstractions-group";
function applicationIdFromActorId(actorId) {
    if (actorId.includes("//") && actorId.includes("::")) {
        const suffix = actorId.split("//", 2)[1];
        const qualified = suffix.split("@", 1)[0];
        const parts = qualified.split("::", 2);
        if (parts.length === 2) {
            return parts[1];
        }
    }
    if (actorId.includes(":") && actorId.includes("@")) {
        return actorId.split(":", 2)[1].split("@", 1)[0];
    }
    return "";
}
function canonicalActorTarget(target) {
    if (target.includes("@")) {
        return target;
    }
    const [actorType, actorName] = target.split(":", 2);
    const selfId = host.selfId();
    const namespace = applicationIdFromActorId(selfId);
    const nodeId = selfId.split("@", 2)[1];
    if (!actorType || !actorName || !namespace || !nodeId) {
        return target;
    }
    return `${actorName}//${actorType}::${namespace}@${nodeId}`;
}
class AbstractionsActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "",
            application_id: "",
            role: "abstractions",
            count: 0,
            workflow_status: "",
            workflow_signals: [],
            received: [],
            timer_ticks: 0,
            reminder_ticks: 0,
            joined_group: "",
            last_spawned_id: "",
        };
    }
    onInit(config) {
        const actorId = String(config.actor_id ?? "");
        const args = config.args ?? {};
        this.state = this.getDefaultState();
        this.state.actor_id = actorId;
        this.state.application_id = applicationIdFromActorId(actorId);
        this.state.role = String(args.role ?? "abstractions");
        this.state.count = Number(args.initial_count ?? 0);
        if (this.state.role === "channel") {
            const group = String(args.group ?? DEFAULT_GROUP);
            host.processGroups.join(group);
            this.state.joined_group = group;
        }
    }
    onIncrement(payload) {
        this.state.count += Number(payload.amount ?? 1);
        return { actor_id: this.state.actor_id, count: this.state.count };
    }
    onStatus() {
        return {
            actor_id: this.state.actor_id,
            application_id: this.state.application_id,
            count: this.state.count,
            joined_group: this.state.joined_group,
            last_spawned_id: this.state.last_spawned_id,
            received: [...this.state.received],
            reminder_ticks: this.state.reminder_ticks,
            role: this.state.role,
            self_id: host.selfId(),
            timer_ticks: this.state.timer_ticks,
            workflow_signals: [...this.state.workflow_signals],
            workflow_status: this.state.workflow_status,
        };
    }
    onSchedule_timer(payload) {
        return { timer_id: host.sendAfter(Number(payload.delay_ms ?? 100), "tick", { kind: "timer" }) };
    }
    onSchedule_reminder(payload) {
        return { reminder_id: host.sendAfter(Number(payload.delay_ms ?? 140), "reminder", { kind: "reminder" }) };
    }
    onTick() {
        this.state.timer_ticks += 1;
        return {};
    }
    onReminder() {
        this.state.reminder_ticks += 1;
        return {};
    }
    onKv_put(payload) {
        const key = String(payload.key ?? "");
        const value = String(payload.value ?? "");
        const result = host.kvPut(key, value);
        if (result.startsWith("ERROR:")) {
            return { error: `kv_put: ${result}` };
        }
        return { ok: true, key, value };
    }
    onKv_get(payload) {
        const key = String(payload.key ?? "");
        return { key, value: host.kvGet(key) };
    }
    onTs_write(payload) {
        const tuple = Array.isArray(payload.tuple) ? payload.tuple : [];
        const result = host.ts.write(tuple);
        if (result.startsWith("ERROR:")) {
            return { error: `ts_write: ${result}` };
        }
        return { ok: true, tuple };
    }
    onTs_read(payload) {
        const pattern = Array.isArray(payload.pattern) ? payload.pattern : [];
        return { tuple: host.ts.read(pattern) };
    }
    onBlob_upload(payload) {
        const blobId = String(payload.blob_id ?? "");
        const result = host.blobUpload(blobId, String(payload.data ?? ""), String(payload.content_type ?? "text/plain"));
        if (result.startsWith("ERROR:")) {
            return { error: `blob_upload: ${result}` };
        }
        return { ok: true, blob_id: blobId };
    }
    onBlob_download(payload) {
        const blobId = String(payload.blob_id ?? "");
        return { blob_id: blobId, data: host.blobDownload(blobId) };
    }
    onGroup_members(payload) {
        try {
            return { members: host.processGroups.members(String(payload.group ?? "")) };
        }
        catch (error) {
            return { error: `pg_members: ERROR: ${String(error)}` };
        }
    }
    onSend_event(payload) {
        const result = host.send(canonicalActorTarget(String(payload.target ?? "")), "publish", { channel: payload.channel, body: payload.body });
        if (result.startsWith("ERROR:")) {
            return { error: `send: ${result}` };
        }
        return { ok: true };
    }
    onBroadcast_event(payload) {
        try {
            host.processGroups.broadcast(String(payload.group ?? ""), "publish", {
                channel: payload.channel,
                body: payload.body,
            });
            return { ok: true };
        }
        catch (error) {
            return { error: `broadcast: ERROR: ${String(error)}` };
        }
    }
    onPublish(payload) {
        this.state.received.push(`${String(payload.channel ?? "")}:${String(payload.body ?? "")}`);
        return {};
    }
    onStop_actor(payload) {
        const actorId = String(payload.actor_id ?? "");
        try {
            host.stop(actorId);
            return { ok: true, actor_id: actorId };
        }
        catch (error) {
            return { error: `stop: ERROR: ${String(error)}` };
        }
    }
    run(payload) {
        const orderId = String(payload.order_id ?? "unknown");
        this.state.workflow_status = `running:${orderId}`;
        return { status: this.state.workflow_status };
    }
    signal(name, payload) {
        if (name === "cancel") {
            this.state.workflow_signals.push(`cancel:${String(payload.reason ?? "unknown")}`);
            this.state.workflow_status = "cancelled";
        }
    }
    query(name) {
        if (name !== "status") {
            return { error: `unknown query: ${name}` };
        }
        return { status: this.state.workflow_status, signals: [...this.state.workflow_signals] };
    }
}
const router = new ActorRouter({
    abstractions: () => new AbstractionsActor(),
    ephemeral: () => new AbstractionsActor(),
    workflow: () => new AbstractionsActor(),
    channel: () => new AbstractionsActor(),
    controller: () => new AbstractionsActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
