// @ts-check
import { footerSegments, formatFooter } from "../footer.mjs";
import { getState, subscribe } from "../model-client.mjs";

/**
 * Right-hand TUI-style side panel: session title, Context / MCP / LSP / Todo /
 * Models / Slash / Built-ins text trees with ▾/▸ triangles, and a status-bar
 * footer (`Chat · <model> <service> · think off` on the left, context use
 * `<used>K (<percent>%)` on the right).
 * Monospace terminal styling lives entirely in this component's shadow DOM —
 * the chat keeps its existing fonts.
 *
 * Data flow: agt-app owns the network and hands validated snapshots to
 * {@link AgtPanel#setState}; tool toggles are reported upward via the
 * `agt-toggle-tool` CustomEvent and (item48) MCP server toggles via
 * `agt-toggle-mcp` (agt-app does the POSTs and refetches state, which
 * reverts the optimistic row on failure).
 *
 * Model domain (item59): the panel SUBSCRIBES FIRST to the model client's
 * `model_changed` stream, THEN reads getState() (late-arriver rule) — the
 * footer (service+model+context window) and the FYI-only Models tree render
 * from that frozen state and re-render when a `model_changed` event arrives
 * (the current model is marked). The Models tree is never clickable and the
 * footer renders blank until the model client's init has resolved.
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
 * `/api/state` snapshot (item27 pinned contract; item48 added the per-MCP
 * `enabled` toggle state; item57 renamed `provider` to `service`).
 *
 * @typedef {object} StateSnapshot
 * @property {string} service
 * @property {string} model
 * @property {{ id: string, title: string }} session
 * @property {{ path: string | null, branch: string | null }} repo
 * @property {{ tokens: number, context_window?: number }} context
 * @property {ToolState[]} tools
 * @property {McpServerState[]} mcp
 * @property {unknown[]} lsp
 * @property {unknown} todo
 */

/**
 * The footer/Models-tree slice of the model domain (item59): current
 * service+model plus the current model's context window from the roster
 * entry. Null while the model client's init has not resolved (the footer
 * renders blank, never an error).
 *
 * @typedef {object} ModelViewState
 * @property {string | null} service
 * @property {string | null} model
 * @property {number | null} contextWindow
 */

/**
 * One MCP server row from `/api/state` (item48): connection state as
 * before, plus the on/off toggle state (whether the server's tools are
 * currently exposed to the model).
 *
 * @typedef {object} McpServerState
 * @property {string} name
 * @property {string} status
 * @property {boolean} enabled
 */

/**
 * One toggle row handed to the shared row renderer (internal).
 *
 * @typedef {object} ToggleRow
 * @property {string} name identity sent in the toggle event detail
 * @property {string} label full row text after the [x]/[ ] mark
 * @property {boolean} enabled
 * @property {string} ariaLabel
 */

/**
 * One row of the Skills tree (item49 /api/skills listing).
 *
 * @typedef {object} SkillRow
 * @property {string} name skill name (the folder name)
 * @property {string} description frontmatter description
 * @property {string} source "local" | "user" | "builtin" (in-code, item50)
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
  /** @type {Readonly<StateSnapshot> | null} */
  #snapshot = null;
  /** @type {ModelViewState | null} */
  #modelState = null;
  /** @type {(() => void) | null} */
  #unsubscribeModels = null;

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
        display: flex; align-items: baseline; justify-content: space-between;
        gap: 8px; border-top: 1px solid #1e293b; padding: 8px 10px;
        color: #a7f3d0; white-space: nowrap; overflow: hidden;
      }
      .agt-p-footer-left { overflow: hidden; text-overflow: ellipsis; }
      .agt-p-footer .agt-p-dim { color: #64748b; }
      .agt-p-footer-right { flex: none; color: #64748b; }
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
    for (const name of [
      "Context",
      "MCP",
      "LSP",
      "Todo",
      "Models",
      "Skills",
      "Slash",
      "Built-ins",
    ]) {
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

    // Model domain (item59, late-arriver rule): subscribe FIRST, read the
    // model client's frozen state SECOND — render value-or-blank, then live.
    this.#subscribeModels();
    this.#applyModelState(this.#readModelState());

    // Initial graceful state until the first /api/state snapshot arrives.
    this.setState(null);
  }

  /**
   * Subscribe to the `model_changed` stream (D38: the ONLY channel by
   * which this panel learns a swap). A panel mounted before the model
   * client's init (late arriver) cannot subscribe yet — the attempt fails
   * silently and is RETRIED on the next setState; until then the footer
   * and Models tree render blank, never an error.
   */
  #subscribeModels() {
    if (this.#unsubscribeModels) return;
    try {
      this.#unsubscribeModels = subscribe((event) => this.#onModelChanged(event));
    } catch {
      this.#unsubscribeModels = null;
    }
  }

  /**
   * Read the model domain's frozen state (subscribe-first-read-second): the
   * current service+model plus the current model's context window resolved
   * from the ROSTER (no fallback map — the roster is the only source, and a
   * missing entry means "unknown window"). Returns null when the model
   * client has not initialised yet or the server state has no selection —
   * the caller renders blank, never an error.
   *
   * @returns {ModelViewState | null}
   */
  #readModelState() {
    try {
      const snap = getState();
      if (!snap.service) return null;
      let contextWindow = null;
      if (snap.model) {
        const entry = snap.roster[snap.service];
        const row = entry?.models.find((m) => m.id === snap.model) ?? null;
        contextWindow = row ? row.context_window : null;
      }
      return { service: snap.service, model: snap.model, contextWindow };
    } catch {
      // getState() before init(): the late-arriver blank, not an error.
      return null;
    }
  }

  /**
   * Apply a model-state read: store it and re-render the footer and the
   * FYI-only Models tree.
   *
   * @param {ModelViewState | null} state
   */
  #applyModelState(state) {
    this.#modelState = state;
    this.#renderModels();
    if (this.#footerEl) this.#renderFooter();
  }

  /**
   * `model_changed` arrived: the event IS the state change (D38) — apply
   * its service+model directly (the context window is resolved from the
   * roster, "unknown" when the client is not initialised) and re-render.
   *
   * @param {import("../model-client.mjs").ModelChangedEvent} event
   */
  #onModelChanged(event) {
    let contextWindow = null;
    try {
      const entry = getState().roster[event.service];
      const row = entry?.models.find((m) => m.id === event.model) ?? null;
      contextWindow = row ? row.context_window : null;
    } catch {
      // not initialised yet — unknown window
    }
    this.#applyModelState({
      service: event.service,
      model: event.model,
      contextWindow,
    });
  }

  /**
   * Render the Models tree — FYI-ONLY (glossary: reusable-picker): one dim
   * header line per service, then its models `✓ id (K)` with the context
   * window from the roster; the currently-selected service's model carries
   * the ✓ mark. No click handlers, no toggles — /model is the only way to
   * switch.
   */
  #renderModels() {
    const section = this.#sections.get("Models");
    if (!section) return;
    if (!this.#modelState) {
      this.#setLines(section, [{ text: "(unavailable)", cls: "agt-p-dim" }]);
      return;
    }
    let roster = {};
    try {
      roster = getState().roster;
    } catch {
      roster = {};
    }
    const services = Object.values(roster);
    if (services.length === 0) {
      this.#setLines(section, [{ text: "(none)", cls: "agt-p-dim" }]);
      return;
    }
    /** @type {Array<{ text: string, cls: string }>} */
    const lines = [];
    for (const entry of services) {
      lines.push({ text: entry.service, cls: "agt-p-dim" });
      for (const model of entry.models) {
        const current =
          entry.service === this.#modelState?.service &&
          model.id === this.#modelState?.model;
        const k = Math.round(model.context_window / 1000);
        lines.push({
          text: `${current ? "✓" : " "} ${model.id} (${Number.isFinite(k) ? k : "?"}K)`,
          cls: "agt-p-line",
        });
      }
    }
    this.#setLines(section, lines);
  }

  /**
   * Drop the model_changed subscription (the page owns one panel for its
   * lifetime; the disconnect path exists for injection-style tests).
   */
  disconnectedCallback() {
    if (this.#unsubscribeModels) {
      this.#unsubscribeModels();
      this.#unsubscribeModels = null;
    }
  }

  /**
   * Consume a `/api/state` snapshot (deep-frozen by agt-app). `null` renders
   * the graceful "(unavailable)" state (backend not yet rebuilt / 404).
   *
   * @param {Readonly<StateSnapshot> | null} snapshot
   */
  setState(snapshot) {
    if (!this.#rendered) return;
    // Late-arriver retry: a panel mounted before the model client's init
    // could not subscribe (#subscribeModels failed silently) — each setState
    // retries the subscription and re-reads the frozen state, so the
    // footer/Models tree fill as soon as init has resolved.
    this.#subscribeModels();
    this.#snapshot = snapshot ?? null;
    this.#applyModelState(this.#readModelState());
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
      // item48: MCP rows are toggle rows like Built-ins — server name +
      // connection state with an [x]/[ ] on/off mark; clicking fires the
      // bubbling `agt-toggle-mcp` event (agt-app does the POST /api/mcp and
      // refetches state, which reverts the optimistic row on failure).
      const rows = Array.isArray(snapshot.mcp)
        ? snapshot.mcp.map((entry) => ({
            name: entry.name,
            label: `${entry.name} ${capitalize(String(entry.status))}`,
            enabled: Boolean(entry.enabled),
            ariaLabel: `Toggle MCP ${entry.name}`,
          }))
        : [];
      this.#renderToggles(mcp, rows, "agt-toggle-mcp");
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

    // Footer tokens ride the snapshot; service/model/context-window ride
    // the model domain (item59) — render footer + keep the Models tree fresh.
    if (this.#footerEl) this.#renderFooter();
    this.#renderModels();
  }

  /**
   * Render the status-bar footer from the model domain's frozen state
   * (item59): left = `Chat · <model> <service> · think off` (service/think
   * spans fainter than the model), right = context use `<used>K
   * (<percent>%)` over the model's context window resolved from the ROSTER
   * (percent omitted for an unknown window). The context tokens come from
   * the /api/state snapshot. Renders BLANK (never an error) while the
   * model client's init has not resolved.
   */
  #renderFooter() {
    const footer = this.#footerEl;
    if (!footer) return;
    if (!this.#modelState) {
      footer.replaceChildren();
      return;
    }
    const footerInput = {
      model: this.#modelState.model,
      service: this.#modelState.service,
      context: this.#snapshot?.context ?? null,
    };
    const segments = footerSegments(footerInput);
    const { right } = formatFooter(footerInput, this.#modelState.contextWindow);
    const left = document.createElement("span");
    left.className = "agt-p-footer-left";
    const head = document.createElement("span");
    head.textContent = `${segments.mode} · ${segments.model} `;
    const service = document.createElement("span");
    service.className = "agt-p-dim";
    service.textContent = segments.service;
    const think = document.createElement("span");
    think.className = "agt-p-dim";
    think.textContent = ` · think ${segments.think}`;
    left.append(head, service, think);
    const rightEl = document.createElement("span");
    rightEl.className = "agt-p-footer-right";
    rightEl.textContent = right;
    footer.replaceChildren(left, rightEl);
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
   * @param {string} rawText the command line that was run (e.g. "/models")
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
   * Render the Built-ins tool rows (toggle rows via {@link #renderToggles}).
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
   * Expand the MCP tree (e.g. after running /mcp, item54) — the rows are
   * the item48 per-server toggle rows rendered by setState().
   */
  openMcp() {
    this.#sections.get("MCP")?.setCollapsed(false);
  }

  /**
   * Render the Skills tree (slash /skills, item49): one row per skill from
   * GET /api/skills — `name [source] — description` with the local/user
   * /builtin source tag (the server already applies
   * local-masks-user-masks-builtin). Collapses
   * every other tree, expands Skills (one tree expanded at a time). Rows
   * are informational (no click action — the chat agent loads a skill body
   * via its ReadSkill tool).
   *
   * @param {readonly SkillRow[]} skills
   */
  showSkills(skills) {
    if (!this.#rendered) return;
    for (const [name, section] of this.#sections) {
      section.setCollapsed(name === "Skills" ? false : true);
    }
    const section = this.#sections.get("Skills");
    if (!section) return;
    if (!Array.isArray(skills) || skills.length === 0) {
      this.#setLines(section, [{ text: "(none)", cls: "agt-p-dim" }]);
      return;
    }
    this.#setLines(
      section,
      skills.map((skill) => ({
        text: `${skill.name} [${skill.source}] — ${skill.description}`,
        cls: "agt-p-line",
      })),
    );
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
    this.#renderToggles(
      section,
      this.#tools.map((tool) => ({
        name: tool.name,
        label: tool.name,
        enabled: Boolean(tool.enabled),
        ariaLabel: `Toggle tool ${tool.name}`,
      })),
      "agt-toggle-tool",
    );
  }

  /**
   * Shared toggle-row renderer (item48): one `[x]/[ ]` row per entry, used
   * by both the Built-ins tree and the MCP tree. Rows are buttons-in-a-
   * label with a checkbox; flipping one updates the mark optimistically and
   * fires the bubbling `eventName` CustomEvent with
   * `{ name, enabled }` (agt-app does the REST POST and refetches
   * /api/state, which reverts the optimistic row on failure).
   *
   * @param {Section} section
   * @param {ToggleRow[]} rows
   * @param {"agt-toggle-tool" | "agt-toggle-mcp"} eventName
   */
  #renderToggles(section, rows, eventName) {
    if (rows.length === 0) {
      this.#setLines(section, [{ text: "(none)", cls: "agt-p-dim" }]);
      return;
    }
    const frag = document.createDocumentFragment();
    for (const row of rows) {
      const el = document.createElement("label");
      el.className = "agt-p-tool";

      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = Boolean(row.enabled);
      box.setAttribute("aria-label", row.ariaLabel);

      const mark = document.createElement("span");
      mark.className = "agt-p-mark";
      mark.textContent = box.checked ? "[x]" : "[ ]";

      const label = document.createElement("span");
      label.textContent = row.label;

      box.addEventListener("change", () => {
        const enabled = box.checked;
        mark.textContent = enabled ? "[x]" : "[ ]";
        if (this.#onToggle) this.#onToggle(row.name, enabled);
        this.dispatchEvent(
          new CustomEvent(eventName, {
            detail: { name: row.name, enabled },
            bubbles: true,
            composed: true,
          }),
        );
      });

      el.append(box, mark, label);
      frag.append(el);
    }
    section.content.replaceChildren(frag);
  }

  #setUnavailable() {
    this.setSessionTitle("(unavailable)");
    for (const name of [
      "Context",
      "MCP",
      "LSP",
      "Todo",
      "Models",
      "Skills",
      "Built-ins",
    ]) {
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
