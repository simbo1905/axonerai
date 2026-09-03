// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import {
  FOOTER_MODE,
  FOOTER_THINK,
  footerSegments,
  formatFooter,
} from "./footer.mjs";

/** Minimal /api/state-shaped snapshot for the footer tests. */
const snapshot = {
  provider: "mistral",
  model: "zai-glm-5-2",
  context: { tokens: 12345 },
};

test("mode/think slots are hardcoded Chat/off for now", () => {
  assert.equal(FOOTER_MODE, "Chat");
  assert.equal(FOOTER_THINK, "off");
});

test("left is `Chat · <model> <provider> · think off`", () => {
  const { left } = formatFooter(snapshot, 131072);
  assert.equal(left, "Chat · zai-glm-5-2 mistral · think off");
});

test("right formats tokens to 1dp K and the percent of the context window", () => {
  assert.equal(
    formatFooter({ ...snapshot, context: { tokens: 12345 } }, 131072).right,
    "12.3K (9%)",
  );
  assert.equal(
    formatFooter({ ...snapshot, context: { tokens: 65536 } }, 131072).right,
    "65.5K (50%)",
  );
  assert.equal(
    formatFooter({ ...snapshot, context: { tokens: 66000 } }, 131072).right,
    "66.0K (50%)",
  );
});

test("percent rounds to the nearest whole percent (also over the window)", () => {
  assert.equal(
    formatFooter({ ...snapshot, context: { tokens: 1000 } }, 131072).right,
    "1.0K (1%)",
  );
  assert.equal(
    formatFooter({ ...snapshot, context: { tokens: 1234567 } }, 131072).right,
    "1234.6K (942%)",
  );
});

test("unknown model (null context window) omits the percent", () => {
  assert.equal(formatFooter(snapshot, null).right, "12.3K");
  assert.equal(formatFooter(snapshot, 0).right, "12.3K");
});

test("non-numeric tokens render an empty right slot", () => {
  assert.equal(
    formatFooter({ provider: "mistral", model: "m", context: { tokens: "x" } }, 131072)
      .right,
    "",
  );
  assert.equal(formatFooter({ provider: "mistral", model: "m" }, null).right, "");
});

test("missing model/provider fall back to (unknown)", () => {
  assert.equal(footerSegments({}).model, "(unknown)");
  assert.equal(footerSegments({}).provider, "(unknown)");
  assert.equal(footerSegments(null).model, "(unknown)");
});
