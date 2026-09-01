// @ts-check

export class AgtComposer extends HTMLElement {
  #disabled = false;
  #rendered = false;

  /** @param {boolean} value */
  set disabled(value) {
    this.#disabled = Boolean(value);
    this.#applyDisabled();
  }

  get disabled() {
    return this.#disabled;
  }

  connectedCallback() {
    if (this.#rendered) return;
    this.#rendered = true;

    const textarea = document.createElement("textarea");
    textarea.className = "composer-input";
    textarea.rows = 2;
    textarea.placeholder = "Ask the agent…";

    const button = document.createElement("button");
    button.className = "composer-send";
    button.type = "button";
    button.textContent = "Send";

    button.addEventListener("click", () => this.#send());
    textarea.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        this.#send();
      }
    });

    this.replaceChildren(textarea, button);
    this.#applyDisabled();
  }

  #send() {
    if (this.#disabled) return;
    const textarea = this.querySelector("textarea");
    if (!textarea) return;
    const text = textarea.value.trim();
    if (!text) return;
    textarea.value = "";
    this.dispatchEvent(
      new CustomEvent("agt-send", { detail: { text }, bubbles: true, composed: true }),
    );
  }

  #applyDisabled() {
    const textarea = this.querySelector("textarea");
    const button = this.querySelector("button");
    if (textarea) textarea.disabled = this.#disabled;
    if (button) button.disabled = this.#disabled;
  }
}

customElements.define("agt-composer", AgtComposer);
