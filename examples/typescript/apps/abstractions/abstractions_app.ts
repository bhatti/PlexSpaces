// SPDX-License-Identifier: LGPL-2.1-or-later

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

type State = {
  actor_id: string;
  application_id: string;
  role: string;
  count: number;
  workflow_status: string;
  workflow_signals: string[];
  received: string[];
  timer_ticks: number;
  reminder_ticks: number;
  joined_group: string;
  last_spawned_id: string;
};

const DEFAULT_GROUP = "abstractions-group";

function applicationIdFromActorId(actorId: string): string {
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

function canonicalActorTarget(target: string): string {
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

class AbstractionsActor extends PlexSpacesActor<State> {
  getDefaultState(): State {
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

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
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

  onIncrement(payload: Record<string, unknown>): Record<string, unknown> {
    this.state.count += Number(payload.amount ?? 1);
    return { actor_id: this.state.actor_id, count: this.state.count };
  }

  onStatus(): Record<string, unknown> {
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

  onSchedule_timer(payload: Record<string, unknown>): Record<string, unknown> {
    return { timer_id: host.sendAfter(Number(payload.delay_ms ?? 100), "tick", { kind: "timer" }) };
  }

  onSchedule_reminder(payload: Record<string, unknown>): Record<string, unknown> {
    return { reminder_id: host.sendAfter(Number(payload.delay_ms ?? 140), "reminder", { kind: "reminder" }) };
  }

  onTick(): Record<string, unknown> {
    this.state.timer_ticks += 1;
    return {};
  }

  onReminder(): Record<string, unknown> {
    this.state.reminder_ticks += 1;
    return {};
  }

  onKv_put(payload: Record<string, unknown>): Record<string, unknown> {
    const key = String(payload.key ?? "");
    const value = String(payload.value ?? "");
    const result = host.kvPut(key, value);
    if (result.startsWith("ERROR:")) {
      return { error: `kv_put: ${result}` };
    }
    return { ok: true, key, value };
  }

  onKv_get(payload: Record<string, unknown>): Record<string, unknown> {
    const key = String(payload.key ?? "");
    return { key, value: host.kvGet(key) };
  }

  onTs_write(payload: Record<string, unknown>): Record<string, unknown> {
    const tuple = Array.isArray(payload.tuple) ? payload.tuple : [];
    const result = host.ts.write(tuple);
    if (result.startsWith("ERROR:")) {
      return { error: `ts_write: ${result}` };
    }
    return { ok: true, tuple };
  }

  onTs_read(payload: Record<string, unknown>): Record<string, unknown> {
    const pattern = Array.isArray(payload.pattern) ? payload.pattern : [];
    return { tuple: host.ts.read(pattern) };
  }

  onBlob_upload(payload: Record<string, unknown>): Record<string, unknown> {
    const blobId = String(payload.blob_id ?? "");
    const result = host.blobUpload(blobId, String(payload.data ?? ""), String(payload.content_type ?? "text/plain"));
    if (result.startsWith("ERROR:")) {
      return { error: `blob_upload: ${result}` };
    }
    return { ok: true, blob_id: blobId };
  }

  onBlob_download(payload: Record<string, unknown>): Record<string, unknown> {
    const blobId = String(payload.blob_id ?? "");
    return { blob_id: blobId, data: host.blobDownload(blobId) };
  }

  onGroup_members(payload: Record<string, unknown>): Record<string, unknown> {
    try {
      return { members: host.processGroups.members(String(payload.group ?? "")) };
    } catch (error) {
      return { error: `pg_members: ERROR: ${String(error)}` };
    }
  }

  onSend_event(payload: Record<string, unknown>): Record<string, unknown> {
    const result = host.send(
      canonicalActorTarget(String(payload.target ?? "")),
      "publish",
      { channel: payload.channel, body: payload.body },
    );
    if (result.startsWith("ERROR:")) {
      return { error: `send: ${result}` };
    }
    return { ok: true };
  }

  onBroadcast_event(payload: Record<string, unknown>): Record<string, unknown> {
    try {
      host.processGroups.broadcast(String(payload.group ?? ""), "publish", {
        channel: payload.channel,
        body: payload.body,
      });
      return { ok: true };
    } catch (error) {
      return { error: `broadcast: ERROR: ${String(error)}` };
    }
  }

  onPublish(payload: Record<string, unknown>): Record<string, unknown> {
    this.state.received.push(`${String(payload.channel ?? "")}:${String(payload.body ?? "")}`);
    return {};
  }

  onStop_actor(payload: Record<string, unknown>): Record<string, unknown> {
    const actorId = String(payload.actor_id ?? "");
    try {
      host.stop(actorId);
      return { ok: true, actor_id: actorId };
    } catch (error) {
      return { error: `stop: ERROR: ${String(error)}` };
    }
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const orderId = String(payload.order_id ?? "unknown");
    this.state.workflow_status = `running:${orderId}`;
    return { status: this.state.workflow_status };
  }

  signal(name: string, payload: Record<string, unknown>): void {
    if (name === "cancel") {
      this.state.workflow_signals.push(`cancel:${String(payload.reason ?? "unknown")}`);
      this.state.workflow_status = "cancelled";
    }
  }

  query(name: string): Record<string, unknown> {
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
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
