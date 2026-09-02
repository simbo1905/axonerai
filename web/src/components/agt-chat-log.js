// @ts-check
import "./agt-msg.js";
import "./agt-tool-line.js";

/**
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 */

export class AgtChatLog extends HTMLElement {
  /** @type {Readonly<ChatEvent[]>} */
  #events = [];
  #verbose = false;

  /** @param {Readonly<ChatEvent[]>} events */
  set events(events) {
    this.#events = events;
    this.#render();
  }

  get events() {
    return this.#events;
  }

  /**
   * /verbose flag: when OFF, tool_call events are stored but NOT rendered;
   * when ON they render as dim terminal-style agt-tool-line rows interleaved
   * in store (arrival) order. Toggling re-renders, history included.
   *
   * @param {boolean} verbose
   */
  set verbose(verbose) {
    this.#verbose = Boolean(verbose);
    this.#render();
  }

  get verbose() {
    return this.#verbose;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const fragment = document.createDocumentFragment();
    for (const event of this.#events) {
      if (event._type === "tool_call") {
        if (!this.#verbose) continue;
        const line = /** @type {HTMLElement & { event: Readonly<ChatEvent> }} */ (
          document.createElement("agt-tool-line")
        );
        line.event = event;
        fragment.append(line);
        continue;
      }
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
