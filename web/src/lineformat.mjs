// @ts-check
/**
 * Loader for the WASM wire-line parser shared with the server, built from
 * `wasm/lineformat/` by `make wasm-lineformat`. The generated glue at
 * `/assets/lineformat.js` owns the wasm plumbing; this wrapper only manages
 * one-time initialisation and exposes a typed, thin entry point.
 *
 * The glue's default export fetches `lineformat_bg.wasm` relative to its own
 * module URL, so it must be served over HTTP (file:// will not work).
 */

/**
 * @typedef {import("/assets/lineformat.js").default} WasmInit
 * @typedef {(line: string, max: number) => any} WasmParseLine
 * @typedef {(line: string) => boolean} WasmIsValid
 * @typedef {(text: string) => any} WasmExtractToolCallMeta
 */

/**
 * One rollout wire line split into its parts, mirroring the Rust `WireFrame`.
 *
 * @typedef {Object} WireFrame
 * @property {number} ts Unix epoch ms prefix of the line.
 * @property {string} type Value of the payload's `_type` ("" when absent).
 * @property {string} text The line's JSON payload, possibly truncated at
 * `maxBytes` (char-boundary safe).
 * @property {boolean} truncated True when the payload was cut; the browser's
 * truncation rule is deterministic: strict JSON parse failure ⇒ partial.
 */

/**
 * Lenient metadata extracted from a (possibly truncated) `tool_call` event
 * payload, mirroring the Rust `ToolCallMeta`. `*_pretty_head` carry the raw
 * (possibly cut) payload string contents up to the cut.
 *
 * @typedef {Object} ToolCallMeta
 * @property {string} tool
 * @property {number} duration_ms
 * @property {number} bytes_up
 * @property {number} bytes_down
 * @property {number} ts
 * @property {string} args_pretty_head
 * @property {string} result_pretty_head
 */

/** @type {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid, extract_tool_call_meta: WasmExtractToolCallMeta }> | null} */
let initPromise = null;

/** @type {{ parse_line: WasmParseLine, is_valid: WasmIsValid, extract_tool_call_meta: WasmExtractToolCallMeta } | null} */
let glue = null;

/**
 * Idempotently start (or join) the WASM initialisation: dynamically import the
 * generated glue and run its default init, which fetches and instantiates
 * `lineformat_bg.wasm`. Safe to call repeatedly; every caller gets the same
 * promise.
 *
 * @returns {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid, extract_tool_call_meta: WasmExtractToolCallMeta }>} the initialised glue module
 */
export function initLineformat() {
  if (!initPromise) {
    initPromise = /** @type {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid, extract_tool_call_meta: WasmExtractToolCallMeta }>} */ (
      import("/assets/lineformat.js").then(async (/** @type {any} */ mod) => {
        await mod.default();
        glue = mod;
        return mod;
      })
    );
  }
  return initPromise;
}

/**
 * Parse one rollout wire line `<ts>\0<json>` into a {@link WireFrame}.
 * Throws if the line does not start with `<ascii digits>\0` (corruption —
 * reading must HALT there, never skip).
 *
 * @param {string} line one physical rollout line (without its trailing newline)
 * @param {number} [maxBytes=1024] byte limit for the payload text
 * @returns {Promise<WireFrame>} the parsed frame
 * @throws {Error} if the WASM module was not initialised via
 * {@link initLineformat}, or the line's ts prefix is invalid
 */
export async function parseWireLine(line, maxBytes = 1024) {
  if (!glue) {
    if (initPromise) {
      glue = await initPromise;
    } else {
      throw new Error(
        "lineformat not initialised: await initLineformat() before calling parseWireLine()",
      );
    }
  }
  try {
    /** @type {any} */
    let raw = glue.parse_line(line, maxBytes);
    let text = String(raw.text);
    // The item26.5 catch-up stream emits three-part lines
    // `ts\0type\0text\n` (the `_type` segment precedes the JSON payload,
    // which the server already egress-truncated at 1024 bytes). A JSON
    // payload always starts with `{`, so a leading `<type>\0` segment is
    // unambiguous. Re-parse with the extra segment budgeted so the payload
    // cut still honours `maxBytes` exactly.
    const sep = text.indexOf("\0");
    if (sep > 0 && text.slice(0, sep) === String(raw.type)) {
      const extra = sep + 1;
      raw = glue.parse_line(line, maxBytes + extra);
      text = String(raw.text).slice(extra);
      if (text.length > maxBytes) {
        text = text.slice(0, maxBytes);
      }
    }
    /** @type {WireFrame} */
    return {
      ts: Number(raw.ts),
      type: String(raw.type),
      text,
      truncated: Boolean(raw.truncated),
    };
  } catch (error) {
    throw new Error(`corrupt wire line: ${String(error)}`);
  }
}

/**
 * Leniently extract the metadata of a (possibly truncated) `tool_call` event
 * payload — the shared Rust/WASM scan (see
 * `wasm/lineformat/src/lib.rs::extract_tool_call_meta`). Metadata fields
 * serialize before the payload, so they are complete on an egress-truncated
 * line; the `*_pretty_head` fields carry the raw (possibly cut) payload
 * string contents.
 *
 * @param {string} text the (possibly truncated) tool_call JSON payload
 * @returns {Promise<ToolCallMeta | null>} null when the metadata is missing
 * @throws {Error} if the WASM module was not initialised via
 * {@link initLineformat}
 */
export async function extractToolCallMeta(text) {
  if (!glue) {
    if (initPromise) {
      glue = await initPromise;
    } else {
      throw new Error(
        "lineformat not initialised: await initLineformat() before calling extractToolCallMeta()",
      );
    }
  }
  /** @type {any} */
  const meta = glue.extract_tool_call_meta(text);
  if (meta === null || meta === undefined) {
    return null;
  }
  /** @type {ToolCallMeta} */
  return {
    tool: String(meta.tool),
    duration_ms: Number(meta.duration_ms),
    bytes_up: Number(meta.bytes_up),
    bytes_down: Number(meta.bytes_down),
    ts: Number(meta.ts),
    args_pretty_head: String(meta.args_pretty_head),
    result_pretty_head: String(meta.result_pretty_head),
  };
}
