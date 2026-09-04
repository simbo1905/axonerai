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

test("dispatch with a validator-backed type and no handler throws", () => {
  // "ready" IS in VALIDATOR_TYPES but is not registered yet — a missing
  // handler for a validator-backed type is a bug, not data.
  assert.throws(
    () =>
      dispatch(
        /** @type {any} */ (
          deepFreeze({ _type: "ready", version: "1", websocket_path: "/ws" })
        ),
      ),
    /no handler for ready/,
  );
});

test("dispatch with an unknown _type logs malformed and drops (no handler runs)", () => {
  const originalError = console.error;
  /** @type {unknown[][]} */
  const calls = [];
  console.error = (...args) => {
    calls.push(args);
  };
  try {
    /** @type {string[]} */
    const handled = [];
    // A handler registered for a known type must NOT fire for an unknown one.
    registerHandler("pong", (event) => {
      handled.push(/** @type {PongEvent} */ (event)._type);
    });
    const result = dispatch(/** @type {any} */ (deepFreeze({ _type: "nope" })));
    assert.equal(result, undefined, "unknown _type is dropped");
    assert.deepEqual(handled, [], "no handler fired");
    assert.equal(calls.length, 1, "one console.error call");
    assert.equal(
      calls[0][0],
      "[dispatch] malformed/unsupported frame (unknown _type)",
      "unexpected console.error prefix",
    );
    assert.equal(calls[0][1], "nope", "expected _type value logged");
  } finally {
    console.error = originalError;
  }
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
