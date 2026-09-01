// @ts-check

/**
 * Frozen connection-status value object.
 *
 * @typedef {object} StatusValue
 * @property {"connecting" | "connected" | "disconnected" | "error"} state
 * @property {string} [detail]
 */

const LABELS = {
  connecting: "Connecting…",
  connected: "Connected",
  disconnected: "Disconnected",
  error: "Error",
};

export class AgtStatus extends HTMLElement {
  /** @type {Readonly<StatusValue>} */
  #status = { state: "connecting" };

  /** @param {Readonly<StatusValue>} status */
  set status(status) {
    this.#status = status;
    this.#render();
  }

  get status() {
    return this.#status;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const state = this.#status.state;
    const pill = document.createElement("span");
    pill.className = `pill pill-${state}`;
    pill.textContent = LABELS[state];
    this.replaceChildren(pill);
  }
}

customElements.define("agt-status", AgtStatus);
