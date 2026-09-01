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
