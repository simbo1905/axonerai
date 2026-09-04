// @ts-check
import {
  validateReady,
  validatePong,
  validateAssistant,
  validateError,
  validateAck,
  validateSession_meta,
  validateTool_call,
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
 * An `ack` reply to a client control-plane frame (e.g. `rename`).
 *
 * @typedef {object} AckEvent
 * @property {"ack"} _type
 * @property {string} for_type
 * @property {string | null} message
 * @property {boolean} ok
 */

/**
 * A `session_meta` event sent by the server after the WebSocket connects.
 *
 * @typedef {object} SessionMetaEvent
 * @property {"session_meta"} _type
 * @property {number} created_at
 * @property {string} session_id
 * @property {string} title
 */

/**
 * A `tool_call` event: the server's notification that a tool ran. Payloads
 * are the abridged pretty-printed JSON (metadata first, payload last on the
 * wire, so a truncated frame keeps usable metadata).
 *
 * The optional `abridged` flag is schema-sanctioned
 * (`schemas/tool_call.jdt.json` `optionalProperties`): the browser's
 * catch-up reconstruction synthesizes partial `tool_call`s whose pretty
 * heads ARE truncated, and marks them so the renderer can show an
 * unconditional ellipsis on the summary row.
 *
 * @typedef {object} ToolCallEvent
 * @property {"tool_call"} _type
 * @property {string | null} id
 * @property {string} session_id
 * @property {string} tool
 * @property {string} args_pretty
 * @property {string} result_pretty
 * @property {number} bytes_up
 * @property {number} bytes_down
 * @property {number} duration_ms
 * @property {number} ts
 * @property {boolean} [abridged] true only on catch-up reconstructions
 */

/**
 * Any event the server may send over the wire.
 *
 * @typedef {ReadyEvent | PongEvent | AssistantEvent | ErrorEvent | AckEvent | SessionMetaEvent | ToolCallEvent} WireEvent
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
 * Recursively freeze a value (objects and arrays) in place.
 *
 * DAG-only: the input must not contain reference cycles (wire events, UI
 * state and console envelopes never do), so this needs no visited-set.
 * Primitives are left untouched and already-frozen subtrees are skipped
 * without being re-walked.
 *
 * @template T
 * @param {T} value
 * @returns {Readonly<T>}
 */
export function deepFreeze(value) {
  Object.freeze(value);
  for (const key in value) {
    const child = /** @type {Record<string, unknown>} */ (value)[key];
    if (
      child !== null &&
      typeof child === "object" &&
      !Object.isFrozen(child)
    ) {
      deepFreeze(child);
    }
  }
  return /** @type {Readonly<T>} */ (value);
}

const validators = {
  ready: validateReady,
  pong: validatePong,
  assistant: validateAssistant,
  error: validateError,
  ack: validateAck,
  session_meta: validateSession_meta,
  tool_call: validateTool_call,
};

/**
 * Replace the validator for a `_type`. Used by the WASM loader
 * (`initWasmValidators`) to install the Rust/WASM-generated validators over
 * the generated `.mjs` fallback once `/assets/validators.js` finishes
 * initialising; both validator sets implement the identical schema contract
 * (same drop/malformed semantics, same `{instancePath, schemaPath}` error
 * shape), so the swap is invisible to `parseWireEvent` callers.
 *
 * @param {string} type a `_type` already present in the validator registry
 * @param {(data: unknown) => {instancePath: string, schemaPath: string}[]} validate
 * @returns {void}
 */
export function registerValidator(type, validate) {
  validators[/** @type {keyof typeof validators} */ (type)] = validate;
}

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
