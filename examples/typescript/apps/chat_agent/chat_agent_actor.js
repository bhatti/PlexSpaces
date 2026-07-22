// SPDX-License-Identifier: AGPL-3.0-or-later
//
// ChatAgentActor — Cloudflare Agents SDK equivalent (TypeScript WASM)
//
// Demonstrates conversation state in KV, LLM calls via httpClient service link,
// and durable alarm for periodic summarization.
//
// ## Cloudflare Agents SDK vs PlexSpaces TypeScript
//
// | Cloudflare Agents SDK              | PlexSpaces TypeScript                        |
// |------------------------------------|----------------------------------------------|
// | this.env.AI.run(model, {messages}) | host.httpClient("llm-link").fetch(...)       |
// | this.storage.get/put               | host.kv.get() / host.kv.put()               |
// | storage.setAlarm(ts)               | host.alarm.set(ts)                           |
// | onAlarm() callback                 | on__alarm__() handler (reminder facet)       |
// | connection.send(reply)             | return reply (sync response)                 |
// | Agent.schedule(cron, ...)          | host.alarm.set(nextRunMs) inside __alarm__   |
// | env.AI binding                     | service_links.llm-link in app-config.toml   |
// | Durable Object per-agent storage   | virtual_actor + reminder facets              |
//
// NOTE: LLM calls require ANTHROPIC_API_KEY (or equivalent) configured
// in service_links. test.sh validates state and alarm logic only.
import { PlexSpacesActor, host } from "@plexspaces/sdk";
// ========================================================================
// ChatAgentActor
// ========================================================================
class ChatAgentActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            agent_id: "",
            total_messages: 0,
            total_summarizations: 0,
        };
    }
    onInit(config) {
        this.state.agent_id = String(config.actor_id ?? "");
        host.info(`ChatAgentActor init: agent_id=${this.state.agent_id}`);
    }
    // ---------- chat handler ----------
    // Equivalent to Cloudflare Agents SDK onMessage():
    //   1. Load conversation history from KV (this.storage.get)
    //   2. Append user message
    //   3. Call LLM via service link (this.env.AI.run)
    //   4. Append assistant reply, persist to KV (this.storage.put)
    //   5. Schedule summarization alarm after threshold (storage.setAlarm)
    onChat(payload) {
        const message = String(payload.message ?? "").trim();
        if (!message) {
            return { error: "message is required" };
        }
        // Load history from KV — equivalent to: await this.storage.get('history')
        let history = [];
        try {
            const raw = host.kv.get("history");
            if (raw) {
                history = JSON.parse(raw);
            }
        }
        catch {
            history = [];
        }
        const now = host.nowMs();
        history.push({ role: "user", content: message, timestamp: now });
        // Call LLM via service link — equivalent to: await this.env.AI.run(model, {messages})
        let assistantReply = "";
        try {
            const llmBody = JSON.stringify({
                model: "claude-3-5-haiku-20241022",
                max_tokens: 1024,
                messages: history.map((m) => ({ role: m.role, content: m.content })),
            });
            const resp = host.httpClient("llm-link").post("/v1/messages", JSON.parse(llmBody));
            // Parse Anthropic response: content[0].text
            const body = JSON.parse(resp.body);
            assistantReply =
                body?.content?.[0]?.text ??
                    body?.choices?.[0]?.message?.content ??
                    "[no response]";
        }
        catch (e) {
            // LLM unavailable — store user message, return placeholder
            assistantReply = "[LLM unavailable — message stored]";
            host.warn(`ChatAgentActor: LLM call failed: ${e}`);
        }
        history.push({ role: "assistant", content: assistantReply, timestamp: host.nowMs() });
        // Persist history — equivalent to: await this.storage.put('history', history)
        host.kv.put("history", JSON.stringify(history));
        this.state.total_messages++;
        // Schedule summarization alarm after threshold — equivalent to: storage.setAlarm(ts)
        if (history.length > ChatAgentActor.ALARM_THRESHOLD) {
            const alarmAt = host.alarm.get();
            if (alarmAt === 0) {
                host.alarm.set(host.nowMs() + ChatAgentActor.ALARM_DELAY_MS);
                host.info(`ChatAgentActor: alarm set for summarization in 5 minutes`);
            }
        }
        return {
            status: "ok",
            reply: assistantReply,
            history_length: history.length,
        };
    }
    // ---------- get_history handler ----------
    onGet_history(_payload) {
        let history = [];
        try {
            const raw = host.kv.get("history");
            if (raw) {
                history = JSON.parse(raw);
            }
        }
        catch {
            history = [];
        }
        return {
            status: "ok",
            history,
            count: history.length,
        };
    }
    // ---------- clear handler ----------
    // Equivalent to: await this.storage.delete('history'); await this.state.storage.deleteAlarm()
    onClear(_payload) {
        host.kv.delete("history");
        host.kv.delete("summary");
        host.alarm.delete();
        return { status: "ok", cleared: true };
    }
    // ---------- __alarm__ handler ----------
    // Durable alarm callback — equivalent to Cloudflare Agents SDK onAlarm().
    // Dispatched by the PlexSpaces reminder facet when the scheduled timestamp fires.
    // Summarizes conversation history and stores it, then clears history.
    on__alarm__(_payload) {
        host.info(`ChatAgentActor: alarm fired — summarizing history`);
        let history = [];
        try {
            const raw = host.kv.get("history");
            if (raw) {
                history = JSON.parse(raw);
            }
        }
        catch {
            history = [];
        }
        if (history.length === 0) {
            return { status: "ok", action: "no_history_to_summarize" };
        }
        // Summarize via LLM — equivalent to: await this.env.AI.run(model, {messages: [...]})
        let summary = "";
        try {
            const summaryPrompt = `Summarize this conversation concisely (2-3 sentences): ${JSON.stringify(history.map((m) => ({ role: m.role, content: m.content })))}`;
            const llmBody = JSON.stringify({
                model: "claude-3-5-haiku-20241022",
                max_tokens: 256,
                messages: [{ role: "user", content: summaryPrompt }],
            });
            const resp = host.httpClient("llm-link").post("/v1/messages", JSON.parse(llmBody));
            const body = JSON.parse(resp.body);
            summary =
                body?.content?.[0]?.text ??
                    body?.choices?.[0]?.message?.content ??
                    "[summary unavailable]";
        }
        catch {
            summary = `[${history.length} messages — LLM unavailable for summary]`;
        }
        // Store summary, clear history — equivalent to: this.storage.put('summary', s); this.storage.delete('history')
        host.kv.put("summary", summary);
        host.kv.delete("history");
        this.state.total_summarizations++;
        host.info(`ChatAgentActor: summarized ${history.length} messages`);
        return {
            status: "ok",
            action: "summarized",
            messages_summarized: history.length,
        };
    }
}
// How many messages before we set the summarization alarm.
ChatAgentActor.ALARM_THRESHOLD = 10;
// Summarization alarm delay: 5 minutes in milliseconds.
ChatAgentActor.ALARM_DELAY_MS = 300000;
// ========================================================================
// WASM export
// ========================================================================
const _actor = new ChatAgentActor();
export const actor = {
    init: (configJson) => _actor.init(configJson),
    handle: (from, msgType, payloadJson) => _actor.handle(from, msgType, payloadJson),
    getState: () => _actor.getState(),
    setState: (stateJson) => _actor.setState(stateJson),
};
