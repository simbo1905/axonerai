// @ts-check
/**
 * One console entry rendered devtools-style (item32): a monospace row
 * `HH:MM:SS.mmm  [LEVEL]  text` with the emitting pageId as a dim suffix.
 * Level colouring: error red, warn amber, info blue, log default. Multi-line
 * text renders inside a wrapped, scrollable pre block.
 *
 * Self-contained styling in shadow DOM; used by agt-console-app only.
 */

/**
 * Format a unix-epoch millisecond timestamp as a local 24-hour wall clock
 * with milliseconds: `hh:mm:ss.mmm`. Non-finite input renders as
 * `--:--:--.---`.
 *
 * @param {number} ms unix epoch milliseconds
 * @returns {string}
 */
function formatClockMs(ms) {
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "--:--:--.---";
  const pad = (/** @type {number} */ n, width = 2) =>
    String(n).padStart(width, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

/** Uppercase devtools-style level tag: `ERROR`, `WARN`, … */
const LEVEL_TAGS = {
  log: "LOG",
  info: "INFO",
  warn: "WARN",
  error: "ERROR",
};

export class AgtConsoleLine extends HTMLElement {
  /** @type {Readonly<import("../console-model.mjs").ConsoleEntry> | null} */
  #entry = null;

  /** @param {Readonly<import("../console-model.mjs").ConsoleEntry>} entry */
  set entry(entry) {
    this.#entry = entry;
    this.#render();
  }

  /** @returns {Readonly<import("../console-model.mjs").ConsoleEntry> | null} */
  get entry() {
    return this.#entry;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const entry = this.#entry;
    if (!entry) return;
    const shadow = this.shadowRoot ?? this.attachShadow({ mode: "open" });

    if (!shadow.querySelector("style")) {
      const style = document.createElement("style");
      style.textContent = `
        :host { display: block; }
        .row {
          display: flex; align-items: baseline; gap: 8px;
          padding: 2px 10px;
          border-bottom: 1px solid rgba(30, 41, 59, 0.5);
          font: 12px/1.5 ui-monospace, "SF Mono", Menlo, Consolas, monospace;
        }
        .ts { flex: none; color: #64748b; }
        .lvl { flex: none; min-width: 5ch; font-weight: 600; }
        .lvl.log { color: #94a3b8; }
        .lvl.info { color: #60a5fa; }
        .lvl.warn { color: #fbbf24; }
        .lvl.error { color: #f87171; }
        .row.error { background: rgba(127, 29, 29, 0.18); }
        .row.warn { background: rgba(120, 53, 15, 0.14); }
        .text {
          flex: 1; min-width: 0; margin: 0;
          color: #e5e7eb;
          white-space: pre-wrap; word-break: break-word;
          max-height: 200px; overflow-y: auto;
        }
        .page {
          flex: none; max-width: 12ch;
          overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
          color: #475569; font-size: 11px;
        }
      `;
      shadow.append(style);
    }

    const row = document.createElement("div");
    row.className = `row ${entry.level}`;
    row.dataset.level = entry.level;
    row.dataset.id = entry.id;

    const ts = document.createElement("span");
    ts.className = "ts";
    ts.textContent = formatClockMs(entry.ts);

    const lvl = document.createElement("span");
    lvl.className = `lvl ${entry.level}`;
    lvl.textContent = `[${LEVEL_TAGS[entry.level] ?? entry.level.toUpperCase()}]`;

    const text = document.createElement("pre");
    text.className = "text";
    text.textContent = entry.text;

    const page = document.createElement("span");
    page.className = "page";
    page.textContent = entry.pageId;

    row.append(ts, lvl, text, page);
    const style = shadow.querySelector("style");
    shadow.replaceChildren(...(style ? [style] : []), row);
  }
}

customElements.define("agt-console-line", AgtConsoleLine);
