// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import {
  MCP_DISABLED_KEY_PREFIX,
  mcpDisabledKey,
  parseDisabledServers,
} from "./mcp-prefs.mjs";

test("mcpDisabledKey embeds the repo folder under the agt.mcp-disabled prefix", () => {
  assert.equal(
    mcpDisabledKey("/Users/Shared/axonerai"),
    "agt.mcp-disabled:/Users/Shared/axonerai",
  );
  assert.equal(MCP_DISABLED_KEY_PREFIX, "agt.mcp-disabled:");
});

test("two different folders get different keys (no shared toggles)", () => {
  const a = mcpDisabledKey("/Users/Shared/axonerai");
  const b = mcpDisabledKey("/home/dev/other-checkout");
  assert.ok(a !== b, "different folders must get different keys");
  assert.equal(a, "agt.mcp-disabled:/Users/Shared/axonerai");
  assert.equal(b, "agt.mcp-disabled:/home/dev/other-checkout");
});

test("parseDisabledServers: valid JSON array of strings is frozen", () => {
  const parsed = parseDisabledServers(JSON.stringify(["tavily", "context7"]));
  assert.deepEqual(parsed, ["tavily", "context7"]);
  assert.ok(Object.isFrozen(parsed), "result must be deep-frozen");
});

test("parseDisabledServers: missing value is a frozen empty list", () => {
  const parsed = parseDisabledServers(null);
  assert.deepEqual(parsed, []);
  assert.ok(Object.isFrozen(parsed));
  assert.deepEqual(parseDisabledServers(""), []);
});

test("parseDisabledServers: invalid JSON is dropped (null)", () => {
  assert.equal(parseDisabledServers("{ not json"), null);
  assert.equal(parseDisabledServers("tavily"), null);
});

test("parseDisabledServers: non-array JSON is dropped (null)", () => {
  assert.equal(parseDisabledServers('{"tavily":true}'), null);
  assert.equal(parseDisabledServers('"tavily"'), null);
  assert.equal(parseDisabledServers("42"), null);
  assert.equal(parseDisabledServers("true"), null);
});

test("parseDisabledServers: non-string or empty elements invalidate the payload", () => {
  assert.equal(parseDisabledServers('["tavily", 42]'), null);
  assert.equal(parseDisabledServers('["tavily", null]'), null);
  assert.equal(parseDisabledServers('["tavily", ""]'), null);
  assert.equal(parseDisabledServers('[{"name":"tavily"}]'), null);
});

test("parseDisabledServers: empty array is a valid frozen empty list", () => {
  const parsed = parseDisabledServers("[]");
  assert.deepEqual(parsed, []);
  assert.ok(Object.isFrozen(parsed));
});
