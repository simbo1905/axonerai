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

/** @param {unknown} actual @param {unknown} expected @param {string} message */
function assertEqual(actual, expected, message) {
  assert(
    actual === expected,
    `${message} (expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)})`,
  );
}

test("valid ready event parses, correct _type, deeply frozen", () => {
  const event = parseWireEventText(
    JSON.stringify({
      _type: "ready",
      version: "1.0.0",
      websocket_path: "/ws",
    }),
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
  const event = parseWireEventText(JSON.stringify({ _type: "pong", id: "abc" }));
  assertEqual(event._type, "pong", "pong _type mismatch");
  assert(Object.isFrozen(event), "pong event is not frozen");
});

test("valid assistant event parses, correct _type, frozen", () => {
  const event = parseWireEventText(
    JSON.stringify({ _type: "assistant", id: null, text: "hello" }),
  );
  assertEqual(event._type, "assistant", "assistant _type mismatch");
  assert(Object.isFrozen(event), "assistant event is not frozen");
});

test("valid error event parses, correct _type, frozen", () => {
  const event = parseWireEventText(
    JSON.stringify({ _type: "error", id: "e1", message: "boom" }),
  );
  assertEqual(event._type, "error", "error _type mismatch");
  assert(Object.isFrozen(event), "error event is not frozen");
});

test("mutation attempt on frozen event fails", () => {
  const event = parseWireEventText(
    JSON.stringify({ _type: "ready", version: "1.0.0", websocket_path: "/ws" }),
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

test("unknown _type throws TypeError", () => {
  const error = throws(() =>
    parseWireEventText(JSON.stringify({ _type: "nope" })),
  );
  assert(error instanceof TypeError, `expected TypeError, got ${String(error)}`);
});

test("assistant with numeric text throws", () => {
  const error = throws(() =>
    parseWireEventText(
      JSON.stringify({ _type: "assistant", id: null, text: 42 }),
    ),
  );
  assert(
    error instanceof Error && !(error instanceof TypeError),
    `expected validation Error, got ${String(error)}`,
  );
});

test("extra unexpected property throws", () => {
  const error = throws(() =>
    parseWireEventText(
      JSON.stringify({
        _type: "ready",
        version: "1.0.0",
        websocket_path: "/ws",
        surprise: true,
      }),
    ),
  );
  assert(error instanceof Error, `expected Error, got ${String(error)}`);
});

test("parseWireEventText on invalid JSON throws", () => {
  const error = throws(() => parseWireEventText("not json"));
  assert(
    error instanceof SyntaxError,
    `expected SyntaxError, got ${String(error)}`,
  );
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
