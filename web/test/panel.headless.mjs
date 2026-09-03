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

// ------------------------------------------------- stub fetch (before app)

const realFetch = window.fetch.bind(window);

/** @type {any} */
let fixture = await (await realFetch("/test/fixtures/state.json")).json();

/** @type {Array<{ name: string, enabled: boolean }>} */
const postCalls = [];

/** @type {Array<{ model: string }>} */
const modelPosts = [];

window.fetch = /** @type {typeof window.fetch} */ (
  async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof Request ? input.url : String(input);
    if (url === "/api/state") {
      return new Response(JSON.stringify(fixture), {
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
    if (url === "/api/model" && init && init.method === "POST") {
      const body = /** @type {{ model: string }} */ (
        JSON.parse(String(init.body))
      );
      modelPosts.push(body);
      if (body.model === "bogus-model") {
        return new Response(
          JSON.stringify({
            ok: false,
            error: "unknown model 'bogus-model' for provider 'mistral'",
          }),
          { status: 400, headers: { "Content-Type": "application/json" } },
        );
      }
      // Emulate the server swap: the response IS the updated snapshot.
      fixture.model = body.model;
      return new Response(JSON.stringify(fixture), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return realFetch(/** @type {RequestInfo} */ (input), init);
  }
);

// ------------------------------------------------------------ stub client

/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEvent = null;

/** @type {string[]} */
const renames = [];

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
    window.__PANEL_STUB__ = {
      emit,
      renames,
      postCalls,
      modelPosts,
    };
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
  async sendPrompt() {
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

/** @param {string} key */
async function pressKey(key) {
  ta().dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
  );
  await tick();
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
  assertEqual(footerRightText(), "12.3K (9%)", "footer right context use");
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

await test("typing / opens the menu with all 6 commands", async () => {
  await type("/");
  const menu = menuEl();
  assert(menu.hidden === false, "menu should be open after typing /");
  const options = [...menu.querySelectorAll("[role=option]")];
  assertEqual(options.length, 6, "expected 6 commands in the menu");
  assertEqual(
    options.map((o) => o.textContent).join("|"),
    [
      "/modelslist models for the current provider and switch",
      "/built-insshow the built-in tools with on/off toggles",
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
  await pressKey("ArrowDown");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-1",
    "ArrowDown should move to the second option",
  );
  for (let i = 0; i < 5; i++) await pressKey("ArrowDown");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-0",
    "ArrowDown should wrap back to the first option",
  );
  await pressKey("ArrowUp");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-5",
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

await test("Enter on /models opens the Models tree; other trees collapse", async () => {
  assert(isCollapsed("Context") === false, "Context expanded before command");
  assert(isCollapsed("MCP") === false, "MCP expanded before command");
  await type("/models");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("/models"), "slash invocation echo");
  await waitFor(() => isCollapsed("Models") === false, "Models expanded");
  assertEqual(ta().value, "", "input cleared after running the command");
  const rows = [...section("Models").querySelectorAll(".agt-p-model")];
  assertEqual(rows.length, 2, "mistral roster rows rendered");
  assert(
    rows[0].textContent?.includes("zai-glm-5-2") &&
      rows[0].textContent?.includes("131K"),
    `first row should be zai-glm-5-2 with its context window, got ${JSON.stringify(rows[0].textContent)}`,
  );
  assert(
    rows[1].textContent?.includes("mistral-medium-latest") &&
      rows[1].textContent?.includes("131K"),
    `second row should be mistral-medium-latest with its context window, got ${JSON.stringify(rows[1].textContent)}`,
  );
  assert(isCollapsed("Context"), "Context should be collapsed after a command");
  assert(isCollapsed("MCP"), "MCP should be collapsed after a command");
  assert(isCollapsed("LSP"), "LSP should be collapsed after a command");
  assert(isCollapsed("Todo"), "Todo should be collapsed after a command");
  assert(isCollapsed("Slash"), "Slash should be collapsed after a command");
  assert(
    isCollapsed("Built-ins"),
    "Built-ins should be collapsed after a command",
  );
});

await test("selecting a Models row POSTs /api/model and the footer reflects the swap", async () => {
  const rows = [...section("Models").querySelectorAll(".agt-p-model")];
  const target = need(
    rows.find((row) => row.textContent?.includes("mistral-medium-latest")),
    "mistral-medium-latest row missing",
  );
  target.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await waitFor(() => modelPosts.length === 1, "POST /api/model call");
  assertEqual(
    JSON.stringify(modelPosts[0]),
    JSON.stringify({ model: "mistral-medium-latest" }),
    "POST /api/model body",
  );
  await waitFor(
    () => footerLeftText() === "Chat · mistral-medium-latest mistral · think off",
    "footer model after swap",
  );
  assertEqual(
    footerLeftText(),
    "Chat · mistral-medium-latest mistral · think off",
    "footer left after swap",
  );
  assertEqual(footerRightText(), "12.3K (9%)", "footer right after swap");
  assertEqual(
    /** @type {any} */ (app).snapshot?.model,
    "mistral-medium-latest",
    "app snapshot model swapped",
  );
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

// ---------------------------------------------------------------- results

window.__PANEL_TEST_RESULTS__ = { pass, fail, details };
document.title = "panel-tests-done";
