// @ts-check

/**
 * @typedef {import("./wire.mjs").WireEvent} WireEvent
 */

/**
 * A handler for a validated, deep-frozen wire event of a specific `_type`.
 * It receives the SAME frozen event object that was passed to
 * {@link dispatch} (no copy is made).
 *
 * @typedef {function(WireEvent): void} WireEventHandler
 */

/**
 * The wire `_type`s that have validators in wire.mjs (and therefore are the
 * only types plumbing may deliver to business logic).
 *
 * @type {ReadonlySet<string>}
 */
const VALIDATOR_TYPES = new Set(["ready", "pong", "assistant", "error"]);

/**
 * Registered handlers, keyed by wire `_type`. Module-level registry: there is
 * one UI (agt-app) per document, and it registers one handler per type.
 *
 * @type {Map<string, WireEventHandler>}
 */
const handlers = new Map();

/**
 * Register the handler for a wire event `_type`, replacing (overwriting) any
 * handler previously registered for that type.
 *
 * @param {WireEvent["_type"]} type one of the wire `_type` strings
 * @param {WireEventHandler} handler receives the same frozen typed event
 */
export function registerHandler(type, handler) {
  handlers.set(type, handler);
}

/**
 * @param {WireEvent["_type"]} type
 * @returns {boolean} whether a handler is registered for `type`
 */
export function hasHandler(type) {
  return handlers.has(type);
}

/**
 * Dispatch a validated, deep-frozen wire event to the handler registered for
 * its `_type`.
 *
 * - `event` null/undefined, not an object, or missing a string `_type` →
 *   `TypeError` (programming error — plumbing should have dropped it);
 * - no handler registered for the type → `Error("no handler for <_type>")`
 *   (a bug, not data corruption);
 * - otherwise the handler is called with the SAME frozen event and its
 *   return value is returned.
 *
 * @param {WireEvent} event
 * @returns {void}
 */
export function dispatch(event) {
  if (
    event === null ||
    typeof event !== "object" ||
    typeof /** @type {any} */ (event)._type !== "string"
  ) {
    throw new TypeError(
      "dispatch expects a validated wire event with a string _type",
    );
  }
  const type = /** @type {any} */ (event)._type;
  const handler = handlers.get(type);
  if (!handler) {
    throw new Error(`no handler for ${type}`);
  }
  return handler(event);
}
