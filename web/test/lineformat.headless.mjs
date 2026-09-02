// @ts-check
import { initLineformat, parseWireLine } from "/src/lineformat.mjs";

/** @param {unknown} condition @param {string} message */
function assert(condition, message) {
  if (!condition) throw new Error(message);
}

/** @type {{ name: string, ok: boolean, error?: string }[]} */
const details = [];
let pass = 0;
let fail = 0;

/**
 * @param {string} name
 * @param {() => Promise<void>} fn
 */
async function test(name, fn) {
  try {
    await fn();
    pass += 1;
    details.push({ name, ok: true });
  } catch (error) {
    fail += 1;
    details.push({
      name,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

await test("parseWireLine throws before initLineformat", async () => {
  // Runs first, while the loader module is still uninitialised.
  /** @type {unknown} */
  let thrown;
  try {
    await parseWireLine("1717238400000\0{}");
  } catch (error) {
    thrown = error;
  }
  assert(thrown instanceof Error, "expected an Error before init");
  assert(
    /** @type {Error} */ (thrown).message.includes("initLineformat"),
    `unexpected message: ${/** @type {Error} */ (thrown).message}`,
  );
});

await test("valid line splits into ts/type/text", async () => {
  await initLineformat();
  const json = `{"_type":"assistant","text":"hi"}`;
  const frame = await parseWireLine(`1717238400000\0${json}`);
  assert(frame.ts === 1717238400000, `ts mismatch: ${frame.ts}`);
  assert(frame.type === "assistant", `type mismatch: ${frame.type}`);
  assert(frame.text === json, `text mismatch: ${frame.text}`);
  assert(frame.truncated === false, "small payload must not be flagged");
});

await test("corrupt line rejects (halt, never skip)", async () => {
  await initLineformat();
  /** @type {unknown} */
  let thrown;
  try {
    await parseWireLine("oops\n");
  } catch (error) {
    thrown = error;
  }
  assert(thrown instanceof Error, "corrupt line must reject");
  assert(
    /** @type {Error} */ (thrown).message.includes("corrupt wire line"),
    `unexpected message: ${/** @type {Error} */ (thrown).message}`,
  );
});

await test("oversized tool_call line truncates at 1024 and is detectably partial", async () => {
  await initLineformat();
  const json = `{"_type":"tool_call","tool":"WebSearch","result_json":"${"x".repeat(3000)}"}`;
  const frame = await parseWireLine(`1717238400001\0${json}`, 1024);
  assert(frame.truncated === true, "oversized payload must be flagged");
  assert(frame.type === "tool_call", `metadata type survives: ${frame.type}`);
  assert(frame.text.length === 1024, `cut exactly at the limit: ${frame.text.length}`);
  assert(
    frame.text.includes('"tool":"WebSearch"'),
    "metadata fields survive the cut (they serialize before the payload)",
  );
  // Truncation-detection rule (deterministic): strict parse failure ⇒ partial.
  let parsed = true;
  try {
    JSON.parse(frame.text);
  } catch {
    parsed = false;
  }
  assert(parsed === false, "cut payload must fail a strict JSON parse");
});

await test("multi-byte UTF-8 payload is cut on a codepoint boundary", async () => {
  await initLineformat();
  const json = `{"_type":"assistant","text":"${"\u{1F600}".repeat(200)}"}`;
  const frame = await parseWireLine(`1717238400002\0${json}`, 64);
  assert(frame.truncated === true, "oversized emoji payload must be flagged");
  assert(!frame.text.includes("\uFFFD"), "no replacement char — cut is codepoint-safe");
  assert(frame.text.length <= 64, `byte budget respected: ${frame.text.length}`);
});

// @ts-ignore - ambient declaration in web/types/global.d.ts
window.__LINEFORMAT_TEST_RESULTS__ = { pass, fail, details };
document.title = "lineformat-tests-done";
