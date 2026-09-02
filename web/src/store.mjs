// @ts-check
import { deepFreeze } from "./wire.mjs";

/**
 * @typedef {import("./wire.mjs").ChatEvent} ChatEvent
 */

/**
 * Minimal, framework-free, in-memory event store: an append-only log of
 * frozen typed events plus subscribe/notify. This is the seam that a future
 * (planned, not built yet) WebWorker + BroadcastChannel + IndexedDB
 * architecture will replace — the API shape (append-only, immutable
 * snapshots, subscription callbacks) is deliberately kept ready for that,
 * but this implementation is in-memory only.
 *
 * @typedef {object} Store
 * @property {() => Readonly<ChatEvent[]>} getEvents Returns the current
 *   frozen event array (the same reference between changes).
 * @property {(event: ChatEvent) => Readonly<ChatEvent[]>} append Appends a
 *   single event; rejects (throws `TypeError`) non-frozen or non-object
 *   events — only frozen validated events may enter the log. Returns the
 *   new frozen array.
 * @property {(events: readonly ChatEvent[]) => Readonly<ChatEvent[]>} appendAll
 *   Batched `append`; same validation, a single notify. Returns the new
 *   frozen array. If any event fails validation nothing is appended.
 * @property {(listener: (events: Readonly<ChatEvent[]>) => void) => () => void} subscribe
 *   Registers a listener; it is called with the new frozen array after each
 *   change. Returns an `unsubscribe()` function. Subscribing the same
 *   function twice is allowed (two entries → two calls per change);
 *   listeners added during a notify are NOT called for that change.
 * @property {number} size Number of events currently in the log.
 */

/**
 * Create an in-memory event store (see {@link Store}).
 *
 * @returns {Readonly<Store>}
 */
export function createStore() {
  /** @type {Readonly<ChatEvent[]>} */
  let events = deepFreeze([]);
  /** @type {Array<(events: Readonly<ChatEvent[]>) => void>} */
  const listeners = [];
  let notifying = false;

  /**
   * Store invariant: only frozen validated event objects enter the log.
   *
   * @param {ChatEvent} event
   * @returns {void}
   */
  function assertFrozenEvent(event) {
    if (
      event === null ||
      typeof event !== "object" ||
      !Object.isFrozen(event)
    ) {
      throw new TypeError(
        "store: only frozen event objects may be appended (got " +
          (event === null ? "null" : typeof event) +
          ")",
      );
    }
  }

  /**
   * Replace the log with a NEW deep-frozen array (never mutate in place),
   * then notify listeners with it.
   *
   * @param {ChatEvent[]} next
   * @returns {Readonly<ChatEvent[]>}
   */
  function commit(next) {
    events = deepFreeze(next);
    notify();
    return events;
  }

  function notify() {
    notifying = true;
    try {
      // Snapshot so listeners added during this notify are not called for
      // it; re-check membership so listeners removed during the notify are
      // skipped.
      for (const listener of [...listeners]) {
        if (listeners.includes(listener)) listener(events);
      }
    } finally {
      notifying = false;
    }
  }

  /** @type {Store} */
  const store = {
    getEvents: () => events,

    /**
     * @param {ChatEvent} event
     */
    append: (event) => {
      assertFrozenEvent(event);
      return commit([...events, event]);
    },

    /**
     * @param {readonly ChatEvent[]} batch
     */
    appendAll: (batch) => {
      const incoming = Array.from(batch);
      for (const event of incoming) assertFrozenEvent(event);
      return commit([...events, ...incoming]);
    },

    /**
     * @param {(events: Readonly<ChatEvent[]>) => void} listener
     */
    subscribe: (listener) => {
      listeners.push(listener);
      let active = true;
      return () => {
        if (!active) return;
        active = false;
        const index = listeners.indexOf(listener);
        if (index !== -1) listeners.splice(index, 1);
      };
    },

    get size() {
      return events.length;
    },
  };

  return Object.freeze(store);
}
