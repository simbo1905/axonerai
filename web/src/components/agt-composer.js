// @ts-check
import { filterCommands, parseInput } from "../commands.mjs";

/**
 * Composer with the slash-command menu.
 *
 * Typing `/` as the FIRST character opens the command list above the input:
 * ↑/↓ move the highlight (wrapping), Enter completes+runs the highlighted
 * command, Tab completes the name, Esc closes, click selects, typing filters
 * by prefix. ARIA listbox/option with aria-activedescendant; focus stays in
 * the textarea the whole time. Running a command dispatches `agt-command`
 * {detail:{name, args, rawText}} upward — commands are control plane and
 * never go to the model. Non-slash input keeps the exact chat-send behaviour.
 */
export class AgtComposer extends HTMLElement {
  #disabled = false;
  #rendered = false;
  /** @type {HTMLTextAreaElement | null} */
  #textarea = null;
  /** @type {HTMLDivElement | null} */
  #menu = null;
  #menuOpen = false;
  #highlight = 0;
  /** @type {ReturnType<typeof filterCommands>} */
  #matches = [];

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

    const menu = document.createElement("div");
    menu.className = "slash-menu";
    menu.id = "agt-slash-menu";
    menu.setAttribute("role", "listbox");
    menu.setAttribute("aria-label", "Slash commands");
    menu.hidden = true;
    // Keep focus in the textarea when a menu option is pressed, so the click
    // lands before any blur-driven close.
    menu.addEventListener("mousedown", (e) => e.preventDefault());
    menu.addEventListener("click", (e) => {
      const target = /** @type {HTMLElement} */ (e.target);
      const option = /** @type {HTMLElement | null} */ (
        target instanceof Element ? target.closest(".slash-option") : null
      );
      if (option instanceof HTMLElement && option.dataset.name) {
        this.#runCommand(option.dataset.name);
      }
    });

    const textarea = document.createElement("textarea");
    textarea.className = "composer-input";
    textarea.id = "agt-composer-input";
    textarea.rows = 2;
    textarea.placeholder = "Ask the agent… (type / for commands)";
    textarea.setAttribute("aria-autocomplete", "list");
    textarea.setAttribute("aria-controls", "agt-slash-menu");
    textarea.setAttribute("aria-expanded", "false");

    const button = document.createElement("button");
    button.className = "composer-send";
    button.type = "button";
    button.textContent = "Send";

    button.addEventListener("click", () => this.#send());
    textarea.addEventListener("input", () => this.#updateMenu());
    textarea.addEventListener("keydown", (e) => this.#onKeydown(e));
    textarea.addEventListener("blur", () => {
      // Small delay so a click on a menu option registers first.
      setTimeout(() => this.#closeMenu(), 120);
    });

    this.replaceChildren(menu, textarea, button);
    this.#textarea = textarea;
    this.#menu = menu;
    this.#applyDisabled();
  }

  #send() {
    if (this.#disabled) return;
    const textarea = this.#textarea;
    if (!textarea) return;
    const text = textarea.value.trim();
    if (!text) return;
    // Slash-leading input is control plane on every send path (Enter keydown
    // routes through #runParsed too); only chat reaches the model.
    if (text.startsWith("/")) {
      this.#runParsed(text);
      return;
    }
    textarea.value = "";
    this.dispatchEvent(
      new CustomEvent("agt-send", { detail: { text }, bubbles: true, composed: true }),
    );
  }

  #onKeydown(/** @type {KeyboardEvent} */ e) {
    const textarea = this.#textarea;
    if (!textarea) return;

    if (this.#menuOpen) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const count = this.#matches.length;
        if (count === 0) return;
        this.#highlight =
          e.key === "ArrowDown"
            ? (this.#highlight + 1) % count
            : (this.#highlight - 1 + count) % count;
        this.#renderMenu();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        this.#closeMenu();
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        const match = this.#matches[this.#highlight];
        if (match) {
          textarea.value = `/${match.name} `;
          textarea.setSelectionRange(textarea.value.length, textarea.value.length);
          this.#updateMenu();
        }
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        const match = this.#matches[this.#highlight];
        if (match) {
          this.#runCommand(match.name);
        } else {
          this.#runParsed(textarea.value);
        }
        return;
      }
      return;
    }

    // Enter sends (slash-leading input is control plane); Shift+Enter keeps
    // the textarea's default newline.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      this.#send();
    }
  }

  #updateMenu() {
    const textarea = this.#textarea;
    const menu = this.#menu;
    if (!textarea || !menu) return;
    const value = textarea.value;
    if (!value.startsWith("/") || this.#disabled) {
      this.#closeMenu();
      return;
    }
    this.#matches = filterCommands(value);
    if (this.#matches.length === 0) {
      this.#closeMenu();
      return;
    }
    if (this.#highlight >= this.#matches.length) this.#highlight = 0;
    this.#menuOpen = true;
    menu.hidden = false;
    textarea.setAttribute("aria-expanded", "true");
    this.#renderMenu();
  }

  #renderMenu() {
    const menu = this.#menu;
    const textarea = this.#textarea;
    if (!menu || !textarea) return;
    menu.replaceChildren(
      ...this.#matches.map((command, i) => {
        const option = document.createElement("div");
        option.className =
          i === this.#highlight ? "slash-option active" : "slash-option";
        option.id = `agt-slash-opt-${i}`;
        option.dataset.name = command.name;
        option.setAttribute("role", "option");
        option.setAttribute("aria-selected", i === this.#highlight ? "true" : "false");

        const name = document.createElement("span");
        name.className = "slash-option-name";
        name.textContent = `/${command.name}`;
        const desc = document.createElement("span");
        desc.className = "slash-option-desc";
        desc.textContent = command.description;
        option.append(name, desc);
        return option;
      }),
    );
    const active = this.#matches[this.#highlight];
    if (active) {
      textarea.setAttribute("aria-activedescendant", `agt-slash-opt-${this.#highlight}`);
    } else {
      textarea.removeAttribute("aria-activedescendant");
    }
  }

  #closeMenu() {
    const menu = this.#menu;
    const textarea = this.#textarea;
    this.#menuOpen = false;
    if (menu) menu.hidden = true;
    if (textarea) {
      textarea.setAttribute("aria-expanded", "false");
      textarea.removeAttribute("aria-activedescendant");
    }
  }

  /**
   * Complete + run a highlighted (or clicked) menu command. Args come from
   * the raw input when the input actually names that command, else "".
   *
   * @param {string} name
   */
  #runCommand(name) {
    const textarea = this.#textarea;
    if (!textarea) return;
    const rawText = textarea.value;
    const parsed = parseInput(rawText);
    const args =
      parsed.kind === "command" && parsed.name === name && !parsed.error
        ? parsed.args
        : "";
    // A menu selection RESOLVES a typo'd prefix ("/m" → "/models"): dispatch
    // the canonical command text so the runner re-parses the command that was
    // actually selected, never the raw prefix that failed to parse.
    const resolved = args ? `/${name} ${args}` : `/${name}`;
    textarea.value = "";
    this.#closeMenu();
    this.dispatchEvent(
      new CustomEvent("agt-command", {
        detail: { name, args, rawText: resolved },
        bubbles: true,
        composed: true,
      }),
    );
  }

  /**
   * Run whatever the raw input parses to (menu closed / no matches). The
   * error, if any, travels in rawText — agt-app re-parses and reports it in
   * the panel Slash section.
   *
   * @param {string} rawText
   */
  #runParsed(rawText) {
    const textarea = this.#textarea;
    if (!textarea) return;
    const parsed = parseInput(rawText);
    if (parsed.kind === "chat") {
      this.#send();
      return;
    }
    textarea.value = "";
    this.#closeMenu();
    this.dispatchEvent(
      new CustomEvent("agt-command", {
        detail: { name: parsed.name, args: parsed.args, rawText },
        bubbles: true,
        composed: true,
      }),
    );
  }

  #applyDisabled() {
    const textarea = this.#textarea;
    const button = /** @type {HTMLButtonElement | null} */ (
      this.querySelector("button")
    );
    if (textarea) {
      textarea.disabled = this.#disabled;
      if (this.#disabled) this.#closeMenu();
    }
    if (button) button.disabled = this.#disabled;
  }
}

customElements.define("agt-composer", AgtComposer);
