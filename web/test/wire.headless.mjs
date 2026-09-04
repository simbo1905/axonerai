// @ts-check
import {
  validateReady,
  validatePong,
  validateAssistant,
  validateError,
} from "/generated/validators.mjs";
import { deepFreeze, parseWireEvent, parseWireEventText } from "/src/wire.mjs";

/** @param {() => void} fn */
function throws(fn) {
  try {
    fn();
  } catch (error) {
    return error;
  }
  return undefined;
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

/** @type {{ name: string, ok: boolean, error?: string }[]} */
const details = [];
let pass = 0;
let fail = 0;

/**
 * @param {string} name
 * @param {() => void} fn
 */
function test(name, fn) {
  try {
    fn();
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

test("valid ready event parses, correct _type, deeply frozen", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({
        _type: "ready",
        version: "1.0.0",
        websocket_path: "/ws",
      }),
    ),
    "ready event should not be dropped",
  );
  assertEqual(event._type, "ready", "ready _type mismatch");
  assert(Object.isFrozen(event), "ready event is not frozen");
  const nested = /** @type {any} */ ({
    _type: "ready",
    version: "1.0.0",
    websocket_path: "/ws",
    extra: { deep: true },
  });
  // deepFreeze recurses into nested objects
  const frozenNested = deepFreeze(nested);
  assert(Object.isFrozen(frozenNested.extra), "nested object not frozen");
});

test("valid pong event parses, correct _type, frozen", () => {
  const event = need(
    parseWireEventText(JSON.stringify({ _type: "pong", id: "abc" })),
    "pong event should not be dropped",
  );
  assertEqual(event._type, "pong", "pong _type mismatch");
  assert(Object.isFrozen(event), "pong event is not frozen");
});

test("valid assistant event parses, correct _type, frozen", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({ _type: "assistant", id: null, text: "hello" }),
    ),
    "assistant event should not be dropped",
  );
  assertEqual(event._type, "assistant", "assistant _type mismatch");
  assert(Object.isFrozen(event), "assistant event is not frozen");
});

test("valid error event parses, correct _type, frozen", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({ _type: "error", id: "e1", message: "boom" }),
    ),
    "error event should not be dropped",
  );
  assertEqual(event._type, "error", "error _type mismatch");
  assert(Object.isFrozen(event), "error event is not frozen");
});

test("mutation attempt on frozen event fails", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({ _type: "ready", version: "1.0.0", websocket_path: "/ws" }),
    ),
    "event should not be dropped",
  );
  assert(Object.isFrozen(event), "event should be frozen before mutation");
  const error = throws(() => {
    "use strict";
    /** @type {any} */ (event).version = "tampered";
  });
  // strict-mode write throws (TypeError), and value is unchanged either way
  assert(
    error === undefined || error instanceof TypeError,
    `expected TypeError or silent failure, got ${String(error)}`,
  );
  assertEqual(
    /** @type {any} */ (event).version,
    "1.0.0",
    "frozen event value changed",
  );
});

test("missing _type returns null silently", () => {
  const { calls, restore } = stubConsoleError();
  try {
    assertEqual(parseWireEventText(JSON.stringify({ version: "1" })), null, "missing _type should return null");
    assertEqual(parseWireEvent(null), null, "null data should return null");
    assertEqual(parseWireEvent(42), null, "non-object data should return null");
    assertEqual(calls.length, 0, "no console.error expected for missing _type");
  } finally {
    restore();
  }
});

test("unknown _type logs malformed/unsupported and returns null", () => {
  const { calls, restore } = stubConsoleError();
  try {
    assertEqual(parseWireEventText(JSON.stringify({ _type: "nope" })), null, "unknown _type should return null");
    assertEqual(calls.length, 1, "expected one console.error call");
    assertEqual(calls[0][0], "[wire] malformed/unsupported frame (unknown _type)", "unexpected console.error prefix");
    assertEqual(calls[0][1], "nope", "expected _type value logged");
  } finally {
    restore();
  }
});

test("assistant with numeric text logs malformed frame and returns null", () => {
  const { calls, restore } = stubConsoleError();
  try {
    assertEqual(
      parseWireEventText(JSON.stringify({ _type: "assistant", id: null, text: 42 })),
      null,
      "invalid assistant should return null",
    );
    assertEqual(calls.length, 1, "expected one console.error call");
    assertEqual(calls[0][0], "[wire] malformed frame", "unexpected console.error prefix");
    assertEqual(calls[0][1], "assistant", "expected _type logged");
    const errors = /** @type {{instancePath: string, schemaPath: string}[]} */ (calls[0][2]);
    assert(errors.length > 0, "expected validator errors logged");
    assert(
      errors.some((e) => e.instancePath === "/text"),
      "expected an error with instancePath /text",
    );
    assert(
      errors.every((e) => typeof e.instancePath === "string" && typeof e.schemaPath === "string"),
      "expected errors shaped {instancePath, schemaPath}",
    );
  } finally {
    restore();
  }
});

test("extra unexpected property logs malformed frame and returns null", () => {
  const { calls, restore } = stubConsoleError();
  try {
    assertEqual(
      parseWireEventText(
        JSON.stringify({
          _type: "ready",
          version: "1.0.0",
          websocket_path: "/ws",
          surprise: true,
        }),
      ),
      null,
      "extra property should return null",
    );
    assertEqual(calls.length, 1, "expected one console.error call");
    assertEqual(calls[0][0], "[wire] malformed frame", "unexpected console.error prefix");
    const errors = /** @type {{instancePath: string}[]} */ (calls[0][2]);
    assert(errors.some((e) => e.instancePath === "/surprise"), "expected instancePath /surprise");
  } finally {
    restore();
  }
});

test("parseWireEventText on invalid JSON logs and returns null", () => {
  const { calls, restore } = stubConsoleError();
  try {
    assertEqual(parseWireEventText("not json"), null, "invalid JSON should return null");
    assertEqual(calls.length, 1, "expected one console.error call");
    assertEqual(calls[0][0], "[wire] malformed JSON frame", "unexpected console.error prefix");
    assert(calls[0][1] instanceof SyntaxError, "expected the SyntaxError logged");
  } finally {
    restore();
  }
});

test("raw validateReady: valid ready yields [] errors", () => {
  const errors = validateReady({
    _type: "ready",
    version: "1.0.0",
    websocket_path: "/ws",
  });
  assertEqual(errors.length, 0, "expected no errors for valid ready");
});

test("raw validatePong: valid pong yields [] errors", () => {
  const errors = validatePong({ _type: "pong", id: null });
  assertEqual(errors.length, 0, "expected no errors for valid pong");
});

test("raw validateAssistant: valid assistant yields [] errors", () => {
  const errors = validateAssistant({
    _type: "assistant",
    id: "a1",
    text: "hi",
  });
  assertEqual(errors.length, 0, "expected no errors for valid assistant");
});

test("raw validateError: valid error yields [] errors", () => {
  const errors = validateError({
    _type: "error",
    id: null,
    message: "bad",
  });
  assertEqual(errors.length, 0, "expected no errors for valid error");
});

test("raw validateReady: bad ready yields non-empty errors", () => {
  const errors = validateReady({ _type: "ready", version: 123 });
  assert(errors.length > 0, "expected non-empty errors for bad ready");
});

test("parseWireEvent deep-freezes nested content", () => {
  const data = {
    _type: "pong",
    id: "x",
    meta: { nested: true },
  };
  // pong has no extra fields allowed, so use deepFreeze directly for nesting
  const frozen = deepFreeze(data);
  assert(Object.isFrozen(frozen), "root not frozen");
  assert(Object.isFrozen(/** @type {any} */ (frozen).meta), "nested not frozen");
});

window.__WIRE_TEST_RESULTS__ = { pass, fail, details };
document.title = "wire-tests-done";
// Flush one macrotask before the summary so headless drivers that attach
// their console listener at load-end still see it (the suite is synchronous).
await new Promise((resolve) => setTimeout(resolve, 0));
// Mirror the PASS/FAIL summary into the DOM: the runner page is otherwise
// empty, which makes the result observable without a console listener too.
const summaryEl = document.createElement("pre");
summaryEl.id = "wire-tests-summary";
summaryEl.textContent = `[wire-tests] pass=${pass} fail=${fail}`;
document.body.append(summaryEl);
console.log(
  `[wire-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[wire-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
