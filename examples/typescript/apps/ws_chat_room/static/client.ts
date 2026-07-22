// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Browser thin-node client for ws_chat_room.
// Supports group rooms (ChatRoomActor) and direct messages (DM).
// JWT is decoded client-side to pre-fill username/tenant.

import { ActorID, WsThinClient } from "@plexspaces/sdk";

// ─── JWT decode (no verification — server already validates) ──────────────────

function decodeJwtPayload(token: string): Record<string, unknown> | null {
  try {
    const part = token.split(".")[1];
    if (!part) return null;
    const padded = part.replace(/-/g, "+").replace(/_/g, "/");
    const json = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, "="));
    return JSON.parse(json) as Record<string, unknown>;
  } catch {
    return null;
  }
}

// ─── DOM refs ─────────────────────────────────────────────────────────────────

const overlay      = document.getElementById("overlay")!;
const connError    = document.getElementById("conn-error")!;
const connectBtn   = document.getElementById("connect-btn") as HTMLButtonElement;
const wsUrlInput   = document.getElementById("ws-url") as HTMLInputElement;
const jwtInput     = document.getElementById("jwt-token") as HTMLInputElement;
const usernameInput= document.getElementById("username") as HTMLInputElement;
const tenantInput  = document.getElementById("tenant") as HTMLInputElement;
const leaderNodeInput = document.getElementById("leader-node-id") as HTMLInputElement;
const myNameDisplay  = document.getElementById("my-name-display")!;
const myNodeDisplay  = document.getElementById("my-node-display")!;
const discBtn      = document.getElementById("disc-btn") as HTMLButtonElement;
const searchInput  = document.getElementById("search-input") as HTMLInputElement;
const groupsList   = document.getElementById("groups-list")!;
const usersList    = document.getElementById("users-list")!;
const newGroupBtn  = document.getElementById("new-group-btn") as HTMLButtonElement;
const chatArea     = document.getElementById("chat-area")!;
const noChat       = document.getElementById("no-chat")!;
const groupModal   = document.getElementById("group-modal")!;
const groupNameInput = document.getElementById("group-name-input") as HTMLInputElement;
const groupConfirm = document.getElementById("group-confirm") as HTMLButtonElement;
const groupCancel  = document.getElementById("group-cancel") as HTMLButtonElement;
const groupError   = document.getElementById("group-error")!;

// ─── State ────────────────────────────────────────────────────────────────────

let client: WsThinClient | null = null;
let myUsername  = "alice";
let myActorId   = "";
let leaderNodeId = "test-node-8091";
let appNs = "ts-ws-chat-room";

// Derive a stable node-id from the username so virtual actors work across refreshes.
// Format: "<username>.io" — deterministic, human-readable, no randomness needed.
function stableNodeId(username: string): string {
  return `${username}.io`;
}

// Conversations: groups and DMs
type MsgEntry = { sender: string; text: string; mine: boolean; time: string };
type Conversation =
  | { kind: "group"; id: string; name: string; actorId: string; members: string[]; messages: MsgEntry[]; joined: boolean }
  | { kind: "dm"; id: string; peer: string; peerActorId: string; messages: MsgEntry[] };

const conversations = new Map<string, Conversation>();
let activeConvId: string | null = null;

// Known users (received via presence updates)
const knownUsers = new Map<string, { online: boolean; actorId: string }>();

// ─── JWT auto-fill ────────────────────────────────────────────────────────────

jwtInput.addEventListener("input", () => {
  const token = jwtInput.value.trim();
  if (!token) return;
  const payload = decodeJwtPayload(token);
  if (!payload) return;
  if (payload["sub"] && typeof payload["sub"] === "string") {
    usernameInput.value = payload["sub"];
  }
  if (payload["tenant_id"] && typeof payload["tenant_id"] === "string") {
    tenantInput.value = payload["tenant_id"];
  }
});

// ─── Helpers ──────────────────────────────────────────────────────────────────

function showError(msg: string): void {
  connError.textContent = msg;
  connError.style.display = "block";
}

function hideError(): void {
  connError.style.display = "none";
}

function nowTime(): string {
  const d = new Date();
  return `${d.getHours().toString().padStart(2,"0")}:${d.getMinutes().toString().padStart(2,"0")}`;
}

function escHtml(s: string): string {
  return s.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");
}

function initials(name: string): string {
  return name.slice(0, 2).toUpperCase();
}

// ─── Connect ──────────────────────────────────────────────────────────────────

connectBtn.addEventListener("click", async () => {
  hideError();
  const wsUrl    = wsUrlInput.value.trim();
  const jwt      = jwtInput.value.trim() || undefined;
  myUsername     = usernameInput.value.trim() || "user";
  const tenant   = tenantInput.value.trim() || "default";
  leaderNodeId   = leaderNodeInput.value.trim() || "test-node-8091";

  if (!wsUrl) { showError("WebSocket URL is required"); return; }

  connectBtn.disabled = true;
  connectBtn.textContent = "Connecting…";

  try {
    // Use a stable node-id derived from username so the server routes messages to
    // the same virtual actor across page refreshes.  If the previous session is
    // still registered (TCP didn't close yet), wait briefly and retry once.
    const nodeId = stableNodeId(myUsername);
    const mkClient = () => new WsThinClient({
      wsUrl,
      jwtToken: jwt,
      nodeId,
      tenant,
      namespace: appNs,
    });

    client = mkClient();
    client.onMessage(handleIncomingMessage);

    let assignedNodeId: string;
    try {
      assignedNodeId = await client.connect();
    } catch (connErr) {
      // Server rejects duplicate node_id if old session not yet cleaned up.
      // Wait 1s for TCP close to propagate, then retry once.
      if ((connErr as Error).message.includes("already registered")) {
        await new Promise(r => setTimeout(r, 1000));
        client = mkClient();
        client.onMessage(handleIncomingMessage);
        assignedNodeId = await client.connect();
      } else {
        throw connErr;
      }
    }

    myActorId = client.localActorId(myUsername, "ChatClient", appNs);
    myNameDisplay.textContent = myUsername;
    myNodeDisplay.textContent = `node: ${assignedNodeId}`;

    overlay.classList.add("hidden");

    // Announce ourselves online
    announcePresence();

    // Auto-join default lobby
    joinGroup("lobby");

  } catch (err) {
    showError(`Connection failed: ${(err as Error).message}`);
    connectBtn.disabled = false;
    connectBtn.textContent = "Connect";
    client = null;
  }
});

// ─── Presence ─────────────────────────────────────────────────────────────────

function announcePresence(): void {
  if (!client) return;
  const presenceId = new ActorID(myUsername, "PresenceActor", appNs, leaderNodeId).toString();
  client.tell(presenceId, "online", { actor_id: myActorId }).catch(() => {});
}

// ─── Handle incoming messages ─────────────────────────────────────────────────

function handleIncomingMessage(from: string, msgType: string, payload: unknown): void {
  const p = payload as Record<string, unknown>;

  if (msgType === "chat_message") {
    // Group message — server sends to ALL members including sender
    const roomId = (p["room_id"] as string | undefined) ?? "";
    const senderActorId = (p["sender"] as string | undefined) ?? from;
    const text = (p["text"] as string | undefined) ?? "";
    // Prefer server-resolved username, fall back to parsing the actor_id
    let senderName = (p["sender_username"] as string | undefined) ?? senderActorId;
    if (senderName === senderActorId) {
      try { senderName = ActorID.parse(senderActorId).name; } catch {}
    }
    const convId = `group:${roomId}`;
    let conv = conversations.get(convId);
    if (!conv) {
      conv = makeGroupConv(roomId);
      conversations.set(convId, conv);
      renderSidebar();
    }
    // Deduplicate: sender adds message locally on send, server echoes it back.
    // Only skip dedup for our own messages that were already added locally.
    const isDup = senderName === myUsername &&
      conv.messages.length > 0 &&
      conv.messages[conv.messages.length - 1]!.text === text &&
      conv.messages[conv.messages.length - 1]!.sender === myUsername;
    if (!isDup) {
      const entry: MsgEntry = { sender: senderName, text, mine: senderName === myUsername, time: nowTime() };
      conv.messages.push(entry);
      if (activeConvId === convId) appendMsgToPane(entry);
      else bumpBadge(convId);
    }
    return;
  }

  if (msgType === "dm_message") {
    // DM message
    const senderActorId = (p["sender"] as string | undefined) ?? from;
    const text = (p["text"] as string | undefined) ?? "";
    let senderName = senderActorId;
    try { senderName = ActorID.parse(senderActorId).name; } catch {}
    const peer = senderName === myUsername ? ((p["to"] as string | undefined) ?? "") : senderName;
    const convId = dmConvId(peer);
    let conv = conversations.get(convId);
    if (!conv) {
      conv = makeDmConv(peer, senderActorId);
      conversations.set(convId, conv);
      renderSidebar();
    }
    const entry: MsgEntry = { sender: senderName, text, mine: senderName === myUsername, time: nowTime() };
    conv.messages.push(entry);
    if (activeConvId === convId) appendMsgToPane(entry);
    else bumpBadge(convId);
    return;
  }

  if (msgType === "presence_update") {
    const username = (p["username"] as string | undefined) ?? "";
    const online   = (p["online"] as boolean | undefined) ?? false;
    const actorId  = (p["actor_id"] as string | undefined) ?? "";
    if (username && username !== myUsername) {
      knownUsers.set(username, { online, actorId });
      renderSidebar();
    }
    return;
  }

  if (msgType === "member_joined" || msgType === "member_left") {
    const roomId    = (p["room_id"] as string | undefined) ?? "";
    const members   = (p["members"] as string[] | undefined) ?? [];
    const memberInfo = (p["member_info"] as Record<string, string> | undefined) ?? {};
    const convId = `group:${roomId}`;
    const conv = conversations.get(convId);
    if (conv && conv.kind === "group") {
      conv.members = members;
      // Populate knownUsers from the authoritative member_info map
      for (const [actorId, uname] of Object.entries(memberInfo)) {
        if (uname !== myUsername) {
          knownUsers.set(uname, { online: true, actorId });
        }
      }
      renderSidebar();
      if (activeConvId === convId) updateChatHeader();
    }
  }
}

// ─── Conversation factories ────────────────────────────────────────────────────

function makeGroupConv(roomName: string): Conversation & { kind: "group" } {
  return {
    kind: "group",
    id: `group:${roomName}`,
    name: roomName,
    actorId: new ActorID(roomName, "ChatRoomActor", appNs, leaderNodeId).toString(),
    members: [],
    messages: [],
    joined: false,
  };
}

function makeDmConv(peer: string, peerActorId: string): Conversation & { kind: "dm" } {
  return { kind: "dm", id: dmConvId(peer), peer, peerActorId, messages: [] };
}

function dmConvId(peer: string): string { return `dm:${peer}`; }

// ─── Join group ───────────────────────────────────────────────────────────────

async function joinGroup(roomName: string): Promise<void> {
  if (!client) return;
  const convId = `group:${roomName}`;
  let conv = conversations.get(convId) as (Conversation & { kind: "group" }) | undefined;
  if (!conv) {
    const c = makeGroupConv(roomName);
    conversations.set(convId, c);
    conv = c;
  }

  try {
    const resp = await client.ask(conv.actorId, "join", { actor_id: myActorId, username: myUsername }, 10_000) as {
      members?: string[];
      member_info?: Record<string, string>;
      history?: Array<{ senderActorId: string; sender: string; text: string; ts: number }>;
      error?: string;
    };
    if (resp.error) throw new Error(resp.error);
    conv.joined = true;
    conv.members = resp.members ?? [];

    // Populate knownUsers from authoritative member_info
    const memberInfo = resp.member_info ?? {};
    for (const [actorId, uname] of Object.entries(memberInfo)) {
      if (uname !== myUsername) {
        knownUsers.set(uname, { online: true, actorId });
      }
    }

    // Replay server-side history (replaces any local messages from before join)
    conv.messages = [];
    for (const h of (resp.history ?? [])) {
      const senderName = h.sender ?? h.senderActorId;
      conv.messages.push({ sender: senderName, text: h.text, mine: senderName === myUsername, time: nowTime() });
    }
    conv.messages.push({ sender: "system", text: `You joined #${roomName}`, mine: true, time: nowTime() });

    renderSidebar();
    selectConversation(convId);
  } catch (err) {
    showGroupError(`Failed to join ${roomName}: ${(err as Error).message}`);
  }
}

function showGroupError(msg: string): void {
  groupError.textContent = msg;
  groupError.style.display = "block";
}

// ─── New group modal ───────────────────────────────────────────────────────────

newGroupBtn.addEventListener("click", () => {
  groupError.style.display = "none";
  groupNameInput.value = "";
  groupModal.classList.remove("hidden");
  groupNameInput.focus();
});

groupCancel.addEventListener("click", () => groupModal.classList.add("hidden"));

groupConfirm.addEventListener("click", async () => {
  const name = groupNameInput.value.trim();
  if (!name) { showGroupError("Group name required"); return; }
  groupModal.classList.add("hidden");
  await joinGroup(name);
});

groupNameInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") groupConfirm.click();
});

// ─── Select conversation ──────────────────────────────────────────────────────

function selectConversation(convId: string): void {
  activeConvId = convId;
  renderChatArea();
  renderSidebar();
}

// ─── Sidebar rendering ─────────────────────────────────────────────────────────

function renderSidebar(): void {
  const q = searchInput.value.toLowerCase();

  // Groups
  groupsList.innerHTML = "";
  for (const [id, conv] of conversations) {
    if (conv.kind !== "group") continue;
    if (q && !conv.name.toLowerCase().includes(q)) continue;
    const el = document.createElement("div");
    el.className = `conv-item${id === activeConvId ? " active" : ""}`;
    el.innerHTML = `
      <div class="avatar av-group">#</div>
      <div class="conv-info">
        <div class="conv-name">${escHtml(conv.name)}</div>
        <div class="conv-sub">${conv.members.length} member${conv.members.length !== 1 ? "s" : ""}${conv.joined ? "" : " · not joined"}</div>
      </div>`;
    el.addEventListener("click", () => {
      if (!conv.joined) joinGroup(conv.name);
      else selectConversation(id);
    });
    groupsList.appendChild(el);
  }

  // Users — collect from group member lists + presence updates
  usersList.innerHTML = "";
  const allUsers = new Set<string>();
  for (const name of knownUsers.keys()) allUsers.add(name);
  for (const [, conv] of conversations) {
    if (conv.kind === "group") {
      for (const m of conv.members) {
        try { const n = ActorID.parse(m).name; if (n !== myUsername) allUsers.add(n); } catch {}
      }
    }
  }

  for (const uname of Array.from(allUsers).sort()) {
    if (q && !uname.toLowerCase().includes(q)) continue;
    const info = knownUsers.get(uname);
    const dmId = dmConvId(uname);
    const el = document.createElement("div");
    el.className = `conv-item${dmId === activeConvId ? " active" : ""}`;
    el.innerHTML = `
      <div class="avatar av-user">${escHtml(initials(uname))}</div>
      <div class="conv-info">
        <div class="conv-name">${escHtml(uname)}</div>
        <div class="conv-sub">${info?.online ? "Online" : "Offline"}</div>
      </div>
      <div class="${info?.online ? "online-dot" : "offline-dot"}"></div>`;
    el.addEventListener("click", () => openDm(uname));
    usersList.appendChild(el);
  }
}

searchInput.addEventListener("input", renderSidebar);

// ─── Open DM ──────────────────────────────────────────────────────────────────

function openDm(peer: string): void {
  const convId = dmConvId(peer);
  if (!conversations.has(convId)) {
    // Derive deterministic actor_id using the stable node convention: peer.io
    const info = knownUsers.get(peer);
    const peerActorId = info?.actorId ?? new ActorID(peer, "ChatClient", appNs, stableNodeId(peer)).toString();
    conversations.set(convId, makeDmConv(peer, peerActorId));
  }
  selectConversation(convId);
}

// ─── Render chat area ─────────────────────────────────────────────────────────

function renderChatArea(): void {
  if (!activeConvId) { chatArea.innerHTML = ""; chatArea.appendChild(noChat); return; }
  const conv = conversations.get(activeConvId);
  if (!conv) return;

  chatArea.innerHTML = `
    <div id="chat-header" style="background:#075e54;color:white;padding:10px 16px;display:flex;align-items:flex-start;gap:12px;flex-shrink:0">
      <div class="ch-avatar">${conv.kind === "group" ? "#" : escHtml(initials(conv.kind === "dm" ? conv.peer : conv.name))}</div>
      <div style="flex:1;min-width:0">
        <div style="font-size:15px;font-weight:600">${conv.kind === "group" ? "#" + escHtml(conv.name) : escHtml(conv.peer)}</div>
        <div style="font-size:11px;opacity:.8;margin-top:2px" id="chat-sub"></div>
        ${conv.kind === "group" ? `<div id="members-chips" style="display:flex;flex-wrap:wrap;gap:4px;margin-top:5px"></div>` : ""}
      </div>
      ${conv.kind === "group" && !conv.joined
        ? `<button onclick="window.__joinGroup && window.__joinGroup('${escHtml(conv.name)}')" style="background:rgba(255,255,255,.2);border:none;color:white;border-radius:5px;padding:4px 10px;font-size:12px;cursor:pointer">Join</button>`
        : ""}
    </div>
    <div id="messages" style="flex:1;overflow-y:auto;padding:12px 16px;display:flex;flex-direction:column;gap:6px;background:#e5ddd5"></div>
    <div id="input-row" style="background:white;padding:8px 12px;display:flex;gap:8px;border-top:1px solid #ddd;flex-shrink:0">
      <input id="msg-input" type="text" placeholder="Type a message…" style="flex:1;border:1px solid #ccc;border-radius:20px;padding:8px 14px;font-size:14px;outline:none" ${client ? "" : "disabled"} />
      <button id="send-btn" ${client ? "" : "disabled"} style="background:#075e54;color:white;border:none;border-radius:50%;width:36px;height:36px;font-size:17px;cursor:pointer;display:flex;align-items:center;justify-content:center;padding:0;flex-shrink:0">➤</button>
    </div>`;

  // Re-bind refs after innerHTML replacement
  const msgsDiv = chatArea.querySelector("#messages")!;
  const newMsgInput = chatArea.querySelector("#msg-input") as HTMLInputElement;
  const newSendBtn  = chatArea.querySelector("#send-btn") as HTMLButtonElement;

  // Replay messages
  for (const entry of conv.messages) {
    msgsDiv.appendChild(buildMsgEl(entry));
  }
  msgsDiv.scrollTop = msgsDiv.scrollHeight;

  updateChatHeader();

  newSendBtn.addEventListener("click", () => sendCurrentMessage(newMsgInput));
  newMsgInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendCurrentMessage(newMsgInput); }
  });
  newMsgInput.focus();

  // expose joinGroup for inline button
  (window as unknown as Record<string, unknown>)["__joinGroup"] = (n: string) => joinGroup(n);
}

function updateChatHeader(): void {
  if (!activeConvId) return;
  const conv = conversations.get(activeConvId);
  if (!conv) return;
  const sub = chatArea.querySelector("#chat-sub");
  const chips = chatArea.querySelector("#members-chips");
  if (conv.kind === "group") {
    if (sub) sub.textContent = `${conv.members.length} member${conv.members.length !== 1 ? "s" : ""}`;
    if (chips) {
      chips.innerHTML = "";
      for (const m of conv.members) {
        let n = m;
        try { n = ActorID.parse(m).name; } catch {}
        const sp = document.createElement("span");
        sp.className = "chip";
        sp.textContent = n;
        sp.style.cssText = "background:rgba(255,255,255,.2);border-radius:10px;padding:2px 8px;font-size:11px;cursor:pointer";
        sp.title = "Open DM";
        sp.addEventListener("click", () => openDm(n));
        chips.appendChild(sp);
      }
    }
  } else {
    const info = knownUsers.get(conv.peer);
    if (sub) sub.textContent = info?.online ? "Online" : "Offline";
  }
}

// ─── Append single message ─────────────────────────────────────────────────────

function buildMsgEl(entry: MsgEntry): HTMLElement {
  const el = document.createElement("div");
  if (entry.sender === "system") {
    el.className = "msg system";
    el.textContent = entry.text;
    el.style.cssText = "align-self:center;background:rgba(255,255,255,.7);color:#666;font-size:11px;border-radius:12px;padding:3px 12px";
    return el;
  }
  el.className = `msg ${entry.mine ? "mine" : "theirs"}`;
  el.style.cssText = `max-width:68%;padding:7px 11px;border-radius:8px;font-size:14px;line-height:1.4;word-break:break-word;${entry.mine ? "align-self:flex-end;background:#dcf8c6;border-bottom-right-radius:2px" : "align-self:flex-start;background:white;border-bottom-left-radius:2px;box-shadow:0 1px 2px rgba(0,0,0,.1)"}`;
  el.innerHTML = `
    ${!entry.mine ? `<div style="font-size:11px;font-weight:600;color:#075e54;margin-bottom:2px">${escHtml(entry.sender)}</div>` : ""}
    <div>${escHtml(entry.text)}</div>
    <div style="font-size:10px;color:#aaa;text-align:right;margin-top:2px">${entry.time}</div>`;
  return el;
}

function appendMsgToPane(entry: MsgEntry): void {
  const msgsDiv = chatArea.querySelector("#messages");
  if (!msgsDiv) return;
  msgsDiv.appendChild(buildMsgEl(entry));
  msgsDiv.scrollTop = msgsDiv.scrollHeight;
}

function bumpBadge(_convId: string): void {
  // Could show unread count — for now just re-render sidebar to reflect last message
  renderSidebar();
}

// ─── Send message ──────────────────────────────────────────────────────────────

function sendCurrentMessage(input: HTMLInputElement): void {
  const text = input.value.trim();
  if (!text || !client || !activeConvId) return;
  input.value = "";

  const conv = conversations.get(activeConvId);
  if (!conv) return;

  if (conv.kind === "group") {
    // Don't add locally — server echoes back to all members including sender.
    // The handleIncomingMessage dedup handles the echo so it's shown exactly once.
    client.tell(conv.actorId, "send", { sender_actor_id: myActorId, text }).catch((err: Error) => {
      appendMsgToPane({ sender: "system", text: `Send failed: ${err.message}`, mine: false, time: nowTime() });
    });
  } else {
    // DM: tell the peer's ChatClient actor directly; add locally since no server echo
    const dmEntry: MsgEntry = { sender: myUsername, text, mine: true, time: nowTime() };
    conv.messages.push(dmEntry);
    appendMsgToPane(dmEntry);
    client.tell(conv.peerActorId, "dm_message", { sender: myActorId, text, to: conv.peer }).catch((err: Error) => {
      appendMsgToPane({ sender: "system", text: `DM failed: ${err.message}`, mine: false, time: nowTime() });
    });
  }
}


// ─── Disconnect ───────────────────────────────────────────────────────────────

discBtn.addEventListener("click", async () => {
  if (!client) return;
  // Leave all joined groups
  for (const [, conv] of conversations) {
    if (conv.kind === "group" && conv.joined) {
      client.ask(conv.actorId, "leave", { actor_id: myActorId }, 3_000).catch(() => {});
    }
  }
  await client.disconnect();
  client = null;
  conversations.clear();
  knownUsers.clear();
  activeConvId = null;
  chatArea.innerHTML = "";
  chatArea.appendChild(noChat);
  overlay.classList.remove("hidden");
  connectBtn.disabled = false;
  connectBtn.textContent = "Connect";
  hideError();
});
