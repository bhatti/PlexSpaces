// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Actor base class (TypeScript SDK)
//
// Use inheritance to define actors with minimal boilerplate. Override
// getDefaultState() and onOpName(payload) methods; the base class handles
// init/handle/getState/setState and dispatch by payload.op.

/**
 * WIT host log function (imported from plexspaces:actor/host).
 * 
 * jco componentize uses virtual imports for WIT host interfaces.
 * Pattern: import { functionName } from 'namespace:package/interface@version'
 * 
 * The import is provided by the runtime when the component is instantiated.
 * In non-WASM environments (Node.js verification), this import will be undefined.
 * 
 * Type definitions are generated in src/generated/ but are not imported here
 * to keep the SDK simple. The virtual import works at runtime without types.
 */
// @ts-ignore - Virtual import provided by jco componentize at runtime
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-expect-error - Virtual import - types are optional (generated in src/generated/)
import { log as hostLog } from 'plexspaces:actor/host@0.1.0';
import { getActorDefinition } from './decorators.js';
import { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from './wit-payload.js';

/**
 * Log levels matching the WIT host implementation
 */
type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

/**
 * Safe logging helper that uses WIT host.log function.
 * 
 * Logging must never throw - if host.log is unavailable or fails,
 * we silently ignore (critical for WASM safety).
 * 
 * @param level Log level (trace, debug, info, warn, error)
 * @param location Code location (file:line)
 * @param message Log message
 * @param extra Optional extra string (already serialized, no recursion)
 */
function actorLog(level: LogLevel, location: string, message: string, extra?: string): void {
  try {
    // Call host log function if available (provided by jco componentize/runtime)
    if (typeof hostLog === 'function') {
      const entry = extra ? `[${location}] ${message} ${extra}` : `[${location}] ${message}`;
      // Call WIT host log function (virtual import provided by jco componentize)
      hostLog(level, entry);
    }
    // If host.log is not available, silently ignore (no-op)
    // This allows the code to work in non-WASM environments (e.g., Node.js verification)
  } catch {
    // Logging must never throw - swallow all errors silently
  }
}

/**
 * Base class for PlexSpaces actors (actor-world WIT: init, handle, get-state, set-state).
 *
 * Subclass and override:
 * - getDefaultState(): initial state
 * - onInit(config): optional, called from init() with parsed config
 * - on<Op>(payload): handler for message op (e.g. onDeposit, onBalance).
 *   Op is derived from payload.op or payload; method name is "on" + capitalized op.
 *
 * Example:
 *   class BankAccountActor extends PlexSpacesActor<BankAccountState> {
 *     getDefaultState() { return { account_id: '', balance: 0, transactions: [] }; }
 *     onDeposit(p: { amount: number }) { ...; return { status: 'ok', balance: this.state.balance }; }
 *     onBalance() { return { account: this.state.account_id, balance: this.state.balance }; }
 *   }
 */
export abstract class PlexSpacesActor<TState extends object = Record<string, unknown>> {
  protected state: TState;
  // Cache serialized state to avoid WASM traps when getState() is called after handle()
  private cachedStateJson: string | null = null;

  constructor() {
    this.state = this.getDefaultState();
    // Don't serialize state in constructor — defer to getState()
    // The framework re-instantiates after every handle(), so caching here is wasted
    this.cachedStateJson = null;
  }

  /** Return initial state. Override in subclass. */
  abstract getDefaultState(): TState;

  /** Optional: called from init() with parsed config. Override to apply config to state. */
  protected onInit(_config: Record<string, unknown>): void {
    // default: no-op; subclass can set state from config
  }

  /**
   * WIT `init(config: payload) -> result<_, actor-error>`.
   * Success: return (unit). Failure: throw (jco maps throws to `err` for function-return `result`).
   */
  init(configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView): void {
    try {
      const text = decodeWitPayloadUtf8(configJson);
      const config = text.trim() ? (JSON.parse(text) as Record<string, unknown>) : {};
      this.onInit(config);
      // Cache state after init (state is typically small/flat here)
      this.cachedStateJson = null; // Invalidate; lazy-serialize in getState()
    } catch {
      throw new Error('ERROR:init failed');
    }
  }

  /**
   * WIT `handle(...) -> result<payload, actor-error>` (`payload` is `list<u8>` → `Uint8Array` in jco).
   * Dispatches by msgType for Workflow behavior (workflow_run, workflow_signal:name, workflow_query:name),
   * then by payload.op (or payload) to on<Op>(payload). Returns UTF-8 JSON bytes.
   * Uses iterative serializer to avoid WASM recursion.
   *
   * Workflow behavior (aligned with Rust Workflow trait and Python @workflow_actor):
   * - msgType "workflow_run" -> run(payload)
   * - msgType "workflow_signal:name" -> signal(name, payload)
   * - msgType "workflow_query:name" -> query(name, payload)
   */
  handle(
    _fromActor: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView,
  ): Uint8Array {
    try {
      const text = decodeWitPayloadUtf8(payloadJson);
      const payload = text.trim() ? (JSON.parse(text) as Record<string, unknown>) : {};
      const definition = getActorDefinition(this);
      // Workflow behavior: route by msgType when actor implements run/signal/query (aligned with crates/behavior Workflow trait)
      if (msgType === "workflow_run") {
        const runMethod = definition?.runHandler;
        const runFn = runMethod
          ? (this as unknown as Record<string, (p: Record<string, unknown>) => unknown>)[runMethod]
          : (this as unknown as Record<string, (p: Record<string, unknown>) => unknown>).run;
        if (typeof runFn === "function") {
          const result = runFn.call(this, payload);
          this.cachedStateJson = null;
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        }
      }
      if (msgType.startsWith("workflow_signal:")) {
        const name = msgType.slice("workflow_signal:".length).trim();
        const signalMethod = definition?.signalHandlers?.[name];
        const signalFn = signalMethod
          ? (this as unknown as Record<string, Function>)[signalMethod]
          : (this as unknown as Record<string, Function>).signal;
        if (typeof signalFn === "function") {
          if (signalMethod) {
            (signalFn as (payload: Record<string, unknown>) => void).call(this, payload);
          } else {
            (signalFn as (name: string, payload: Record<string, unknown>) => void).call(this, name, payload);
          }
          this.cachedStateJson = null;
          return encodeWitPayloadUtf8('{}');
        }
      }
      if (msgType.startsWith("workflow_query:")) {
        const name = msgType.slice("workflow_query:".length).trim();
        const queryMethod = definition?.queryHandlers?.[name];
        const queryFn = queryMethod
          ? (this as unknown as Record<string, Function>)[queryMethod]
          : (this as unknown as Record<string, Function>).query;
        if (typeof queryFn === "function") {
          const result = queryMethod
            ? (queryFn as (payload: Record<string, unknown>) => unknown).call(this, payload)
            : (queryFn as (name: string, payload: Record<string, unknown>) => unknown).call(this, name, payload);
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        }
      }

      // Payload key order: message_type -> op -> msg_type; fallback to msgType so data-only payloads route by message type (e.g. tasks_ready)
      const opRaw = payload.message_type ?? payload.op ?? payload.msg_type;
      const op = (typeof opRaw === "string" && opRaw ? opRaw : msgType) as string;
      const decoratedMethod = this.resolveDecoratedHandler(op, definition);
      const opKey = typeof op === "string" ? this.capitalize(op) : "";
      const methodName = decoratedMethod ?? (opKey ? `on${opKey}` : "");
      const method = methodName && typeof (this as unknown as Record<string, unknown>)[methodName] === "function"
        ? (this as unknown as Record<string, (p: Record<string, unknown>) => unknown>)[methodName]
        : null;
      if (method) {
        let result: unknown;
        try {
          result = method.call(this, payload);
        } catch (handlerError) {
          const errorMsg = handlerError instanceof Error ? handlerError.message : String(handlerError);
          actorLog('error', 'actor.ts:handle', `Handler ${methodName} failed`, errorMsg);
          throw new Error('ERROR:' + errorMsg);
        }

        // Do not cache state here. The framework re-instantiates after every handle()
        // (wasmtime#8943 workaround), so caching is wasted work that risks stack overflow.
        // State will be serialized lazily in getState() if needed.
        this.cachedStateJson = null;

        // Use iterative serializer to avoid WASM recursion.
        // The iterative serializer uses an explicit work stack instead of recursive function calls.
        try {
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        } catch (jsonError) {
          const errorMsg = jsonError instanceof Error ? jsonError.message : String(jsonError);
          actorLog('error', 'actor.ts:handle', 'JSON serialization failed', errorMsg);
          throw new Error('ERROR:JSON serialization failed: ' + errorMsg);
        }
      }
      actorLog('warn', 'actor.ts:handle', 'Unknown operation', String(op));
      return encodeWitPayloadUtf8(iterativeStringify({ error: 'unknown_op', op: String(op) }));
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      actorLog('error', 'actor.ts:handle', 'Handle failed', errorMsg);
      if (e instanceof Error && errorMsg.startsWith('ERROR:')) {
        throw e;
      }
      throw new Error('ERROR:' + errorMsg);
    }
  }

  /** WIT `get-state() -> result<payload, actor-error>`. Returns JSON state as UTF-8 bytes. */
  getState(): Uint8Array {
    if (this.cachedStateJson !== null) {
      return encodeWitPayloadUtf8(this.cachedStateJson);
    }
    try {
      const serialized = iterativeStringify(this.state);
      this.cachedStateJson = serialized;
      return encodeWitPayloadUtf8(serialized);
    } catch {
      return encodeWitPayloadUtf8('{}');
    }
  }

  /** WIT `set-state(state: payload) -> result<_, actor-error>`. */
  setState(stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView): void {
    try {
      const text = decodeWitPayloadUtf8(stateJson);
      if (text.trim()) {
        this.state = JSON.parse(text) as TState;
        this.cachedStateJson = null; // Invalidate
      }
    } catch {
      throw new Error('ERROR:set_state failed');
    }
  }

  protected capitalize(s: string): string {
    if (!s) return "";
    // Handle snake_case: "load_model" -> "Load_model" (preserve underscores, only capitalize first char)
    // This matches handler naming: onLoad_model, onPredict_batch, etc.
    // Only capitalize the first character, keep the rest as-is (preserves underscores and case)
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  protected resolveDecoratedHandler(op: string, definition = getActorDefinition(this)): string | null {
    if (!definition) return null;
    return definition.handlers[op]?.methodName ?? null;
  }

  /**
   * Serialize object to JSON string using fully iterative approach (zero recursion).
   * 
   * jco componentize compiles JS to WASM (StarlingMonkey) with a tiny call stack.
   * Native JSON.stringify recurses per-element and per-nesting-level, hitting
   * stack limits with arrays of 2+ items. This iterative serializer uses a work
   * stack instead of recursive function calls.
   */
  protected json(obj: unknown): string {
    return iterativeStringify(obj);
  }

  protected error(message: string): string {
    return "ERROR:" + message;
  }
}

/**
 * Base class for Workflow behavior actors (aligned with Rust Workflow trait and Python @workflow_actor).
 *
 * Implement run(), signal(), and query() for durable workflows (Temporal/Restate-style).
 * Message routing: framework sends msgType "workflow_run" | "workflow_signal:name" | "workflow_query:name";
 * PlexSpacesActor.handle() dispatches to these methods when present.
 *
 * Example:
 *   class OrderFulfillmentActor extends WorkflowActor<OrderState> {
 *     getDefaultState() { return { orderId: '', status: 'pending', steps: [] }; }
 *     run(payload: Record<string, unknown>) { ...; return { status: 'completed' }; }
 *     signal(name: string, data: Record<string, unknown>) { if (name === 'cancel') this.state.status = 'cancelled'; }
 *     query(name: string, params: Record<string, unknown>) { if (name === 'status') return this.state; return {}; }
 *   }
 */
export abstract class WorkflowActor<TState extends object = Record<string, unknown>> extends PlexSpacesActor<TState> {
  /** Main workflow execution (exclusive). Called when msgType is "workflow_run". Return result for ask/call. */
  abstract run(payload: Record<string, unknown>): Record<string, unknown> | unknown;
  /** Handle external signal (e.g. cancel). Called when msgType is "workflow_signal:name". */
  abstract signal(name: string, data: Record<string, unknown>): void;
  /** Read-only query. Called when msgType is "workflow_query:name". Return result for ask/call. */
  abstract query(name: string, params: Record<string, unknown>): Record<string, unknown> | unknown;
}

// ─── Fully Iterative JSON Serializer (ZERO recursion, ZERO method calls in loop) ───

// Character codes for escaping (pre-computed, avoids method calls)
const CHAR_QUOTE = 34;       // "
const CHAR_BACKSLASH = 92;   // \
const CHAR_NEWLINE = 10;     // \n
const CHAR_CR = 13;          // \r
const CHAR_TAB = 9;          // \t
const CHAR_SPACE = 32;       // space

// Escape table: for chars 0-31, pre-build the escape sequence
// This avoids ANY computation during string escaping
const ESCAPE_TABLE: string[] = [];
for (let i = 0; i < 128; i++) {
    if (i === CHAR_QUOTE) ESCAPE_TABLE[i] = '\\"';
    else if (i === CHAR_BACKSLASH) ESCAPE_TABLE[i] = '\\\\';
    else if (i === CHAR_NEWLINE) ESCAPE_TABLE[i] = '\\n';
    else if (i === CHAR_CR) ESCAPE_TABLE[i] = '\\r';
    else if (i === CHAR_TAB) ESCAPE_TABLE[i] = '\\t';
    else if (i < CHAR_SPACE) {
        // \u00XX
        const h1 = (i >> 4) & 0xf;
        const h0 = i & 0xf;
        ESCAPE_TABLE[i] =
            '\\u00' +
            String.fromCharCode(h1 < 10 ? 48 + h1 : 87 + h1) +
            String.fromCharCode(h0 < 10 ? 48 + h0 : 87 + h0);
    } else {
        ESCAPE_TABLE[i] = ''; // No escape needed
    }
}

/**
 * Escape a string for JSON. Uses pre-computed escape table.
 * No method calls in the hot loop (only charCodeAt and fromCharCode).
 */
function escapeStr(s: string): string {
    let out = '"';
    for (let i = 0, len = s.length; i < len; i++) {
        const c = s.charCodeAt(i);
        if (c < 128) {
            const esc = ESCAPE_TABLE[c];
            if (esc) {
                out += esc;
            } else {
                out += String.fromCharCode(c);
            }
        } else {
            // Non-ASCII: pass through (valid UTF-8 in JSON)
            out += String.fromCharCode(c);
        }
    }
    out += '"';
    return out;
}

/**
 * Fully iterative JSON serializer. ZERO recursion.
 *
 * Instead of recursive calls, we use an explicit work stack of "tokens" to emit.
 * The stack contains either literal strings to output, or values to process.
 * We process the stack in a single while loop with no function calls except
 * escapeStr() for string values (which itself has no recursion).
 *
 * This design keeps WASM call stack depth constant regardless of data size.
 */

// Work item types — using number tags (not strings) to avoid allocation
const TAG_VALUE = 0;
const TAG_LITERAL = 1;

/**
 * Fully iterative JSON serializer with zero recursion.
 * Uses a work stack instead of recursive function calls.
 */
function iterativeStringify(root: unknown): string {
    // Stack of work items: [tag, payload]
    // We use two parallel arrays instead of objects to minimize allocation
    const stackTags: number[] = [];
    const stackPayloads: unknown[] = [];
    let sp = 0; // stack pointer

    // Push root value
    stackTags[0] = TAG_VALUE;
    stackPayloads[0] = root;
    sp = 1;

    // Output fragments
    const fragments: string[] = [];
    let fragCount = 0;

    while (sp > 0) {
        // Pop
        sp--;
        const tag = stackTags[sp];
        const payload = stackPayloads[sp];
        // Help GC
        stackPayloads[sp] = null;

        if (tag === TAG_LITERAL) {
            // Literal string — just emit
            fragments[fragCount++] = payload as string;
            continue;
        }

        // TAG_VALUE — process the value
        if (payload === null || payload === undefined) {
            fragments[fragCount++] = 'null';
            continue;
        }

        const t = typeof payload;

        if (t === 'string') {
            fragments[fragCount++] = escapeStr(payload as string);
            continue;
        }

        if (t === 'number') {
            fragments[fragCount++] = '' + (payload as number);
            continue;
        }

        if (t === 'boolean') {
            fragments[fragCount++] = (payload as boolean) ? 'true' : 'false';
            continue;
        }

        if (t === 'function') {
            fragments[fragCount++] = 'null';
            continue;
        }

        // Object or Array
        // Duck-type array detection (avoids Array.isArray which may recurse)
        const obj = payload as Record<string, unknown>;
        const len = obj['length'];
        const isArr = typeof len === 'number' && len >= 0 && (len >>> 0) === len;

        if (isArr) {
            const arr = payload as unknown[];
            const arrLen = arr.length;
            if (arrLen === 0) {
                fragments[fragCount++] = '[]';
                continue;
            }

            // Push tokens in REVERSE order (stack is LIFO):
            //   [ elem0 , elem1 , elem2 ]
            // Push: ]  elemN  ,  elemN-1  ,  ...  ,  elem0  [

            stackTags[sp] = TAG_LITERAL;
            stackPayloads[sp] = ']';
            sp++;

            for (let i = arrLen - 1; i >= 0; i--) {
                stackTags[sp] = TAG_VALUE;
                stackPayloads[sp] = arr[i];
                sp++;
                if (i > 0) {
                    stackTags[sp] = TAG_LITERAL;
                    stackPayloads[sp] = ',';
                    sp++;
                }
            }

            stackTags[sp] = TAG_LITERAL;
            stackPayloads[sp] = '[';
            sp++;

            continue;
        }

        // Plain object
        // Use Object.getOwnPropertyNames() which is safer than for...in in StarlingMonkey
        // This avoids recursion while still iterating all properties
        let keys: string[] = [];
        try {
            // Object.getOwnPropertyNames() returns all own properties (non-enumerable too)
            // This is safer than for...in which may trigger recursion in StarlingMonkey
            const allProps = Object.getOwnPropertyNames(obj);
            for (let i = 0; i < allProps.length; i++) {
                const k = allProps[i];
                // Skip function properties and undefined values
                const v = (obj as Record<string, unknown>)[k];
                if (v !== undefined && typeof v !== 'function') {
                    keys.push(k);
                }
            }
        } catch {
            // If Object.getOwnPropertyNames fails, return empty object
            fragments[fragCount++] = '{}';
            continue;
        }

        if (keys.length === 0) {
            fragments[fragCount++] = '{}';
            continue;
        }

        // Push in reverse: }  valN  "keyN":  ,  ...  ,  val0  "key0":  {
        stackTags[sp] = TAG_LITERAL;
        stackPayloads[sp] = '}';
        sp++;

        for (let i = keys.length - 1; i >= 0; i--) {
            stackTags[sp] = TAG_VALUE;
            stackPayloads[sp] = (obj as Record<string, unknown>)[keys[i]];
            sp++;

            stackTags[sp] = TAG_LITERAL;
            stackPayloads[sp] = escapeStr(keys[i]) + ':';
            sp++;

            if (i > 0) {
                stackTags[sp] = TAG_LITERAL;
                stackPayloads[sp] = ',';
                sp++;
            }
        }

        stackTags[sp] = TAG_LITERAL;
        stackPayloads[sp] = '{';
        sp++;
    }

    // Join fragments without Array.join()
    let result = '';
    for (let i = 0; i < fragCount; i++) {
        result += fragments[i];
    }
    return result;
}
