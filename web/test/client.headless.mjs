// @ts-check
// Single-page headless test for the vanilla websocket client
// (`/assets/client.mjs`): the OUTGOING IO boundary. window.WebSocket is a
// stub BEFORE the client module is imported (injection-style, one page), and
// console.error is captured so the tests can prove the boundary rule —
// every outgoing frame is JTD-validated against its generated schema before
// `send`; an invalid frame is logged and NOT sent. Incoming frames are
// covered by wire.headless.mjs (parseWireEventText) and are only exercised
// here to resolve a pending prompt reply. Results land on
// window.__CLIENT_TEST_RESULTS__ and document.title becomes
// "client-tests-done".

import { validatePrompt, validateRename } from "/generated/validators.mjs";

// ---------------------------------------------------- stub WebSocket (first)

/** @type {FakeWebSocket[]} */
const sockets = [];

class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  url = "";
  readyState = FakeWebSocket.CONNECTING;
  /** @type {(() => void) | null} */
  onopen = null;
  /** @type {(() => void) | null} */
  onclose = null;
  /** @type {((e: Event) => void) | null} */
  onerror = null;
  /** @type {((ev: { data: string }) => void) | null} */
  onmessage = null;
  /** @type {string[]} frames handed to send() in order */
  sent = [];

  /** @type {Map<string, Array<() => void>>} */
  #listeners = new Map();

  /** @param {string} url */
  constructor(url) {
    this.url = url;
    sockets.push(this);
  }

  /**
   * @param {string} type
   * @param {() => void} fn
   * @param {{ once?: boolean }} [_opts]
   */
  addEventListener(type, fn, _opts) {
    const list = this.#listeners.get(type) ?? [];
    list.push(fn);
    this.#listeners.set(type, list);
  }

  /** @param {string} type */
  #fire(type) {
    for (const fn of this.#listeners.get(type) ?? []) fn();
  }

  /** Test driver: transition to OPEN and fire the open handlers. */
  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.#fire("open");
    if (this.onopen) this.onopen();
  }

  /** Test driver: deliver an incoming frame text. @param {string} data */
  receive(data) {
    if (this.onmessage) this.onmessage({ data });
  }

  /** @param {string} data */
  send(data) {
    this.sent.push(data);
  }

  close() {
    this.readyState = FakeWebSocket.CLOSED;
    if (this.onclose) this.onclose();
  }
}

window.WebSocket = /** @type {typeof WebSocket} */ (
  /** @type {unknown} */ (FakeWebSocket)
);

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

/** @param {unknown} condition @param {string} message */
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
 * Stub console.error to capture calls; returns a restore function plus the
 * captured calls array.
 *
 * @returns {{ calls: unknown[][], restore: () => void }}
 */
function stubConsoleError() {
  /** @type {unknown[][]} */
  const calls = [];
  const original = console.error;
  console.error = (...args) => {
    calls.push(args);
  };
  return {
    calls,
    restore: () => {
      console.error = original;
    },
  };
}

// ------------------------------------------------------------ load client

await import("/assets/client.mjs");

const client = window.AgtClient;
if (!client) throw new Error("window.AgtClient missing after import");

// connect() resolves only once the socket opens — start it, then drive the
// stub socket to OPEN and await the connection.
const connected = client.connect({});
await new Promise((resolve) => setTimeout(resolve, 0));
sock().open();
await connected;

/** The single stub socket created by connect(). */
function sock() {
  const s = sockets[sockets.length - 1];
  if (!s) throw new Error("no stub socket created");
  return s;
}

// ----------------------------------------------------------------- tests

await test("connect opens a websocket to /ws on the current host", () => {
  const s = sock();
  assertEqual(s.url, "ws://localhost:9516/ws", "stub socket url");
  assertEqual(s.readyState, FakeWebSocket.OPEN, "socket is open");
});

await test("sendRename validates against the rename schema and sends the frame", () => {
  const s = sock();
  const before = s.sent.length;
  client.sendRename?.("fresh title");
  assertEqual(s.sent.length, before + 1, "exactly one frame sent");
  assertEqual(
    s.sent[s.sent.length - 1],
    '{"_type":"rename","title":"fresh title"}',
    "frame text",
  );
});

await test("sendRename with a non-string title logs malformed and does NOT send", () => {
  const stubbed = stubConsoleError();
  try {
    const s = sock();
    const before = s.sent.length;
    client.sendRename?.(/** @type {any} */ (42));
    assertEqual(s.sent.length, before, "no frame sent");
    assertEqual(stubbed.calls.length, 1, "one console.error");
    assert(
      String(stubbed.calls[0][0]).includes("rename"),
      "error names the offending frame kind",
    );
  } finally {
    stubbed.restore();
  }
});

await test("sendPrompt validates against the prompt schema and sends the frame", async () => {
  const s = sock();
  const before = s.sent.length;
  const reply = client.sendPrompt("hello agent", "req_test_1");
  assertEqual(s.sent.length, before + 1, "exactly one frame sent");
  assertEqual(
    s.sent[s.sent.length - 1],
    '{"_type":"prompt","id":"req_test_1","text":"hello agent"}',
    "frame text",
  );
  // Resolve via the incoming assistant reply (id-matched, validated).
  s.receive('{"_type":"assistant","id":"req_test_1","text":"hi there"}');
  assertEqual(await reply, "hi there", "reply text resolved by id");
});

await test("sendPrompt with a non-string text logs malformed and does NOT send", async () => {
  const stubbed = stubConsoleError();
  try {
    const s = sock();
    const before = s.sent.length;
    // Race a 1s fallback rejection so a client that SENDS an invalid frame
    // (and then waits forever for a reply that never comes) fails fast
    // instead of hanging the suite.
    const outcome = await Promise.race([
      client.sendPrompt(/** @type {any} */ (42), "req_test_2").then(
        () => "sent",
        (error) => ({ rejected: error }),
      ),
      new Promise((resolve) =>
        setTimeout(() => resolve("timed out"), 1000),
      ),
    ]);
    assert(
      outcome !== "sent" && outcome !== "timed out",
      `sendPrompt must reject for a malformed frame (got ${JSON.stringify(outcome)})`,
    );
    assert(
      String(/** @type {{rejected: Error}} */ (outcome).rejected.message).includes("malformed"),
      `rejection mentions malformed: ${/** @type {{rejected: Error}} */ (outcome).rejected.message}`,
    );
    assertEqual(s.sent.length, before, "no frame sent");
    assertEqual(stubbed.calls.length, 1, "one console.error");
    assert(
      String(stubbed.calls[0][0]).includes("prompt"),
      "error names the offending frame kind",
    );
  } finally {
    stubbed.restore();
  }
});

await test("the generated prompt validator rejects an extra field", () => {
  const errors = validatePrompt({
    _type: "prompt",
    id: "req_x",
    text: "hi",
    extra: true,
  });
  assert(errors.length > 0, "extra field must produce errors");
  assertEqual(validateRename({ _type: "rename", title: "t" }).length, 0, "rename valid");
});

// ---------------------------------------------------------------- results

window.__CLIENT_TEST_RESULTS__ = { pass, fail, details };
document.title = "client-tests-done";
// Mirror the PASS/FAIL summary into the DOM: the result stays observable
// without a console listener.
{
  const summaryEl = document.createElement("pre");
  summaryEl.id = "client-tests-summary";
  summaryEl.textContent = `[client-tests] pass=${pass} fail=${fail}`;
  document.body.append(summaryEl);
}
console.log(
  `[client-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[client-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
