// @ts-check

/**
 * Right-hand TUI-style side panel: session title, Context / MCP / LSP / Todo /
 * Slash / Built-ins text trees with ▾/▸ triangles, and a `path:branch` footer.
 * Monospace terminal styling lives entirely in this component's shadow DOM —
 * the chat keeps its existing fonts.
 *
 * Data flow: agt-app owns the network and hands validated snapshots to
 * {@link AgtPanel#setState}; tool toggles are reported upward via the
 * `agt-toggle-tool` CustomEvent (agt-app does the POST and refetches state,
 * which reverts the optimistic row on failure).
 */

/**
 * A single tool row from `/api/state`.
 *
 * @typedef {object} ToolState
 * @property {string} name
 * @property {boolean} enabled
 * @property {string} [source] "builtin" | "mcp"
 */

/**
 * `/api/state` snapshot (item27 pinned contract).
 *
 * @typedef {object} StateSnapshot
 * @property {string} provider
 * @property {string} model
 * @property {{ id: string, title: string }} session
 * @property {{ path: string | null, branch: string | null }} repo
 * @property {{ tokens: number }} context
 * @property {ToolState[]} tools
 * @property {Array<{ name: string, status: string }>} mcp
 * @property {unknown[]} lsp
 * @property {unknown} todo
 */

/**
 * Collapsible section state kept across re-renders (internal).
 *
 * @typedef {object} Section
 * @property {HTMLDivElement} root
 * @property {HTMLDivElement} content
 * @property {() => boolean} isCollapsed
 * @property {(collapsed: boolean) => void} setCollapsed
 */

const STORAGE_KEY = "agt-panel-collapsed";
const MAX_SLASH_LINES = 200;

/**
 * Build one collapsible section (DRY helper: triangle header + content box).
 *
 * @param {string} name section label, also exposed as `data-name` for tests
 * @returns {Section}
 */
function createSection(name) {
  const root = document.createElement("div");
  root.className = "agt-p-section";
  root.dataset.name = name;
  root.dataset.collapsed = "0";

  const header = document.createElement("button");
  header.type = "button";
  header.className = "agt-p-head";
  header.setAttribute("aria-expanded", "true");

  const tri = document.createElement("span");
  tri.className = "agt-p-tri";
  tri.textContent = "▾";
  const label = document.createElement("span");
  label.className = "agt-p-label";
  label.textContent = name;
  header.append(tri, label);

  const content = document.createElement("div");
  content.className = "agt-p-content";

  header.addEventListener("click", () => {
    setCollapsed(!isCollapsed());
  });

  function isCollapsed() {
    return root.dataset.collapsed === "1";
  }

  /**
   * @param {boolean} collapsed
   */
  function setCollapsed(collapsed) {
    root.dataset.collapsed = collapsed ? "1" : "0";
    content.hidden = collapsed;
    tri.textContent = collapsed ? "▸" : "▾";
    header.setAttribute("aria-expanded", collapsed ? "false" : "true");
  }

  root.append(header, content);
  return { root, content, isCollapsed, setCollapsed };
}

/**
 * "connected" → "Connected" (status strings arrive lowercase).
 *
 * @param {string} status
 */
function capitalize(status) {
  return status.length > 0 ? status[0].toUpperCase() + status.slice(1) : status;
}

export class AgtPanel extends HTMLElement {
  #rendered = false;
  /** @type {Map<string, Section>} */
  #sections = new Map();
  /** @type {HTMLSpanElement | null} */
  #titleEl = null;
  /** @type {HTMLDivElement | null} */
  #slashLog = null;
  /** @type {HTMLDivElement | null} */
  #footerEl = null;
  /** @type {HTMLDivElement | null} */
  #bodyEl = null;
  /** @type {ToolState[]} */
  #tools = [];
  /** @type {((name: string, enabled: boolean) => void) | null} */
  #onToggle = null;
  /** @type {HTMLButtonElement | null} */
  #collapseBtn = null;
  #collapsed = false;

  connectedCallback() {
    if (this.#rendered) return;
    this.#rendered = true;

    const shadow = this.attachShadow({ mode: "open" });

    const style = document.createElement("style");
    style.textContent = `
      :host {
        display: block;
        height: 100%;
        min-height: 0;
        font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
        font-size: 12px;
        line-height: 1.5;
        color: #a7f3d0;
        background: #0a0f1c;
        border: 1px solid #1e293b;
        border-radius: 10px;
        overflow: hidden;
      }
      .agt-p-panel { display: flex; flex-direction: column; height: 100%; }
      .agt-p-headbar {
        display: flex; align-items: center; gap: 6px;
        padding: 8px 10px; border-bottom: 1px solid #1e293b;
      }
      .agt-p-title {
        flex: 1; color: #e5e7eb; font-weight: 600;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
      }
      .agt-p-collapse {
        background: none; border: 1px solid #334155; border-radius: 4px;
        color: #94a3b8; cursor: pointer; font: inherit; padding: 0 6px;
      }
      .agt-p-collapse:hover { color: #e5e7eb; border-color: #64748b; }
      .agt-p-body { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0; }
      .agt-p-section .agt-p-head {
        display: flex; align-items: center; gap: 6px; width: 100%;
        background: none; border: none; padding: 3px 10px;
        color: #7dd3fc; cursor: pointer; font: inherit; text-align: left;
      }
      .agt-p-section .agt-p-head:hover { color: #bae6fd; }
      .agt-p-tri { width: 1ch; flex: none; }
      .agt-p-content { padding: 1px 10px 6px 27px; white-space: pre-wrap; }
      .agt-p-content[hidden] { display: none; }
      .agt-p-line { color: #a7f3d0; }
      .agt-p-dim { color: #64748b; }
      .agt-p-err { color: #fca5a5; }
      .agt-p-slash-log {
        max-height: 170px; overflow-y: auto;
        border-left: 1px solid #334155; padding-left: 6px; margin-left: -7px;
      }
      .agt-p-tool {
        display: flex; align-items: center; gap: 6px; cursor: pointer;
        color: #a7f3d0;
      }
      .agt-p-tool input[type="checkbox"] {
        appearance: none; position: absolute; opacity: 0; width: 1px; height: 1px;
      }
      .agt-p-tool .agt-p-mark { width: 3ch; flex: none; color: #fbbf24; }
      .agt-p-tool input:focus-visible ~ .agt-p-mark {
        outline: 1px solid #3b82f6;
      }
      .agt-p-footer {
        border-top: 1px solid #1e293b; padding: 8px 10px;
        color: #64748b; word-break: break-all;
      }
    `;

    const panel = document.createElement("div");
    panel.className = "agt-p-panel";

    const headbar = document.createElement("div");
    headbar.className = "agt-p-headbar";
    this.#titleEl = document.createElement("span");
    this.#titleEl.className = "agt-p-title";
    this.#titleEl.textContent = "(no session)";
    this.#collapseBtn = document.createElement("button");
    this.#collapseBtn.type = "button";
    this.#collapseBtn.className = "agt-p-collapse";
    this.#collapseBtn.textContent = "▸";
    this.#collapseBtn.title = "Collapse panel";
    this.#collapseBtn.setAttribute("aria-label", "Collapse panel");
    headbar.append(this.#titleEl, this.#collapseBtn);

    this.#bodyEl = document.createElement("div");
    this.#bodyEl.className = "agt-p-body";
    for (const name of ["Context", "MCP", "LSP", "Todo", "Slash", "Built-ins"]) {
      const section = createSection(name);
      this.#sections.set(name, section);
      this.#bodyEl.append(section.root);
    }

    this.#footerEl = document.createElement("div");
    this.#footerEl.className = "agt-p-footer";
    this.#footerEl.textContent = "(unavailable)";

    panel.append(headbar, this.#bodyEl, this.#footerEl);
    shadow.append(style, panel);

    this.#slashLog = /** @type {HTMLDivElement} */ (
      this.#sections.get("Slash")?.content.querySelector(".agt-p-slash-log")
    );
    if (!this.#slashLog) {
      this.#slashLog = document.createElement("div");
      this.#slashLog.className = "agt-p-slash-log";
      this.#sections.get("Slash")?.content.append(this.#slashLog);
    }

    this.#collapsed = localStorage.getItem(STORAGE_KEY) === "1";
    this.#applyCollapsed();
    this.#collapseBtn.addEventListener("click", () => {
      this.#collapsed = !this.#collapsed;
      localStorage.setItem(STORAGE_KEY, this.#collapsed ? "1" : "0");
      this.#applyCollapsed();
    });

    // Initial graceful state until the first /api/state snapshot arrives.
    this.setState(null);
  }

  /**
   * Consume a `/api/state` snapshot (deep-frozen by agt-app). `null` renders
   * the graceful "(unavailable)" state (backend not yet rebuilt / 404).
   *
   * @param {Readonly<StateSnapshot> | null} snapshot
   */
  setState(snapshot) {
    if (!this.#rendered) return;
    if (snapshot === null || snapshot === undefined) {
      this.#setUnavailable();
      return;
    }

    this.setSessionTitle(
      typeof snapshot.session?.title === "string" && snapshot.session.title
        ? snapshot.session.title
        : "(no session)",
    );

    const context = this.#sections.get("Context");
    if (context) {
      const tokens =
        typeof snapshot.context?.tokens === "number"
          ? snapshot.context.tokens
          : null;
      this.#setLines(context, [
        tokens === null
          ? { text: "(unavailable)", cls: "agt-p-dim" }
          : { text: `${tokens} tokens`, cls: "agt-p-line" },
      ]);
    }

    const mcp = this.#sections.get("MCP");
    if (mcp) {
      this.#setLines(
        mcp,
        Array.isArray(snapshot.mcp) && snapshot.mcp.length > 0
          ? snapshot.mcp.map((entry) => ({
              text: `${entry.name} ${capitalize(String(entry.status))}`,
              cls: "agt-p-line",
            }))
          : [{ text: "(none)", cls: "agt-p-dim" }],
      );
    }

    const lsp = this.#sections.get("LSP");
    if (lsp) {
      this.#setLines(lsp, [
        Array.isArray(snapshot.lsp) && snapshot.lsp.length > 0
          ? { text: snapshot.lsp.map(String).join("\n"), cls: "agt-p-line" }
          : { text: "(none)", cls: "agt-p-dim" },
      ]);
    }

    const todo = this.#sections.get("Todo");
    if (todo) {
      // Coming soon — not a feature yet.
      this.#setLines(todo, [{ text: "(none)", cls: "agt-p-dim" }]);
    }

    this.#tools = Array.isArray(snapshot.tools) ? [...snapshot.tools] : [];
    this.#renderTools();

    if (this.#footerEl) {
      const path = snapshot.repo?.path ?? "(unknown)";
      const branch = snapshot.repo?.branch ?? "(unknown)";
      this.#footerEl.textContent = `${path}:${branch}`;
    }
  }

  /**
   * Update the session title (live: WS `session_meta`, rename ack).
   *
   * @param {string} title
   */
  setSessionTitle(title) {
    if (this.#titleEl && typeof title === "string" && title) {
      this.#titleEl.textContent = title;
    }
  }

  /**
   * Land a slash-command invocation echo: since item32 the command RESULT
   * goes to the console bus (console.html); the Slash tree keeps only the
   * command line that was run. Collapses every other tree, expands Slash,
   * appends the echoed command line to the scrollable Slash log.
   *
   * @param {string} rawText the command line that was run (e.g. "/model")
   */
  echoSlash(rawText) {
    if (!this.#rendered) return;
    for (const [name, section] of this.#sections) {
      section.setCollapsed(name === "Slash" ? false : true);
    }
    const log = this.#slashLog;
    if (!log) return;
    const div = document.createElement("div");
    div.className = "agt-p-line";
    div.textContent = String(rawText).split("\n", 1)[0] ?? String(rawText);
    log.append(div);
    while (log.children.length > MAX_SLASH_LINES) {
      log.firstElementChild?.remove();
    }
    log.scrollTop = log.scrollHeight;
  }

  /**
   * Render the Built-ins tool rows. `onToggle` (optional) is called with
   * `(name, enabled)` when a row flips; the element ALSO fires the bubbling
   * `agt-toggle-tool` CustomEvent so agt-app can stay decoupled. Row updates
   * are optimistic; agt-app refetches /api/state which reverts on failure.
   *
   * @param {ToolState[]} tools
   * @param {((name: string, enabled: boolean) => void) | null} [onToggle]
   */
  showBuiltins(tools, onToggle = null) {
    this.#tools = Array.isArray(tools) ? [...tools] : [];
    this.#onToggle = onToggle ?? null;
    this.#renderTools();
  }

  /** Expand the Built-ins tree (e.g. after running /built-ins). */
  openBuiltins() {
    this.#sections.get("Built-ins")?.setCollapsed(false);
  }

  /**
   * Replace a section's content with one div per line.
   *
   * @param {Section} section
   * @param {Array<{ text: string, cls: string }>} lines
   */
  #setLines(section, lines) {
    const content = section.content;
    content.replaceChildren(
      ...lines.map((line) => {
        const div = document.createElement("div");
        div.className = line.cls;
        div.textContent = line.text;
        return div;
      }),
    );
  }

  #renderTools() {
    const section = this.#sections.get("Built-ins");
    if (!section) return;
    if (this.#tools.length === 0) {
      this.#setLines(section, [{ text: "(none)", cls: "agt-p-dim" }]);
      return;
    }
    const frag = document.createDocumentFragment();
    for (const tool of this.#tools) {
      const row = document.createElement("label");
      row.className = "agt-p-tool";

      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = Boolean(tool.enabled);
      box.setAttribute("aria-label", `Toggle tool ${tool.name}`);

      const mark = document.createElement("span");
      mark.className = "agt-p-mark";
      mark.textContent = box.checked ? "[x]" : "[ ]";

      const name = document.createElement("span");
      name.textContent = tool.name;

      box.addEventListener("change", () => {
        const enabled = box.checked;
        mark.textContent = enabled ? "[x]" : "[ ]";
        if (this.#onToggle) this.#onToggle(tool.name, enabled);
        this.dispatchEvent(
          new CustomEvent("agt-toggle-tool", {
            detail: { name: tool.name, enabled },
            bubbles: true,
            composed: true,
          }),
        );
      });

      row.append(box, mark, name);
      frag.append(row);
    }
    section.content.replaceChildren(frag);
  }

  #setUnavailable() {
    this.setSessionTitle("(unavailable)");
    for (const name of ["Context", "MCP", "LSP", "Todo", "Built-ins"]) {
      const section = this.#sections.get(name);
      if (section) {
        this.#setLines(section, [{ text: "(unavailable)", cls: "agt-p-dim" }]);
      }
    }
    if (this.#footerEl) this.#footerEl.textContent = "(unavailable)";
  }

  #applyCollapsed() {
    if (this.#bodyEl) this.#bodyEl.hidden = this.#collapsed;
    if (this.#footerEl) this.#footerEl.hidden = this.#collapsed;
    if (this.#collapseBtn) {
      this.#collapseBtn.textContent = this.#collapsed ? "◂" : "▸";
      this.#collapseBtn.title = this.#collapsed ? "Expand panel" : "Collapse panel";
      this.#collapseBtn.setAttribute(
        "aria-label",
        this.#collapsed ? "Expand panel" : "Collapse panel",
      );
    }
    if (this.#collapsed) {
      this.setAttribute("collapsed", "");
    } else {
      this.removeAttribute("collapsed");
    }
  }
}

customElements.define("agt-panel", AgtPanel);
