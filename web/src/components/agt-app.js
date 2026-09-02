// @ts-check
import { deepFreeze, parseWireEvent } from "/src/wire.mjs";
import { dispatch, registerHandler } from "../dispatch.mjs";
import { createStore } from "../store.mjs";
import "./agt-status.js";
import "./agt-chat-log.js";
import "./agt-composer.js";

/**
 * @typedef {import("/src/wire.mjs").WireEvent} WireEvent
 * @typedef {import("/src/wire.mjs").PromptEvent} PromptEvent
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 * @typedef {import("./agt-status.js").StatusValue} StatusValue
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

  /** Frozen chat-state snapshot (server events + prompt records). */
  get state() {
    return this.#store.getEvents();
  }

  get status() {
    return this.#status;
  }

  connectedCallback() {
    if (!this.#wired) {
      this.#wired = true;

      const status = document.createElement("agt-status");
      const log = document.createElement("agt-chat-log");
      const composer = document.createElement("agt-composer");
      composer.addEventListener("agt-send", (e) => {
        const detail = /** @type {CustomEvent} */ (e).detail;
        if (detail && typeof detail.text === "string") {
          this.#sendPrompt(detail.text);
        }
      });
      this.replaceChildren(status, log, composer);

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
  }
}

customElements.define("agt-app", AgtApp);
