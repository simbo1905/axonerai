// @ts-check
/**
 * Node tests for the shared WASM wire-line parser (web/assets/lineformat.js,
 * built by `make wasm-lineformat`). The glue is dynamically imported — the
 * same module the browser loads — and the wasm fetch is satisfied from disk
 * because Node's fetch cannot read file:// URLs. If the glue is absent the
 * suite skips so `node --test` stays green pre-build.
 */
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const glueUrl = new URL("../assets/lineformat.js", import.meta.url);
const gluePath = fileURLToPath(glueUrl);
const wasmPath = gluePath.replace(/\.js$/, "_bg.wasm");
const hasGlue = existsSync(gluePath) && existsSync(wasmPath);

/** @type {any} */
let mod;

/**
 * Patch fetch so the glue's wasm load works under Node, dynamically import
 * the glue (the same dynamic import the browser performs) and run its init.
 */
async function loadGlue() {
  if (mod) return mod;
  const wasmBytes = readFileSync(wasmPath);
  const realFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    new Response(/** @type {BodyInit} */ (wasmBytes), {
      headers: { "content-type": "application/wasm" },
    }));
  try {
    mod = await import(glueUrl.href);
    await mod.default();
  } finally {
    globalThis.fetch = realFetch;
  }
  return mod;
}

const skipMessage = "web/assets/lineformat.js absent — run `make wasm-lineformat`";

test("parse_line splits a valid wire line", { skip: hasGlue ? false : skipMessage }, async () => {
  const glue = await loadGlue();
  const frame = glue.parse_line('1717238400000\0{"_type":"assistant","text":"hi"}', 1024);
  assert.equal(frame.ts, 1717238400000);
  assert.equal(frame.type, "assistant");
  assert.equal(frame.text, '{"_type":"assistant","text":"hi"}');
  assert.equal(frame.truncated, false);
});

test("is_valid enforces the strict <digits>\\0 prefix", { skip: hasGlue ? false : skipMessage }, async () => {
  const glue = await loadGlue();
  assert.equal(glue.is_valid("1717238400000\0{}"), true);
  assert.equal(glue.is_valid("12\0"), true, "empty payload keeps a valid prefix");
  assert.equal(glue.is_valid(""), false, "empty line");
  assert.equal(glue.is_valid("abc\0{}"), false, "letters");
  assert.equal(glue.is_valid(" 12\0{}"), false, "space before digits");
  assert.equal(glue.is_valid("+1\0{}"), false, "sign");
  assert.equal(glue.is_valid("12 {}"), false, "no NUL after digits");
  assert.equal(glue.is_valid("12"), false, "digits only");
  assert.equal(glue.is_valid("12345678901234567\0{}"), false, "17 digits");
  assert.equal(glue.is_valid("1234567890123456\0{}"), true, "16 digits ok");
});

test("parse_line rejects corrupt lines", { skip: hasGlue ? false : skipMessage }, async () => {
  const glue = await loadGlue();
  assert.throws(() => glue.parse_line("oops\n", 1024));
  assert.throws(() => glue.parse_line("", 1024));
});

test("parse_line truncates oversized payloads at maxBytes", { skip: hasGlue ? false : skipMessage }, async () => {
  const glue = await loadGlue();
  const big = `{"_type":"tool_call","tool":"WebSearch","result_json":"${"x".repeat(3000)}"}`;
  const frame = glue.parse_line(`1717238400001\0${big}`, 1024);
  assert.equal(frame.truncated, true);
  assert.equal(frame.text.length, 1024);
  assert.ok(frame.text.includes('"tool":"WebSearch"'), "metadata survives the cut");
  // Truncation-detection rule: strict JSON parse of a cut payload fails.
  assert.throws(() => JSON.parse(frame.text));
});

test("parse_line never splits a UTF-8 codepoint", { skip: hasGlue ? false : skipMessage }, async () => {
  const glue = await loadGlue();
  const payload = '{"text":"' + "\u{1F600}".repeat(10) + '"}';
  const frame = glue.parse_line(`1717238400002\0${payload}`, 12);
  assert.equal(frame.truncated, true);
  assert.ok(frame.text.length < payload.length, "cut applied");
  assert.ok(!frame.text.includes("\uFFFD"), "no replacement char: cut is codepoint-safe");
});
