/**
 * Ambient augmentation for the headless browser test harness
 * (web/test/wire.headless.mjs), which exposes its results on `window`.
 */

declare global {
  interface Window {
    __WIRE_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
  }
}

export {};
