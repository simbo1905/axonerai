// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { frontierOf, mergeCatchup, reviveHistoryRecords } from "./history.mjs";
import { parseWireEvent } from "./wire.mjs";

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

// ------------------------------------------------- reviveHistoryRecords

/**
 * Stub console.error/warn to capture calls; returns the captured arrays plus
 * a restore function.
 *
 * @returns {{ errors: unknown[][], warns: unknown[][], restore: () => void }}
 */
function stubConsole() {
  /** @type {unknown[][]} */
  const errors = [];
  /** @type {unknown[][]} */
  const warns = [];
  const originalError = console.error;
  const originalWarn = console.warn;
  console.error = (...args) => {
    errors.push(args);
  };
  console.warn = (...args) => {
    warns.push(args);
  };
  return {
    errors,
    warns,
    restore: () => {
      console.error = originalError;
      console.warn = originalWarn;
    },
  };
}

/** A validated frozen assistant event, as the app would persist it.
 *
 * @param {string} text
 */
function assistantEvent(text) {
  const event = parseWireEvent({ _type: "assistant", id: null, text });
  if (!event) throw new Error(`assistant fixture invalid: ${text}`);
  return event;
}

test("revive returns valid records frozen (event and record), unchanged", () => {
  const stubbed = stubConsole();
  try {
    const records = [
      { sessionId: "s1", ts: 100, event: assistantEvent("local one") },
      {
        sessionId: "s1",
        ts: 200,
        event: { _type: "prompt", id: "req_1", text: "hello" },
      },
    ];
    const revived = reviveHistoryRecords(records);
    assert.equal(revived.length, 2, "both records returned");
    assert.equal(revived[0].ts, 100);
    assert.equal(revived[0].sessionId, "s1");
    assert.equal(
      /** @type {any} */ (revived[0].event).text,
      "local one",
      "event content survives",
    );
    assert.ok(Object.isFrozen(revived[0]), "record is frozen");
    assert.ok(Object.isFrozen(revived[0].event), "event is frozen");
    assert.ok(Object.isFrozen(revived[1].event), "prompt event is frozen");
    assert.equal(
      /** @type {any} */ (revived[1].event)._type,
      "prompt",
      "prompt records round-trip",
    );
    assert.equal(stubbed.errors.length, 0, "no error logging for valid records");
    assert.equal(stubbed.warns.length, 0, "no warn logging for valid records");
  } finally {
    stubbed.restore();
  }
});

test("revive drops an invalid stored event and logs it", () => {
  const stubbed = stubConsole();
  try {
    const records = [
      // assistant with a numeric text — fails the JTD validator
      { sessionId: "s1", ts: 100, event: { _type: "assistant", id: null, text: 42 } },
      { sessionId: "s1", ts: 200, event: assistantEvent("kept") },
    ];
    const revived = reviveHistoryRecords(records);
    assert.equal(revived.length, 1, "invalid record dropped, valid kept");
    assert.equal(
      /** @type {any} */ (revived[0].event).text,
      "kept",
      "the valid record survives",
    );
    assert.ok(stubbed.errors.length > 0, "the drop is logged");
  } finally {
    stubbed.restore();
  }
});

test("revive drops an invalid stored prompt record and logs it", () => {
  const stubbed = stubConsole();
  try {
    const records = [
      { sessionId: "s1", ts: 100, event: { _type: "prompt", id: "p1" } },
    ];
    const revived = reviveHistoryRecords(records);
    assert.equal(revived.length, 0, "malformed prompt dropped");
    assert.ok(stubbed.errors.length > 0, "the drop is logged");
  } finally {
    stubbed.restore();
  }
});

test("revive drops records with unknown _type / missing _type and logs them", () => {
  const stubbed = stubConsole();
  try {
    const records = [
      { sessionId: "s1", ts: 100, event: { _type: "alien", size: 1 } },
      { sessionId: "s1", ts: 200, event: { version: "1" } },
    ];
    const revived = reviveHistoryRecords(records);
    assert.equal(revived.length, 0, "both dropped");
    assert.ok(
      stubbed.errors.length + stubbed.warns.length >= 2,
      "every drop is logged",
    );
  } finally {
    stubbed.restore();
  }
});

test("revive drops malformed record wrappers and logs them", () => {
  const stubbed = stubConsole();
  try {
    const records = [
      null,
      "garbage",
      { ts: 100, event: assistantEvent("no sessionId") },
      { sessionId: "s1", event: assistantEvent("no ts") },
      { sessionId: "s1", ts: "not-a-number", event: assistantEvent("bad ts") },
    ];
    const revived = reviveHistoryRecords(records);
    assert.equal(revived.length, 0, "every malformed wrapper dropped");
    assert.ok(
      stubbed.warns.length >= 5,
      "every drop is logged (warn level)",
    );
  } finally {
    stubbed.restore();
  }
});

test("revive never returns unfrozen nested data", () => {
  const records = [
    { sessionId: "s1", ts: 100, event: assistantEvent("frozen check") },
  ];
  const revived = reviveHistoryRecords(records);
  assert.ok(Object.isFrozen(revived[0]), "record frozen");
  assert.ok(Object.isFrozen(revived[0].event), "event frozen");
  // The input structured clones are untouched (no in-place mutation of IDB
  // read results beyond the returned copies).
  assert.ok(Object.isFrozen(revived[0].event), "returned event is the frozen one");
});
