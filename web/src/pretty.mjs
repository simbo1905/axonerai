// @ts-check
/**
 * Loader for the WASM pretty-printer of (possibly abridged) JSON, built from
 * `wasm/pretty-json/` by `make wasm-pretty`. The generated glue at
 * `/assets/pretty-json.js` owns the wasm plumbing; this wrapper only manages
 * one-time initialisation and exposes a typed, thin entry point.
 *
 * The glue's default export fetches `pretty-json_bg.wasm` relative to its own
 * module URL, so it must be served over HTTP (file:// will not work).
 */

/**
 * @typedef {import("/assets/pretty-json.js").default} WasmInit
 * @typedef {(input: string, indent: number) => string} WasmPrettyPrint
 */

/** @type {Promise<{ pretty_print: WasmPrettyPrint }> | null} */
let initPromise = null;

/** @type {{ pretty_print: WasmPrettyPrint } | null} */
let glue = null;

/**
 * Idempotently start (or join) the WASM initialisation: dynamically import the
 * generated glue and run its default init, which fetches and instantiates
 * `pretty-json_bg.wasm`. Safe to call repeatedly; every caller gets the same
 * promise.
 *
 * @returns {Promise<{ pretty_print: WasmPrettyPrint }>} the initialised glue module
 */
export function initPretty() {
  if (!initPromise) {
    initPromise = /** @type {Promise<{ pretty_print: WasmPrettyPrint }>} */ (
      import("/assets/pretty-json.js").then(async (/** @type {any} */ mod) => {
        await mod.default();
        glue = mod;
        return mod;
      })
    );
  }
  return initPromise;
}

/**
 * Pretty-print a JSON string that may be well-formed or abridged (truncated
 * mid-token with a trailing `…`). Throws if {@link initPretty} has not been
 * awaited first.
 *
 * @param {string} text the (possibly abridged) JSON text
 * @param {number} [indent=2] indent width, typically 2 or 4
 * @returns {Promise<string>} pretty-printed text with open brackets closed
 * @throws {Error} if the WASM module was not initialised via {@link initPretty}
 */
export async function prettyPrintAbridged(text, indent = 2) {
  if (!glue) {
    if (initPromise) {
      glue = await initPromise;
    } else {
      throw new Error(
        "pretty-json not initialised: await initPretty() before calling prettyPrintAbridged()",
      );
    }
  }
  return glue.pretty_print(text, indent);
}
