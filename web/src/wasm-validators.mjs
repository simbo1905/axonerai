// @ts-check
/**
 * Loader for the WASM wire-event validators built from `wasm/validators/` by
 * `make wasm-validators` (jtd-codegen --target rust → cdylib+wasm, pure-Rust
 * unit-tested first). The generated glue at `/assets/validators.js` owns the
 * wasm plumbing; this wrapper only manages one-time initialisation and
 * installs the `validate_*` exports into `wire.mjs`'s registry via
 * `registerValidator`.
 *
 * `parseWireEvent` stays synchronous: the generated `.mjs` validators remain
 * installed until the WASM validators finish initialising, and both sets
 * implement the identical schema contract, so caller behaviour is identical
 * whichever set handles a frame. If the WASM glue fails to load, the `.mjs`
 * validators stay in place (graceful degradation).
 *
 * The glue's default export fetches `validators_bg.wasm` relative to its own
 * module URL, so it must be served over HTTP (file:// will not work).
 */

import { registerValidator } from "/src/wire.mjs";

/**
 * @typedef {import("/assets/validators.js").default} WasmInit
 * @typedef {(instance: unknown) => { instancePath: string, schemaPath: string }[]} WasmValidate
 */

/** @type {Promise<WasmInit> | null} */
let initPromise = null;

/**
 * The `_type` → generated Rust export mapping for the events the browser
 * accepts on the chat wire (`wire.mjs`'s registry keys). The control-plane
 * validators (`rename`, `session_rename`, `console_entry`) are generated in
 * the same crate but are not part of the WS data plane.
 *
 * @type {Map<string, string>}
 */
const WASM_EXPORTS = new Map([
  ["ready", "validate_ready"],
  ["pong", "validate_pong"],
  ["assistant", "validate_assistant"],
  ["error", "validate_error"],
  ["ack", "validate_ack"],
  ["session_meta", "validate_session_meta"],
  ["tool_call", "validate_tool_call"],
]);

/**
 * Idempotently start (or join) the WASM initialisation: dynamically import
 * the generated glue, run its default init (fetches and instantiates
 * `validators_bg.wasm`), then swap each `.mjs` validator in `wire.mjs` for
 * its Rust/WASM counterpart. Safe to call repeatedly; every caller gets the
 * same promise. A failure rejects the promise with the loader registered
 * validators left untouched (the `.mjs` fallback stays in place).
 *
 * @returns {Promise<WasmInit>} the initialised glue module
 */
export function initWasmValidators() {
  if (!initPromise) {
    initPromise = /** @type {Promise<WasmInit>} */ (
      import("/assets/validators.js").then(async (/** @type {any} */ mod) => {
        await mod.default();
        for (const [type, exportName] of WASM_EXPORTS) {
          registerValidator(type, /** @type {WasmValidate} */ (mod[exportName]));
        }
        return mod;
      })
    );
  }
  return initPromise;
}
