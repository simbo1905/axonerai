// @ts-check
import { filterCommands, parseInput } from "../commands.mjs";
import "./agt-picker-menu.js";

/** @typedef {import("./agt-picker-menu.js").PickerSection} PickerSection */
/** @typedef {import("./agt-picker-menu.js").PickerItem} PickerItem */

/**
 * Composer: chat input + slash-command menu + reusable picker popup.
 *
 * Slash mode: typing `/` as the FIRST character opens the command list
 * above the input — ↑/↓ move the highlight (wrapping), Enter
 * completes+runs the highlighted command, Tab completes the name, Esc
 * closes, click selects, typing filters by prefix. Running a command
 * dispatches `agt-command` {detail:{name, args, rawText}} upward —
 * commands are control plane and never go to the model. Non-slash input
 * keeps the exact chat-send behaviour.
 *
 * Picker mode (item59): {@link AgtComposer#openPicker} opens the SAME
 * shared `agt-picker-menu` component (glossary: reusable-picker) fed by
 * caller-provided sections (e.g. the /model service/model sections). While
 * open: ↑/↓/Enter/Escape route into the picker, typing filters its rows
 * (`filter-as-you-type`), and a selection is re-dispatched upward as
 * `agt-picker-select` {detail:{kind, id, section}} — the composer itself
 * never owns model state and never calls the backend (architecture moves
 * #3/#4).
 *
 * The error line (`showError`) is where failed control-plane actions
 * (e.g. a 400 from the model swap) surface next to the input.
 */
export class AgtComposer extends HTMLElement {
  #disabled = false;
  #rendered = false;
  /** @type {HTMLTextAreaElement | null} */
  #textarea = null;
  /** @type {HTMLDivElement | null} */
  #row = null;
  /** @type {import("./agt-picker-menu.js").AgtPickerMenu | null} */
  #picker = null;
  /** @type {HTMLDivElement | null} */
  #error = null;
  /** @type {"slash" | "models" | null} null = the popup is closed. */
  #pickerKind = null;
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

    const picker = /** @type {import("./agt-picker-menu.js").AgtPickerMenu} */ (
      document.createElement("agt-picker-menu")
    );
    picker.menuId = "agt-slash-menu";
    picker.listLabel = "Slash commands";
    // The composer owns what a selection MEANS; the picker is render-only.
    picker.addEventListener("agt-picker-select", (e) => {
      const detail = /** @type {CustomEvent} */ (e).detail;
      if (!detail || typeof detail.id !== "string") return;
      if (this.#pickerKind === "slash") {
        this.#runCommand(detail.id);
        return;
      }
      // models (or any future feed): hand the typed selection upward —
      // agt-app decides what it means and owns the backend call.
      const kind = this.#pickerKind;
      this.#closePicker();
      this.dispatchEvent(
        new CustomEvent("agt-picker-select", {
          detail: { kind, ...detail },
          bubbles: true,
          composed: true,
        }),
      );
    });

    const row = document.createElement("div");
    row.className = "composer-row";

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

    const error = document.createElement("div");
    error.className = "composer-error";
    error.hidden = true;
    error.setAttribute("role", "status");
    error.setAttribute("aria-live", "polite");

    row.append(textarea, button);
    this.replaceChildren(picker, row, error);
    this.#picker = picker;
    this.#row = row;
    this.#error = error;
    this.#textarea = textarea;

    button.addEventListener("click", () => this.#send());
    textarea.addEventListener("input", () => this.#onInput());
    textarea.addEventListener("keydown", (e) => this.#onKeydown(e));
    textarea.addEventListener("blur", () => {
      // Small delay so a click on a menu option registers first.
      setTimeout(() => this.#closePicker(), 120);
    });
    this.#applyDisabled();
  }

  /**
   * Open the shared picker popup with caller-provided sections (render
   * only — the caller owns the data, e.g. the /model runner feeds
   * model-client getState()). `kind` tags the selection event so agt-app
   * can route it.
   *
   * @param {readonly import("./agt-picker-menu.js").PickerSection[]} sections
   * @param {"models"} kind
   */
  openPicker(sections, kind = "models") {
    const picker = this.#picker;
    const textarea = this.#textarea;
    if (!picker || !textarea) return;
    this.#pickerKind = kind;
    picker.sections = sections;
    picker.open();
    textarea.setAttribute("aria-expanded", "true");
  }

  /** Close the popup (any mode). */
  #closePicker() {
    const picker = this.#picker;
    const textarea = this.#textarea;
    this.#pickerKind = null;
    if (picker) picker.close();
    if (textarea) {
      textarea.setAttribute("aria-expanded", "false");
      textarea.removeAttribute("aria-activedescendant");
    }
  }

  /**
   * Surface a control-plane failure (e.g. a 400 from the model swap) in
   * the composer error line. Cleared by the next input or send.
   *
   * @param {string} message
   */
  showError(message) {
    const error = this.#error;
    if (!error) return;
    error.textContent = message;
    error.hidden = false;
  }

  #clearError() {
    const error = this.#error;
    if (error) {
      error.textContent = "";
      error.hidden = true;
    }
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
    this.#clearError();
    this.dispatchEvent(
      new CustomEvent("agt-send", { detail: { text }, bubbles: true, composed: true }),
    );
  }

  #onInput() {
    const textarea = this.#textarea;
    const picker = this.#picker;
    this.#clearError();
    if (!textarea || !picker) return;
    if (this.#pickerKind === "models") {
      // Filter-as-you-type over the picker's rows.
      picker.filter(textarea.value);
      const active = picker.activeId;
      if (active) textarea.setAttribute("aria-activedescendant", active);
      else textarea.removeAttribute("aria-activedescendant");
      return;
    }
    this.#updateSlashMenu();
  }

  #onKeydown(/** @type {KeyboardEvent} */ e) {
    const textarea = this.#textarea;
    const picker = this.#picker;
    if (!textarea || !picker) return;

    if (this.#pickerKind === "models") {
      if (picker.handleKey(e)) {
        e.preventDefault();
        const active = picker.activeId;
        if (active) textarea.setAttribute("aria-activedescendant", active);
        else textarea.removeAttribute("aria-activedescendant");
        if (!picker.isOpen) this.#pickerKind = null;
      }
      return;
    }

    if (this.#pickerKind === "slash") {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        picker.moveHighlight(e.key === "ArrowDown" ? 1 : -1);
        const active = picker.activeId;
        if (active) textarea.setAttribute("aria-activedescendant", active);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        this.#closePicker();
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        const match = this.#matches[picker.activeIndex];
        if (match) {
          textarea.value = `/${match.name} `;
          textarea.setSelectionRange(textarea.value.length, textarea.value.length);
          this.#updateSlashMenu();
        }
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        const match = this.#matches[picker.activeIndex];
        if (match) {
          picker.confirm();
        } else {
          this.#closePicker();
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

  /**
   * Slash mode: prefix-filter the command registry and render the
   * "Commands" section through the shared picker (same UX as before, now
   * the reusable component).
   */
  #updateSlashMenu() {
    const textarea = this.#textarea;
    const picker = this.#picker;
    if (!textarea || !picker) return;
    const value = textarea.value;
    if (!value.startsWith("/") || this.#disabled) {
      this.#closePicker();
      return;
    }
    this.#matches = filterCommands(value);
    if (this.#matches.length === 0) {
      this.#closePicker();
      return;
    }
    this.#pickerKind = "slash";
    picker.sections = [
      {
        name: "Commands",
        items: this.#matches.map((command) => ({
          id: command.name,
          label: `/${command.name}`,
          meta: command.description,
        })),
      },
    ];
    picker.open();
    textarea.setAttribute("aria-expanded", "true");
    const active = picker.activeId;
    if (active) textarea.setAttribute("aria-activedescendant", active);
    else textarea.removeAttribute("aria-activedescendant");
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
    // A menu selection RESOLVES a typo'd prefix ("/m" → "/model"): dispatch
    // the canonical command text so the runner re-parses the command that was
    // actually selected, never the raw prefix that failed to parse.
    const resolved = args ? `/${name} ${args}` : `/${name}`;
    textarea.value = "";
    this.#closePicker();
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
    this.#closePicker();
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
      if (this.#disabled) this.#closePicker();
    }
    if (button) button.disabled = this.#disabled;
  }
}

customElements.define("agt-composer", AgtComposer);
