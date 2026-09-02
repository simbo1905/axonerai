// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { deepFreeze } from "./wire.mjs";
import { dispatch, hasHandler, registerHandler } from "./dispatch.mjs";

/**
 * @import { AssistantEvent, PongEvent, WireEvent } from "./wire.mjs"
 */

test("handler receives the identical frozen event object", () => {
  /** @type {WireEvent[]} */
  const received = [];
  registerHandler("assistant", (event) => {
    received.push(event);
  });
  const event = deepFreeze(
    /** @type {AssistantEvent} */ ({
      _type: "assistant",
      id: "req_1",
      text: "hello",
    }),
  );
  dispatch(event);
  assert.equal(received.length, 1);
  assert.ok(Object.is(received[0], event), "handler got a different object");
  assert.ok(Object.isFrozen(received[0]));
});

test("dispatch returns the handler's return value", () => {
  registerHandler("pong", (event) => /** @type {PongEvent} */ (event).id);
  const result = dispatch(
    deepFreeze(/** @type {PongEvent} */ ({ _type: "pong", id: "req_2" })),
  );
  assert.equal(result, "req_2");
});

test("dispatch with null or missing _type throws TypeError", () => {
  assert.throws(() => dispatch(/** @type {any} */ (null)), TypeError);
  assert.throws(() => dispatch(/** @type {any} */ (undefined)), TypeError);
  assert.throws(() => dispatch(/** @type {any} */ ({})), TypeError);
  assert.throws(
    () => dispatch(/** @type {any} */ ({ version: "1" })),
    TypeError,
  );
  assert.throws(
    () => dispatch(/** @type {any} */ ({ _type: 42 })),
    TypeError,
  );
});

test("dispatch with an unknown-but-valid-shaped type and no handler throws", () => {
  assert.throws(
    () => dispatch(/** @type {any} */ (deepFreeze({ _type: "nope" }))),
    /no handler for nope/,
  );
});

test("register/overwrite/hasHandler semantics", () => {
  assert.equal(hasHandler("error"), false);

  /** @type {string[]} */
  const calls = [];
  registerHandler("error", () => {
    calls.push("first");
  });
  assert.equal(hasHandler("error"), true);
  dispatch(/** @type {any} */ (deepFreeze({ _type: "error", id: null, message: "a" })));
  assert.deepEqual(calls, ["first"]);

  registerHandler("error", () => {
    calls.push("second");
  });
  assert.equal(hasHandler("error"), true);
  dispatch(/** @type {any} */ (deepFreeze({ _type: "error", id: null, message: "b" })));
  assert.deepEqual(calls, ["first", "second"]);
});
