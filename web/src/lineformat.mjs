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

/** @type {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid }> | null} */
let initPromise = null;

/** @type {{ parse_line: WasmParseLine, is_valid: WasmIsValid } | null} */
let glue = null;

/**
 * Idempotently start (or join) the WASM initialisation: dynamically import the
 * generated glue and run its default init, which fetches and instantiates
 * `lineformat_bg.wasm`. Safe to call repeatedly; every caller gets the same
 * promise.
 *
 * @returns {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid }>} the initialised glue module
 */
export function initLineformat() {
  if (!initPromise) {
    initPromise = /** @type {Promise<{ parse_line: WasmParseLine, is_valid: WasmIsValid }>} */ (
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
    const frame = glue.parse_line(line, maxBytes);
    /** @type {WireFrame} */
    return {
      ts: Number(frame.ts),
      type: String(frame.type),
      text: String(frame.text),
      truncated: Boolean(frame.truncated),
    };
  } catch (error) {
    throw new Error(`corrupt wire line: ${String(error)}`);
  }
}
