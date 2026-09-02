// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { createStore } from "./store.mjs";
import { deepFreeze } from "./wire.mjs";

/** @typedef {import("./wire.mjs").ChatEvent} ChatEvent */

/**
 * @param {string} text
 * @returns {ChatEvent}
 */
function frozenEvent(text) {
  return /** @type {ChatEvent} */ (
    deepFreeze(/** @type {any} */ ({ _type: "prompt", id: text, text }))
  );
}

test("append stores the identical frozen object", () => {
  const store = createStore();
  const event = frozenEvent("a");
  store.append(event);
  assert.equal(store.size, 1);
  assert.equal(store.getEvents()[0], event);
});

test("getEvents returns a frozen array (and the same reference between changes)", () => {
  const store = createStore();
  assert.ok(Object.isFrozen(store.getEvents()));
  const before = store.getEvents();
  store.append(frozenEvent("a"));
  assert.ok(Object.isFrozen(store.getEvents()));
  assert.ok(store.getEvents() !== before, "expected a new array reference");
});

test("append of a non-frozen object throws TypeError", () => {
  const store = createStore();
  assert.throws(
    () => store.append(/** @type {any} */ ({ _type: "prompt", id: "x", text: "x" })),
    TypeError,
  );
  assert.equal(store.size, 0);
});

test("append of null or a primitive throws TypeError", () => {
  const store = createStore();
  assert.throws(() => store.append(/** @type {any} */ (null)), TypeError);
  assert.throws(() => store.append(/** @type {any} */ ("nope")), TypeError);
  assert.throws(() => store.append(/** @type {any} */ (42)), TypeError);
  assert.throws(() => store.append(/** @type {any} */ (undefined)), TypeError);
  assert.equal(store.size, 0);
});

test("append returns a NEW frozen array; the old reference is unchanged", () => {
  const store = createStore();
  store.append(frozenEvent("a"));
  const old = store.getEvents();
  const next = store.append(frozenEvent("b"));
  assert.ok(next !== old, "expected a new array reference");
  assert.ok(Object.isFrozen(next));
  assert.equal(old.length, 1);
  assert.equal(next.length, 2);
});

test("subscribe: listener receives the new array; unsubscribe stops notifications", () => {
  const store = createStore();
  /** @type {Readonly<ChatEvent[]>[]} */
  const seen = [];
  const unsubscribe = store.subscribe((events) => seen.push(events));

  store.append(frozenEvent("a"));
  assert.equal(seen.length, 1);
  assert.equal(seen[0], store.getEvents());

  unsubscribe();
  unsubscribe(); // idempotent
  store.append(frozenEvent("b"));
  assert.equal(seen.length, 1);
});

test("listener added during a notify is not called for that change", () => {
  const store = createStore();
  /** @type {Readonly<ChatEvent[]>[]} */
  const lateSeen = [];
  store.subscribe(() => {
    store.subscribe((events) => lateSeen.push(events));
  });

  store.append(frozenEvent("a"));
  assert.equal(lateSeen.length, 0, "late listener fired for its own notify");

  store.append(frozenEvent("b"));
  assert.equal(lateSeen.length, 1);
});

test("appendAll batch appends with a single notify", () => {
  const store = createStore();
  let notifications = 0;
  store.subscribe(() => {
    notifications += 1;
  });

  const next = store.appendAll([frozenEvent("a"), frozenEvent("b")]);
  assert.equal(store.size, 2);
  assert.equal(notifications, 1);
  assert.equal(next.length, 2);
  assert.ok(Object.isFrozen(next));
});

test("appendAll validates every event and appends nothing on failure", () => {
  const store = createStore();
  store.append(frozenEvent("a"));
  assert.throws(
    () => store.appendAll([frozenEvent("b"), /** @type {any} */ ({ _type: "prompt" })]),
    TypeError,
  );
  assert.equal(store.size, 1);
});

test("duplicate subscribe of the same function is allowed (two entries)", () => {
  const store = createStore();
  let calls = 0;
  /** @param {Readonly<ChatEvent[]>} _events */
  const listener = (_events) => {
    calls += 1;
  };
  const unsubscribe = store.subscribe(listener);
  store.subscribe(listener);

  store.append(frozenEvent("a"));
  assert.equal(calls, 2);

  unsubscribe();
  store.append(frozenEvent("b"));
  assert.equal(calls, 3, "only one entry should remain after unsubscribe");
});

test("store façade is frozen and size reflects the log", () => {
  const store = createStore();
  assert.ok(Object.isFrozen(store));
  assert.equal(store.size, 0);
  store.appendAll([frozenEvent("a"), frozenEvent("b")]);
  assert.equal(store.size, 2);
});
