// @ts-check
import { deepFreeze, parseWireEvent } from "/src/wire.mjs";
import { dispatch, registerHandler } from "../dispatch.mjs";
import { createStore } from "../store.mjs";
import { COMMANDS, parseInput } from "../commands.mjs";
import "./agt-status.js";
import "./agt-chat-log.js";
import "./agt-composer.js";
import "./agt-panel.js";

/**
 * @typedef {import("/src/wire.mjs").WireEvent} WireEvent
 * @typedef {import("/src/wire.mjs").PromptEvent} PromptEvent
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 * @typedef {import("/src/wire.mjs").ErrorEvent} ErrorEvent
 * @typedef {import("./agt-status.js").StatusValue} StatusValue
 * @typedef {import("./agt-panel.js").StateSnapshot} StateSnapshot
 */

/**
 * A `session_meta` event sent by the server after the WebSocket connects.
 * Validated by the generated web/generated/session_meta.mjs JTD validator.
 * NOTE: wire.mjs's registry does not accept this `_type` yet (wire-layer
 * follow-on), so the client passes these frames through pre-validated; the
 * local typedef keeps handlers typed here without touching wire.mjs.
 *
 * @typedef {object} SessionMetaEvent
 * @property {"session_meta"} _type
 * @property {number} created_at
 * @property {string} session_id
 * @property {string} title
 */

/**
 * An `ack` reply to a client control-plane frame (e.g. `rename`). Validated
 * by the generated web/generated/ack.mjs JTD validator (same follow-on note).
 *
 * @typedef {object} AckEvent
 * @property {"ack"} _type
 * @property {string} for_type
 * @property {string | null} message
 * @property {boolean} ok
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

      this.#wireClient();
    }
    this.#render();
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
    registerHandler(
      /** @type {any} */ ("session_meta"),
      (/** @type {unknown} */ rawEvent) => {
        const event = /** @type {SessionMetaEvent} */ (rawEvent);
        this.#panel?.setSessionTitle(event.title);
      },
    );
    registerHandler(
      /** @type {any} */ ("ack"),
      (/** @type {unknown} */ rawEvent) => {
        const event = /** @type {AckEvent} */ (rawEvent);
        if (event.for_type !== "rename") return;
        const title = this.#pendingRename;
        this.#pendingRename = null;
        if (event.ok && title !== null) {
          this.#panel?.setSessionTitle(title);
          this.#panel?.showSlash(`renamed: ${title}`);
          this.#fetchState();
        } else {
          const message = event.message ? `: ${event.message}` : "";
          this.#panel?.showSlash(`error: rename failed${message}`);
        }
      },
    );
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
    this.#panel?.setState(this.#snapshot);
    return this.#snapshot;
  }

  /**
   * Slash command control plane (never sent to the model).
   *
   * @param {{ rawText: string }} detail
   */
  async #runCommand(detail) {
    const panel = this.#panel;
    if (!panel) return;
    const parsed = parseInput(detail.rawText);
    if (parsed.kind !== "command") return;

    if (parsed.error === "empty") {
      panel.showSlash("error: empty command — type / for the command list");
      return;
    }
    if (parsed.error === "unknown") {
      panel.showSlash(`error: unknown command '/${parsed.name}' — try /help`);
      return;
    }
    if (parsed.error === "missing-args") {
      panel.showSlash(`usage: /${parsed.name} <title>`);
      return;
    }

    switch (parsed.name) {
      case "model": {
        const snapshot = this.#snapshot ?? (await this.#fetchState());
        if (!snapshot) {
          panel.showSlash("error: /api/state unavailable");
          return;
        }
        panel.showSlash(
          `model: ${snapshot.model} (provider: ${snapshot.provider})`,
        );
        return;
      }
      case "built-ins": {
        if (!this.#snapshot) await this.#fetchState();
        panel.showSlash("built-ins: opened the Built-ins tree");
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
        panel.showSlash(`verbose: ${this.#verbose.enabled ? "on" : "off"}`);
        return;
      }
      case "rename": {
        const client = this.#client;
        if (!client || this.#status.state !== "connected") {
          panel.showSlash("error: not connected — cannot rename");
          return;
        }
        if (typeof client.sendRename !== "function") {
          panel.showSlash("error: client does not support rename");
          return;
        }
        this.#pendingRename = parsed.args;
        try {
          await client.sendRename(parsed.args);
        } catch (error) {
          this.#pendingRename = null;
          panel.showSlash(
            `error: rename failed — ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
        }
        return;
      }
      case "help": {
        panel.showSlash(
          COMMANDS.map(
            (command) => `/${command.name} — ${command.description}`,
          ).join("\n"),
        );
        return;
      }
      default:
        panel.showSlash(`error: unhandled command '/${parsed.name}'`);
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
    const panel = this.#panel;
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
      panel?.showSlash(
        `error: toggle ${name} failed — ${
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
   * Append a validated, deep-frozen event to the store; the store's notify
   * subscription re-renders from the new frozen snapshot.
   *
   * @param {ChatEvent} event
   */
  #pushEvent(event) {
    this.#store.append(event);
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
    if (log) log.events = this.#store.getEvents();

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
