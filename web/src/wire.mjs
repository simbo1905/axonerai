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
 * Validate, deep-freeze and return a wire event.
 *
 * @param {unknown} data an already-parsed JSON value
 * @returns {WireEvent}
 * @throws {TypeError} if `_type` is missing or not a known event type
 * @throws {Error} if the payload does not validate against the event's schema
 */
export function parseWireEvent(data) {
  const type =
    data !== null && typeof data === "object" && !Array.isArray(data)
      ? /** @type {any} */ (data)._type
      : undefined;
  if (typeof type !== "string" || !(type in validators)) {
    throw new TypeError(
      `unknown wire event _type: ${JSON.stringify(type ?? null)}`,
    );
  }
  const errors = validators[/** @type {keyof typeof validators} */ (type)](data);
  if (errors.length > 0) {
    const detail = errors
      .map((e) => `${e.instancePath || "/"} (${e.schemaPath})`)
      .join(", ");
    throw new Error(`invalid ${type} event: ${detail}`);
  }
  return /** @type {WireEvent} */ (deepFreeze(data));
}

/**
 * Parse a JSON text as a wire event.
 *
 * @param {string} text
 * @returns {WireEvent}
 * @throws {SyntaxError} if `text` is not valid JSON
 * @throws {TypeError} if `_type` is missing or not a known event type
 * @throws {Error} if the payload does not validate against the event's schema
 */
export function parseWireEventText(text) {
  return parseWireEvent(JSON.parse(text));
}
