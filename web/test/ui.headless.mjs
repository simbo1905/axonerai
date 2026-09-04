// @ts-check
// Headless UI test for the vanilla web-components chat UI.
//
// Runs in Chrome against a served tree (root-absolute imports), stubs
// window.AgtClient with a fake client that mimics web/assets/client.mjs
// (validate every incoming frame via parseWireEventText, dispatch to
// onEvent, resolve/reject sendPrompt by matching id) and emits recorded
// server frames. Results land on window.__UI_TEST_RESULTS__ and
// document.title becomes "ui-tests-done".
import { parseWireEventText } from "/src/wire.mjs";

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

/**
 * @typedef {import("/src/wire.mjs").ChatEvent} ChatEvent
 * @typedef {import("/src/wire.mjs").PromptEvent} PromptEvent
 * @typedef {import("/src/wire.mjs").AssistantEvent} AssistantEvent
 * @typedef {import("/src/wire.mjs").ErrorEvent} ErrorEvent
 * @typedef {HTMLElement & { event: Readonly<ChatEvent> }} AgtMsgLike
 */

/** @returns {AgtMsgLike[]} */
function renderedMsgs() {
  return /** @type {AgtMsgLike[]} */ ([...document.querySelectorAll("agt-msg")]);
}

// ------------------------------------------------------------ stub client

/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEvent = null;

/**
 * @typedef {{ kind: "assistant", text: string, delay: number } | { kind: "error", message: string, delay: number }} ReplyScript
 */

/** @type {ReplyScript | null} */
let nextReply = null;

/**
 * Mimic client.mjs's onmessage: validate + freeze, dispatch; dropped (null)
 * frames are already logged by wire.mjs.
 *
 * @param {unknown} frame
 */
function emit(frame) {
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
    window.__UI_STUB__ = {
      handlers: captured,
      emit,
      /**
       * @param {ReplyScript} reply
       */
      setNextReply(reply) {
        nextReply = reply;
      },
    };
    captured.onOpen();
    await Promise.resolve();
    emit({ _type: "ready", version: "9.9.9-test", websocket_path: "/ws" });
    return { dispose() {} };
  },
  /**
   * @param {string} _text
   * @param {string} [id]
   * @returns {Promise<string>}
   */
  sendPrompt(_text, id) {
    const script =
      nextReply ??
      /** @type {ReplyScript} */ ({
        kind: "assistant",
        text: "ok",
        delay: 0,
      });
    nextReply = null;
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        if (script.kind === "assistant") {
          emit({ _type: "assistant", id: id ?? null, text: script.text });
          resolve(script.text);
        } else {
          emit({ _type: "error", id: id ?? null, message: script.message });
          reject(new Error(script.message));
        }
      }, script.delay);
    });
  },
  dispose() {},
};

// ------------------------------------------------------------- load the UI

// Tiny extension: stub /api/state (fixture JSON) and /api/tools BEFORE the
// app is imported so agt-app's control-plane fetches stay console-clean.
// Assertions below are unmodified.
const realFetch = window.fetch.bind(window);
/** @type {any} */
const stateFixture = await (
  await realFetch("/test/fixtures/state.json")
).json();
window.fetch = /** @type {typeof window.fetch} */ (async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof Request ? input.url : String(input);
  if (url === "/api/state") {
    return new Response(JSON.stringify(stateFixture), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  if (url === "/api/tools" && init && init.method === "POST") {
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  if (url === "/api/mcp" && init && init.method === "POST") {
    // item48: the browser boot may re-apply stored MCP toggle prefs (the
    // panel headless suite seeds them on this origin) — emulate the
    // server-side per-server suppression in the fixture.
    const body = /** @type {{ server: string, enabled: boolean }} */ (
      JSON.parse(String(init.body))
    );
    const server = /** @type {{ name: string, enabled: boolean } | undefined} */ (
      stateFixture.mcp?.find((/** @type {{ name: string }} */ m) => m.name === body.server)
    );
    if (server) server.enabled = body.enabled;
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  return realFetch(/** @type {RequestInfo} */ (input), init);
});

await import("/src/components/agt-app.js");

const app = need(document.querySelector("agt-app"), "agt-app element missing");
// Non-null once the stub client's connect() has registered it.
const stub = /** @type {NonNullable<Window["__UI_STUB__"]>} */ (
  window.__UI_STUB__
);

/** @returns {any} */
function composerEl() {
  return need(document.querySelector("agt-composer"), "composer missing");
}

// ----------------------------------------------------------------- tests

await test("ready event renders as frozen system message and status pill shows Connected", async () => {
  const ready = /** @type {AgtMsgLike} */ (
    need(
      await waitFor(
        () =>
          renderedMsgs().find((m) =>
            String(m.textContent).includes("9.9.9-test"),
          ),
        "ready message",
      ),
      "ready message not found",
    )
  );
  assertEqual(ready.event._type, "ready", "ready _type mismatch");
  assert(Object.isFrozen(ready.event), "rendered ready event is not frozen");
  const pill = need(
    await waitFor(() => document.querySelector("agt-status .pill"), "status pill"),
    "status pill not rendered",
  );
  assert(
    pill.classList.contains("pill-connected"),
    `expected connected pill, got ${pill.className}`,
  );
});

await test("prompt renders as You bubble and composer is disabled while in flight", async () => {
  const composer = composerEl();
  const textarea = /** @type {HTMLTextAreaElement} */ (
    composer.querySelector("textarea")
  );
  const button = /** @type {HTMLButtonElement} */ (
    composer.querySelector("button")
  );

  stub.setNextReply({
    kind: "assistant",
    text: "Hello from the stub agent",
    delay: 150,
  });
  textarea.value = "  hello agent  ";
  button.click();

  const you = /** @type {AgtMsgLike} */ (
    need(
      await waitFor(
        () => renderedMsgs().find((m) => m.event._type === "prompt"),
        "prompt bubble",
      ),
      "prompt bubble not found",
    )
  );
  assertEqual(
    /** @type {PromptEvent} */ (you.event).text,
    "hello agent",
    "prompt text mismatch",
  );
  assert(Object.isFrozen(you.event), "prompt record is not frozen");
  assert(
    textarea.disabled === true,
    "textarea should be disabled while the request is in flight",
  );
  assert(
    button.disabled === true,
    "send button should be disabled while the request is in flight",
  );

  const agent = /** @type {AgtMsgLike} */ (
    need(
      await waitFor(
        () => renderedMsgs().find((m) => m.event._type === "assistant"),
        "assistant bubble",
      ),
      "assistant bubble not found",
    )
  );
  assertEqual(
    /** @type {AssistantEvent} */ (agent.event).text,
    "Hello from the stub agent",
    "reply text mismatch",
  );
  assert(Object.isFrozen(agent.event), "assistant event is not frozen");

  await waitFor(
    () => !composer.querySelector("textarea").disabled,
    "composer re-enabled after reply",
  );
});

await test("messages render in order with correct roles", () => {
  const types = renderedMsgs().map((m) => m.event._type);
  assertEqual(
    JSON.stringify(types),
    JSON.stringify(["ready", "prompt", "assistant"]),
    "unexpected message order",
  );
  const roles = renderedMsgs().map(
    (m) =>
      need(
        m.querySelector(".msg-role"),
        "role element missing",
      ).textContent,
  );
  assertEqual(
    JSON.stringify(roles),
    JSON.stringify(["System", "You", "Agent"]),
    "unexpected role labels",
  );
});

await test("error reply renders as error bubble and composer re-enables", async () => {
  const composer = composerEl();
  const textarea = /** @type {HTMLTextAreaElement} */ (
    composer.querySelector("textarea")
  );

  stub.setNextReply({
    kind: "error",
    message: "No provider configured",
    delay: 80,
  });
  textarea.value = "make me an error";
  composer.querySelector("button").click();

  const error = /** @type {AgtMsgLike} */ (
    need(
      await waitFor(
        () => renderedMsgs().find((m) => m.event._type === "error"),
        "error bubble",
      ),
      "error bubble not found",
    )
  );
  assertEqual(
    /** @type {ErrorEvent} */ (error.event).message,
    "No provider configured",
    "error text mismatch",
  );
  assert(
    error.classList.contains("msg-error"),
    `error bubble missing msg-error class, got ${error.className}`,
  );
  assert(Object.isFrozen(error.event), "error event is not frozen");

  await waitFor(
    () => !composer.querySelector("textarea").disabled,
    "composer re-enabled after error",
  );
});

await test("every rendered event is the frozen object held in app state", () => {
  const state = /** @type {Readonly<ChatEvent[]>} */ (
    /** @type {any} */ (app).state
  );
  assert(Object.isFrozen(state), "app state array is not frozen");
  for (const entry of state) {
    assert(Object.isFrozen(entry), `state entry not frozen: ${entry._type}`);
  }
  const rendered = renderedMsgs().map((m) => m.event);
  assert(rendered.length > 0, "no rendered messages found");
  for (const event of rendered) {
    assert(
      state.includes(event),
      "rendered event is not the same frozen object stored in state",
    );
  }
});

await test("disconnect disables the composer; reconnect re-enables it", async () => {
  const composer = composerEl();
  stub.handlers.onClose();
  assert(
    composer.querySelector("textarea").disabled === true,
    "composer should be disabled when disconnected",
  );
  need(
    await waitFor(
      () => document.querySelector("agt-status .pill.pill-disconnected"),
      "disconnected pill",
    ),
    "disconnected pill not rendered",
  );

  stub.handlers.onOpen();
  await waitFor(
    () => !composer.querySelector("textarea").disabled,
    "composer re-enabled after reconnect",
  );
});

// ---------------------------------------------------------------- results

window.__UI_TEST_RESULTS__ = { pass, fail, details };
document.title = "ui-tests-done";
console.log(
  `[ui-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[ui-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
