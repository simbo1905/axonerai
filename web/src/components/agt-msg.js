// @ts-check

/**
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 */

/**
 * Derive the display role, label and text for a frozen chat event.
 *
 * @param {ChatEvent} event
 * @returns {{ role: string, label: string, text: string }}
 */
function describe(event) {
  switch (event._type) {
    case "prompt":
      return { role: "you", label: "You", text: event.text };
    case "assistant":
      return { role: "agent", label: "Agent", text: event.text };
    case "error":
      return { role: "error", label: "Error", text: event.message };
    case "ready":
      return {
        role: "system",
        label: "System",
        text: `agent ready (version ${event.version})`,
      };
    case "pong":
      return {
        role: "system",
        label: "System",
        text: event.id ? `pong (${event.id})` : "pong",
      };
    // Unreachable through agt-chat-log (tool_call renders via agt-tool-line
    // when /verbose is ON and is skipped when OFF; ack/session_meta are not
    // pushed to the store) — kept exhaustive so a stray frozen frame still
    // renders defensively instead of crashing the log.
    case "tool_call":
      return { role: "system", label: "Tool", text: event.tool };
    case "ack":
      return {
        role: "system",
        label: "System",
        text: `ack ${event.for_type}${event.ok ? "" : " (failed)"}`,
      };
    case "session_meta":
      return { role: "system", label: "System", text: event.title };
  }
}

export class AgtMsg extends HTMLElement {
  /** @type {Readonly<ChatEvent> | null} */
  #event = null;

  /** @param {Readonly<ChatEvent>} event */
  set event(event) {
    this.#event = event;
    this.#render();
  }

  /** @returns {Readonly<ChatEvent> | null} */
  get event() {
    return this.#event;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const event = this.#event;
    if (!event) return;
    const { role, label, text } = describe(event);
    const time = new Date().toLocaleTimeString();
    this.className = `msg msg-${role}`;
    const head = document.createElement("div");
    head.className = "msg-head";
    const roleEl = document.createElement("span");
    roleEl.className = "msg-role";
    roleEl.textContent = label;
    const timeEl = document.createElement("time");
    timeEl.textContent = time;
    head.append(roleEl, timeEl);
    const body = document.createElement("div");
    body.className = "msg-text";
    body.textContent = text;
    this.replaceChildren(head, body);
  }
}

customElements.define("agt-msg", AgtMsg);
