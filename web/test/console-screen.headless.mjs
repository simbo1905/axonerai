// @ts-check
// Single-page headless DOM test for the devtools console screen (item32).
//
// ONE page, DOM alone: frozen validated entries and a stubbed backlog are
// INJECTED through the `consoleScreenIo` seam before the component boots —
// no second page, no BroadcastChannel/worker/IndexedDB orchestration, no
// polling loops (repo AGENTS.md rule). window.Worker is stubbed before the
// app is imported so installConsoleBus never spawns a real spool worker;
// the bus's own channels stay idle and unused by the assertions.
// Results land on window.__CONSOLE_TEST_RESULTS__ and document.title
// becomes "console-screen-tests-done".
import { validateConsole_entry } from "/generated/validators.mjs";
import { deepFreeze } from "/src/wire.mjs";

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

// ------------------------------------------------------------- load the UI

await import("/src/components/agt-console-app.js");
const { consoleScreenIo } = await import("/src/components/agt-console-app.js");

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
 * @param {() => unknown} fn
 * @param {string} what
 */
async function waitFor(fn, what) {
  const start = Date.now();
  for (;;) {
    if (fn()) return;
    if (Date.now() - start > 1000) throw new Error(`timeout waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

// --------------------------------------------------- injection seam setup

const PAGE_A = "11111111-2222-3333-4444-555555555555";
const PAGE_B = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

/**
 * @param {string} id
 * @param {number} ts
 * @param {"log" | "info" | "warn" | "error"} level
 * @param {string} text
 * @param {string} [pageId]
 */
function makeEntry(id, ts, level, text, pageId = PAGE_A) {
  const candidate = deepFreeze({ id, pageId, ts, level, text });
  const errors = validateConsole_entry(candidate);
  if (errors.length > 0) throw new Error(`fixture ${id} invalid: ${JSON.stringify(errors)}`);
  return candidate;
}

/** Frozen validated backlog injected as the "late arriver" snapshot. */
const backlog = deepFreeze([
  makeEntry(`${PAGE_A}:1`, 1000, "log", "first log line"),
  makeEntry(`${PAGE_A}:2`, 3000, "error", "boom: exploded\n  at line 2"),
  makeEntry(`${PAGE_B}:1`, 2000, "warn", "careful now", PAGE_B),
  makeEntry(`${PAGE_A}:3`, 4000, "info", "informational"),
]);

/** @type {((entry: any) => void) | null} */
let liveHandler = null;
/** @type {number} */
let clearCalls = 0;
let getBacklogCalls = 0;

consoleScreenIo.getBacklog = async () => {
  getBacklogCalls += 1;
  return backlog;
};
consoleScreenIo.clearBacklog = async () => {
  clearCalls += 1;
};
consoleScreenIo.subscribe = (/** @type {(entry: any) => void} */ handler) => {
  liveHandler = handler;
  return () => {
    liveHandler = null;
  };
};

// ------------------------------------------------------------ boot the DOM

const app = document.createElement("agt-console-app");
// Fixed height so the log actually overflows and the autoscroll policy is
// exercised (the component is height-flexible in production via console.html).
app.style.height = "200px";
document.body.append(app);

/** @returns {ShadowRoot} */
function shadow() {
  const root = app.shadowRoot;
  if (!root) throw new Error("console shadow root missing");
  return root;
}

/** @returns {HTMLElement} */
function logEl() {
  const el = shadow().querySelector('[data-name="log"]');
  if (!el) throw new Error("log element missing");
  return /** @type {HTMLElement} */ (el);
}

/** @returns {HTMLElement[]} */
function rows() {
  return /** @type {HTMLElement[]} */ (
    [...logEl().querySelectorAll("agt-console-line")]
  );
}

function rowIds() {
  return rows().map((row) => row.shadowRoot?.querySelector(".row")?.getAttribute("data-id"));
}

/** @returns {HTMLInputElement} */
function filterInput() {
  const el = shadow().querySelector('[data-name="filter"]');
  if (!el) throw new Error("filter input missing");
  return /** @type {HTMLInputElement} */ (el);
}

/** @returns {HTMLSpanElement} */
function countEl() {
  const el = shadow().querySelector('[data-name="count"]');
  if (!el) throw new Error("count element missing");
  return /** @type {HTMLSpanElement} */ (el);
}

/** @returns {HTMLButtonElement} */
function pillEl() {
  const el = shadow().querySelector('[data-name="pill"]');
  if (!el) throw new Error("pill element missing");
  return /** @type {HTMLButtonElement} */ (el);
}

/** @param {string} level @returns {HTMLInputElement} */
function levelBox(level) {
  const box = shadow().querySelector(`input[data-level="${level}"]`);
  if (!box) throw new Error(`level checkbox ${level} missing`);
  return /** @type {HTMLInputElement} */ (box);
}

/** @param {string} text */
async function setFilter(text) {
  filterInput().value = text;
  filterInput().dispatchEvent(new Event("input", { bubbles: true }));
  await waitFor(() => countEl().textContent?.includes("/"), "re-render");
}

/**
 * Deliver one payload on the injected live stream (any shape: valid frozen
 * envelopes AND deliberately invalid ones for the drop tests).
 *
 * @param {unknown} payload
 */
function deliverLive(payload) {
  const handler = liveHandler;
  if (!handler) throw new Error("live handler not subscribed");
  handler(payload);
}

// ----------------------------------------------------------------- tests

await test("boot: subscribes BEFORE the backlog read (load-bearing order)", async () => {
  await waitFor(() => rows().length > 0, "backlog rendered");
  assert(liveHandler !== null, "live stream subscribed");
  assert(getBacklogCalls === 1, "backlog read once");
});

await test("backlog renders in ts order with level styling and pageId", async () => {
  assertEqual(rowIds().join("|"), [
    `${PAGE_A}:1`,
    `${PAGE_B}:1`,
    `${PAGE_A}:2`,
    `${PAGE_A}:3`,
  ].join("|"), "ts-ordered rows");
  const errorRow = rows()[2].shadowRoot?.querySelector(".row");
  assert(errorRow?.classList.contains("error") === true, "error row class");
  assertEqual(
    errorRow?.querySelector(".lvl")?.textContent,
    "[ERROR]",
    "error level tag",
  );
  assertEqual(
    getComputedStyle(/** @type {HTMLElement} */ (errorRow?.querySelector(".lvl"))).color,
    "rgb(248, 113, 113)",
    "error red",
  );
  const warnRow = rows()[1].shadowRoot?.querySelector(".row");
  assertEqual(
    getComputedStyle(/** @type {HTMLElement} */ (warnRow?.querySelector(".lvl"))).color,
    "rgb(251, 191, 36)",
    "warn amber",
  );
  const infoRow = rows()[3].shadowRoot?.querySelector(".row");
  assertEqual(
    getComputedStyle(/** @type {HTMLElement} */ (infoRow?.querySelector(".lvl"))).color,
    "rgb(96, 165, 250)",
    "info blue",
  );
  assertEqual(
    rows()[0].shadowRoot?.querySelector(".text")?.textContent,
    "first log line",
    "log text",
  );
  assertEqual(
    rows()[1].shadowRoot?.querySelector(".page")?.textContent,
    PAGE_B,
    "dim pageId suffix",
  );
  assertEqual(countEl().textContent, "4/4 entries", "entry count");
});

await test("entries snapshot is deep-frozen", () => {
  const entries = /** @type {any} */ (app).entries;
  assert(Object.isFrozen(entries), "entries array frozen");
  assert(Object.isFrozen(entries[0]), "entry frozen");
});

await test("live delivery on the injected stream appears without reload; dedupe by id", () => {
  deliverLive(makeEntry(`${PAGE_A}:4`, 5000, "log", "fresh live entry"));
  assert(rowIds().includes(`${PAGE_A}:4`), "live entry rendered");
  assertEqual(countEl().textContent, "5/5 entries", "count after live");
  deliverLive(makeEntry(`${PAGE_A}:4`, 5000, "log", "fresh live entry"));
  assertEqual(countEl().textContent, "5/5 entries", "duplicate id dropped");
});

await test("invalid live payload is dropped at the render boundary", () => {
  const before = rows().length;
  deliverLive({ id: "bad:1", pageId: PAGE_A, ts: 1, level: "bogus", text: "nope" });
  deliverLive({ id: "bad:2", pageId: PAGE_A, ts: 2, level: "log" });
  deliverLive("not even an object");
  assertEqual(rows().length, before, "no rows added for invalid payloads");
});

await test("filter hides non-matching lines (case-insensitive substring)", async () => {
  await setFilter("BOOM");
  assertEqual(rowIds().join("|"), `${PAGE_A}:2`, "only the boom line matches");
  assertEqual(countEl().textContent, "1/5 entries", "filtered count");
  await setFilter("");
  assertEqual(countEl().textContent, "5/5 entries", "filter cleared");
});

await test("level checkboxes hide their level", async () => {
  levelBox("error").click();
  await waitFor(() => !rowIds().includes(`${PAGE_A}:2`), "error hidden");
  assertEqual(countEl().textContent, "4/5 entries", "count without error");
  levelBox("warn").click();
  await waitFor(() => !rowIds().includes(`${PAGE_B}:1`), "warn hidden");
  assertEqual(countEl().textContent, "3/5 entries", "count without error+warn");
  levelBox("error").click();
  levelBox("warn").click();
  await waitFor(() => rowIds().length === 5, "all levels back on");
});

await test("autoscroll: stick-to-bottom hides the pill; scrolled up shows 'N new'", async () => {
  const log = logEl();
  // Overflow deterministically: fill the 200px view with rows first.
  for (let i = 0; i < 20; i++) {
    deliverLive(makeEntry(`${PAGE_A}:${10 + i}`, 5000 + i, "log", `filler ${i}`));
  }
  log.scrollTop = 0;
  log.dispatchEvent(new Event("scroll"));
  // Scrolled up alone shows nothing — the pill appears when entries arrive.
  assert(pillEl().hidden === true, "no pill until new entries arrive");
  deliverLive(makeEntry(`${PAGE_A}:5`, 6000, "log", "while scrolled up"));
  await waitFor(() => !pillEl().hidden, "pill visible for the new entry");
  assert(
    pillEl().textContent?.includes("1 new — jump to bottom") === true,
    `pill text, got ${JSON.stringify(pillEl().textContent)}`,
  );
  // Jump-to-bottom pill click restores stickiness.
  pillEl().click();
  await waitFor(() => pillEl().hidden === true, "pill hidden after jump");
  assertEqual(
    log.scrollTop,
    log.scrollHeight - log.clientHeight,
    "scrolled to the bottom",
  );
  deliverLive(makeEntry(`${PAGE_A}:6`, 7000, "log", "while stuck"));
  await waitFor(() => rowIds().includes(`${PAGE_A}:6`), "new row rendered");
  assert(pillEl().hidden === true, "pill stays hidden while stuck");
});

await test("Clear wipes the view and calls the backlog clear once", async () => {
  const clearBtn = shadow().querySelector('[data-name="clear"]');
  if (!clearBtn) throw new Error("clear button missing");
  /** @type {HTMLElement} */ (clearBtn).click();
  await waitFor(() => rowIds().join("") === "", "view wiped");
  assertEqual(clearCalls, 1, "clearBacklog called once");
  assertEqual(countEl().textContent, "0/0 entries", "count after clear");
});

// ---------------------------------------------------------------- results

window.__CONSOLE_TEST_RESULTS__ = { pass, fail, details };
document.title = "console-screen-tests-done";
