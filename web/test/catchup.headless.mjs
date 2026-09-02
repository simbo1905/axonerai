// @ts-check
// Headless ?s=<uuid> catch-up test over the line protocol (item29).
//
// Runs in Chrome against a served tree in TWO page runs (separated by a
// real reload; the run number lives in sessionStorage):
//   run 1 — IndexedDB is wiped, stub fetch serves a canned line-format body
//           (`ts\0type\0json\n`, incl. a complete tool_call and a
//           truncated-at-1024 tool_call) for after=0; asserts catch-up
//           landed in the store in ts order BEFORE the WS connect, verbose
//           OFF hides tool lines, verbose ON shows them (formatted
//           bytes/duration, expanding shows the pretty payload with the
//           abridged truncation), and live events advance the frontier;
//           then reloads.
//   run 2 — the stub serves only a NEWER frame (ts = after + 1, where
//           after is the IndexedDB frontier the client requested); asserts
//           the frontier was respected (old frames NOT re-fetched, no
//           duplicates).
// window.fetch and window.AgtClient are stubbed BEFORE the app is imported.
// Results land on window.__CATCHUP_TEST_RESULTS__; document.title becomes
// "catchup-tests-done-1" / "catchup-tests-done-2".
import { deepFreeze } from "/src/wire.mjs";

const SESSION = "01890a5d-ac96-774b-bcce-b302099a8057";
const RUN_KEY = "catchup-run";
/** @type {number} */
const run = Number(sessionStorage.getItem(RUN_KEY) ?? "1");

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
 * @template T
 * @param {() => T} fn
 * @param {string} what
 * @param {number} [timeout]
 * @returns {Promise<T>}
 */
async function waitFor(fn, what, timeout = 5000) {
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

// ------------------------------------------------- canned line-format body

/**
 * Serialize a tool_call event in the exact wire field order the server
 * writes (metadata first, payload LAST — src/wire.rs), so a truncated
 * variant keeps complete metadata for the lenient scan.
 *
 * @param {{ id: string | null, tool: string, args_pretty: string, result_pretty: string, bytes_up: number, bytes_down: number, duration_ms: number, ts: number }} fields
 */
function toolCallWire(fields) {
  return `{"_type":"tool_call","id":${JSON.stringify(fields.id)},"session_id":${JSON.stringify(SESSION)},"tool":${JSON.stringify(fields.tool)},"bytes_up":${fields.bytes_up},"bytes_down":${fields.bytes_down},"duration_ms":${fields.duration_ms},"ts":${fields.ts},"args_pretty":${JSON.stringify(fields.args_pretty)},"result_pretty":${JSON.stringify(fields.result_pretty)}}`;
}

const COMPLETE_TOOL_CALL = toolCallWire({
  id: null,
  tool: "Calculator",
  args_pretty: '{"expr": "2+2"}',
  result_pretty: "4",
  bytes_up: 12,
  bytes_down: 340,
  duration_ms: 750,
  ts: 1001,
});

// An oversized tool_call: the server egress-truncates the payload at 1024
// bytes — metadata serializes first, so the cut lands deep inside
// args_pretty and strict JSON.parse fails (partial tool_call).
const BIG_TOOL_CALL = toolCallWire({
  id: null,
  tool: "WebSearch",
  args_pretty: `{"query": "axonerai rollout wasm deep dive", "pages": [${"x".repeat(3000)}]}`,
  result_pretty: '{"took_ms": 42}',
  bytes_up: 120,
  bytes_down: 4567,
  duration_ms: 8123,
  ts: 1002,
});
// Cut the PAYLOAD (not the `ts\0type\0` prefix) at 1024 bytes, like the
// server's egress truncation does.
const TRUNCATED_TOOL_CALL = BIG_TOOL_CALL.slice(0, 1024);
assert(
  TRUNCATED_TOOL_CALL.length === 1024,
  "truncated tool_call must be exactly 1024 bytes",
);

/** The full canned catch-up body (run 1, after=0). */
function cannedBody() {
  return [
    `1000\0assistant\0${JSON.stringify({
      _type: "assistant",
      id: "req_h1",
      text: "hello from history",
    })}\n`,
    `1001\0tool_call\0${COMPLETE_TOOL_CALL}\n`,
    `1002\0tool_call\0${TRUNCATED_TOOL_CALL}\n`,
  ].join("");
}

// ------------------------------------------------- stub fetch (before app)

const realFetch = window.fetch.bind(window);

/** @type {number | null} */
let requestedAfter = null;

window.fetch = /** @type {typeof window.fetch} */ (
  async (input, init) => {
    const url =
      typeof input === "string"
        ? input
        : input instanceof Request
          ? input.url
          : String(input);
    const match = url.match(
      new RegExp(`^/api/session/${SESSION}\\?after=(\\d+)$`),
    );
    if (match) {
      const after = Number(match[1]);
      requestedAfter = after;
      if (after === 0) {
        return new Response(cannedBody(), {
          status: 200,
          headers: { "Content-Type": "application/x-rollout-line" },
        });
      }
      // Frontier respected: only frames strictly newer than the requested
      // high-watermark, i.e. here exactly one synthetic newer frame.
      const newer = `${after + 1}\0assistant\0${JSON.stringify({
        _type: "assistant",
        id: "req_new",
        text: "post-reload replay",
      })}\n`;
      return new Response(newer, {
        status: 200,
        headers: { "Content-Type": "application/x-rollout-line" },
      });
    }
    return realFetch(/** @type {RequestInfo} */ (input), init);
  }
);

// ------------------------------------------------------------ stub client

/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEvent = null;

/**
 * Validate + freeze via the app's own wire layer, then deliver like
 * client.mjs would.
 *
 * @param {unknown} frame
 */
function emit(frame) {
  const event = /** @type {import("/src/wire.mjs").WireEvent} */ (
    deepFreeze(/** @type {any} */ (frame))
  );
  if (onEvent) onEvent(event);
}

/** @type {any} */
window.AgtClient = {
  async connect(/** @type {any} */ opts) {
    onEvent = opts.onEvent ?? null;
    captured.onOpen();
    await tick();
    emit({
      _type: "ready",
      version: "9.9.9-catchup-test",
      websocket_path: "/ws",
    });
    emit({
      _type: "session_meta",
      created_at: 1,
      session_id: SESSION,
      title: "catchup-test",
    });
    await tick();
    emit({ _type: "assistant", id: "req_live", text: "live reply" });
    return { dispose() {} };
  },
  async sendPrompt() {
    return "ok";
  },
  dispose() {},
};

/** @type {{ onOpen: () => void, onClose: () => void, onError: (e: Event) => void }} */
let captured = {
  onOpen: () => {},
  onClose: () => {},
  onError: () => {},
};

// ------------------------------------------------------------- helpers

/** @returns {HTMLElement} */
function appEl() {
  return need(
    /** @type {HTMLElement | null} */ (document.querySelector("agt-app")),
    "agt-app element missing",
  );
}

/** @returns {Readonly<import("/src/wire.mjs").ChatEvent[]>} */
function state() {
  return /** @type {any} */ (appEl()).state;
}

/** @param {string} text */
function stateTexts(text) {
  return state().filter(
    (event) =>
      (/** @type {any} */ (event).text ?? "").includes(text),
  );
}

/** @returns {NodeListOf<Element>} */
function toolLines() {
  return document.querySelectorAll("agt-chat-log agt-tool-line");
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

// ----------------------------------------------------------------- run 1

if (run === 1) {
  // Fresh history: wipe IndexedDB BEFORE the app boots; drop any stale
  // run-1 results from a previous execution.
  localStorage.removeItem("catchup-run1-results");
  await new Promise((resolve, reject) => {
    const req = indexedDB.deleteDatabase("agt");
    req.onsuccess = () => resolve(null);
    req.onerror = () => reject(req.error ?? new Error("deleteDatabase failed"));
    req.onblocked = () => resolve(null);
  });

  await import("/src/components/agt-app.js");

  await test("catch-up appends validated events to the store in ts order before the WS connect", async () => {
    await waitFor(
      () => stateTexts("hello from history").length === 1,
      "catch-up assistant in store",
    );
    await waitFor(
      () => stateTexts("live reply").length === 1,
      "live reply in store",
    );
    const events = state();
    // tool_call events (complete + partial reconstruction) reached the store.
    const toolCalls = events.filter((e) => /** @type {any} */ (e)._type === "tool_call");
    assertEqual(toolCalls.length, 2, "two tool_call events in store");
    const calculator = toolCalls[0];
    assertEqual(
      /** @type {any} */ (calculator).tool,
      "Calculator",
      "complete tool_call survives strict parse",
    );
    const partial = toolCalls[1];
    assertEqual(/** @type {any} */ (partial).tool, "WebSearch", "partial tool_call tool");
    assertEqual(/** @type {any} */ (partial).bytes_up, 120, "partial bytes_up");
    assertEqual(/** @type {any} */ (partial).bytes_down, 4567, "partial bytes_down");
    assertEqual(/** @type {any} */ (partial).duration_ms, 8123, "partial duration_ms");
    assertEqual(/** @type {any} */ (partial).ts, 1002, "partial ts");
    assertEqual(/** @type {any} */ (partial).abridged, true, "partial flagged abridged");
    assert(
      /** @type {any} */ (partial).args_pretty.includes("axonerai rollout wasm deep dive"),
      "partial args_pretty carries the raw truncated head",
    );
    assert(Object.isFrozen(partial), "partial event deep-frozen");
    // ts order: assistant(1000) < tool_call(1001) < tool_call(1002) < live.
    const indexOf = (/** @type {string} */ needle) =>
      events.findIndex((e) =>
        (/** @type {any} */ (e).text ?? /** @type {any} */ (e).tool ?? "").includes(needle),
      );
    const liveIndex = indexOf("live reply");
    assert(
      indexOf("hello from history") < indexOf("Calculator") &&
        indexOf("Calculator") < indexOf("WebSearch") &&
        indexOf("WebSearch") < liveIndex,
      `store order must be catch-up (ts order) before live events: ${events.map((e) => /** @type {any} */ (e)._type).join(",")}`,
    );
    assertEqual(requestedAfter, 0, "boot catch-up requests after=0");
  });

  await test("verbose OFF hides tool lines", () => {
    assertEqual(toolLines().length, 0, "no agt-tool-line rendered");
  });

  await test("verbose ON shows tool lines; expanding shows pretty payload (abridged head visible)", async () => {
    await type("/verbose");
    await pressKey("Enter");
    await waitFor(() => toolLines().length === 2, "two tool lines rendered");
    const lines = [...toolLines()];
    const summary = (/** @type {Element} */ el) =>
      /** @type {HTMLElement | null} */ (
        /** @type {HTMLElement} */ (el).shadowRoot?.querySelector(".summary")
      )?.textContent ?? "";
    // formatted bytes + duration from format.mjs
    assert(summary(lines[0]).includes("↑12B"), `calculator bytes up: ${summary(lines[0])}`);
    assert(summary(lines[0]).includes("↓340B"), `calculator bytes down: ${summary(lines[0])}`);
    assert(summary(lines[0]).includes("750ms"), `calculator duration: ${summary(lines[0])}`);
    assert(summary(lines[1]).includes("↑120B"), `websearch bytes up: ${summary(lines[1])}`);
    assert(summary(lines[1]).includes("↓4.5KB"), `websearch bytes down: ${summary(lines[1])}`);
    assert(summary(lines[1]).includes("8s"), `websearch duration: ${summary(lines[1])}`);
    assert(summary(lines[1]).includes("WebSearch"), "tool name in line");
    // triangle collapsed by default
    const tri = (/** @type {Element} */ el) =>
      /** @type {HTMLElement | null} */ (
        /** @type {HTMLElement} */ (el).shadowRoot?.querySelector(".tri")
      )?.textContent ?? "";
    assertEqual(tri(lines[1]), "▸", "collapsed triangle");
    // expand the abridged (partial) line lazily
    need(
      /** @type {HTMLElement | null} */ (
        /** @type {HTMLElement} */ (lines[1]).shadowRoot?.querySelector(".line")
      ),
      "line button missing",
    ).dispatchEvent(new MouseEvent("click", { bubbles: true }));
    const pre = await waitFor(
      () => {
        const el = /** @type {HTMLElement} */ (lines[1]).shadowRoot?.querySelector(
          'pre[data-part="args"]',
        );
        return el && (el.textContent ?? "").length > 0 ? el : null;
      },
      "expanded args payload",
    );
    assert(
      (need(pre, "expanded args pre missing").textContent ?? "").includes(
        "axonerai rollout wasm deep dive",
      ),
      "expanded payload contains the abridged head",
    );
  });

  await test("history frontier advanced past the live stamps (reloaded catch-up gets only newer)", async () => {
    // The live ready + assistant events were stamped Date.now() and
    // persisted; wait for the IDB writes to settle via a microtask drain.
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 50));
    // Persist this run's results through the reload, then trigger it.
    localStorage.setItem(
      "catchup-run1-results",
      JSON.stringify({ pass, fail, details }),
    );
    sessionStorage.setItem(RUN_KEY, "2");
    location.reload();
  });
} else {
  // ----------------------------------------------------------------- run 2
  await import("/src/components/agt-app.js");

  await test("reload respects the frontier: only newer frames replay, no duplicates", async () => {
    await waitFor(
      () => stateTexts("post-reload replay").length === 1,
      "newer frame caught up after reload",
    );
    assertEqual(
      stateTexts("hello from history").length,
      0,
      "old catch-up frame must NOT be re-fetched (frontier respected)",
    );
    assert(
      /** @type {number} */ (requestedAfter) > 1_000_000_000_000,
      `frontier request carried the IDB high-watermark (got ${requestedAfter})`,
    );
    assertEqual(
      stateTexts("live reply").length,
      1,
      "live reply present exactly once (from this run's connect)",
    );
    assertEqual(toolLines().length, 0, "no tool lines without tool frames");
  });
}

// ---------------------------------------------------------------- results

// Merge in run 1's results (persisted through the reload by localStorage).
/** @type {{ pass: number, fail: number, details: typeof details }} */
const run1 = JSON.parse(localStorage.getItem("catchup-run1-results") ?? "null") ?? {
  pass: 0,
  fail: run === 1 ? 0 : -1,
  details: [],
};

window.__CATCHUP_TEST_RESULTS__ = {
  pass: pass + run1.pass,
  fail: fail + run1.fail,
  details: [...run1.details, ...details],
  run,
};
document.title = `catchup-tests-done-${run}`;
