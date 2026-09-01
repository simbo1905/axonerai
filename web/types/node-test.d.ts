/**
 * Minimal ambient declarations for the Node built-in test runner and strict
 * assert module, as used by the handwritten web tests. The repo has no
 * package.json / @types/node, so these keep `tsc --noEmit` green without
 * adding a dependency.
 */

declare module "node:test" {
  /**
   * @param {string} name
   * @param {() => void | Promise<void>} fn
   */
  function test(name: string, fn: () => void | Promise<void>): void;
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
