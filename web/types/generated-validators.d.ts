/**
 * Ambient types for the generated JTD validator barrel
 * (`web/generated/validators.mjs`, machine-generated, no JSDoc).
 *
 * The tsconfig `paths` entry for the validators barrel maps that import
 * specifier here, so handwritten call sites get real types without tsc
 * checking the generated `.mjs` sources themselves.
 */

/** A single failed JTD validation. */
export interface ValidationError {
  /** JSON pointer to the offending part of the instance, e.g. "/text". */
  instancePath: string;
  /** JSON pointer to the offending part of the schema. */
  schemaPath: string;
}

/**
 * Validate a `ready` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateReady(instance: any): ValidationError[];

/**
 * Validate a `pong` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validatePong(instance: any): ValidationError[];

/**
 * Validate an `assistant` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateAssistant(instance: any): ValidationError[];

/**
 * Validate an `error` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateError(instance: any): ValidationError[];

/**
 * Validate an `ack` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateAck(instance: any): ValidationError[];

/**
 * Validate a `session_meta` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateSession_meta(instance: any): ValidationError[];

/**
 * Validate a console envelope against its schema
 * (`schemas/console_entry.jdt.json` — the agt-console BroadcastChannel /
 * IndexedDB spool contract).
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateConsole_entry(instance: any): ValidationError[];

/**
 * Validate a `tool_call` event against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateTool_call(instance: any): ValidationError[];

/**
 * Validate an outgoing `rename` control-plane frame against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateRename(instance: any): ValidationError[];

/**
 * Validate a `session_rename` rollout record against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateSession_rename(instance: any): ValidationError[];

/**
 * Validate an outgoing `prompt` frame against its schema
 * (`schemas/prompt.jdt.json` — the client → server chat prompt contract).
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validatePrompt(instance: any): ValidationError[];

/**
 * Validate a `/api/models` payload against its schema.
 * @param {any} instance
 * @returns {ValidationError[]}
 */
export function validateProvider_models(instance: any): ValidationError[];
