import test from "node:test";
import assert from "node:assert/strict";
import { deepFreeze, parseWireEvent, parseWireEventText } from "./wire.mjs";

test("deepFreeze freezes nested objects and arrays", () => {
  const value = deepFreeze({ a: { b: [1, { c: 2 }] } });
  assert.ok(Object.isFrozen(value));
  assert.ok(Object.isFrozen(value.a));
  assert.ok(Object.isFrozen(value.a.b));
  assert.ok(Object.isFrozen(value.a.b[1]));
});

test("ready event parses and is deeply frozen", () => {
  const event = parseWireEvent({
    _type: "ready",
    version: "0.1.1",
    websocket_path: "/ws",
  });
  assert.equal(event._type, "ready");
  assert.equal(event.version, "0.1.1");
  assert.equal(event.websocket_path, "/ws");
  assert.ok(Object.isFrozen(event));
});

test("pong event parses and is deeply frozen", () => {
  const event = parseWireEvent({ _type: "pong", id: null });
  assert.deepEqual(event, { _type: "pong", id: null });
  assert.ok(Object.isFrozen(event));
});

test("assistant event parses and is deeply frozen", () => {
  const event = parseWireEvent({
    _type: "assistant",
    id: "req_1",
    text: "hello",
  });
  assert.equal(event.text, "hello");
  assert.ok(Object.isFrozen(event));
});

test("error event parses and is deeply frozen", () => {
  const event = parseWireEvent({ _type: "error", id: null, message: "boom" });
  assert.equal(event.message, "boom");
  assert.ok(Object.isFrozen(event));
});

test("unknown _type throws TypeError", () => {
  assert.throws(
    () => parseWireEvent({ _type: "nope" }),
    (err) => err instanceof TypeError && err.message.includes("nope"),
  );
});

test("missing _type throws TypeError", () => {
  assert.throws(
    () => parseWireEvent({ version: "1" }),
    (err) => err instanceof TypeError,
  );
});

test("assistant with numeric text throws with instancePath detail", () => {
  assert.throws(
    () => parseWireEvent({ _type: "assistant", id: null, text: 42 }),
    (err) =>
      err instanceof Error &&
      !(err instanceof TypeError) &&
      err.message.includes("assistant") &&
      err.message.includes("/text"),
  );
});

test("error event with extra unexpected property throws", () => {
  assert.throws(
    () => parseWireEvent({
      _type: "error",
      id: null,
      message: "boom",
      extra: true,
    }),
    (err) => err instanceof Error && err.message.includes("extra"),
  );
});

test("parseWireEventText parses valid JSON", () => {
  const event = parseWireEventText('{"_type":"pong","id":"req_1"}');
  assert.deepEqual(event, { _type: "pong", id: "req_1" });
});

test("parseWireEventText on invalid JSON throws", () => {
  assert.throws(() => parseWireEventText("{not json"), SyntaxError);
});
