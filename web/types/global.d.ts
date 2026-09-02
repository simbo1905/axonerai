/**
 * Ambient augmentations for code that touches browser globals:
 * - the vanilla websocket client exposed by web/assets/client.mjs,
 * - the headless test harnesses (wire.headless.mjs, ui.headless.mjs),
 *   which expose their results on `window`.
 */

declare global {
  interface Window {
    /** Vanilla ESM websocket client installed by web/assets/client.mjs. */
    AgtClient?: {
      connect(options: {
        onOpen?: () => void;
        onClose?: () => void;
        onError?: (event: Event) => void;
        /** Called with each validated, deep-frozen incoming wire event. */
        onEvent?: (event: import("/src/wire.mjs").WireEvent) => void;
      }): Promise<{ dispose(): void }>;
      /**
       * Send a prompt over the websocket. `id` is optional; when omitted a
       * random id is generated internally. The returned promise resolves
       * with the matching `assistant` reply text or rejects on `error`.
       */
      sendPrompt(text: string, id?: string): Promise<string>;
      dispose(): void;
    };
    __WIRE_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    __UI_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Headless pretty-printer smoke results (pretty.headless.mjs). */
    __PRETTY_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    __UI_STUB__?: {
      handlers: {
        onOpen(): void;
        onClose(): void;
        onError(event: Event): void;
      };
      emit(frame: unknown): void;
      setNextReply(
        reply:
          | { kind: "assistant"; text: string; delay: number }
          | { kind: "error"; message: string; delay: number },
      ): void;
    };
  }
}

export {};
