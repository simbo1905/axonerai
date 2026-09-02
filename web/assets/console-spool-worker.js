// @ts-check
/**
 * Console spool worker (item32): the ONE module worker that both reads the
 * `agt-console` BroadcastChannel and spools validated envelopes into
 * IndexedDB (db `agt-console`, store `entries`, keyPath `id`, ts index) as a
 * ring buffer of the newest 2000 entries.
 *
 * Persist-then-broadcast (item31.9): after the committing readwrite
 * transaction fires `tx.oncomplete` (commit — NOT `request.onsuccess`,
 * which is only the in-transaction request finishing and can still roll
 * back), the worker re-broadcasts each validated, deep-frozen envelope on
 * the SECOND channel `agt-console-spooled`. The console screen's live
 * stream is that spooled channel and only it, so every delivered entry is
 * already durable.
 *
 * Every envelope is JTD-validated against schemas/console_entry.jdt.json
 * before it touches IndexedDB; invalid payloads are console.error'd (raw)
 * and DROPPED (repo drop/malformed semantics) — the repo rule is "JTD
 * validators for things that go over IO".
 *
 * NOTE: workers have no access to sessionStorage/localStorage, so IndexedDB
 * is the only durable spool available here (design decision, item32).
 */
import { validate as validateConsole_entry } from "/generated/console_entry.mjs";
import { deepFreeze } from "/src/wire.mjs";

const DB_NAME = "agt-console";
const STORE = "entries";
const MAX_ENTRIES = 2000;

/** Fan-out channel: commit → re-broadcast (persist-then-broadcast). */
const spooledChannel = new BroadcastChannel("agt-console-spooled");

/** @returns {Promise<IDBDatabase>} */
function openDb() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) {
        const store = db.createObjectStore(STORE, { keyPath: "id" });
        store.createIndex("ts", "ts", { unique: false });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("openDb failed"));
  });
}

/** @type {Promise<IDBDatabase> | null} */
let dbPromise = null;
/** @returns {Promise<IDBDatabase>} */
function db() {
  if (!dbPromise) dbPromise = openDb();
  return dbPromise;
}

/**
 * Upsert one frozen envelope (id keyPath dedupes) and trim the store back to
 * the newest MAX_ENTRIES records (oldest by ts first). `onCommit` runs from
 * inside `tx.oncomplete` — AFTER the transaction has committed, when the
 * write is durable and visible to later readonly snapshots; a
 * `request.onsuccess` hook would announce entries that can still abort.
 *
 * @param {IDBDatabase} dbh
 * @param {object} entry validated, deep-frozen envelope
 * @param {() => void} onCommit called from tx.oncomplete with the entry
 *   durable
 * @returns {Promise<void>}
 */
function putAndTrim(dbh, entry, onCommit) {
  return new Promise((resolve, reject) => {
    const tx = dbh.transaction(STORE, "readwrite");
    const store = tx.objectStore(STORE);
    store.put(entry);
    const countRequest = store.count();
    countRequest.onsuccess = () => {
      let excess = countRequest.result - MAX_ENTRIES;
      if (excess <= 0) return;
      const cursorRequest = store.index("ts").openCursor();
      cursorRequest.onsuccess = () => {
        const cursor = cursorRequest.result;
        if (!cursor || excess <= 0) return;
        cursor.delete();
        excess -= 1;
        cursor.continue();
      };
      cursorRequest.onerror = () => reject(cursorRequest.error);
    };
    countRequest.onerror = () => reject(countRequest.error);
    tx.oncomplete = () => {
      onCommit();
      resolve();
    };
    tx.onerror = () => reject(tx.error ?? new Error("putAndTrim failed"));
    tx.onabort = () => reject(tx.error ?? new Error("putAndTrim aborted"));
  });
}

/**
 * @param {unknown} entry validated, deep-frozen envelope
 */
async function spool(entry) {
  try {
    const dbh = await db();
    await putAndTrim(dbh, entry, () => {
      // Commit (tx.oncomplete) → re-broadcast: the console screen's live
      // stream is the spooled channel, so it only ever sees durable
      // entries. Single sender (this worker) ⇒ live order = commit order.
      spooledChannel.postMessage(entry);
    });
    self.postMessage({ type: "debug-spool-ok", id: /** @type {{id?: string}} */ (entry).id });
  } catch (error) {
    console.error("[console-spool] spool write failed", error);
    self.postMessage({
      type: "debug-spool-error",
      error: String(error),
      id: /** @type {{id?: string}} */ (entry).id,
    });
  }
}

const channel = new BroadcastChannel("agt-console");
channel.onmessage = (event) => {
  const data = event.data;
  const errors = validateConsole_entry(data);
  if (errors.length > 0) {
    console.error("[console-spool] invalid envelope (dropped)", data, errors);
    return;
  }
  spool(deepFreeze(data));
};
// Readiness handshake: the bus holds early envelopes until this dedicated
// message arrives — BroadcastChannel has no buffering, so anything posted
// before this subscription would be lost forever. onmessage is set FIRST.
self.postMessage({ type: "agt-console-ready" });
