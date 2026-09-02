/**
 * Minimal ambient declarations for the Node built-in test runner and strict
 * assert module, as used by the handwritten web tests. The repo has no
 * package.json / @types/node, so these keep `tsc --noEmit` green without
 * adding a dependency.
 */

declare module "node:test" {
  /** Minimal test context supporting teardown hooks. */
  export interface TestContext {
    /** Register a callback to run after the test finishes. */
    after(fn: () => void | Promise<void>): void;
  }
  /** Per-test options, as used by the handwritten web tests. */
  export interface TestOptions {
    /** Skip the test; a string message documents why (shown in output). */
    skip?: boolean | string;
  }
  /**
   * @param {string} name
   * @param {(t: TestContext) => void | Promise<void>} fn
   */
  function test(name: string, fn: (t: TestContext) => void | Promise<void>): void;
  /**
   * @param {string} name
   * @param {TestOptions} options
   * @param {(t: TestContext) => void | Promise<void>} fn
   */
  function test(
    name: string,
    options: TestOptions,
    fn: (t: TestContext) => void | Promise<void>,
  ): void;
  export default test;
}

declare module "node:assert/strict" {
  /** @param {unknown} value @param {string} [message] */
  export function ok(value: unknown, message?: string): void;
  /** @param {unknown} actual @param {unknown} expected @param {string} [message] */
  export function equal(actual: unknown, expected: unknown, message?: string): void;
  /** @param {unknown} actual @param {unknown} expected @param {string} [message] */
  export function deepEqual(actual: unknown, expected: unknown, message?: string): void;
  /**
   * @param {() => unknown} block
   * @param {((error: unknown) => boolean | void) | Error | RegExp | (new (...args: any[]) => Error)} [error]
   * @param {string} [message]
   */
  export function throws(
    block: () => unknown,
    error?: ((error: unknown) => boolean | void) | Error | RegExp | (new (...args: any[]) => Error),
    message?: string,
  ): void;
}

declare module "node:fs" {
  /**
   * @param {string | URL} path
   * @returns {boolean}
   */
  export function existsSync(path: string | URL): boolean;
  /**
   * @param {string | URL} path
   * @returns {Uint8Array}
   */
  export function readFileSync(path: string | URL): Uint8Array;
}

declare module "node:url" {
  /**
   * @param {URL} url
   * @returns {string}
   */
  export function fileURLToPath(url: URL): string;
}
