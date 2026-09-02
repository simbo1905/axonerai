// @ts-check
/**
 * Devtools-style console popup (item32), hosted by /console.html. On connect:
 * - installs the console bus for its own context (captures its own console);
 * - SUBSCRIBES to the spooled live channel FIRST — the bus's
 *   `agt-console-spooled` stream, on which the spool worker re-broadcasts
 *   each envelope only AFTER the IndexedDB commit (persist-then-broadcast,
 *   item31.9). Boot order is load-bearing: subscribe → buffer → read the
 *   backlog snapshot. A commit before the snapshot lands in it; a commit
 *   after it re-broadcasts after the subscription. The two overlap modes are
 *   absorbed by dedupe-by-id (belt, not load-bearing);
 * - reads the IndexedDB backlog (LATE ARRIVERS: everything committed before
 *   this context subscribed) via console-bus.getBacklog();
 * - renders a classic scrollable console: text filter, level checkboxes,
 *   entry count, Clear button, and a devtools-style autoscroll policy
 *   (stick to bottom unless the user scrolled up, then a "N new — jump to
 *   bottom" pill).
 *
 * Entries live in an immutable append-only frozen snapshot array (repo store
 * style); every render works from the frozen snapshot. Pure logic
 * (merge/dedupe, autoscroll arithmetic) lives in ../console-model.mjs and is
 * unit-tested in node; the DOM is tested in a single-page headless runner
 * that injects frozen entries via the {@link consoleScreenIo} seam — never
 * by orchestrating BroadcastChannel/worker/IDB across pages.
 */
import {
  clearBacklog,
  getBacklog,
  getConsoleBus,
  installConsoleBus,
} from "../console-bus.mjs";
import {
  isAtBottom,
  mergeEntries,
  newCountOnAppend,
  seqOf,
} from "../console-model.mjs";
import { deepFreeze } from "../wire.mjs";
import { validateConsole_entry } from "/generated/validators.mjs";
import "./agt-console-line.js";

const LEVELS = /** @type {const} */ (["log", "info", "warn", "error"]);
/** Distance from the bottom (px) that still counts as "stuck to bottom". */
const STICKY_SLACK = 32;

/**
 * IO seams between the console screen and the console bus. Production
 * defaults come from console-bus.mjs; the single-page headless DOM test
 * overrides these to inject frozen validated entries and a stubbed
 * backlog/clear directly (repo rule: test the DOM alone — never orchestrate
 * BroadcastChannel/worker/IndexedDB across pages).
 *
 * @type {{
 *   getBacklog: () => Promise<ReadonlyArray<Readonly<import("../console-model.mjs").ConsoleEntry>>>,
 *   clearBacklog: () => Promise<void>,
 *   subscribe: (handler: (entry: Readonly<import("../console-model.mjs").ConsoleEntry>) => void) => () => void,
 * }}
 */
export const consoleScreenIo = {
  getBacklog,
  clearBacklog,
  subscribe: (handler) =>
    getConsoleBus()?.subscribe(handler) ?? (() => {}),
};

export class AgtConsoleApp extends HTMLElement {
  /** Immutable append-only log of validated frozen envelopes. */
  /** @type {ReadonlyArray<Readonly<import("../console-model.mjs").ConsoleEntry>>} */
  #entries = deepFreeze([]);
  /** @type {Set<string>} seen envelope ids (dedupe across backlog + live) */
  #seen = new Set();
  /** @type {Record<typeof LEVELS[number], boolean>} */
  #levels = { log: true, info: true, warn: true, error: true };
  /** @type {string} lowercase substring filter ("" = show all) */
  #filter = "";
  /** Autoscroll policy: stick to bottom unless the user scrolled up. */
  #stick = true;
  /** Entries that arrived while the user had scrolled up. */
  #newCount = 0;
  #wired = false;
  /** @type {(() => void) | null} */
  #unsubscribe = null;
  /** @type {HTMLInputElement | null} */
  #filterInput = null;
  /** @type {HTMLDivElement | null} */
  #logEl = null;
  /** @type {HTMLButtonElement | null} */
  #pillEl = null;
  /** @type {HTMLSpanElement | null} */
  #countEl = null;

  /** Frozen snapshot of all entries (filtered rendering is view-side). */
  get entries() {
    return this.#entries;
  }

  /** Number of entries visible under the current filter + level toggles. */
  get visibleCount() {
    return this.#visible().length;
  }

  connectedCallback() {
    if (this.#wired) return;
    this.#wired = true;

    // Capture this popup's own console too (idempotent per context).
    installConsoleBus();

    const shadow = this.attachShadow({ mode: "open" });

    const style = document.createElement("style");
    style.textContent = `
      :host {
        display: flex; flex-direction: column;
        font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
        font-size: 12px; line-height: 1.5;
        background: #06080f; color: #e5e7eb;
      }
      .toolbar {
        display: flex; align-items: center; gap: 10px;
        padding: 6px 10px; flex: none;
        background: #0a0f1c; border-bottom: 1px solid #1e293b;
      }
      .filter {
        flex: 0 1 240px; min-width: 120px;
        background: #0f172a; color: #e5e7eb;
        border: 1px solid #334155; border-radius: 4px;
        padding: 3px 8px; font: inherit;
      }
      .filter:focus { outline: 1px solid #3b82f6; border-color: #3b82f6; }
      label.level {
        display: flex; align-items: center; gap: 3px; cursor: pointer;
        color: #94a3b8; user-select: none;
      }
      label.level input { accent-color: #3b82f6; cursor: pointer; }
      label.level.error { color: #f87171; }
      label.level.warn { color: #fbbf24; }
      label.level.info { color: #60a5fa; }
      .count { margin-left: auto; color: #64748b; white-space: nowrap; }
      .clear {
        background: none; border: 1px solid #334155; border-radius: 4px;
        color: #94a3b8; cursor: pointer; font: inherit; padding: 2px 10px;
      }
      .clear:hover { color: #e5e7eb; border-color: #64748b; }
      .log {
        flex: 1; min-height: 0; overflow-y: auto;
        position: relative;
      }
      .log .empty { padding: 8px 10px; color: #475569; }
      .pill {
        position: sticky; bottom: 8px; margin: 0 auto; display: block;
        background: #1d4ed8; color: #ffffff;
        border: none; border-radius: 999px; padding: 3px 14px;
        font: inherit; cursor: pointer;
        box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
      }
      .pill[hidden] { display: none; }
    `;

    const toolbar = document.createElement("div");
    toolbar.className = "toolbar";

    this.#filterInput = document.createElement("input");
    this.#filterInput.type = "text";
    this.#filterInput.className = "filter";
    this.#filterInput.dataset.name = "filter";
    this.#filterInput.placeholder = "filter";
    this.#filterInput.setAttribute("aria-label", "Filter console entries");
    this.#filterInput.addEventListener("input", () => {
      this.#filter = this.#filterInput?.value.toLowerCase() ?? "";
      this.#render();
    });
    toolbar.append(this.#filterInput);

    for (const level of LEVELS) {
      const label = document.createElement("label");
      label.className = `level ${level}`;
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = true;
      box.dataset.level = level;
      box.addEventListener("change", () => {
        this.#levels[level] = box.checked;
        this.#render();
      });
      label.append(box, document.createTextNode(level));
      toolbar.append(label);
    }

    this.#countEl = document.createElement("span");
    this.#countEl.className = "count";
    this.#countEl.dataset.name = "count";

    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "clear";
    clear.dataset.name = "clear";
    clear.textContent = "Clear";
    clear.addEventListener("click", () => {
      void this.#clear();
    });

    toolbar.append(this.#countEl, clear);

    this.#logEl = document.createElement("div");
    this.#logEl.className = "log";
    this.#logEl.dataset.name = "log";
    this.#logEl.addEventListener("scroll", () => this.#onScroll());

    this.#pillEl = document.createElement("button");
    this.#pillEl.type = "button";
    this.#pillEl.className = "pill";
    this.#pillEl.dataset.name = "pill";
    this.#pillEl.hidden = true;
    this.#pillEl.addEventListener("click", () => this.#jumpToBottom());

    shadow.append(style, toolbar, this.#logEl, this.#pillEl);

    this.#render();
    void this.#boot();
  }

  /**
   * Boot — ORDER IS LOAD-BEARING (item31.9): subscribe to the spooled live
   * channel FIRST (buffering until the backlog lands), THEN open the
   * readonly IndexedDB snapshot. A transaction committing before the
   * snapshot is created is in it; one committing after fires
   * `tx.oncomplete` → re-broadcast → arrives live. The overlap case
   * (commit before snapshot, re-broadcast after subscription) is absorbed
   * by dedupe-by-id. Reversing the order re-opens a symmetric gap.
   */
  async #boot() {
    /** @type {import("../console-model.mjs").ConsoleEntry[]} */
    const buffered = [];
    let ready = false;
    this.#unsubscribe = consoleScreenIo.subscribe((entry) => {
      // Render-boundary validation: the IDB backlog is already validated;
      // the live BroadcastChannel payload is not trusted. Invalid payloads
      // are dropped silently here — the spool worker reports them.
      if (validateConsole_entry(entry).length > 0) return;
      if (ready) this.#merge([entry]);
      else buffered.push(entry);
    });
    const backlog = await consoleScreenIo.getBacklog();
    this.#merge(backlog);
    ready = true;
    this.#merge(buffered);
  }

  disconnectedCallback() {
    if (this.#unsubscribe) {
      this.#unsubscribe();
      this.#unsubscribe = null;
    }
  }

  /**
   * Merge entries into the append-only log: dedupe by id, sort ts → seq,
   * freeze a new snapshot, render. Autoscroll: while stuck to bottom the
   * view follows; otherwise each NEW entry bumps the "N new" pill.
   *
   * @param {readonly import("../console-model.mjs").ConsoleEntry[]} incoming
   */
  #merge(incoming) {
    const fresh = incoming.filter((entry) => !this.#seen.has(entry.id));
    if (fresh.length === 0) return;
    for (const entry of fresh) this.#seen.add(entry.id);
    this.#entries = deepFreeze(mergeEntries(this.#entries, fresh));
    this.#newCount = newCountOnAppend(this.#newCount, this.#stick, fresh.length);
    this.#render();
  }

  /**
   * @param {Readonly<import("../console-model.mjs").ConsoleEntry>} entry
   * @returns {boolean}
   */
  #isVisible(entry) {
    return this.#levels[entry.level] && entry.text.toLowerCase().includes(this.#filter);
  }

  /** @returns {ReadonlyArray<Readonly<import("../console-model.mjs").ConsoleEntry>>} */
  #visible() {
    if (this.#filter === "" && Object.values(this.#levels).every(Boolean)) {
      return this.#entries;
    }
    return this.#entries.filter((entry) => this.#isVisible(entry));
  }

  #render() {
    const log = this.#logEl;
    if (!log) return;
    const visible = this.#visible();
    if (visible.length === 0) {
      const empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "(no entries)";
      log.replaceChildren(empty);
    } else {
      const frag = document.createDocumentFragment();
      for (const entry of visible) {
        const line = document.createElement("agt-console-line");
        /** @type {import("./agt-console-line.js").AgtConsoleLine} */ (line).entry = entry;
        frag.append(line);
      }
      log.replaceChildren(frag);
    }
    if (this.#countEl) {
      this.#countEl.textContent = `${visible.length}/${this.#entries.length} entries`;
    }
    this.#updatePill();
    if (this.#stick) this.#scrollToBottom();
  }

  #onScroll() {
    const log = this.#logEl;
    if (!log) return;
    const atBottom = isAtBottom(log, STICKY_SLACK);
    this.#stick = atBottom;
    if (atBottom) this.#newCount = 0;
    this.#updatePill();
  }

  #updatePill() {
    if (!this.#pillEl) return;
    this.#pillEl.hidden = this.#stick || this.#newCount === 0;
    this.#pillEl.textContent = `${this.#newCount} new — jump to bottom`;
  }

  #jumpToBottom() {
    this.#stick = true;
    this.#newCount = 0;
    this.#updatePill();
    this.#scrollToBottom();
  }

  #scrollToBottom() {
    const log = this.#logEl;
    if (log) log.scrollTop = log.scrollHeight;
  }

  async #clear() {
    await consoleScreenIo.clearBacklog();
    this.#entries = deepFreeze([]);
    this.#seen.clear();
    this.#newCount = 0;
    this.#render();
  }
}

customElements.define("agt-console-app", AgtConsoleApp);
