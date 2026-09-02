// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { deepFreeze, parseWireEvent, parseWireEventText } from "./wire.mjs";

/**
 * @import { ReadyEvent, PongEvent, AssistantEvent, ErrorEvent } from "./wire.mjs"
 */

/** Stub console.error, returning captured calls; restore in test teardown.
 *
 * @param {import("node:test").TestContext} t
 */
function stubConsoleError(t) {
  /** @type {unknown[][]} */
  const calls = [];
  const original = console.error;
  console.error = (...args) => {
    calls.push(args);
  };
  t.after(() => {
    console.error = original;
  });
  return calls;
}

test("deepFreeze freezes nested objects and arrays", () => {
  const value = deepFreeze({ a: { b: [1, { c: 2 }] } });
  assert.ok(Object.isFrozen(value));
  assert.ok(Object.isFrozen(value.a));
  assert.ok(Object.isFrozen(value.a.b));
  assert.ok(Object.isFrozen(value.a.b[1]));
});

test("ready event parses and is deeply frozen", () => {
  const event = /** @type {ReadyEvent} */ (parseWireEvent({
    _type: "ready",
    version: "0.1.1",
    websocket_path: "/ws",
  }));
  assert.equal(event._type, "ready");
  assert.equal(event.version, "0.1.1");
  assert.equal(event.websocket_path, "/ws");
  assert.ok(Object.isFrozen(event));
});

test("pong event parses and is deeply frozen", () => {
  const event = /** @type {PongEvent} */ (parseWireEvent({ _type: "pong", id: null }));
  assert.deepEqual(event, { _type: "pong", id: null });
  assert.ok(Object.isFrozen(event));
});

test("assistant event parses and is deeply frozen", () => {
  const event = /** @type {AssistantEvent} */ (parseWireEvent({
    _type: "assistant",
    id: "req_1",
    text: "hello",
  }));
  assert.equal(event.text, "hello");
  assert.ok(Object.isFrozen(event));
});

test("error event parses and is deeply frozen", () => {
  const event = /** @type {ErrorEvent} */ (parseWireEvent({ _type: "error", id: null, message: "boom" }));
  assert.equal(event.message, "boom");
  assert.ok(Object.isFrozen(event));
});

test("missing _type returns null silently", (t) => {
  const calls = stubConsoleError(t);
  assert.equal(parseWireEvent({ version: "1" }), null);
  assert.equal(parseWireEvent(null), null);
  assert.equal(parseWireEvent("nonsense"), null);
  assert.equal(parseWireEvent(42), null);
  assert.equal(calls.length, 0);
});

test("unknown _type logs malformed/unsupported and returns null", (t) => {
  const calls = stubConsoleError(t);
  assert.equal(parseWireEvent({ _type: "nope" }), null);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "[wire] malformed/unsupported frame (unknown _type)");
  assert.equal(calls[0][1], "nope");
});

test("assistant with numeric text logs malformed frame and returns null", (t) => {
  const calls = stubConsoleError(t);
  assert.equal(parseWireEvent({ _type: "assistant", id: null, text: 42 }), null);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "[wire] malformed frame");
  assert.equal(calls[0][1], "assistant");
  const errors = /** @type {{instancePath: string, schemaPath: string}[]} */ (
    calls[0][2]
  );
  assert.ok(errors.length > 0);
  assert.ok(
    errors.some((e) => e.instancePath === "/text"),
    "expected an error with instancePath /text",
  );
  assert.ok(
    errors.every(
      (e) =>
        typeof e.instancePath === "string" && typeof e.schemaPath === "string",
    ),
  );
});

test("error event with extra unexpected property logs and returns null", (t) => {
  const calls = stubConsoleError(t);
  assert.equal(
    parseWireEvent({
      _type: "error",
      id: null,
      message: "boom",
      extra: true,
    }),
    null,
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "[wire] malformed frame");
  const errors = /** @type {{instancePath: string}[]} */ (calls[0][2]);
  assert.ok(errors.some((e) => e.instancePath === "/extra"));
});

test("parseWireEventText parses valid JSON", () => {
  const event = parseWireEventText('{"_type":"pong","id":"req_1"}');
  assert.deepEqual(event, { _type: "pong", id: "req_1" });
  assert.ok(Object.isFrozen(event));
});

test("parseWireEventText on invalid JSON logs and returns null", (t) => {
  const calls = stubConsoleError(t);
  assert.equal(parseWireEventText("{not json"), null);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "[wire] malformed JSON frame");
  assert.ok(calls[0][1] instanceof SyntaxError);
});
