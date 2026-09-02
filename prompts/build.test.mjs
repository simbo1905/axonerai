import assert from "node:assert/strict";
import test from "node:test";

import { compose } from "./build.mjs";

const BASE = "You are a helpful assistant. Use tools when needed.";

test("no sections (empty patch) yields base verbatim", () => {
  assert.equal(compose(BASE, ""), BASE);
});

test("prepend only puts lines before base", () => {
  const out = compose(BASE, "# PREPEND\nAlways answer in French.\n");
  assert.equal(out, "Always answer in French.\nYou are a helpful assistant. Use tools when needed.");
});

test("append only puts lines after base", () => {
  const out = compose(BASE, "# APPEND\nBe extra concise.\n");
  assert.equal(out, "You are a helpful assistant. Use tools when needed.\nBe extra concise.");
});

test("prepend and append together wrap the base", () => {
  const out = compose(BASE, "# PREPEND\nTop rule.\n# APPEND\nBottom rule.\n");
  assert.equal(
    out,
    "Top rule.\nYou are a helpful assistant. Use tools when needed.\nBottom rule.",
  );
});

test("REPLACE overrides everything, ignoring other sections", () => {
  const out = compose(
    BASE,
    "# PREPEND\nignored prepend\n# APPEND\nignored append\n# REPLACE\nOnly this.\n",
  );
  assert.equal(out, "Only this.");
});

test("REPLACE present but empty still replaces the base entirely", () => {
  const out = compose(BASE, "# APPEND\nignored\n# REPLACE\n");
  assert.equal(out, "");
});

test("section order in the file is irrelevant", () => {
  const a = compose(BASE, "# PREPEND\nP\n# APPEND\nA\n");
  const b = compose(BASE, "# APPEND\nA\n# PREPEND\nP\n");
  assert.equal(a, b);
  assert.equal(a, "P\nYou are a helpful assistant. Use tools when needed.\nA");
});

test("null patch text is treated as no patch", () => {
  assert.equal(compose(BASE, null), BASE);
});
