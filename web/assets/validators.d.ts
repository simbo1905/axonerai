/* tslint:disable */
/* eslint-disable */

export function validate_ack(instance: any): any;

export function validate_assistant(instance: any): any;

export function validate_console_entry(instance: any): any;

export function validate_error(instance: any): any;

export function validate_pong(instance: any): any;

export function validate_prompt(instance: any): any;

export function validate_provider_models(instance: any): any;

export function validate_ready(instance: any): any;

export function validate_rename(instance: any): any;

export function validate_session_meta(instance: any): any;

export function validate_session_rename(instance: any): any;

export function validate_tool_call(instance: any): any;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly validate_ack: (a: any) => any;
    readonly validate_assistant: (a: any) => any;
    readonly validate_console_entry: (a: any) => any;
    readonly validate_error: (a: any) => any;
    readonly validate_pong: (a: any) => any;
    readonly validate_prompt: (a: any) => any;
    readonly validate_provider_models: (a: any) => any;
    readonly validate_ready: (a: any) => any;
    readonly validate_rename: (a: any) => any;
    readonly validate_session_meta: (a: any) => any;
    readonly validate_session_rename: (a: any) => any;
    readonly validate_tool_call: (a: any) => any;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
