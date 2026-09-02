// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { frontierOf, mergeCatchup } from "./history.mjs";

test("mergeCatchup filters frames at or below the frontier", () => {
  const frames = [
    { ts: 100 },
    { ts: 200 },
    { ts: 300 },
  ];
  assert.deepEqual(mergeCatchup(frames, 200), [{ ts: 300 }]);
});

test("mergeCatchup keeps everything past a zero frontier (sorted)", () => {
  const frames = [{ ts: 5 }, { ts: 2 }, { ts: 9 }];
  assert.deepEqual(mergeCatchup(frames, 0), [{ ts: 2 }, { ts: 5 }, { ts: 9 }]);
});

test("mergeCatchup sorts ascending by ts (file order)", () => {
  const frames = [
    { ts: 900, type: "assistant" },
    { ts: 100, type: "tool_call" },
    { ts: 500, type: "assistant" },
  ];
  assert.deepEqual(
    mergeCatchup(frames, 0).map((frame) => frame.ts),
    [100, 500, 900],
  );
});

test("mergeCatchup excludes the frontier boundary itself (exclusive)", () => {
  const frames = [{ ts: 41 }, { ts: 42 }, { ts: 43 }];
  assert.deepEqual(
    mergeCatchup(frames, 42).map((frame) => frame.ts),
    [43],
  );
});

test("mergeCatchup returns a new array and never mutates the input", () => {
  const frames = [{ ts: 200 }, { ts: 100 }];
  const copy = [...frames];
  const merged = mergeCatchup(frames, 0);
  assert.ok(merged !== frames, "expected a new array");
  assert.deepEqual(frames, copy, "input untouched");
});

test("mergeCatchup on an empty frame list yields an empty list", () => {
  assert.deepEqual(mergeCatchup([], 123), []);
});

test("frontierOf is the max ts across records", () => {
  assert.equal(frontierOf([{ ts: 10 }, { ts: 70 }, { ts: 30 }]), 70);
});

test("frontierOf is 0 for no records", () => {
  assert.equal(frontierOf([]), 0);
});

test("frontierOf survives unsorted and single-entry histories", () => {
  assert.equal(frontierOf([{ ts: 3 }]), 3);
  assert.equal(frontierOf([{ ts: 30 }, { ts: 3 }]), 30);
});
