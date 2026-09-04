// @ts-check
/**
 * IndexedDB session history: validated frozen events persisted per session
 * under db `agt`, objectStore `events` (keyPath `["sessionId","ts"]`, plus an
 * index by `sessionId`). The UI checks a session's high-watermark (max ts)
 * before catching up over the line protocol, so a reload/fork gets up to
 * speed without duplicates.
 *
 * The READ boundary ({@link getAll}) re-asserts the IO contract: every
 * stored record is re-validated (JTD) and deep-frozen before it is returned;
 * a stored record that no longer validates is logged and dropped, never
 * returned. The pure helpers ({@link mergeCatchup}, {@link frontierOf},
 * {@link reviveHistoryRecords}) are covered by node tests; the IDB paths run
 * in headless Chrome. No fake-indexeddb dependency.
 */

import { deepFreeze, parseWireEvent } from "./wire.mjs";
import { validatePrompt } from "../generated/validators.mjs";

/**
 * @typedef {import("./wire.mjs").ChatEvent} ChatEvent
 */

/**
 * One persisted history record.
 *
 * @typedef {object} HistoryRecord
 * @property {string} sessionId
 * @property {number} ts stamp: the server `_ts` for catch-up records,
 * `Date.now()` at receipt for live events
 * @property {ChatEvent} event the validated frozen event (or the partial
 * tool_call reconstruction)
 */

/**
 * One parsed catch-up wire frame (a subset of the lineformat `WireFrame`).
 *
 * @typedef {object} CatchupFrame
 * @property {number} ts
 */

/**
 * Filter catch-up frames to those strictly newer than the frontier
 * high-watermark and return them sorted ascending by ts (file order).
 * Pure: returns a new array, never mutates the input.
 *
 * @template {CatchupFrame} T
 * @param {readonly T[]} frames
 * @param {number} frontierTs max ts already held (0 for an empty history);
 * the comparison is exclusive — a frame at exactly the frontier is skipped
 * @returns {T[]}
 */
export function mergeCatchup(frames, frontierTs) {
  return frames
    .filter((frame) => frame.ts > frontierTs)
    .sort((a, b) => a.ts - b.ts);
}

/**
 * Max `ts` across records — the high-watermark to catch up after. 0 when
 * there are no records.
 *
 * @param {readonly { ts: number }[]} records
 * @returns {number}
 */
export function frontierOf(records) {
  let frontier = 0;
  for (const record of records) {
    if (record.ts > frontier) frontier = record.ts;
  }
  return frontier;
}

/**
 * Open (creating if needed) the `agt` history database: version 1,
 * objectStore `events` with keyPath `["sessionId","ts"]` and a `sessionId`
 * index. Resolves with the same connection for repeated calls.
 *
 * @returns {Promise<IDBDatabase>}
 */
export function openHistory() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("agt", 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains("events")) {
        const store = db.createObjectStore("events", {
          keyPath: ["sessionId", "ts"],
        });
        store.createIndex("sessionId", "sessionId", { unique: false });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("openHistory failed"));
    request.onblocked = () =>
      reject(new Error("openHistory blocked by another connection"));
  });
}

/**
 * Batch-put records into the `events` store (single transaction). Records
 * must carry `{sessionId, ts, event}`; the validated frozen event is stored
 * as-is.
 *
 * @param {IDBDatabase} db
 * @param {string} sessionId
 * @param {readonly HistoryRecord[]} records
 * @returns {Promise<void>}
 */
export function appendEvents(db, sessionId, records) {
  if (records.length === 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("events", "readwrite");
    const store = tx.objectStore("events");
    for (const record of records) {
      store.put({ sessionId, ts: record.ts, event: record.event });
    }
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error("appendEvents failed"));
    tx.onabort = () => reject(tx.error ?? new Error("appendEvents aborted"));
  });
}

/**
 * Max `ts` recorded for `sessionId`, or 0.
 *
 * @param {IDBDatabase} db
 * @param {string} sessionId
 * @returns {Promise<number>}
 */
export async function getFrontier(db, sessionId) {
  return frontierOf(await getAll(db, sessionId));
}

/**
 * Revive one stored event at the read boundary: validate → freeze → return,
 * or log + return `null` for anything that does not validate. `prompt`
 * records (client-side, persisted alongside server events) validate against
 * `schemas/prompt.jdt.json`; every other `_type` goes through
 * {@link parseWireEvent} (the wire validator registry, which logs malformed
 * frames itself).
 *
 * @param {unknown} raw
 * @returns {ChatEvent | null}
 */
function reviveEvent(raw) {
  if (raw === null || typeof raw !== "object") {
    console.warn("[history] dropping stored record with non-object event");
    return null;
  }
  const type = /** @type {any} */ (raw)._type;
  if (type === "prompt") {
    const errors = validatePrompt(raw);
    if (errors.length > 0) {
      console.error("[history] dropping stored prompt record: malformed", errors);
      return null;
    }
    return /** @type {ChatEvent} */ (deepFreeze(raw));
  }
  if (typeof type !== "string") {
    console.warn("[history] dropping stored record without a string _type");
    return null;
  }
  const event = parseWireEvent(raw);
  if (event === null) {
    // Unknown _type / validator failure — parseWireEvent already logged it.
    return null;
  }
  return event;
}

/**
 * Re-assert the IO contract on records read back from IndexedDB: every
 * returned record is re-validated and deep-frozen (record AND event); an
 * invalid stored record is logged and dropped, never returned. Pure: takes
 * the raw structured-clone records `getAll` reads and returns a new array.
 *
 * @param {unknown[]} rawRecords
 * @returns {HistoryRecord[]}
 */
export function reviveHistoryRecords(rawRecords) {
  /** @type {HistoryRecord[]} */
  const revived = [];
  for (const raw of rawRecords) {
    if (raw === null || typeof raw !== "object") {
      console.warn("[history] dropping malformed stored record (non-object)");
      continue;
    }
    const record = /** @type {any} */ (raw);
    if (typeof record.sessionId !== "string" || typeof record.ts !== "number") {
      console.warn("[history] dropping malformed stored record", record);
      continue;
    }
    const event = reviveEvent(record.event);
    if (event === null) continue;
    revived.push(
      deepFreeze({
        sessionId: record.sessionId,
        ts: record.ts,
        event,
      }),
    );
  }
  return revived;
}

/**
 * All records for `sessionId` (via the `sessionId` index), in key order.
 * Read-boundary rule: every record is re-validated and deep-frozen here —
 * invalid stored records are logged and dropped, never returned.
 *
 * @param {IDBDatabase} db
 * @param {string} sessionId
 * @returns {Promise<HistoryRecord[]>}
 */
export function getAll(db, sessionId) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction("events", "readonly");
    const index = tx.objectStore("events").index("sessionId");
    const request = index.getAll(sessionId);
    request.onsuccess = () =>
      resolve(reviveHistoryRecords(/** @type {unknown[]} */ (request.result)));
    request.onerror = () => reject(request.error ?? new Error("getAll failed"));
  });
}
