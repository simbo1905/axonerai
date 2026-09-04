// @ts-check
import { initPretty, prettyPrintAbridged } from "/src/pretty.mjs";

/** @param {unknown} condition @param {string} message */
function assert(condition, message) {
  if (!condition) throw new Error(message);
}

/**
 * Count `{`/`}` and `[`/`]` outside string literals; returns true when each
 * pair is balanced and no string is left unterminated.
 *
 * @param {string} text
 * @returns {boolean}
 */
function bracketsBalanced(text) {
  let braces = 0;
  let brackets = 0;
  let inString = false;
  let escaped = false;
  for (const c of text) {
    if (inString) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') inString = false;
      continue;
    }
    if (c === '"') inString = true;
    else if (c === "{") braces += 1;
    else if (c === "}") braces -= 1;
    else if (c === "[") brackets += 1;
    else if (c === "]") brackets -= 1;
  }
  return braces === 0 && brackets === 0 && !inString;
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

await test("prettyPrintAbridged throws before initPretty", async () => {
  // Runs first, while the loader module is still uninitialised.
  /** @type {unknown} */
  let thrown;
  try {
    await prettyPrintAbridged("{}");
  } catch (error) {
    thrown = error;
  }
  assert(thrown instanceof Error, "expected an Error before init");
  assert(
    /** @type {Error} */ (thrown).message.includes("initPretty"),
    `unexpected message: ${/** @type {Error} */ (thrown).message}`,
  );
});

await test("oracle: valid JSON pretty-prints as expected", async () => {
  await initPretty();
  const input = `{"a":[1,2,{"b":null}],"c":"x"}`;
  const expected = `{
  "a": [
    1,
    2,
    {
      "b": null
    }
  ],
  "c": "x"
}`;
  const out = await prettyPrintAbridged(input, 2);
  assert(out === expected, `oracle mismatch:\n${out}`);
});

await test("abridged: truncation is pretty-printed with marker and closed brackets", async () => {
  await initPretty();
  // Server abridges a pretty-printed payload: cut mid-string, append one ….
  const input = `{
  "name": "axoner",
  "payload": "some very long tool-call output that got cu…`;
  const out = await prettyPrintAbridged(input, 2);
  assert(out.includes("\n"), "expected newlines in output");
  assert(bracketsBalanced(out), `brackets not balanced:\n${out}`);
  assert(out.includes("…"), "expected the truncation marker … in output");
  assert(out.trimEnd().endsWith("}"), `expected closing brace, got:\n${out}`);
});

await test("abridged: dangling comma dropped, nested brackets closed", async () => {
  await initPretty();
  const input = `{
  "items": [
    "alpha",…`;
  const out = await prettyPrintAbridged(input, 2);
  assert(!/",\s*\]/.test(out.replace(/"[^"]*"/g, '""')), "dangling comma survived");
  assert(bracketsBalanced(out), `brackets not balanced:\n${out}`);
  assert(out.includes(`"alpha"`), `expected alpha kept:\n${out}`);
});

// @ts-ignore - ambient declaration in web/types/global.d.ts
window.__PRETTY_TEST_RESULTS__ = { pass, fail, details };
document.title = "pretty-tests-done";
// Flush one macrotask before the summary so headless drivers that attach
// their console listener at load-end still see it.
await new Promise((resolve) => setTimeout(resolve, 0));
// Mirror the PASS/FAIL summary into the DOM as well: the result stays
// observable without a console listener.
{
  const summaryEl = document.createElement("pre");
  summaryEl.id = "pretty-tests-summary";
  summaryEl.textContent = `[pretty-tests] pass=${pass} fail=${fail}`;
  document.body.append(summaryEl);
}
console.log(
  `[pretty-tests] pass=${pass} fail=${fail}` +
    details
      .filter((d) => !d.ok)
      .map((d) => `\n[pretty-tests] FAIL ${d.name}: ${d.error}`)
      .join(""),
);
