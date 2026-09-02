// @ts-check
import {
  validateReady,
  validatePong,
  validateAssistant,
  validateError,
} from "../generated/validators.mjs";

/**
 * A `ready` event sent by the server after the WebSocket connects.
 *
 * @typedef {object} ReadyEvent
 * @property {"ready"} _type
 * @property {string} version
 * @property {string} websocket_path
 */

/**
 * A `pong` reply to a client `ping`.
 *
 * @typedef {object} PongEvent
 * @property {"pong"} _type
 * @property {string | null} id
 */

/**
 * An `assistant` message chunk.
 *
 * @typedef {object} AssistantEvent
 * @property {"assistant"} _type
 * @property {string | null} id
 * @property {string} text
 */

/**
 * An `error` event.
 *
 * @typedef {object} ErrorEvent
 * @property {"error"} _type
 * @property {string | null} id
 * @property {string} message
 */

/**
 * Any event the server may send over the wire.
 *
 * @typedef {ReadyEvent | PongEvent | AssistantEvent | ErrorEvent} WireEvent
 */

/**
 * A client `prompt` record kept in UI state alongside server events.
 * Mirrors the `_type: "prompt"` frame the client sends on the wire; it is
 * created (and deep-frozen) client-side so the chat log can render the
 * user's own message before the reply arrives.
 *
 * @typedef {object} PromptEvent
 * @property {"prompt"} _type
 * @property {string} id
 * @property {string} text
 */

/**
 * Any frozen value object that may appear in UI chat state: a server
 * `WireEvent` or a client-side `PromptEvent` record.
 *
 * @typedef {WireEvent | PromptEvent} ChatEvent
 */

/**
 * Recursively freeze a value (objects and arrays, cycle-safe).
 *
 * @template T
 * @param {T} value
 * @returns {Readonly<T>}
 */
export function deepFreeze(value) {
  const seen = new Set();
  /** @param {unknown} current */
  const freeze = (current) => {
    if (current === null || typeof current !== "object" || seen.has(current)) {
      return current;
    }
    seen.add(current);
    for (const key of Reflect.ownKeys(current)) {
      freeze(/** @type {Record<PropertyKey, unknown>} */ (current)[key]);
    }
    Object.freeze(current);
    return current;
  };
  return /** @type {Readonly<T>} */ (freeze(value));
}

const validators = {
  ready: validateReady,
  pong: validatePong,
  assistant: validateAssistant,
  error: validateError,
};

/**
 * Validate, deep-freeze and return a wire event, or return `null` for a
 * frame that should be dropped:
 * - no `_type` (or not a non-null object) → silent drop (no console noise);
 * - unknown `_type` → logged as malformed/unsupported, then dropped;
 * - validator failure → logged as malformed (with `{instancePath, schemaPath}`
 *   errors), then dropped.
 *
 * @param {unknown} data an already-parsed JSON value
 * @returns {WireEvent | null}
 */
export function parseWireEvent(data) {
  const type =
    data !== null && typeof data === "object" && !Array.isArray(data)
      ? /** @type {any} */ (data)._type
      : undefined;
  if (typeof type !== "string") {
    // No `_type`: not a wire frame — drop silently.
    return null;
  }
  if (!(type in validators)) {
    console.error(
      "[wire] malformed/unsupported frame (unknown _type)",
      type,
      data,
    );
    return null;
  }
  const errors = validators[/** @type {keyof typeof validators} */ (type)](data);
  if (errors.length > 0) {
    console.error("[wire] malformed frame", type, errors);
    return null;
  }
  return /** @type {WireEvent} */ (deepFreeze(data));
}

/**
 * Parse a JSON text as a wire event, or return `null` if the text is not
 * valid JSON or the frame must be dropped (see {@link parseWireEvent}).
 *
 * @param {string} text
 * @returns {WireEvent | null}
 */
export function parseWireEventText(text) {
  /** @type {unknown} */
  let data;
  try {
    data = JSON.parse(text);
  } catch (error) {
    console.error("[wire] malformed JSON frame", error);
    return null;
  }
  return parseWireEvent(data);
}
