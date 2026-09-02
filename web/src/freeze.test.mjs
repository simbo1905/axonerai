// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { deepFreeze } from "./wire.mjs";

test("deepFreeze freezes in place and returns the same reference", () => {
  const input = { a: { b: 1 } };
  const result = deepFreeze(input);
  assert.equal(result, input);
});

test("deepFreeze freezes nested objects and arrays at every level", () => {
  const value = deepFreeze({
    user: { name: "Alice", roles: ["admin", "editor"] },
    config: { theme: "dark", settings: { fontSize: 14 } },
    log: [1, { nested: true }],
    grid: [[2]],
  });
  assert.ok(Object.isFrozen(value));
  assert.ok(Object.isFrozen(value.user));
  assert.ok(Object.isFrozen(value.user.roles));
  assert.ok(Object.isFrozen(value.config));
  assert.ok(Object.isFrozen(value.config.settings));
  assert.ok(Object.isFrozen(value.log));
  assert.ok(Object.isFrozen(value.log[1]));
  assert.ok(Object.isFrozen(value.grid));
  assert.ok(Object.isFrozen(value.grid[0]));
});

test("deepFreeze leaves primitives untouched", () => {
  assert.equal(deepFreeze(42), 42);
  assert.equal(deepFreeze("text"), "text");
  assert.equal(deepFreeze(true), true);
  assert.equal(deepFreeze(null), null);
  assert.equal(deepFreeze(undefined), undefined);
});

test("deepFreeze skips already-frozen subtrees (not re-walked)", () => {
  // Structural proof: the frozen subtree is skipped, so its UNFROZEN inner
  // object stays unfrozen — a re-walk would have frozen it too.
  const inner = { deep: { value: 1 } };
  const subtree = Object.freeze({ inner });
  const root = deepFreeze({ subtree });
  assert.ok(Object.isFrozen(subtree));
  assert.ok(Object.isFrozen(root));
  assert.ok(!Object.isFrozen(inner), "already-frozen subtree must be skipped");
});

test("deepFreeze: mutation throws in strict mode (objects)", () => {
  const value = deepFreeze({ a: { b: 1 } });
  assert.throws(() => {
    value.a.b = 2;
  }, TypeError);
});

test("deepFreeze: mutation throws in strict mode (arrays)", () => {
  const value = deepFreeze({ roles: ["admin"] });
  assert.throws(() => {
    value.roles.push("editor");
  }, TypeError);
  assert.throws(() => {
    value.roles[0] = "guest";
  }, TypeError);
});

// NOTE: cyclic input is OUT OF CONTRACT. deepFreeze is the DAG-only
// canonical version (see the vanilla-js-jsdoc skill, "Freeze on the IO
// boundary"): wire events, UI state and console envelopes are acyclic value
// objects, so there is deliberately no visited-set and NO cycle test — a
// cyclic input would recurse infinitely by design.
