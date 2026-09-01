// @ts-check
import "./agt-msg.js";

/**
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 */

export class AgtChatLog extends HTMLElement {
  /** @type {Readonly<ChatEvent[]>} */
  #events = [];

  /** @param {Readonly<ChatEvent[]>} events */
  set events(events) {
    this.#events = events;
    this.#render();
  }

  get events() {
    return this.#events;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const fragment = document.createDocumentFragment();
    for (const event of this.#events) {
      const msg = /** @type {HTMLElement & { event: Readonly<ChatEvent> }} */ (
        document.createElement("agt-msg")
      );
      msg.event = event;
      fragment.append(msg);
    }
    this.replaceChildren(fragment);
    this.scrollTop = this.scrollHeight;
  }
}

customElements.define("agt-chat-log", AgtChatLog);
