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
      /**
       * Send a control-plane `rename` frame over the websocket. The server's
       * reply arrives as an `ack` event (for_type "rename") via onEvent.
       */
      sendRename?(title: string): Promise<void> | void;
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
    /** Headless outgoing-frame validation results (client.headless.mjs). */
    __CLIENT_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Headless panel + slash-menu results (panel.headless.mjs). */
    __PANEL_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Headless console-screen DOM test results (console-screen.headless.mjs — single page, injected entries). */
    __CONSOLE_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Stub client hooks installed by panel.headless.mjs. */
    __PANEL_STUB__?: {
      emit(frame: unknown): void;
      renames: string[];
      prompts: string[];
      postCalls: Array<{ name: string; enabled: boolean }>;
      /** Recorded POST /api/mcp bodies (item48 MCP toggles). */
      mcpPosts: Array<{ server: string; enabled: boolean }>;
      /** Recorded POST /api/model bodies (item59 model swaps). */
      modelPosts: Array<{ service: string; model: string }>;
      /** When true the next POST /api/model is answered 400 (item59). */
      failNextModelPost?: boolean;
    };
    /** Headless pretty-printer smoke results (pretty.headless.mjs). */
    __PRETTY_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Headless WASM-validator results (wasm-validators.headless.mjs). */
    __WASM_VALIDATORS_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
    };
    /** Headless ?s= catch-up results (catchup.headless.mjs). */
    __CATCHUP_TEST_RESULTS__?: {
      pass: number;
      fail: number;
      details: Array<{ name: string; ok: boolean; error?: string }>;
      run: number;
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
