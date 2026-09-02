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

function footerText() {
  const footer = shadow().querySelector(".agt-p-footer");
  return footer ? footer.textContent ?? "" : "";
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

await test("panel boots from /api/state: footer path:branch, MCP tavily Connected, LSP (none)", async () => {
  await waitFor(
    () => footerText() === "/Users/Shared/axonerai:simbo1905",
    "footer path:branch",
  );
  assertEqual(footerText(), "/Users/Shared/axonerai:simbo1905", "footer");
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

await test("typing / opens the menu with all 5 commands", async () => {
  await type("/");
  const menu = menuEl();
  assert(menu.hidden === false, "menu should be open after typing /");
  const options = [...menu.querySelectorAll("[role=option]")];
  assertEqual(options.length, 5, "expected 5 commands in the menu");
  assertEqual(
    options.map((o) => o.textContent).join("|"),
    [
      "/modelshow the current model and provider",
      "/built-insshow the built-in tools with on/off toggles",
      "/verbosetoggle verbose output rendering",
      "/renamerename the session: /rename <title>",
      "/helplist the available commands",
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
  for (let i = 0; i < 4; i++) await pressKey("ArrowDown");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-0",
    "ArrowDown should wrap back to the first option",
  );
  await pressKey("ArrowUp");
  assertEqual(
    menu.querySelector("[aria-selected=true]")?.id,
    "agt-slash-opt-4",
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

await test("Enter on /model runs it: Slash shows model: text with other trees collapsed", async () => {
  assert(isCollapsed("Context") === false, "Context expanded before command");
  assert(isCollapsed("MCP") === false, "MCP expanded before command");
  await type("/model");
  await pressKey("Enter");
  await waitFor(
    () => slashText().includes("model: zai-glm-5-2 (provider: mistral)"),
    "slash model response",
  );
  assertEqual(ta().value, "", "input cleared after running the command");
  assert(isCollapsed("Context"), "Context should be collapsed after a command");
  assert(isCollapsed("MCP"), "MCP should be collapsed after a command");
  assert(isCollapsed("LSP"), "LSP should be collapsed after a command");
  assert(isCollapsed("Todo"), "Todo should be collapsed after a command");
  assert(
    isCollapsed("Slash") === false,
    "Slash section should be expanded",
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
  await waitFor(
    () => slashText().includes("renamed: panel-test"),
    "slash renamed response",
  );
});

await test("/verbose toggles the flag, fires agt-verbose-changed, Slash shows verbose: on|off", async () => {
  /** @type {Array<{ verbose: boolean }>} */
  const events = [];
  window.addEventListener("agt-verbose-changed", (e) => {
    events.push(/** @type {CustomEvent} */ (e).detail);
  });
  await type("/verbose");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("verbose: on"), "verbose on");
  assertEqual(events.length, 1, "agt-verbose-changed fired once");
  assertEqual(events[0].verbose, true, "verbose flag true");
  assertEqual(/** @type {any} */ (app).verbose, true, "app.verbose getter");
  await type("/verbose");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("verbose: off"), "verbose off");
  assertEqual(events.length, 2, "agt-verbose-changed fired twice");
  assertEqual(events[1].verbose, false, "verbose flag false");
});

await test("/help lists the commands in the Slash section", async () => {
  await type("/help");
  await pressKey("Enter");
  await waitFor(() => slashText().includes("/model —"), "help lists /model");
  for (const name of ["model", "built-ins", "verbose", "rename", "help"]) {
    assert(
      slashText().includes(`/${name} — `),
      `help should list /${name}, got ${JSON.stringify(slashText())}`,
    );
  }
});

await test("unknown command reports an error line in Slash", async () => {
  await type("/frobnicate");
  assert(menuEl().hidden === true, "no menu for an unknown command");
  await pressKey("Enter");
  await waitFor(
    () => slashText().includes("unknown command '/frobnicate'"),
    "unknown command error",
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
  await waitFor(() => slashText().includes("/model — "), "help ran via click");
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
