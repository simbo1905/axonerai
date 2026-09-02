// @ts-check
/**
 * One `tool_call` event rendered as a dim terminal-style single line (not a
 * bubble): `▸ <tool> <args first line>… ↑<bytes> ↓<bytes> <duration>
 * <hh:mm:ss>`. Clicking the triangle (▸→▾) lazily expands the pretty-printed
 * payload via the WASM pretty-printer; a WASM failure renders the raw text
 * instead and never breaks the chat.
 *
 * Self-contained styling in shadow DOM (dim monospace over the chat
 * background). Used by agt-chat-log only when the /verbose flag is ON; the
 * partial (catch-up) tool_call records render the same way — their
 * `args_pretty`/`result_pretty` ARE the truncated pretty heads.
 */
import { formatBytes, formatClock, formatDuration } from "../format.mjs";
import { initPretty, prettyPrintAbridged } from "../pretty.mjs";

/** Max characters of the args first line shown on the summary row. */
const ARGS_LINE_MAX = 48;

/**
 * First line of the pretty args, abridged for the summary row.
 *
 * @param {string} argsPretty
 * @param {boolean} abridged true when the record is a truncated catch-up
 * reconstruction (the head is already cut, so the ellipsis is unconditional)
 * @returns {string}
 */
function argsSummary(argsPretty, abridged) {
  const firstLine = (argsPretty.split("\n", 1)[0] ?? "").trim();
  if (firstLine.length > ARGS_LINE_MAX) {
    return `${firstLine.slice(0, ARGS_LINE_MAX)}…`;
  }
  return abridged || argsPretty.includes("\n") ? `${firstLine}…` : firstLine;
}

export class AgtToolLine extends HTMLElement {
  /** @type {Readonly<import("/src/wire.mjs").ToolCallEvent> | null} */
  #event = null;
  #expanded = false;
  /** @type {HTMLDivElement | null} */
  #payload = null;
  /** @type {HTMLSpanElement | null} */
  #tri = null;
  /** Whether the lazy pretty-print expansion already ran. */
  #expandedOnce = false;

  /** @param {Readonly<import("/src/wire.mjs").ToolCallEvent>} event */
  set event(event) {
    this.#event = event;
    this.#expanded = false;
    this.#expandedOnce = false;
    this.#render();
  }

  /** @returns {Readonly<import("/src/wire.mjs").ToolCallEvent> | null} */
  get event() {
    return this.#event;
  }

  /** Whether the payload is expanded (exposed for tests). */
  get expanded() {
    return this.#expanded;
  }

  connectedCallback() {
    this.#render();
  }

  #render() {
    const event = this.#event;
    if (!event) return;
    const abridged = "abridged" in event && Boolean(event.abridged);
    const shadow = this.shadowRoot ?? this.attachShadow({ mode: "open" });

    if (!shadow.querySelector("style")) {
      const style = document.createElement("style");
      style.textContent = `
        :host { display: block; }
        .line {
          display: flex; align-items: baseline; gap: 6px;
          width: 100%; box-sizing: border-box;
          background: transparent; border: none; cursor: pointer;
          padding: 2px 8px; text-align: left;
          font: 12px/1.5 ui-monospace, "SF Mono", Menlo, Consolas, monospace;
          color: #64748b;
        }
        .line:hover { color: #94a3b8; }
        .tri { flex: none; }
        .tool { color: #7dd3fc; }
        .payload {
          padding: 2px 8px 4px 22px;
          font: 12px/1.5 ui-monospace, "SF Mono", Menlo, Consolas, monospace;
          color: #64748b;
          white-space: pre-wrap; word-break: break-word;
        }
        .payload[hidden] { display: none; }
        .payload .label { color: #94a3b8; }
      `;
      shadow.append(style);
    }

    const line = document.createElement("button");
    line.type = "button";
    line.className = "line";

    const tri = document.createElement("span");
    tri.className = "tri";
    tri.textContent = "▸";

    const summary = document.createElement("span");
    summary.className = "summary";
    const tool = document.createElement("span");
    tool.className = "tool";
    tool.textContent = event.tool;
    summary.append(
      tool,
      document.createTextNode(
        ` ${argsSummary(event.args_pretty, abridged)} ↑${formatBytes(event.bytes_up)} ↓${formatBytes(event.bytes_down)} ${formatDuration(event.duration_ms)} ${formatClock(event.ts)}`,
      ),
    );

    line.append(tri, summary);
    line.addEventListener("click", () => this.#toggle());

    const payload = document.createElement("div");
    payload.className = "payload";
    payload.hidden = true;

    this.#tri = tri;
    this.#payload = payload;
    // Re-renders keep the (already installed) style node.
    const style = shadow.querySelector("style");
    shadow.replaceChildren(...(style ? [style] : []), line, payload);
  }

  async #toggle() {
    this.#expanded = !this.#expanded;
    if (this.#tri) this.#tri.textContent = this.#expanded ? "▾" : "▸";
    if (this.#payload) this.#payload.hidden = !this.#expanded;
    if (this.#expanded && !this.#expandedOnce) {
      this.#expandedOnce = true;
      await this.#renderPayload();
    }
  }

  /**
   * Lazy pretty-print of the payload heads on first expand. Any failure
   * (wasm init, printer) falls back to the raw text — never breaks chat.
   */
  async #renderPayload() {
    const event = this.#event;
    const payload = this.#payload;
    if (!event || !payload) return;
    payload.replaceChildren();

    /**
     * @param {string} label
     * @param {string} text
     * @returns {Promise<HTMLElement>}
     */
    const section = async (label, text) => {
      const container = document.createElement("div");
      const head = document.createElement("div");
      head.className = "label";
      head.textContent = label;
      const pre = document.createElement("pre");
      pre.dataset.part = label === "args" ? "args" : "result";
      try {
        await initPretty();
        pre.textContent = await prettyPrintAbridged(text);
      } catch {
        pre.textContent = text;
      }
      container.append(head, pre);
      return container;
    };

    payload.append(
      await section("args", event.args_pretty),
      await section("result", event.result_pretty),
    );
  }
}

customElements.define("agt-tool-line", AgtToolLine);
