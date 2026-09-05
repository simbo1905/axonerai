// @ts-check

/**
 * agt-picker-menu — the REUSABLE scrolling popup picker (item59, glossary
 * `reusable-picker`): the composer's slash-command menu generalised into one
 * render-only web component. The same component instance serves the `/`
 * command menu ("Commands" section) and the /model picker (one section per
 * service), opencode-style:
 *
 * ```
 *   ┌──────────────────────────────┐
 *   │ SECTION HEADER               │   ← .picker-header (not selectable)
 *   │   item A        meta A       │   ← .picker-option (role=option)
 *   │ ▸ item B        meta B       │   ← highlighted row (aria-selected)
 *   │   <blank line>               │   ← .picker-gap between sections
 *   │ SECTION HEADER 2             │
 *   │   item C        meta C       │
 *   └──────────────────────────────┘
 * ```
 *
 * ARCHITECTURE MOVES #3/#4 (docs/FRONTEND-ARCHITECTURE.md): this component is
 * RENDER-ONLY. It owns NO domain state and NEVER calls the backend:
 *
 * - It takes a list of sections `{ name, items: [{ id, label, meta?,
 *   current? }] }` through the `sections` property and re-renders.
 * - Sorting is the CALLER's job: "recently-used items float to the top of
 *   their section" is just the caller passing recency-sorted items (the
 *   model client owns the recency maps). The picker renders the given
 *   order as-is.
 * - It emits ONE event — `agt-picker-select` with
 *   `detail: { id, label, section }` — on click or Enter (`confirm()`).
 *   The consumer (agt-composer) decides what a selection means.
 *
 * Behaviours: keyboard nav (`handleKey`/`moveHighlight`, arrows wrap,
 * Enter selects, Escape closes), filter-as-you-type (`filter(query)` —
 * case-insensitive substring over label+id; sections whose items all miss
 * are dropped), section headers, an explicit blank line between sections,
 * and ARIA listbox/option with `aria-activedescendant` (focus stays in the
 * consumer's input; it reads {@link AgtPickerMenu#activeId}).
 *
 * Light DOM on purpose: the composer is light DOM and the page stylesheet
 * owns the popup's look.
 */

/**
 * One selectable row handed to the picker (internal).
 *
 * @typedef {object} PickerItem
 * @property {string} id stable identity sent in the select event detail
 * @property {string} label primary text
 * @property {string} [meta] fainter secondary text (description, size)
 * @property {boolean} [current] true renders the ✓ current-row mark
 */

/**
 * One section of the picker (internal).
 *
 * @typedef {object} PickerSection
 * @property {string} name header text (a service name, or "Commands")
 * @property {readonly PickerItem[]} items rows in caller-sorted order
 */

export class AgtPickerMenu extends HTMLElement {
  /** @type {readonly PickerSection[]} */
  #sections = [];
  /** @type {readonly PickerSection[]} */
  #visible = [];
  #highlight = 0;
  #open = false;
  /** @type {HTMLDivElement | null} */
  #listbox = null;

  connectedCallback() {
    if (this.#listbox) return;
    const listbox = document.createElement("div");
    listbox.className = "picker-menu";
    listbox.id = this.getAttribute("menu-id") || "agt-picker-menu";
    listbox.setAttribute(
      "role",
      "listbox",
    );
    listbox.setAttribute(
      "aria-label",
      this.getAttribute("list-label") || "Picker",
    );
    listbox.hidden = true;
    // Keep focus in the consumer's input when an option is pressed, so the
    // click lands before any blur-driven close.
    listbox.addEventListener("mousedown", (e) => e.preventDefault());
    listbox.addEventListener("click", (e) => {
      const target = /** @type {HTMLElement} */ (e.target);
      const option =
        target instanceof Element ? target.closest(".picker-option") : null;
      if (option instanceof HTMLElement && option.dataset.id) {
        this.#selectAt(Number(option.dataset.index));
      }
    });
    this.replaceChildren(listbox);
    this.#listbox = listbox;
  }

  /** The listbox element id (the consumer input's aria-controls target). */
  set menuId(/** @type {string} */ id) {
    this.setAttribute("menu-id", id);
    if (this.#listbox) this.#listbox.id = id;
  }

  /** aria-label of the listbox ("Slash commands", "Models", …). */
  set listLabel(/** @type {string} */ label) {
    this.setAttribute("list-label", label);
    if (this.#listbox) this.#listbox.setAttribute("aria-label", label);
  }

  /** The visible sections (post-filter). */
  get sections() {
    return this.#visible;
  }

  /**
   * Replace the section list (caller-sorted; resets any filter) and render.
   *
   * @param {readonly PickerSection[]} sections
   */
  set sections(sections) {
    this.#sections = Array.isArray(sections) ? [...sections] : [];
    this.#visible = this.#sections;
    if (this.#highlight >= this.#count()) this.#highlight = 0;
    this.#render();
  }

  /** Open the popup. */
  open() {
    this.#open = true;
    if (this.#listbox) this.#listbox.hidden = false;
    this.#render();
  }

  /** Close the popup (selection state resets to the first row). */
  close() {
    this.#open = false;
    this.#highlight = 0;
    if (this.#listbox) this.#listbox.hidden = true;
  }

  /** @returns {boolean} whether the popup is open */
  get isOpen() {
    return this.#open;
  }

  /**
   * Filter as you type: case-insensitive substring over each item's
   * label+id; sections whose items all miss are dropped. Empty query shows
   * everything.
   *
   * @param {string} query
   */
  filter(query) {
    const needle = (typeof query === "string" ? query : "")
      .replace(/^\//, "")
      .toLowerCase();
    if (needle === "") {
      this.#visible = this.#sections;
    } else {
      /** @type {PickerSection[]} */
      const out = [];
      for (const section of this.#sections) {
        const items = section.items.filter(
          (item) =>
            item.label.toLowerCase().includes(needle) ||
            item.id.toLowerCase().includes(needle),
        );
        if (items.length > 0) out.push({ name: section.name, items });
      }
      this.#visible = out;
    }
    if (this.#highlight >= this.#count()) this.#highlight = 0;
    this.#render();
  }

  /** Number of currently selectable rows. */
  #count() {
    return this.#visible.reduce((sum, s) => sum + s.items.length, 0);
  }

  /**
   * Move the highlight (wrapping) over the currently selectable rows.
   *
   * @param {1 | -1} delta
   */
  moveHighlight(delta) {
    const count = this.#count();
    if (count === 0) return;
    this.#highlight = (this.#highlight + delta + count) % count;
    this.#render();
  }

  /** Zero-based index of the highlighted row (over all visible items). */
  get activeIndex() {
    return this.#highlight;
  }

  /** DOM id of the highlighted option (for aria-activedescendant), or null. */
  get activeId() {
    const listbox = this.#listbox;
    if (!listbox || !this.#open) return null;
    const active = listbox.querySelector('[role="option"].active');
    return active instanceof HTMLElement ? active.id : null;
  }

  /**
   * Select the highlighted row: emits `agt-picker-select`
   * `{ detail: { id, label, section } }`. No-op when nothing is highlighted.
   */
  confirm() {
    this.#selectAt(this.#highlight);
  }

  /**
   * Keyboard contract: ArrowDown/ArrowUp move (wrap), Enter confirms,
   * Escape closes. Returns true when the key was consumed; the consumer
   * keeps ownership of Tab/completion and of keys pressed while closed.
   *
   * @param {KeyboardEvent} event
   * @returns {boolean}
   */
  handleKey(event) {
    if (!this.#open) return false;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      this.moveHighlight(event.key === "ArrowDown" ? 1 : -1);
      return true;
    }
    if (event.key === "Enter") {
      this.confirm();
      return true;
    }
    if (event.key === "Escape") {
      this.close();
      return true;
    }
    return false;
  }

  /**
   * Emit the selection event for the visible row at `index` and close.
   *
   * @param {number} index
   */
  #selectAt(index) {
    /** @type {{ id: string, label: string, section: string } | null} */
    let picked = null;
    let flat = -1;
    for (const section of this.#visible) {
      for (const item of section.items) {
        flat += 1;
        if (flat === index) {
          picked = { id: item.id, label: item.label, section: section.name };
        }
      }
    }
    if (!picked) {
      // No selectable row (e.g. Enter over an empty filtered list): close.
      if (this.#open) this.close();
      return;
    }
    this.close();
    this.dispatchEvent(
      new CustomEvent("agt-picker-select", {
        detail: picked,
        bubbles: true,
        composed: true,
      }),
    );
  }

  #render() {
    const listbox = this.#listbox;
    if (!listbox) return;
    /** @type {Node[]} */
    const children = [];
    let flat = -1;
    let sectionIndex = -1;
    for (const section of this.#visible) {
      sectionIndex += 1;
      if (sectionIndex > 0) {
        // The blank line between sections (glossary: reusable-picker).
        const gap = document.createElement("div");
        gap.className = "picker-gap";
        gap.setAttribute("aria-hidden", "true");
        children.push(gap);
      }
      const header = document.createElement("div");
      header.className = "picker-header";
      header.textContent = section.name;
      children.push(header);
      for (const item of section.items) {
        flat += 1;
        const active = flat === this.#highlight;
        const option = document.createElement("div");
        option.className = active ? "picker-option active" : "picker-option";
        option.id = `${listbox.id}-opt-${flat}`;
        option.dataset.id = item.id;
        option.dataset.index = String(flat);
        option.setAttribute("role", "option");
        option.setAttribute("aria-selected", active ? "true" : "false");
        if (item.current) {
          const mark = document.createElement("span");
          mark.className = "picker-current-mark";
          mark.textContent = "✓";
          option.append(mark);
        }
        const name = document.createElement("span");
        name.className = "picker-option-name";
        name.textContent = item.label;
        option.append(name);
        if (item.meta) {
          const meta = document.createElement("span");
          meta.className = "picker-option-meta";
          meta.textContent = item.meta;
          option.append(meta);
        }
        children.push(option);
      }
    }
    listbox.replaceChildren(...children);
  }
}

customElements.define("agt-picker-menu", AgtPickerMenu);
