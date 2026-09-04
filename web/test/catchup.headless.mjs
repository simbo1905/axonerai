// @ts-check
// Headless catch-up test for the chat screen's `?s=<uuid>` boot path.
//
// ONE page, injection-style ONLY (docs/ARCHITECTURE.md Decision 2): the
// session's IndexedDB history is seeded DIRECTLY through the same
// openHistory/appendEvents seam the app uses, the server catch-up endpoint
// (/api/session/<uuid>?after=<frontier>) is a stubbed fetch serving
// line-format `ts\0type\0text` frames, and AgtClient is a stub. No reload
// orchestration, no second page, no BroadcastChannel/worker cross-boundary
// play (the spool worker is stubbed out). Assertions cover the frontier
// request, replay order (local records → server frames → live ready), the
// exclusive-frontier duplicate filter, the skipped frame kinds (prompt echo),
// the rendered DOM (tool lines only under /verbose) and the persisted history.
// Results land on window.__CATCHUP_TEST_RESULTS__ and document.title becomes
// "catchup-tests-done".
import { appendEvents, getAll, openHistory } from "/src/history.mjs";
import { parseWireEvent } from "/src/wire.mjs";

// ------------------------------------------------- stub worker (before app)

class StubWorker {
  onmessage = null;
  onerror = null;
  /** @param {unknown} data */
  postMessage(data) {
    void data;
  }
  terminate() {}
  addEventListener(/** @type {string} */ _type, /** @type {unknown} */ _fn) {}
  removeEventListener(/** @type {string} */ _type, /** @type {unknown} */ _fn) {}
  dispatchEvent(/** @type {Event} */ _event) {
    return false;
  }
}
window.Worker = /** @type {typeof Worker} */ (
  /** @type {unknown} */ (StubWorker)
);

// ------------------------------------------------------------- constants

const SESSION_ID = "99999999-1111-4222-8333-444444444444";

// Deterministic reruns: drop any stored MCP toggle prefs on this origin so
// the boot performs no /api/mcp POSTs (other suites on this origin seed them).
localStorage.removeItem("agt.mcp-disabled:/Users/Shared/axonerai");
localStorage.removeItem("agt.mcp-disabled:/srv/other-checkout");

// ------------------------------------------------------------- harness

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
 * @param {unknown} actual
 * @param {unknown} expected
 * @param {string} message
 */
function assertEqual(actual, expected, message) {
  assert(
    actual === expected,
    `${message} (expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)})`,
  );
}

/**
 * @template T
 * @param {T | null | undefined} value
 * @param {string} message
 * @returns {T}
 */
function need(value, message) {
  if (value === null || value === undefined) throw new Error(message);
  return value;
}

/**
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

// -------------------------------------------- seed the session history (IDB)

/**
 * @param {string} text
 */
function assistantEvent(text) {
  const event = parseWireEvent({ _type: "assistant", id: null, text });
  if (!event) throw new Error(`assistant fixture invalid: ${text}`);
  return event;
}

const localToolCall = parseWireEvent({
  _type: "tool_call",
  id: null,
  session_id: SESSION_ID,
  tool: "WebSearch",
  args_pretty: '{"query":"seeded locally"}',
  result_pretty: "[]",
  bytes_up: 20,
  bytes_down: 128,
  duration_ms: 350,
  ts: 2000,
});
if (!localToolCall) throw new Error("tool_call fixture invalid");

const db = await openHistory();
await new Promise((resolve, reject) => {
  const tx = db.transaction("events", "readwrite");
  tx.objectStore("events").clear();
  tx.oncomplete = () => resolve(undefined);
  tx.onerror = () => reject(tx.error ?? new Error("clear failed"));
});
// Deterministic reruns: the store was just cleared, so these three records
// (ts 1000/2000/3000) are the ONLY local history for this session.
await appendEvents(db, SESSION_ID, [
  { sessionId: SESSION_ID, ts: 1000, event: assistantEvent("local one") },
  { sessionId: SESSION_ID, ts: 2000, event: localToolCall },
  { sessionId: SESSION_ID, ts: 3000, event: assistantEvent("local dup") },
]);

// ------------------------------------------------- stub fetch (before app)

const realFetch = window.fetch.bind(window);

/** @type {any} */
const stateFixture = await (await realFetch("/test/fixtures/state.json")).json();

/** @type {string[]} */
const sessionFetches = [];

/**
 * One rollout catch-up line: `ts\0type\0text` (the _type segment precedes
 * the JSON payload — the server's egress format).
 *
 * @param {number} ts
 * @param {string} type
 * @param {unknown} payload
 */
function line(ts, type, payload) {
  return `${ts}\0${type}\0${JSON.stringify(payload)}`;
}

window.fetch = /** @type {typeof window.fetch} */ (async (input, init) => {
  const url =
    typeof input === "string"
      ? input
      : input instanceof Request
        ? input.url
        : String(input);
  if (url === "/api/state") {
    return new Response(JSON.stringify(stateFixture), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  if (url.startsWith(`/api/session/${SESSION_ID}`)) {
    sessionFetches.push(url);
    // The frame at ts 3000 duplicates the local frontier record — it must be
    // filtered by the exclusive-frontier rule (never re-rendered). The
    // ts-6000 prompt is a client echo the rollout persists but the browser
    // must skip.
    const body = [
      line(3000, "assistant", { _type: "assistant", id: null, text: "local dup" }),
      line(4000, "assistant", { _type: "assistant", id: null, text: "server two" }),
      line(5000, "tool_call", {
        _type: "tool_call",
        id: null,
        session_id: SESSION_ID,
        tool: "Grep",
        args_pretty: '{"pattern":"catchup"}',
        result_pretty: "[]",
        bytes_up: 10,
        bytes_down: 64,
        duration_ms: 120,
        ts: 5000,
      }),
      line(6000, "prompt", { _type: "prompt", id: "p1", text: "client echo" }),
      line(7000, "assistant", {
        _type: "assistant",
        id: null,
        text: "server three",
      }),
    ].join("\n");
    return new Response(body, { status: 200 });
  }
  return realFetch(/** @type {RequestInfo} */ (input), init);
});

// ------------------------------------------------------------ stub client

/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEvent = null;

/** @type {any} */
window.AgtClient = {
  async connect(/** @type {any} */ opts) {
    onEvent = opts.onEvent ?? null;
    const captured = {
      onOpen: opts.onOpen ?? (() => {}),
      onClose: opts.onClose ?? (() => {}),
      onError: opts.onError ?? (() => {}),
    };
    captured.onOpen();
    await Promise.resolve();
    // Live ready arrives AFTER the catch-up replay completes.
    const ready = parseWireEvent({
      _type: "ready",
      version: "9.9.9-catchup",
      websocket_path: "/ws",
    });
    if (onEvent && ready) onEvent(ready);
    return { dispose() {} };
  },
  async sendPrompt() {
    return "ok";
  },
  dispose() {},
};

// ------------------------------------------------------------- load the UI

await import("/src/components/agt-app.js");

const app = need(document.querySelector("agt-app"), "agt-app element missing");

// ---------------------------------------------------------------- helpers

/** @typedef {HTMLElement & { event: Readonly<import("/src/wire.mjs").ChatEvent> }} AgtMsgLike */

/** @returns {AgtMsgLike[]} */
function renderedMsgs() {
  return /** @type {AgtMsgLike[]} */ ([...document.querySelectorAll("agt-msg")]);
}

/** @returns {HTMLElement[]} the chat log's direct children in render order */
function logChildren() {
  const log = need(
    /** @type {HTMLElement | null} */ (document.querySelector("agt-chat-log")),
    "chat log missing",
  );
  return [...log.children].map((el) => /** @type {HTMLElement} */ (el));
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

/** @param {string} text */
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

/** Signature used to compare store/IDB contents without timestamps. */
/** @param {import("/src/wire.mjs").ChatEvent} event */
function signature(event) {
  if (event._type === "tool_call") {
    return `tool_call:${/** @type {any} */ (event).tool}`;
  }
  if (event._type === "assistant") {
    return `assistant:${/** @type {any} */ (event).text}`;
  }
  return event._type;
}

const EXPECTED_ORDER = [
  "assistant:local one",
  "tool_call:WebSearch",
  "assistant:local dup",
  "assistant:server two",
  "tool_call:Grep",
  "assistant:server three",
  "ready",
];

// ----------------------------------------------------------------- tests

await test("boot fetches the catch-up stream exactly after the local IDB frontier", async () => {
  await waitFor(
    () =>
      renderedMsgs().some(
        (m) => /** @type {any} */ (m.event).text === "server three",
      ),
    "catch-up replay rendered",
  );
  assertEqual(sessionFetches.length, 1, "one catch-up fetch");
  assertEqual(
    sessionFetches[0],
    `/api/session/${SESSION_ID}?after=3000`,
    "frontier query param (max local ts, exclusive)",
  );
});

await test("replay order: local records → server frames → live ready; no duplicates", async () => {
  await waitFor(
    () => /** @type {any} */ (app).state.map(signature).join("|").endsWith("ready"),
    "live ready appended last",
  );
  const signatures = /** @type {any} */ (app).state.map(signature);
  assertEqual(
    JSON.stringify(signatures),
    JSON.stringify(EXPECTED_ORDER),
    "append-only replay order",
  );
  const dupCount = signatures.filter(
    /** @param {string} s */ (s) => s === "assistant:local dup",
  ).length;
  assertEqual(dupCount, 1, "frontier duplicate filtered exactly once");
  assert(
    !signatures.some(
      /** @param {string} s */ (s) => s.includes("client echo"),
    ),
    "client-echo prompt frame must be skipped",
  );
});

await test("rendered log shows the replay in order; tool lines appear only under /verbose", async () => {
  assertEqual(/** @type {any} */ (app).verbose, false, "verbose starts off");
  assertEqual(
    logChildren().map((el) => el.tagName).join(","),
    "AGT-MSG,AGT-MSG,AGT-MSG,AGT-MSG,AGT-MSG",
    "tool_call records stored but not rendered while verbose is off",
  );
  const texts = renderedMsgs().map(
    (m) => /** @type {any} */ (m.event).text ?? /** @type {any} */ (m.event)._type,
  );
  assertEqual(
    JSON.stringify(texts),
    JSON.stringify([
      "local one",
      "local dup",
      "server two",
      "server three",
      "ready",
    ]),
    "rendered bubble order",
  );

  // /verbose through the composer: the stored tool_call lines render in their
  // arrival-order positions between the bubbles.
  await type("/verbose");
  await pressKey("Enter");
  await waitFor(() => /** @type {any} */ (app).verbose === true, "verbose on");
  await waitFor(
    () => logChildren().filter((el) => el.tagName === "AGT-TOOL-LINE").length === 2,
    "both tool lines rendered",
  );
  assertEqual(
    logChildren().map((el) => el.tagName).join(","),
    "AGT-MSG,AGT-TOOL-LINE,AGT-MSG,AGT-MSG,AGT-TOOL-LINE,AGT-MSG,AGT-MSG",
    "tool lines interleaved in replay order",
  );
  const toolLines = logChildren()
    .filter((el) => el.tagName === "AGT-TOOL-LINE")
    .map((el) => /** @type {any} */ (el).event.tool);
  assertEqual(
    JSON.stringify(toolLines),
    JSON.stringify(["WebSearch", "Grep"]),
    "tool identities survive the replay",
  );
});

await test("persisted history holds exactly local + fresh frames (no duplicate writes)", async () => {
  const records = await getAll(db, SESSION_ID);
  const signatures = records.map((r) =>
    signature(/** @type {import("/src/wire.mjs").ChatEvent} */ (r.event)),
  );
  assertEqual(
    JSON.stringify(signatures),
    JSON.stringify(EXPECTED_ORDER),
    "history converges to the replay order",
  );
  const dups = signatures.filter(
    /** @param {string} s */ (s) => s === "assistant:local dup",
  ).length;
  assertEqual(dups, 1, "the frontier-duplicate frame was never re-put");
});

// ---------------------------------------------------------------- results

window.__CATCHUP_TEST_RESULTS__ = { pass, fail, details, run: 1 };
document.title = "catchup-tests-done";
// Mirror the PASS/FAIL summary into the DOM: the result stays observable
// without a console listener.
{
  const summaryEl = document.createElement("pre");
  summaryEl.id = "catchup-tests-summary";
  summaryEl.textContent = `[catchup-tests] pass=${pass} fail=${fail}`;
  document.body.append(summaryEl);
}
console.log(
  `[catchup-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[catchup-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
