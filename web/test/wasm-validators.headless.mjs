// @ts-check
import { initWasmValidators } from "/src/wasm-validators.mjs";
import { parseWireEvent, parseWireEventText } from "/src/wire.mjs";

/**
 * Single-page headless suite for the WASM validator path: awaits
 * `initWasmValidators()` (so `wire.mjs` runs the Rust/WASM-generated
 * validators installed over the `.mjs` fallback) and then asserts the
 * SAME drop/malformed semantics `web/test/wire.headless.mjs` asserts for
 * the `.mjs` validators. Loaded by `wasm-validators-runner.html`; results
 * land on `window.__WASM_VALIDATORS_TEST_RESULTS__`.
 */

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

await initWasmValidators();

test("valid ready event parses via WASM validator, frozen", () => {
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
});

test("valid tool_call event parses via WASM validator, frozen", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({
        _type: "tool_call",
        id: null,
        session_id: "sess_1",
        tool: "WebSearch",
        args_pretty: "{}",
        result_pretty: "null",
        bytes_up: 20,
        bytes_down: 128,
        duration_ms: 350,
        ts: 1700000000000,
      }),
    ),
    "tool_call event should not be dropped",
  );
  assertEqual(event._type, "tool_call", "tool_call _type mismatch");
  assert(Object.isFrozen(event), "tool_call event is not frozen");
  assertEqual(
    /** @type {any} */ (event).duration_ms,
    350,
    "tool_call duration_ms round-trip",
  );
});

test("valid ack event parses via WASM validator, frozen", () => {
  const event = need(
    parseWireEventText(
      JSON.stringify({
        _type: "ack",
        for_type: "rename",
        ok: true,
        message: null,
      }),
    ),
    "ack event should not be dropped",
  );
  assertEqual(event._type, "ack", "ack _type mismatch");
  assert(Object.isFrozen(event), "ack event is not frozen");
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

test("wrong _type constant on ready is logged as unknown and returns null", () => {
  const { calls, restore } = stubConsoleError();
  try {
    // `_type` IS the dispatch key: a frame whose `_type` is not a registry
    // key is logged as malformed/unsupported before any validator runs (the
    // validator-level enum check is covered by the pure-Rust bad_case_a/f
    // suites against the generated code itself).
    assertEqual(
      parseWireEvent({ _type: "wrong", version: "1.0.0", websocket_path: "/ws" }),
      null,
      "wrong _type constant should return null",
    );
    assertEqual(calls.length, 1, "expected one console.error call");
    assertEqual(calls[0][0], "[wire] malformed/unsupported frame (unknown _type)", "unexpected console.error prefix");
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

window.__WASM_VALIDATORS_TEST_RESULTS__ = { pass, fail, details };
document.title = "wasm-validators-tests-done";
// Flush one macrotask before the summary so headless drivers that attach
// their console listener at load-end still see it.
await new Promise((resolve) => setTimeout(resolve, 0));
// Mirror the PASS/FAIL summary into the DOM as well: the result stays
// observable without a console listener.
{
  const summaryEl = document.createElement("pre");
  summaryEl.id = "wasm-validators-tests-summary";
  summaryEl.textContent = `[wasm-validators-tests] pass=${pass} fail=${fail}`;
  document.body.append(summaryEl);
}
console.log(
  `[wasm-validators-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[wasm-validators-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
