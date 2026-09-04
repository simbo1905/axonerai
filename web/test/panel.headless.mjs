// @ts-check
// Headless panel + slash-menu test for the vanilla web-components chat UI.
//
// Runs in Chrome against a served tree (root-absolute imports). window.fetch
// is stubbed BEFORE the app is imported: GET /api/state serves the static
// fixture web/test/fixtures/state.json (fetched once through the real fetch,
// i.e. served by the python http.server); POST /api/tools records the call
// and emulates server-side persistence by mutating the fixture. AgtClient is
// a stub that mimics web/assets/client.mjs plus sendRename; ack/session_meta
// frames are validated with the generated JTD validators (same contract the
// real client uses). Results land on window.__PANEL_TEST_RESULTS__ and
// document.title becomes "panel-tests-done".
import { validateAck, validateSession_meta } from "/generated/validators.mjs";
import { deepFreeze, parseWireEventText } from "/src/wire.mjs";

// ---------------------------------------------------------------- harness

let pass = 0;
let fail = 0;
/** @type {{ name: string, ok: boolean, error?: string }[]} */
const details = [];

/**
 * @param {string} name
 * @param {() => void | Promise<void>} fn
 */
async function test(name, fn) {
  try {
    await fn();
    pass += 1;
    details.push({ name, ok: true });
  } catch (error) {
    fail += 1;
    details.push({
      name,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

/** @param {boolean} condition @param {string} message */
function assert(condition, message) {
  if (!condition) throw new Error(message);
}

/**
 * Narrow a possibly-null/undefined value or throw.
 *
 * @template T
 * @param {T | null | undefined} value
 * @param {string} message
 * @returns {T}
 */
function need(value, message) {
  if (value === null || value === undefined) throw new Error(message);
  return value;
}

/** @param {unknown} actual @param {unknown} expected @param {string} message */
function assertEqual(actual, expected, message) {
  assert(
    actual === expected,
    `${message} (expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)})`,
  );
}

/**
 * Poll until `fn` returns a truthy value.
 *
 * @template T
 * @param {() => T} fn
 * @param {string} what
 * @param {number} [timeout]
 * @returns {Promise<T>}
 */
async function waitFor(fn, what, timeout = 3000) {
  const start = Date.now();
  for (;;) {
    const value = fn();
    if (value) return value;
    if (Date.now() - start > timeout) {
      throw new Error(`timeout waiting for ${what}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

// ------------------------------------------------- stored MCP prefs (item48)

// Seeded BEFORE agt-app is imported so the boot application path is
// exercised: this fixture folder stores context7 disabled; a DIFFERENT
// folder's key stores tavily disabled (the boot must NOT read it —
// folder-scoped key isolation). Re-seeded on every run so reruns are
// deterministic.
const REPO_PATH = "/Users/Shared/axonerai"; // matches the fixture repo.path
localStorage.setItem(
  `agt.mcp-disabled:${REPO_PATH}`,
  JSON.stringify(["context7"]),
);
localStorage.setItem(
  "agt.mcp-disabled:/srv/other-checkout",
  JSON.stringify(["tavily"]),
);

// ------------------------------------------------- stub fetch (before app)

const realFetch = window.fetch.bind(window);

/** @type {any} */
let fixture = await (await realFetch("/test/fixtures/state.json")).json();

/** @type {Array<{ name: string, enabled: boolean }>} */
const postCalls = [];

/** @type {Array<{ server: string, enabled: boolean }>} */
const mcpPosts = [];

window.fetch = /** @type {typeof window.fetch} */ (
  async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof Request ? input.url : String(input);
    if (url === "/api/state") {
      return new Response(JSON.stringify(fixture), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/mcp" && init && init.method === "POST") {
      // item48: emulate the server-side per-server suppression — the next
      // /api/state reflects it in the mcp[].enabled field.
      const body = /** @type {{ server: string, enabled: boolean }} */ (
        JSON.parse(String(init.body))
      );
      mcpPosts.push(body);
      const server = /** @type {{ name: string, enabled: boolean } | undefined} */ (
        fixture.mcp?.find((/** @type {{ name: string }} */ m) => m.name === body.server)
      );
      if (server) server.enabled = body.enabled;
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/tools" && init && init.method === "POST") {
      const body = /** @type {{ name: string, enabled: boolean }} */ (
        JSON.parse(String(init.body))
      );
      postCalls.push(body);
      // Emulate server-side persistence: the next /api/state reflects it.
      const tool = /** @type {{ name: string, enabled: boolean } | undefined} */ (
        fixture.tools?.find((/** @type {{ name: string }} */ t) => t.name === body.name)
      );
      if (tool) tool.enabled = body.enabled;
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/skills") {
      // item49: the skills listing (local masks user, resolved server-side).
      return new Response(
        JSON.stringify([
          {
            name: "deploy",
            description: "ship the release",
            source: "user",
            path: "/home/u/.axonerai/skills/deploy/SKILL.md",
          },
          {
            name: "greeting",
            description: "greet politely",
            source: "local",
            path: ".axonerai/skills/greeting/SKILL.md",
          },
          {
            name: "lint",
            description: "grade the repo",
            source: "local",
            path: ".axonerai/skills/lint/SKILL.md",
          },
        ]),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    }
    return realFetch(/** @type {RequestInfo} */ (input), init);
  }
);

// ------------------------------------------------------------ stub client

/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEvent = null;

/** @type {string[]} */
const renames = [];

/** @type {string[]} */
const prompts = [];

/**
 * Validate + freeze a control-plane frame the wire.mjs registry does not
 * cover yet, then deliver it like client.mjs would.
 *
 * @param {any} frame
 */
function emitTyped(frame) {
  const type = frame?._type;
  const errors =
    type === "ack"
      ? validateAck(frame)
      : type === "session_meta"
        ? validateSession_meta(frame)
        : [{}];
  if (errors.length > 0) throw new Error(`invalid stub frame ${type}`);
  if (onEvent) {
    onEvent(/** @type {import("/src/wire.mjs").WireEvent} */ (deepFreeze(frame)));
  }
}

/**
 * @param {unknown} frame
 */
function emit(frame) {
  if (
    frame !== null &&
    typeof frame === "object" &&
    (/** @type {any} */ (frame)._type === "ack" ||
      /** @type {any} */ (frame)._type === "session_meta")
  ) {
    emitTyped(frame);
    return;
  }
  const event = parseWireEventText(JSON.stringify(frame));
  if (event === null) return;
  if (onEvent) onEvent(event);
}

/** @type {any} */
window.AgtClient = {
  async connect(/** @type {any} */ opts) {
    onEvent = opts.onEvent ?? null;
    const captured = {
      onOpen: opts.onOpen ?? (() => {}),
      onClose: opts.onClose ?? (() => {}),
      onError: opts.onError ?? (() => {}),
    };
    window.__PANEL_STUB__ = /** @type {any} */ ({
      emit,
      renames,
      prompts,
      postCalls,
      mcpPosts,
    });
    captured.onOpen();
    await Promise.resolve();
    emit({ _type: "ready", version: "9.9.9-test", websocket_path: "/ws" });
    emit({
      _type: "session_meta",
      created_at: 1,
      session_id: "11111111-2222-3333-4444-555555555555",
      title: "axonerai",
    });
    return { dispose() {} };
  },
  /**
   * @param {string} text
   * @param {string} [id]
   */
  async sendPrompt(text, id) {
    prompts.push(text);
    void id;
    return "ok";
  },
  /** @param {string} title */
  sendRename(title) {
    renames.push(title);
  },
  dispose() {},
};

// ------------------------------------------------------------- load the UI

await import("/src/components/agt-app.js");

const app = need(document.querySelector("agt-app"), "agt-app element missing");
const stub = /** @type {NonNullable<Window["__PANEL_STUB__"]>} */ (
  window.__PANEL_STUB__
);

// ---------------------------------------------------------------- helpers

/** @returns {HTMLElement} */
function panelEl() {
  return need(
    /** @type {HTMLElement | null} */ (document.querySelector("agt-panel")),
    "agt-panel element missing",
  );
}

/** @returns {ShadowRoot} */
function shadow() {
  return need(panelEl().shadowRoot, "panel shadow root missing");
}

/** @param {string} name @returns {HTMLElement} */
function section(name) {
  return need(
    /** @type {HTMLElement | null} */ (
      shadow().querySelector(`[data-name="${name}"]`)
    ),
    `section ${name} missing`,
  );
}

/** @param {string} name */
function sectionText(name) {
  const content = section(name).querySelector(".agt-p-content");
  return content ? content.textContent ?? "" : "";
}

/** @param {string} name */
function isCollapsed(name) {
  return section(name).dataset.collapsed === "1";
}

function slashText() {
  return sectionText("Slash");
}

function footerLeftText() {
  const left = shadow().querySelector(".agt-p-footer-left");
  return left ? left.textContent ?? "" : "";
}

function footerRightText() {
  const right = shadow().querySelector(".agt-p-footer-right");
  return right ? right.textContent ?? "" : "";
}

function titleText() {
  const title = shadow().querySelector(".agt-p-title");
  return title ? title.textContent ?? "" : "";
}

/**
 * The checkbox of one MCP toggle row (item48).
 *
 * @param {string} name
 * @returns {HTMLInputElement}
 */
function mcpBox(name) {
  const rows = [...section("MCP").querySelectorAll(".agt-p-tool")];
  const row = need(
    rows.find((r) => r.textContent?.includes(name)),
    `MCP row ${name} missing`,
  );
  return need(
    /** @type {HTMLInputElement | null} */ (
      row.querySelector("input[type=checkbox]")
    ),
    `MCP checkbox for ${name} missing`,
  );
}

/** @returns {HTMLTextAreaElement} */
function ta() {
  return need(
    /** @type {HTMLTextAreaElement | null} */ (
      document.querySelector("agt-composer textarea")
    ),
    "composer textarea missing",
  );
}

/** @returns {HTMLElement} */
function menuEl() {
  return need(
    /** @type {HTMLElement | null} */ (
      document.querySelector("agt-composer .slash-menu")
    ),
    "slash menu missing",
  );
}

/**
 * Set the composer value and fire the input event (like real typing).
 *
 * @param {string} text
 */
async function type(text) {
  const input = ta();
  input.focus();
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
}

/**
 * @param {string} key
 * @param {{ shiftKey?: boolean }} [modifiers]
 */
async function pressKey(key, modifiers = {}) {
  ta().dispatchEvent(
    new KeyboardEvent("keydown", {
      key,
      bubbles: true,
      cancelable: true,
      ...modifiers,
    }),
  );
  await tick();
}

/** Rendered prompt bubbles (chat sends), newest last. */
function promptCount() {
  return [...document.querySelectorAll("agt-msg")].filter(
    (m) => /** @type {any} */ (m).event?._type === "prompt",
  ).length;
}

/** The last echoed command line in the panel's Slash log. */
function lastEcho() {
  const lines = [
    ...shadow().querySelectorAll(".agt-p-slash-log .agt-p-line"),
  ];
  const last = lines[lines.length - 1];
  return last ? last.textContent ?? "" : "";
}

// ----------------------------------------------------------------- tests

await test("panel boots from /api/state: footer status bar, MCP tavily Connected, LSP (none)", async () => {
  await waitFor(
    () => footerLeftText() === "Chat · zai-glm-5-2 mistral · think off",
    "footer status bar",
  );
  assertEqual(
    footerLeftText(),
    "Chat · zai-glm-5-2 mistral · think off",
    "footer left",
  );
  // item41: the fixture's /api/state carries the config context_window
  // (32768) for zai-glm-5-2 — the percent follows the config, not the
  // hardcoded 131072 map.
  assertEqual(footerRightText(), "12.3K (38%)", "footer right context use");
  await waitFor(() => titleText() === "axonerai", "session title");
  assert(
    sectionText("MCP").includes("tavily") &&
      sectionText("MCP").includes("Connected"),
    `MCP should show "tavily Connected", got ${JSON.stringify(sectionText("MCP"))}`,
  );
  assert(
    sectionText("LSP").includes("(none)"),
    `LSP should show "(none)", got ${JSON.stringify(sectionText("LSP"))}`,
  );
  assert(
    sectionText("Todo").includes("(none)"),
    `Todo should show "(none)", got ${JSON.stringify(sectionText("Todo"))}`,
  );
  assert(
    sectionText("Context").includes("12345"),
    `Context should show token count, got ${JSON.stringify(sectionText("Context"))}`,
  );
});

// --- item48: MCP toggle list -------------------------------------------------

await test("boot applies THIS folder's stored disabled servers only (folder-scoped keys)", async () => {
  // Seeded pre-import: repo key = ["context7"], other-folder key = ["tavily"].
  // Boot must POST exactly one /api/mcp disable — context7 — and NOT the
  // other folder's tavily entry.
  await waitFor(() => mcpPosts.length >= 1, "boot POST /api/mcp");
  await waitFor(
    () => mcpBox("context7").checked === false,
    "context7 row disabled after boot apply (refetched state)",
  );
  assertEqual(
    JSON.stringify(mcpPosts),
    JSON.stringify([{ server: "context7", enabled: false }]),
    "boot applies only this folder's stored prefs (no tavily from /srv/other-checkout)",
  );
});

await test("MCP section renders toggle rows with server name + connection state", () => {
  const rows = [...section("MCP").querySelectorAll(".agt-p-tool")];
  assertEqual(rows.length, 2, "fixture MCP servers rendered as toggle rows");
  assert(
    rows[0].textContent?.includes("tavily") &&
      rows[0].textContent?.includes("Connected"),
    `first row should be "tavily Connected", got ${JSON.stringify(rows[0].textContent)}`,
  );
  assert(
    rows[1].textContent?.includes("context7") &&
      rows[1].textContent?.includes("Connected"),
    `second row should be "context7 Connected", got ${JSON.stringify(rows[1].textContent)}`,
  );
  assertEqual(mcpBox("tavily").checked, true, "tavily starts enabled");
  assertEqual(mcpBox("context7").checked, false, "context7 was boot-disabled");
});

await test("toggling an MCP row POSTs /api/mcp, updates the folder-scoped key, and re-renders", async () => {
  const box = mcpBox("tavily");
  box.click();
  await waitFor(() => mcpPosts.length === 2, "toggle POST /api/mcp");
  assertEqual(
    JSON.stringify(mcpPosts[1]),
    JSON.stringify({ server: "tavily", enabled: false }),
    "toggle POST body",
  );
  await waitFor(
    () => mcpBox("tavily").checked === false,
    "row re-rendered from the refetched (persisted server-side) state",
  );
  const stored = JSON.parse(
    localStorage.getItem(`agt.mcp-disabled:${REPO_PATH}`) ?? "[]",
  );
  assert(
    stored.includes("tavily") && stored.includes("context7"),
    `disabled list must hold both servers, got ${JSON.stringify(stored)}`,
  );

  // Toggle back on: the re-enable POSTs and the server leaves the list.
  mcpBox("tavily").click();
  await waitFor(() => mcpPosts.length === 3, "re-enable POST /api/mcp");
  assertEqual(
    JSON.stringify(mcpPosts[2]),
    JSON.stringify({ server: "tavily", enabled: true }),
    "re-enable POST body",
  );
  await waitFor(
    () => mcpBox("tavily").checked === true,
    "row re-enabled from the refetched state",
  );
  const storedAfter = JSON.parse(
    localStorage.getItem(`agt.mcp-disabled:${REPO_PATH}`) ?? "[]",
  );
  assert(
    !storedAfter.includes("tavily") && storedAfter.includes("context7"),
    `re-enabled server removed from the stored list, got ${JSON.stringify(storedAfter)}`,
  );
});

await test("typing / opens the menu with all 8 commands", async () => {
  await type("/");
  const menu = menuEl();
  assert(menu.hidden === false, "menu should be open after typing /");
  const options = [...menu.querySelectorAll("[role=option]")];
  assertEqual(options.length, 8, "expected 8 commands in the menu");
  assertEqual(
    options.map((o) => o.textContent).join("|"),
    [
      "/modelslist models for the current provider and switch",
      "/built-insshow the built-in tools with on/off toggles",
      "/mcpshow the MCP servers with on/off toggles",
      "/skillslist available skills",
      "/verbosetoggle verbose output rendering",
      "/renamerename the session: /rename <title>",
      "/helplist the available commands",
      "/consoleopen the devtools console popup",
    ].join("|"),
    "unexpected menu entries",
  );
  assertEqual(ta().getAttribute("aria-expanded"), "true", "aria-expanded");
  assertEqual(
    options[0].getAttribute("aria-selected"),
    "true",
    "first option selected",
  );
  assertEqual(
    ta().getAttribute("aria-activedescendant"),
    "agt-slash-opt-0",
    "aria-activedescendant",
  );
});

await test("ArrowDown/ArrowUp move the highlight with wrap; Esc closes; input keeps focus", async () => {
  await type("/");
  const menu = menuEl();
  const optionCount = menu.querySelectorAll("[role=option]").length;
  await pressKey("ArrowDown");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-1",
    "ArrowDown should move to the second option",
  );
  // Wrap is count-agnostic: N-1 more downs from index 1 land back on 0.
  for (let i = 0; i < optionCount - 1; i++) await pressKey("ArrowDown");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-0",
    "ArrowDown should wrap back to the first option",
  );
  await pressKey("ArrowUp");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    `agt-slash-opt-${optionCount - 1}`,
    "ArrowUp should wrap to the last option",
  );
  await pressKey("Escape");
  assert(menu.hidden === true, "menu should be closed after Esc");
  assertEqual(ta().getAttribute("aria-expanded"), "false", "aria-expanded");
  assertEqual(document.activeElement, ta(), "input keeps focus");
});

await test("menu closes when input no longer starts with /", async () => {
  await type("/he");
  assert(menuEl().hidden === false, "menu open for /he");
  await type("hello");
  assert(menuEl().hidden === true, "menu should close for non-slash input");
});

await test("/skills opens the Skills tree with name + source tag rows; other trees collapse", async () => {
  await type("/skills");
  await pressKey("Enter");
  await waitFor(() => isCollapsed("Skills") === false, "Skills expanded");
  assertEqual(ta().value, "", "input cleared after running the command");
  const text = sectionText("Skills");
  assert(
    text.includes("greeting [local] — greet politely"),
    `local row should carry name, [local] tag and description, got ${JSON.stringify(text)}`,
  );
  assert(
    text.includes("deploy [user] — ship the release"),
    `user row should carry name, [user] tag and description, got ${JSON.stringify(text)}`,
  );
  assert(
    text.includes("lint [local] — grade the repo"),
    `second local row expected, got ${JSON.stringify(text)}`,
  );
  assert(isCollapsed("Context"), "Context should be collapsed after /skills");
  assert(isCollapsed("Models"), "Models should be collapsed after /skills");
  assert(isCollapsed("MCP"), "MCP should be collapsed after /skills");
  assert(isCollapsed("Slash"), "Slash should be collapsed after /skills");
  // item32: the result line goes to the console bus, not the Slash tree.
  assert(
    !slashText().includes("opened the Skills tree"),
    "skills result must not render in the Slash tree",
  );
  await waitFor(() => slashText().includes("/skills"), "skills invocation echo");
});

await test("/built-ins opens the Built-ins tree with the fixture tools; flipping a toggle POSTs", async () => {
  await type("/built-ins");
  await pressKey("Enter");
  await waitFor(() => isCollapsed("Built-ins") === false, "Built-ins expanded");
  const rows = [
    ...section("Built-ins").querySelectorAll(".agt-p-tool"),
  ];
  assertEqual(rows.length, 2, "fixture tools rendered as rows");
  assert(
    rows[0].textContent?.includes("WebSearch"),
    "first row should be WebSearch",
  );
  assert(
    rows[1].textContent?.includes("tavily_search"),
    "second row should be tavily_search",
  );
  const box = need(
    /** @type {HTMLInputElement | null} */ (
      rows[0].querySelector("input[type=checkbox]")
    ),
    "toggle checkbox missing",
  );
  assert(box.checked === true, "WebSearch starts enabled");
  box.click();
  await waitFor(() => postCalls.length === 1, "POST /api/tools call");
  assertEqual(
    JSON.stringify(postCalls[0]),
    JSON.stringify({ name: "WebSearch", enabled: false }),
    "POST body",
  );
  await waitFor(() => box.checked === false, "row updated (persisted server-side)");
});

await test("/mcp opens the MCP tree with the fixture servers; other trees collapse", async () => {
  await type("/mcp");
  await pressKey("Enter");
  await waitFor(() => isCollapsed("MCP") === false, "MCP expanded");
  assertEqual(ta().value, "", "input cleared after running the command");
  // item54: /mcp is the mirror of /built-ins — the item48 per-server toggle
  // rows come straight from the /api/state snapshot.
  const rows = [...section("MCP").querySelectorAll(".agt-p-tool")];
  assertEqual(rows.length, 2, "fixture MCP servers rendered as rows");
  assert(
    rows[0].textContent?.includes("tavily"),
    `first row should be tavily, got ${JSON.stringify(rows[0].textContent)}`,
  );
  assert(
    rows[1].textContent?.includes("context7"),
    `second row should be context7, got ${JSON.stringify(rows[1].textContent)}`,
  );
  assert(isCollapsed("Built-ins"), "Built-ins should be collapsed after /mcp");
  assert(isCollapsed("Skills"), "Skills should be collapsed after /mcp");
  // item32: the result line goes to the console bus, not the Slash tree.
  assert(
    !slashText().includes("opened the MCP tree"),
    "mcp result must not render in the Slash tree",
  );
  await waitFor(() => slashText().includes("/mcp"), "mcp invocation echo");
});

await test("/rename sends the WS rename and the panel title updates on ack", async () => {
  await type("/rename panel-test");
  await pressKey("Enter");
  assertEqual(stub.renames.length, 1, "WS rename sent");
  assertEqual(stub.renames[0], "panel-test", "rename title");
  assertEqual(ta().value, "", "input cleared");
  // The real server persists the rename; emulate that before the ack-driven
  // state refetch so the snapshot agrees with the ack.
  fixture.session.title = "panel-test";
  stub.emit({ _type: "ack", for_type: "rename", ok: true, message: null });
  await waitFor(() => titleText() === "panel-test", "panel title updated on ack");
  // item32: "renamed: …" goes to the console bus, not the Slash tree.
  assert(
    !slashText().includes("renamed:"),
    "rename result must not render in the Slash tree",
  );
});

await test("/verbose toggles the flag and fires agt-verbose-changed", async () => {
  /** @type {Array<{ verbose: boolean }>} */
  const events = [];
  window.addEventListener("agt-verbose-changed", (e) => {
    events.push(/** @type {CustomEvent} */ (e).detail);
  });
  await type("/verbose");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("/verbose"), "verbose invocation echo");
  assertEqual(events.length, 1, "agt-verbose-changed fired once");
  assertEqual(events[0].verbose, true, "verbose flag true");
  assertEqual(/** @type {any} */ (app).verbose, true, "app.verbose getter");
  await type("/verbose");
  await pressKey("Enter");
  await waitFor(() => events.length === 2, "second toggle");
  assertEqual(events.length, 2, "agt-verbose-changed fired twice");
  assertEqual(events[1].verbose, false, "verbose flag false");
});

await test("/help runs and leaves only the invocation echo in the Slash tree", async () => {
  await type("/help");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("/help"), "help invocation echo");
  // item32: the command list goes to the console bus; the Slash tree keeps
  // only the echoed command line.
  assert(
    !slashText().includes("/models —"),
    "help output must not render in the Slash tree",
  );
});

await test("unknown command leaves only the invocation echo in the Slash tree", async () => {
  await type("/frobnicate");
  assert(menuEl().hidden === true, "no menu for an unknown command");
  await pressKey("Enter");
  await waitFor(
    () => slashText().includes("/frobnicate"),
    "unknown command invocation echo",
  );
  // item32: the error goes to the console bus (console.error).
  assert(
    !slashText().includes("unknown command"),
    "unknown-command error must not render in the Slash tree",
  );
});

await test("click selects a menu option and runs the command", async () => {
  await type("/");
  const options = [...menuEl().querySelectorAll("[role=option]")];
  const helpOption = need(
    options.find((o) => o.textContent?.includes("/help")),
    "help option missing",
  );
  helpOption.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await waitFor(() => ta().value === "", "input cleared by click-select");
  await waitFor(() => slashText().includes("/help"), "help ran via click");
  assert(menuEl().hidden === true, "menu closed after click-select");
});

await test("Tab completes the highlighted command name", async () => {
  await type("/ver");
  await pressKey("Tab");
  assertEqual(ta().value, "/verbose ", "Tab completes with trailing space");
  assert(
    ta().selectionStart === ta().value.length,
    "cursor restored to end",
  );
});

await test("app snapshot is deep-frozen", () => {
  const snapshot = /** @type {any} */ (app).snapshot;
  assert(snapshot !== null, "snapshot should be loaded");
  assert(Object.isFrozen(snapshot), "snapshot not frozen");
  assert(Object.isFrozen(snapshot.tools), "snapshot.tools not frozen");
  assert(Object.isFrozen(snapshot.session), "snapshot.session not frozen");
});

await test("Enter sends chat text to the model path; Shift+Enter keeps it without sending", async () => {
  await waitFor(
    () => !ta().disabled,
    "composer enabled before the keyboard test",
  );
  const before = promptCount();

  // Enter (menu closed, non-slash input) sends the chat.
  await type("hello from the Enter key");
  await pressKey("Enter");
  await waitFor(() => promptCount() === before + 1, "prompt bubble rendered");
  assertEqual(prompts.length, 1, "client got exactly one sendPrompt");
  assertEqual(prompts[0], "hello from the Enter key", "sent text");
  assertEqual(ta().value, "", "input cleared after Enter send");
  await waitFor(() => !ta().disabled, "composer re-enabled after send");

  // Shift+Enter never dispatches the send.
  const beforeShift = promptCount();
  await type("two lines pending");
  await pressKey("Enter", { shiftKey: true });
  await tick();
  assertEqual(promptCount(), beforeShift, "Shift+Enter must not send");
  assertEqual(prompts.length, 1, "no extra sendPrompt for Shift+Enter");
  assertEqual(ta().value, "two lines pending", "text kept in the box");
  await type("");
});

await test("/console opens the popup via window.open (stubbed + asserted); a blocked popup falls back to a tab", async () => {
  const originalOpen = window.open;
  /** @type {Array<{ url: any, target: any, features: any }>} */
  const opens = [];
  window.open = /** @type {typeof window.open} */ (
    /** @type {unknown} */ ((/** @type {any} */ url, /** @type {any} */ target, /** @type {any} */ features) => {
      opens.push({ url, target, features });
      // First call succeeds (popup), later calls simulate a blocked popup.
      return opens.length === 1 ? /** @type {any} */ ({}) : null;
    })
  );
  try {
    await type("/console");
    await pressKey("Enter");
    await waitFor(() => opens.length >= 1, "window.open called");
    assertEqual(opens[0].url, "/console.html", "popup url");
    assertEqual(opens[0].target, "agt-console", "popup target");
    assertEqual(
      opens[0].features,
      "popup,width=920,height=680",
      "popup features",
    );
    assertEqual(opens.length, 1, "no fallback while the popup succeeds");
    await waitFor(() => lastEcho() === "/console", "invocation echo");

    // Blocked popup: the composer run falls back to a regular tab.
    await type("/console");
    await pressKey("Enter");
    await waitFor(() => opens.length === 3, "fallback open after blocked popup");
    assertEqual(opens[2].url, "/console.html", "fallback url");
    assertEqual(opens[2].target, "_blank", "fallback target");
  } finally {
    window.open = originalOpen;
  }
});

// ---------------------------------------------------------------- results

window.__PANEL_TEST_RESULTS__ = { pass, fail, details };
document.title = "panel-tests-done";
// Mirror the PASS/FAIL summary into the DOM: the result stays observable
// without a console listener.
{
  const summaryEl = document.createElement("pre");
  summaryEl.id = "panel-tests-summary";
  summaryEl.textContent = `[panel-tests] pass=${pass} fail=${fail}`;
  document.body.append(summaryEl);
}
console.log(
  `[panel-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[panel-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
