// @ts-check
// Unit tests for the pure console model helpers (item32). Node-only: no
// BroadcastChannel, no worker, no IndexedDB — those boundaries are tested
// alone on their own sides (repo AGENTS.md rule).
import test from "node:test";
import assert from "node:assert/strict";
import {
  createConsoleEntry,
  evictIds,
  formatConsoleArgs,
  isAtBottom,
  mergeEntries,
  newCountOnAppend,
  seqOf,
} from "./console-model.mjs";

const PAGE_A = "aaaaaaaa-1111-2222-3333-444444444444";
const PAGE_B = "bbbbbbbb-1111-2222-3333-444444444444";

/**
 * Deterministic envelope for tests.
 *
 * @param {string} id
 * @param {number} ts
 * @param {string} [text]
 * @param {"log" | "info" | "warn" | "error"} [level]
 * @param {string} [pageId]
 */
function entry(id, ts, text = id, level = "log", pageId = PAGE_A) {
  return { id, pageId, ts, level, text };
}

test("formatConsoleArgs: strings raw, objects JSON 2-space, space-joined", () => {
  assert.equal(formatConsoleArgs(["hello"]), "hello");
  assert.equal(formatConsoleArgs(["hello", "world"]), "hello world");
  assert.equal(formatConsoleArgs(["obj:", { a: 1 }]), 'obj: {\n  "a": 1\n}');
  assert.equal(
    formatConsoleArgs([{ b: [1, 2], c: { d: true } }]),
    '{\n  "b": [\n    1,\n    2\n  ],\n  "c": {\n    "d": true\n  }\n}',
  );
});

test("formatConsoleArgs: numbers/booleans/null via JSON.stringify", () => {
  assert.equal(formatConsoleArgs([42]), "42");
  assert.equal(formatConsoleArgs([true, false, null]), "true false null");
  assert.equal(formatConsoleArgs([0 / 0]), "null", "NaN serializes as null");
});

test("formatConsoleArgs: undefined and circular fall back to String()", () => {
  assert.equal(formatConsoleArgs([undefined]), "undefined");
  const circular = /** @type {any} */ ({});
  circular.self = circular;
  assert.equal(formatConsoleArgs([circular]), "[object Object]");
  assert.equal(formatConsoleArgs([]), "");
});

test("createConsoleEntry: id is `${pageId}:${seq}` and fields map through", () => {
  const originalNow = Date.now;
  Date.now = () => 1_700_000_000_000;
  try {
    assert.deepEqual(createConsoleEntry({
      pageId: PAGE_A,
      seq: 7,
      level: "warn",
      text: "careful",
    }), {
      id: `${PAGE_A}:7`,
      pageId: PAGE_A,
      ts: 1_700_000_000_000,
      level: "warn",
      text: "careful",
    });
  } finally {
    Date.now = originalNow;
  }
});

test("seqOf: extracts the trailing numeric seq, NaN-safe fallback 0", () => {
  assert.equal(seqOf(`${PAGE_A}:1`), 1);
  assert.equal(seqOf(`${PAGE_A}:123`), 123);
  assert.equal(seqOf("weird:id:42"), 42, "pageId itself may contain colons");
  assert.equal(seqOf(`${PAGE_A}:nope`), 0);
  assert.equal(seqOf("nocolon"), 0);
});

test("mergeEntries: dedupes by id and orders by ts → seq", () => {
  const backlog = [entry("a:1", 100), entry("a:2", 300)];
  const live = [entry("a:2", 300), entry("a:3", 200)];
  const merged = mergeEntries(backlog, live);
  assert.deepEqual(
    merged.map((e) => e.id),
    ["a:1", "a:3", "a:2"],
    "a:3 (ts 200) slots between a:1 and a:2; duplicate a:2 dropped",
  );
});

test("mergeEntries: same ts breaks the tie by seq", () => {
  const merged = mergeEntries([], [entry("a:9", 500), entry("a:10", 500)]);
  assert.deepEqual(
    merged.map((e) => e.id),
    ["a:9", "a:10"],
    "seq 9 before seq 10",
  );
});

test("mergeEntries: cross-page ids with equal ts order by their own seq", () => {
  const merged = mergeEntries(
    [],
    [entry("b:1", 700, "", "log", PAGE_B), entry("a:5", 700, "", "log", PAGE_A)],
  );
  assert.deepEqual(
    merged.map((e) => e.id),
    ["b:1", "a:5"],
  );
});

test("mergeEntries: does not mutate inputs and returns a fresh array", () => {
  const existing = [entry("a:1", 100)];
  const incoming = [entry("a:2", 200)];
  const merged = mergeEntries(existing, incoming);
  assert.equal(existing.length, 1);
  assert.deepEqual(existing.map((e) => e.id), ["a:1"]);
  assert.deepEqual(merged.map((e) => e.id), ["a:1", "a:2"]);
  assert.ok(merged !== existing, "must be a fresh array");
});

test("mergeEntries: no fresh ids returns an equal copy without re-sorting work", () => {
  const existing = [entry("a:2", 300), entry("a:1", 100)];
  const merged = mergeEntries(existing, [entry("a:2", 300)]);
  assert.deepEqual(merged, existing);
  assert.ok(merged !== existing, "must be a fresh array");
});

test("evictIds: empty when at or under capacity", () => {
  assert.deepEqual(evictIds([], 2000), []);
  const two = [entry("a:1", 1), entry("a:2", 2)];
  assert.deepEqual(evictIds(two, 2000), []);
  assert.deepEqual(evictIds(two, 2), []);
});

test("evictIds: evicts the OLDEST (ts → seq) beyond capacity", () => {
  const entries = [
    entry("a:1", 100),
    entry("a:2", 200),
    entry("a:3", 300),
    entry("a:4", 400),
  ];
  assert.deepEqual(evictIds(entries, 2), ["a:1", "a:2"]);
});

test("evictIds: production policy — 2000 capacity evicts exactly the excess", () => {
  const entries = Array.from({ length: 2003 }, (_, i) =>
    entry(`a:${i + 1}`, i),
  );
  const evicted = evictIds(entries, 2000);
  assert.equal(evicted.length, 3);
  assert.deepEqual(evicted, ["a:1", "a:2", "a:3"]);
});

test("isAtBottom: stick-to-bottom threshold arithmetic against a fake element", () => {
  assert.equal(
    isAtBottom({ scrollTop: 400, scrollHeight: 800, clientHeight: 400 }),
    true,
    "exactly at the bottom",
  );
  assert.equal(
    isAtBottom({ scrollTop: 380, scrollHeight: 800, clientHeight: 400 }),
    true,
    "20px up still counts as stuck (slack 32)",
  );
  assert.equal(
    isAtBottom({ scrollTop: 368, scrollHeight: 800, clientHeight: 400 }),
    true,
    "exactly at the 32px slack boundary",
  );
  assert.equal(
    isAtBottom({ scrollTop: 367, scrollHeight: 800, clientHeight: 400 }),
    false,
    "33px up is no longer stuck",
  );
  assert.equal(
    isAtBottom({ scrollTop: 0, scrollHeight: 800, clientHeight: 400 }),
    false,
    "top of a scrollable log",
  );
  assert.equal(
    isAtBottom({ scrollTop: 0, scrollHeight: 400, clientHeight: 400 }),
    true,
    "nothing to scroll: always stuck",
  );
  assert.equal(
    isAtBottom({ scrollTop: 0, scrollHeight: 400, clientHeight: 400 }, 0),
    true,
    "custom slack 0",
  );
});

test("newCountOnAppend: stuck keeps the pill at zero; scrolled up accumulates", () => {
  assert.equal(newCountOnAppend(0, true, 3), 0, "stuck: view follows, no pill");
  assert.equal(newCountOnAppend(0, false, 3), 3, "scrolled up: 3 new");
  assert.equal(newCountOnAppend(3, false, 2), 5, "accumulates while scrolled up");
  assert.equal(newCountOnAppend(5, false, 0), 5, "no fresh entries: unchanged");
});
