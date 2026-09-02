// @ts-check
import { deepFreeze, parseWireEvent } from "/src/wire.mjs";
import { dispatch, registerHandler } from "../dispatch.mjs";
import { createStore } from "../store.mjs";
import { COMMANDS, parseInput } from "../commands.mjs";
import { installConsoleBus } from "../console-bus.mjs";
import {
  extractToolCallMeta,
  initLineformat,
  parseWireLine,
} from "/src/lineformat.mjs";
import {
  appendEvents,
  frontierOf,
  getAll,
  mergeCatchup,
  openHistory,
} from "/src/history.mjs";
import "./agt-status.js";
import "./agt-chat-log.js";
import "./agt-composer.js";
import "./agt-panel.js";

/**
 * @typedef {import("/src/wire.mjs").WireEvent} WireEvent
 * @typedef {import("/src/wire.mjs").PromptEvent} PromptEvent
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 * @typedef {import("/src/wire.mjs").ToolCallEvent} ToolCallEvent
 * @typedef {import("/src/lineformat.mjs").WireFrame} WireFrame
 * @typedef {import("./agt-status.js").StatusValue} StatusValue
 * @typedef {import("./agt-panel.js").StateSnapshot} StateSnapshot
 */

/**
 * One persisted history record in IndexedDB (`agt.events`).
 *
 * @typedef {object} HistoryRecord
 * @property {string} sessionId
 * @property {number} ts stamp: server `_ts` for catch-up records,
 * `Date.now()` at receipt for live events
 * @property {ChatEvent} event the validated frozen event (or the partial
 * tool_call reconstruction)
 */

/**
 * Poll for the vanilla client installed by /assets/client.mjs instead of
 * crashing when it has not loaded yet (module load order is not guaranteed).
 *
 * @returns {Promise<NonNullable<Window["AgtClient"]>>}
 */
async function waitForAgtClient() {
  for (let attempt = 0; attempt < 200; attempt++) {
    if (window.AgtClient) return window.AgtClient;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("window.AgtClient unavailable");
}

export class AgtApp extends HTMLElement {
  /** In-memory append-only store of frozen chat events (server + prompt). */
  #store = createStore();
  /** @type {Readonly<StatusValue>} */
  #status = deepFreeze({ state: "connecting" });
  /** @type {Set<string>} */
  #pending = new Set();
  /** @type {NonNullable<Window["AgtClient"]> | null} */
  #client = null;
  #wired = false;
  /** @type {Readonly<StateSnapshot> | null} */
  #snapshot = null;
  /** Verbose UI flag (frozen wrapper; item29 renders with it). */
  #verbose = deepFreeze({ enabled: false });
  /** @type {string | null} */
  #pendingRename = null;
  /** @type {import("./agt-panel.js").AgtPanel | null} */
  #panel = null;
  /** @type {string | null} */
  #sessionId = null;
  /** @type {Promise<IDBDatabase> | null} */
  #historyDb = null;

  /** Frozen chat-state snapshot (server events + prompt records). */
  get state() {
    return this.#store.getEvents();
  }

  get status() {
    return this.#status;
  }

  /** Current verbose UI flag. */
  get verbose() {
    return this.#verbose.enabled;
  }

  /** Frozen /api/state snapshot, or null while unavailable. */
  get snapshot() {
    return this.#snapshot;
  }

  connectedCallback() {
    if (!this.#wired) {
      this.#wired = true;

      // Console tee bus (item32): capture this chat screen's console from
      // boot so log/info/warn/error also flow to the agt-console
      // BroadcastChannel → spool worker → IndexedDB backlog.
      installConsoleBus();

      const main = document.createElement("div");
      main.className = "agt-main";

      const status = document.createElement("agt-status");
      const log = document.createElement("agt-chat-log");
      const composer = document.createElement("agt-composer");
      composer.addEventListener("agt-send", (e) => {
        const detail = /** @type {CustomEvent} */ (e).detail;
        if (detail && typeof detail.text === "string") {
          this.#sendPrompt(detail.text);
        }
      });
      composer.addEventListener("agt-command", (e) => {
        const detail = /** @type {CustomEvent} */ (e).detail;
        if (detail && typeof detail.rawText === "string") {
          this.#runCommand(/** @type {{ rawText: string }} */ (detail));
        }
      });
      main.replaceChildren(status, log, composer);

      const panel = /** @type {import("./agt-panel.js").AgtPanel} */ (
        document.createElement("agt-panel")
      );
      panel.addEventListener("agt-toggle-tool", (e) => {
        const detail = /** @type {CustomEvent} */ (e).detail;
        if (
          detail &&
          typeof detail.name === "string" &&
          typeof detail.enabled === "boolean"
        ) {
          this.#toggleTool(detail.name, detail.enabled);
        }
      });
      this.#panel = panel;

      this.replaceChildren(main, panel);

      // The store drives re-renders: every append notifies, and the
      // subscription re-renders from the new frozen snapshot. #status and
      // #pending changes still render manually below.
      this.#store.subscribe(() => this.#render());

      // /verbose re-renders the log (tool_call lines appear/disappear) —
      // history included.
      window.addEventListener("agt-verbose-changed", () => this.#render());

      this.#boot();
    }
    this.#render();
  }

  /**
   * Boot: with `?s=<uuid>` catch up over the line protocol BEFORE opening
   * the WebSocket; then connect live.
   */
  async #boot() {
    const sessionParam = new URLSearchParams(location.search).get("s");
    if (sessionParam) {
      this.#sessionId = sessionParam;
      await this.#catchUp(sessionParam);
    }
    await this.#wireClient();
  }

  /**
   * Catch up a session from the server's line-protocol endpoint
   * (`/api/session/<uuid>?after=<frontier>`), replaying validated events
   * into the store AND IndexedDB history before the WebSocket opens. Never
   * throws — a failure leaves chat fully functional (live only).
   *
   * @param {string} sessionId
   */
  async #catchUp(sessionId) {
    try {
      await initLineformat();
      const db = await this.#history();
      // Replay the local IndexedDB log first, then catch up from the server
      // after the frontier — only newer lines are fetched (no duplicates).
      const local = await getAll(db, sessionId);
      const frontier = frontierOf(local);
      if (local.length > 0) {
        this.#store.appendAll(
          local.map((record) => deepFreeze(/** @type {ChatEvent} */ (record.event))),
        );
      }
      const res = await fetch(
        `/api/session/${encodeURIComponent(sessionId)}?after=${frontier}`,
      );
      if (!res.ok) return;
      const body = await res.text();
      /** @type {WireFrame[]} */
      const frames = [];
      for (const line of body.split("\n")) {
        if (line.length === 0) continue;
        try {
          frames.push(await parseWireLine(line, 1024));
        } catch (error) {
          // Corruption: HALT — keep everything received up to this point,
          // never skip ahead.
          console.warn("[catchup] halting at corrupt line", error);
          break;
        }
      }
      const fresh = mergeCatchup(frames, frontier);
      /** @type {HistoryRecord[]} */
      const records = [];
      for (const frame of fresh) {
        const event = await this.#frameToEvent(frame, sessionId);
        if (event) records.push({ sessionId, ts: frame.ts, event });
      }
      if (records.length > 0) {
        this.#store.appendAll(records.map((record) => record.event));
        // Fire-and-forget: the store already holds the frames, so #boot must
        // never block on (or fail with) the history spool write.
        appendEvents(db, sessionId, records).catch((error) =>
          console.warn("[history] append failed", error),
        );
      }
    } catch (error) {
      console.warn("[catchup] failed", error);
    }
  }

  /**
   * Convert one parsed catch-up frame into a validated frozen chat event:
   * - strict JSON → `parseWireEvent` (validated, deep-frozen);
   * - strict parse failure AND `tool_call` type → lenient metadata
   *   extraction (shared Rust/WASM scan) → synthesized deep-frozen partial
   *   tool_call event with the truncated pretty heads;
   * - anything else → null (skip).
   *
   * @param {WireFrame} frame
   * @param {string} sessionId
   * @returns {Promise<ChatEvent | null>}
   */
  async #frameToEvent(frame, sessionId) {
    /** @type {unknown} */
    let parsed;
    try {
      parsed = JSON.parse(frame.text);
    } catch {
      parsed = undefined;
    }
    if (parsed !== undefined && parsed !== null) {
      // Client-echo and rollout-internal records are persisted on the wire
      // log but are not server events — skip them silently (they have no
      // JTD validator, so parseWireEvent would drop them noisily).
      if (
        /** @type {any} */ (parsed)._type === "prompt" ||
        /** @type {any} */ (parsed)._type === "session_rename"
      ) {
        return null;
      }
      const event = parseWireEvent(parsed);
      if (!event) return null;
      if (event._type === "session_meta") {
        this.#panel?.setSessionTitle(event.title);
        return null;
      }
      if (event._type === "ack") return null;
      return event;
    }
    if (frame.type === "tool_call") {
      const meta = await extractToolCallMeta(frame.text);
      if (!meta) return null;
      /** @type {ToolCallEvent & { abridged: true }} */
      const partial = {
        _type: "tool_call",
        id: null,
        session_id: sessionId,
        tool: meta.tool,
        duration_ms: meta.duration_ms,
        bytes_up: meta.bytes_up,
        bytes_down: meta.bytes_down,
        ts: meta.ts,
        args_pretty: meta.args_pretty_head,
        result_pretty: meta.result_pretty_head,
        abridged: true,
      };
      return deepFreeze(partial);
    }
    return null;
  }

  /**
   * Lazily open (and keep) the IndexedDB history connection.
   *
   * @returns {Promise<IDBDatabase>}
   */
  #history() {
    const existing = this.#historyDb;
    if (existing) return existing;
    const created = openHistory();
    this.#historyDb = created;
    return created;
  }

  /**
   * Persist one validated event to history under the current session id
   * (stamped at receipt). Fire-and-forget: history failures never break
   * chat.
   *
   * @param {ChatEvent} event
   */
  #recordHistory(event) {
    const sessionId = this.#sessionId;
    if (!sessionId) return;
    const record = { sessionId, ts: Date.now(), event };
    this.#history()
      .then((db) => appendEvents(db, sessionId, [record]))
      .catch((error) => console.warn("[history] append failed", error));
  }

  async #wireClient() {
    this.#registerEventHandlers();
    try {
      const client = await waitForAgtClient();
      this.#client = client;
      await client.connect({
        onOpen: () => {
          this.#setStatus({ state: "connected" });
          this.#fetchState();
        },
        onClose: () => {
          this.#setStatus({ state: "disconnected" });
        },
        onError: () => {
          this.#setStatus({ state: "error" });
        },
        onEvent: (event) => {
          this.#handleEvent(event);
        },
      });
    } catch (error) {
      this.#setStatus({
        state: "error",
        detail: error instanceof Error ? error.message : String(error),
      });
    }
  }

  /**
   * Register the business-logic handlers for each server `_type` on the
   * dispatch layer. Handlers receive the validated, deep-frozen event and
   * append it to frozen state; assistant/error clear the matching pending
   * prompt id so the composer re-enables.
   */
  #registerEventHandlers() {
    registerHandler("ready", (event) => {
      this.#pushEvent(event);
    });
    registerHandler("pong", (event) => {
      this.#pushEvent(event);
    });
    registerHandler("assistant", (event) => {
      this.#pushEvent(event);
      if (event._type === "assistant" && event.id) {
        this.#pending.delete(event.id);
        this.#render();
      }
    });
    registerHandler("error", (event) => {
      this.#pushEvent(event);
      if (event._type === "error" && event.id) {
        this.#pending.delete(event.id);
        this.#render();
      }
    });
    registerHandler("tool_call", (event) => {
      // Stored (and persisted to history) always; rendered only when the
      // /verbose flag is ON (agt-chat-log decides).
      this.#pushEvent(event);
    });
    registerHandler("session_meta", (event) => {
      if (event._type !== "session_meta") return;
      if (!this.#sessionId) this.#sessionId = event.session_id;
      this.#panel?.setSessionTitle(event.title);
    });
    registerHandler("ack", (event) => {
      if (event._type !== "ack" || event.for_type !== "rename") return;
      const title = this.#pendingRename;
      this.#pendingRename = null;
      if (event.ok && title !== null) {
        this.#panel?.setSessionTitle(title);
        console.log(`[slash] renamed: ${title}`);
        this.#fetchState();
      } else {
        const message = event.message ? `: ${event.message}` : "";
        console.error(`[slash] error: rename failed${message}`);
      }
    });
  }

  /**
   * Handle a validated, deep-frozen wire event delivered by the client.
   * Invalid frames are already dropped (and logged) by wire.mjs; this null
   * guard is purely defensive.
   *
   * @param {WireEvent | null} event
   */
  #handleEvent(event) {
    if (event === null) return;
    dispatch(event);
  }

  /**
   * Fetch /api/state (control plane), freeze the snapshot and hand it to the
   * panel. A 404 or network failure renders the panel's graceful
   * "(unavailable)" state — chat keeps working without it.
   *
   * @returns {Promise<Readonly<StateSnapshot> | null>}
   */
  async #fetchState() {
    /** @type {Readonly<StateSnapshot> | null} */
    let snapshot = null;
    try {
      const res = await fetch("/api/state");
      if (res.ok) {
        snapshot = deepFreeze(await res.json());
      }
    } catch {
      snapshot = null;
    }
    this.#snapshot = snapshot;
    if (snapshot && !this.#sessionId) {
      this.#sessionId = snapshot.session?.id ?? null;
    }
    this.#panel?.setState(this.#snapshot);
    return this.#snapshot;
  }

  /**
   * Slash command control plane (never sent to the model). Since item32 the
   * Slash tree keeps only the invocation echo; every RESULT goes to the
   * console bus (`console.log("[slash] …")`, `console.error` for errors) so
   * it lands in the devtools console popup.
   *
   * @param {{ rawText: string }} detail
   */
  async #runCommand(detail) {
    const panel = this.#panel;
    if (!panel) return;
    const parsed = parseInput(detail.rawText);
    if (parsed.kind !== "command") return;

    // Minimal invocation echo: the command line that was run.
    panel.echoSlash(detail.rawText);

    if (parsed.error === "empty") {
      console.error("[slash] error: empty command — type / for the command list");
      return;
    }
    if (parsed.error === "unknown") {
      console.error(
        `[slash] error: unknown command '/${parsed.name}' — try /help`,
      );
      return;
    }
    if (parsed.error === "missing-args") {
      console.log(`[slash] usage: /${parsed.name} <title>`);
      return;
    }

    switch (parsed.name) {
      case "model": {
        const snapshot = this.#snapshot ?? (await this.#fetchState());
        if (!snapshot) {
          console.error("[slash] error: /api/state unavailable");
          return;
        }
        console.log(
          `[slash] model: ${snapshot.model} (provider: ${snapshot.provider})`,
        );
        return;
      }
      case "built-ins": {
        if (!this.#snapshot) await this.#fetchState();
        console.log("[slash] built-ins: opened the Built-ins tree");
        panel.openBuiltins();
        return;
      }
      case "verbose": {
        this.#verbose = deepFreeze({ enabled: !this.#verbose.enabled });
        window.dispatchEvent(
          new CustomEvent("agt-verbose-changed", {
            detail: { verbose: this.#verbose.enabled },
          }),
        );
        console.log(`[slash] verbose: ${this.#verbose.enabled ? "on" : "off"}`);
        return;
      }
      case "rename": {
        const client = this.#client;
        if (!client || this.#status.state !== "connected") {
          console.error("[slash] error: not connected — cannot rename");
          return;
        }
        if (typeof client.sendRename !== "function") {
          console.error("[slash] error: client does not support rename");
          return;
        }
        this.#pendingRename = parsed.args;
        try {
          await client.sendRename(parsed.args);
        } catch (error) {
          this.#pendingRename = null;
          console.error(
            `[slash] error: rename failed — ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
        }
        return;
      }
      case "console": {
        const popup = window.open(
          "/console.html",
          "agt-console",
          "popup,width=920,height=680",
        );
        if (popup === null) {
          // Popup blocked: fall back to a regular tab.
          window.open("/console.html", "_blank");
        }
        console.log("[slash] console: opened the devtools console popup");
        return;
      }
      case "help": {
        console.log(
          `[slash] ${COMMANDS.map(
            (command) => `/${command.name} — ${command.description}`,
          ).join("\n")}`,
        );
        return;
      }
      default:
        console.error(`[slash] error: unhandled command '/${parsed.name}'`);
    }
  }

  /**
   * Flip a tool: POST /api/tools, then refetch /api/state so the panel row
   * converges with the server (reverting the optimistic update on failure).
   *
   * @param {string} name
   * @param {boolean} enabled
   */
  async #toggleTool(name, enabled) {
    try {
      const res = await fetch("/api/tools", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, enabled }),
      });
      if (!res.ok) {
        throw new Error(`POST /api/tools failed (${res.status})`);
      }
    } catch (error) {
      console.error(
        `[slash] error: toggle ${name} failed — ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
    await this.#fetchState();
  }

  /**
   * Send a prompt: append a frozen prompt record, keep the composer disabled
   * while the request is in flight, and let the client resolve/reject the
   * reply by matching id.
   *
   * @param {string} text
   */
  async #sendPrompt(text) {
    const client = this.#client;
    if (!client || this.#status.state !== "connected") return;

    const id = `req_${Math.random().toString(16).slice(2)}_${Date.now().toString(16)}`;
    /** @type {PromptEvent} */
    const prompt = deepFreeze({ _type: "prompt", id, text });

    this.#pending.add(id);
    this.#pushEvent(prompt);

    try {
      await client.sendPrompt(text, id);
    } catch (error) {
      // If no error event arrived via onEvent for this id (e.g. the socket
      // died), synthesize one so the failure is still visible in the log.
      if (this.#pending.has(id)) {
        const synthesized = parseWireEvent({
          _type: "error",
          id,
          message: error instanceof Error ? error.message : String(error),
        });
        this.#pending.delete(id);
        if (synthesized !== null) {
          this.#pushEvent(synthesized);
        }
      }
    } finally {
      this.#pending.delete(id);
      this.#render();
    }
  }

  /**
   * Append a validated, deep-frozen event to the store (the store's notify
   * subscription re-renders from the new frozen snapshot) and persist it to
   * the session's IndexedDB history.
   *
   * @param {ChatEvent} event
   */
  #pushEvent(event) {
    this.#store.append(event);
    this.#recordHistory(event);
  }

  /**
   * @param {StatusValue} status
   */
  #setStatus(status) {
    this.#status = deepFreeze(status);
    this.#render();
  }

  #render() {
    const statusEl = /** @type {import("./agt-status.js").AgtStatus} */ (
      this.querySelector("agt-status")
    );
    if (statusEl) statusEl.status = this.#status;

    const log = /** @type {import("./agt-chat-log.js").AgtChatLog} */ (
      this.querySelector("agt-chat-log")
    );
    if (log) {
      log.verbose = this.#verbose.enabled;
      log.events = this.#store.getEvents();
    }

    const composer = /** @type {import("./agt-composer.js").AgtComposer} */ (
      this.querySelector("agt-composer")
    );
    if (composer) {
      composer.disabled =
        this.#pending.size > 0 || this.#status.state !== "connected";
    }

    if (this.#panel) this.#panel.setState(this.#snapshot);
  }
}

customElements.define("agt-app", AgtApp);
