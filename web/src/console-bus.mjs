// @ts-check
/**
 * Console tee bus (item32): wraps `window.console` log/info/warn/error so
 * every call (a) still behaves natively and (b) posts a JTD-validated,
 * deep-frozen envelope over the `agt-console` BroadcastChannel. A dedicated
 * module worker (`/assets/console-spool-worker.js`) subscribes to that
 * channel, spools envelopes into IndexedDB (`agt-console`) as a ring
 * buffer, and RE-BROADCASTS each committed envelope on the
 * `agt-console-spooled` channel from inside `tx.oncomplete` — the console
 * screen subscribes to that spooled stream and only to it (persist-then-
 * broadcast; ONE worker does the reading, the spooling and the fan-out).
 *
 * Every entry crossing a context boundary (BC post, worker → IDB,
 * IDB → console-tab render) is a validated, deep-frozen envelope:
 * repo rule "JTD validators for things that go over IO".
 *
 * The bus is a per-context singleton: both the chat screen (agt-app) and the
 * devtools popup (console.html → agt-console-app) install it, each capturing
 * its own console with its own pageId.
 *
 * Node-testable: the pure helpers (formatConsoleArgs, createConsoleEntry,
 * mergeEntries, evictIds, autoscroll arithmetic) live in
 * `./console-model.mjs` and run without any browser API; all
 * BroadcastChannel/Worker/IndexedDB access happens inside
 * {@link installConsoleBus} and the backlog helpers.
 */
import { deepFreeze } from "./wire.mjs";
import { validateConsole_entry } from "/generated/validators.mjs";
import {
  createConsoleEntry,
  formatConsoleArgs,
  seqOf,
} from "./console-model.mjs";

/** BroadcastChannel name shared by the bus (producer) and the spool worker. */
export const CONSOLE_CHANNEL = "agt-console";

/**
 * BroadcastChannel the WORKER re-broadcasts on after the IndexedDB commit
 * (`tx.oncomplete`): the console screen's ONLY live stream. This restores
 * persist-then-broadcast for the stream the consumer trusts — the producer
 * channel and the durable log are different streams behind a thread hop, so
 * listening on "agt-console" has a silent-drop window (missed live copy +
 * uncommitted IDB write). See item31.9.
 */
export const CONSOLE_SPOOLED_CHANNEL = "agt-console-spooled";

/** IndexedDB database / object store backing the console backlog. */
export const CONSOLE_DB = "agt-console";
export const CONSOLE_STORE = "entries";

/** Ring-buffer size: keep the newest 2000 entries in IndexedDB. */
export const CONSOLE_MAX_ENTRIES = 2000;

/**
 * A console envelope (`schemas/console_entry.jdt.json`).
 *
 * @typedef {object} ConsoleEntry
 * @property {string} id `${pageId}:${seq}` — dedupe key across contexts
 * @property {string} pageId uuid of the emitting context
 * @property {number} ts epoch milliseconds
 * @property {"log" | "info" | "warn" | "error"} level
 * @property {string} text space-joined, devtools-style serialization
 */

/**
 * Debug spool: append one line to `window.__BUS_DEBUG__` (headless runners
 * dump it on failure). Typed locally so `tsc --noEmit` stays clean without
 * widening the global `Window` surface.
 *
 * @param {string} line
 */
function busDebug(line) {
  const w = /** @type {Window & { __BUS_DEBUG__?: string[] }} */ (window);
  (w.__BUS_DEBUG__ = w.__BUS_DEBUG__ || []).push(line);
}

/**
 * Per-context console bus singleton.
 */
class ConsoleBus {
  /** @type {string} */
  #pageId;
  /** @type {number} */
  #seq = 0;
  /** @type {BroadcastChannel} */
  #channel;
  /** @type {Set<BroadcastChannel>} */
  #subscribers = new Set();
  /** @type {Worker | null} */
  #worker = null;
  /** @type {{ log: (...args: unknown[]) => void, info: (...args: unknown[]) => void, warn: (...args: unknown[]) => void, error: (...args: unknown[]) => void }} */
  #originals;
  /** Recursion guard: while true, wrapped console calls pass through to the
   * originals WITHOUT being re-emitted (bus-internal logging never loops). */
  #reentry = false;
  /** True once the spool worker signalled readiness (its BC subscription is
   * live). Envelopes emitted before that are buffered, never lost. */
  #workerReady = false;
  /** @type {Readonly<ConsoleEntry>[]} envelopes awaiting worker readiness */
  #pending = [];

  /** @param {string} pageId */
  constructor(pageId) {
    this.#pageId = pageId;
    this.#originals = {
      log: console.log.bind(console),
      info: console.info.bind(console),
      warn: console.warn.bind(console),
      error: console.error.bind(console),
    };
    this.#channel = new BroadcastChannel(CONSOLE_CHANNEL);
    try {
      // The worker cannot reach sessionStorage (workers have no DOM storage),
      // so IndexedDB is the ONLY spool — the worker owns it entirely.
      this.#worker = new Worker("/assets/console-spool-worker.js", {
        type: "module",
      });
      // Readiness handshake: the worker sets its BroadcastChannel.onmessage
      // BEFORE posting `agt-console-ready` on the dedicated port, so once
      // this fires the spool is guaranteed to hear everything. Fallbacks:
      // onerror or a 3s timeout flush anyway (live still works spool-less).
      this.#worker.onmessage = (event) => {
        busDebug(
          `worker msg ${JSON.stringify(event.data)?.slice(0, 80)} at ${Date.now() % 100000}`,
        );
        if (
          event.data &&
          /** @type {{ type?: string }} */ (event.data).type ===
            "agt-console-ready"
        ) {
          this.#flushPending();
        }
      };
      this.#worker.onerror = () => this.#flushPending();
      setTimeout(() => this.#flushPending(), 3000);
    } catch (error) {
      // Spooling is best-effort: live subscription still works without it.
      this.#worker = null;
      this.#internal("warn", "[console-bus] spool worker unavailable", error);
    }
    for (const level of /** @type {const} */ (["log", "info", "warn", "error"])) {
      const original = this.#originals[level];
      console[level] = /** @type {typeof console.log} */ (
        (...args) => {
          original(...args);
          if (this.#reentry) return;
          this.#emit(level, args);
        }
      );
    }
  }

  /** The emitting context's pageId. */
  get pageId() {
    return this.#pageId;
  }

  /**
   * Subscribe to the LIVE stream of persisted console envelopes. Returns an
   * unsubscribe function.
   *
   * The live stream is ONLY the `agt-console-spooled` channel: the spool
   * worker re-broadcasts each envelope from inside `tx.oncomplete` of the
   * committing IndexedDB transaction, so every delivered entry is already
   * durable (persist-then-broadcast; item31.9). Consequence of the 3s
   * worker-ready fallback: envelopes flushed while the worker is down are
   * posted on "agt-console" but never spooled, so the console screen shows
   * only the durable log in that mode — a conscious behaviour choice for a
   * backlog viewer.
   *
   * A DEDICATED BroadcastChannel instance per subscriber — a BC instance
   * never receives its own posts, so subscribing on the bus's own posting
   * channel would silently drop this context's own entries.
   *
   * IO boundary: each incoming payload is JTD-validated and deep-frozen
   * BEFORE the handler sees it (structured clones arrive unfrozen); invalid
   * payloads are logged (via the originals, guarded) and dropped — repo
   * drop/malformed semantics.
   *
   * @param {(entry: Readonly<ConsoleEntry>) => void} handler receives the
   * validated, frozen envelope
   * @returns {() => void}
   */
  subscribe(handler) {
    const channel = new BroadcastChannel(CONSOLE_SPOOLED_CHANNEL);
    channel.onmessage = (event) => {
      const data = event.data;
      if (validateConsole_entry(data).length > 0) {
        this.#internal(
          "warn",
          "[console-bus] invalid envelope on channel (dropped)",
          data,
        );
        return;
      }
      handler(/** @type {Readonly<ConsoleEntry>} */ (deepFreeze(data)));
    };
    this.#subscribers.add(channel);
    return () => {
      channel.onmessage = null;
      channel.close();
      this.#subscribers.delete(channel);
    };
  }

  /**
   * Emit one envelope: serialize → validate → deep-freeze → broadcast.
   * Invalid envelopes are logged (via the originals, guarded) and dropped —
   * repo drop/malformed semantics.
   *
   * @param {"log" | "info" | "warn" | "error"} level
   * @param {readonly unknown[]} args
   */
  #emit(level, args) {
    this.#seq += 1;
    const entry = createConsoleEntry({
      pageId: this.#pageId,
      seq: this.#seq,
      level,
      text: formatConsoleArgs(args),
    });
    const errors = validateConsole_entry(entry);
    if (errors.length > 0) {
      this.#internal(
        "error",
        "[console-bus] invalid envelope (dropped)",
        errors,
        entry,
      );
      return;
    }
    try {
      if (this.#worker && !this.#workerReady) {
        // Buffer until the spool worker's BroadcastChannel subscription is
        // live — BroadcastChannel has no buffering, so posting now could
        // lose the envelope forever. Bounded: drop the OLDEST beyond 500.
        busDebug(`buffer at ${Date.now() % 100000}: ${entry.text.slice(0, 30)}`);
        this.#pending.push(deepFreeze(entry));
        if (this.#pending.length > 500) this.#pending.shift();
        return;
      }
      this.#channel.postMessage(deepFreeze(entry));
    } catch (error) {
      this.#internal("warn", "[console-bus] broadcast failed", error);
    }
  }

  /**
   * Spool worker is ready (or gave up): mark ready and flush the buffered
   * envelopes in emission order.
   */
  #flushPending() {
    if (this.#workerReady) return;
    busDebug(`flush at ${Date.now() % 100000}, pending=${this.#pending.length}`);
    this.#workerReady = true;
    for (const entry of this.#pending) {
      try {
        this.#channel.postMessage(entry);
      } catch (error) {
        this.#internal("warn", "[console-bus] broadcast failed", error);
      }
    }
    this.#pending = [];
  }

  /**
   * Bus-internal logging: uses the ORIGINAL console methods under the
   * recursion guard, so internal reports are never re-wrapped/re-emitted.
   *
   * @param {"log" | "info" | "warn" | "error"} level
   * @param {...unknown} args
   */
  #internal(level, ...args) {
    this.#reentry = true;
    try {
      this.#originals[level](...args);
    } finally {
      this.#reentry = false;
    }
  }
}

/** @type {ConsoleBus | null} */
let installed = null;

/**
 * Install the console bus (idempotent: the first call wins for the context).
 * Generates a `pageId` via crypto.randomUUID() when absent.
 *
 * @param {{ pageId?: string }} [options]
 * @returns {ConsoleBus}
 */
export function installConsoleBus({ pageId } = {}) {
  if (installed) return installed;
  const id =
    typeof pageId === "string" && pageId
      ? pageId
      : typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `page-${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
  installed = new ConsoleBus(id);
  return installed;
}

/**
 * The installed bus for this context, or null before installConsoleBus().
 *
 * @returns {ConsoleBus | null}
 */
export function getConsoleBus() {
  return installed;
}

/**
 * Open (creating if needed) the console spool database from the MAIN thread:
 * version 1, objectStore `entries` (keyPath `id`, ts index). The worker
 * spools into it; the console screen reads/clears it directly.
 *
 * @returns {Promise<IDBDatabase>}
 */
function openConsoleDb() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(CONSOLE_DB, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(CONSOLE_STORE)) {
        const store = db.createObjectStore(CONSOLE_STORE, { keyPath: "id" });
        store.createIndex("ts", "ts", { unique: false });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error("openConsoleDb failed"));
    request.onblocked = () =>
      reject(new Error("openConsoleDb blocked by another connection"));
  });
}

/**
 * Read the backlog straight from IndexedDB, sorted ts → seq (late arrivers:
 * everything spooled before this context subscribed). IO boundary: records
 * come back from IDB as unfrozen structured clones, so each validated entry
 * is deep-frozen before it is returned. Never throws.
 *
 * @returns {Promise<Readonly<ConsoleEntry>[]>}
 */
export async function getBacklog() {
  try {
    const db = await openConsoleDb();
    const entries = await new Promise((resolve, reject) => {
      const tx = db.transaction(CONSOLE_STORE, "readonly");
      const request = tx.objectStore(CONSOLE_STORE).getAll();
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error ?? new Error("getAll failed"));
    });
    db.close();
    return /** @type {ConsoleEntry[]} */ (entries)
      .filter((entry) => validateConsole_entry(entry).length === 0)
      .map((entry) => deepFreeze(entry))
      .sort((a, b) => a.ts - b.ts || seqOf(a.id) - seqOf(b.id));
  } catch (error) {
    console.warn("[console-bus] backlog read failed", error);
    return [];
  }
}

/**
 * Wipe the IndexedDB backlog (the Clear button). Never throws.
 *
 * @returns {Promise<void>}
 */
export async function clearBacklog() {
  try {
    const db = await openConsoleDb();
    await /** @type {Promise<void>} */ (
      new Promise((resolve, reject) => {
        const tx = db.transaction(CONSOLE_STORE, "readwrite");
        tx.objectStore(CONSOLE_STORE).clear();
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error ?? new Error("clear failed"));
        tx.onabort = () => reject(tx.error ?? new Error("clear aborted"));
      })
    );
    db.close();
  } catch (error) {
    console.warn("[console-bus] backlog clear failed", error);
  }
}
