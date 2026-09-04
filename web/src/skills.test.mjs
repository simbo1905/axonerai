// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { fetchSkills } from "./skills.mjs";

/**
 * Install a stub global fetch for one test (restored in each finally).
 *
 * @param {() => Promise<Response>} impl
 * @returns {void}
 */
function stubFetch(impl) {
  globalThis.fetch = /** @type {typeof fetch} */ (impl);
}

test("fetchSkills normalizes the /api/skills payload and freezes it", async () => {
  const original = globalThis.fetch;
  try {
    stubFetch(async () =>
      new Response(
        JSON.stringify([
          {
            name: "greeting",
            description: "greet politely",
            source: "local",
            path: ".axonerai/skills/greeting/SKILL.md",
          },
          {
            name: "deploy",
            description: "ship it",
            source: "user",
            path: "/home/u/.axonerai/skills/deploy/SKILL.md",
          },
        ]),
        { status: 200 },
      ),
    );
    const skills = await fetchSkills();
    assert.ok(skills, "payload accepted");
    const rows = /** @type {readonly { name: string, description: string, source: string, path: string }[]} */ (skills);
    assert.equal(rows.length, 2);
    assert.deepEqual(rows[0], {
      name: "greeting",
      description: "greet politely",
      source: "local",
      path: ".axonerai/skills/greeting/SKILL.md",
    });
    assert.equal(rows[1].source, "user");
    assert.ok(Object.isFrozen(rows), "list frozen");
    assert.ok(Object.isFrozen(rows[0]), "rows frozen");
  } finally {
    globalThis.fetch = original;
  }
});

test("fetchSkills drops malformed entries but keeps valid ones", async () => {
  const original = globalThis.fetch;
  try {
    stubFetch(async () =>
      new Response(
        JSON.stringify([
          { name: "good", description: "fine", source: "local", path: "p" },
          { name: "no-description", source: "local", path: "p" },
          { name: "bad-source", description: "x", source: "alien", path: "p" },
          "not-an-object",
          null,
        ]),
        { status: 200 },
      ),
    );
    const skills = await fetchSkills();
    assert.ok(skills);
    const rows = /** @type {readonly { name: string }[]} */ (skills);
    assert.equal(rows.length, 1);
    assert.equal(rows[0].name, "good");
  } finally {
    globalThis.fetch = original;
  }
});

test("fetchSkills returns null on non-ok, non-array and network failure", async () => {
  const original = globalThis.fetch;
  try {
    stubFetch(async () => new Response("nope", { status: 500 }));
    assert.equal(await fetchSkills(), null, "non-ok response");
    stubFetch(async () => new Response("{}", { status: 200 }));
    assert.equal(await fetchSkills(), null, "non-array body");
    stubFetch(async () => {
      throw new Error("down");
    });
    assert.equal(await fetchSkills(), null, "network failure");
  } finally {
    globalThis.fetch = original;
  }
});

// item54: the server drops settings-disabled BUILT-INS from /api/skills;
// the client must have no builtin hardcoding — a listing where a builtin
// (deepresearch) is absent is accepted exactly like any other payload and
// never re-injected client-side.
test("fetchSkills accepts a listing with a builtin filtered out (no client hardcoding)", async () => {
  const original = globalThis.fetch;
  try {
    stubFetch(async () =>
      new Response(
        JSON.stringify([
          {
            name: "greeting",
            description: "greet politely",
            source: "local",
            path: ".axonerai/skills/greeting/SKILL.md",
          },
          {
            name: "deploy",
            description: "ship it",
            source: "user",
            path: "/home/u/.axonerai/skills/deploy/SKILL.md",
          },
        ]),
        { status: 200 },
      ),
    );
    const skills = await fetchSkills();
    assert.ok(skills, "payload accepted without the builtin");
    const rows = /** @type {readonly { name: string, source: string }[]} */ (skills);
    assert.deepEqual(
      rows.map((row) => row.name),
      ["greeting", "deploy"],
      "only the server-sent rows survive — no builtin re-injection",
    );
    assert.ok(Object.isFrozen(rows), "list still frozen");
  } finally {
    globalThis.fetch = original;
  }
});
