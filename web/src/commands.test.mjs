// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { COMMANDS, parseInput, filterCommands } from "./commands.mjs";

test("plain chat text parses as chat", () => {
  assert.deepEqual(parseInput("hello agent"), { kind: "chat" });
  assert.deepEqual(parseInput("  hello  "), { kind: "chat" });
  assert.deepEqual(parseInput(""), { kind: "chat" });
  // A slash that is not the FIRST character is chat, not a command.
  assert.deepEqual(parseInput("a /b c"), { kind: "chat" });
});

test("/ alone parses as an empty command error", () => {
  assert.deepEqual(parseInput("/"), {
    kind: "command",
    name: "",
    args: "",
    error: "empty",
  });
  assert.deepEqual(parseInput("   /   "), {
    kind: "command",
    name: "",
    args: "",
    error: "empty",
  });
});

test("unknown command parses with an unknown error", () => {
  assert.deepEqual(parseInput("/frobnicate"), {
    kind: "command",
    name: "frobnicate",
    args: "",
    error: "unknown",
  });
  assert.deepEqual(parseInput("/frobnicate x y"), {
    kind: "command",
    name: "frobnicate",
    args: "x y",
    error: "unknown",
  });
});

test("/rename without args reports missing-args", () => {
  assert.deepEqual(parseInput("/rename"), {
    kind: "command",
    name: "rename",
    args: "",
    error: "missing-args",
  });
  assert.deepEqual(parseInput("/rename   "), {
    kind: "command",
    name: "rename",
    args: "",
    error: "missing-args",
  });
});

test("/rename with multiword args keeps the full title", () => {
  assert.deepEqual(parseInput("/rename a b"), {
    kind: "command",
    name: "rename",
    args: "a b",
  });
  assert.deepEqual(parseInput("/rename   My   Cool   Session  "), {
    kind: "command",
    name: "rename",
    args: "My Cool Session",
  });
});

test("every registered command parses without error (rename needs args)", () => {
  for (const command of COMMANDS) {
    const parsed = parseInput(`/${command.name}`);
    if (command.argsRequired) {
      assert.deepEqual(parsed, {
        kind: "command",
        name: command.name,
        args: "",
        error: "missing-args",
      });
    } else {
      assert.deepEqual(parsed, {
        kind: "command",
        name: command.name,
        args: "",
      });
    }
  }
});

test("prefix filtering matches command names", () => {
  assert.deepEqual(
    filterCommands("").map((c) => c.name),
    ["model", "built-ins", "verbose", "rename", "help"],
  );
  assert.deepEqual(
    filterCommands("/").map((c) => c.name),
    ["model", "built-ins", "verbose", "rename", "help"],
  );
  assert.deepEqual(
    filterCommands("/b").map((c) => c.name),
    ["built-ins"],
  );
  assert.deepEqual(
    filterCommands("mo").map((c) => c.name),
    ["model"],
  );
  assert.deepEqual(filterCommands("zzz"), []);
});

test("commands are lowercase; input case is not normalized for parsing", () => {
  assert.deepEqual(parseInput("/MODEL"), {
    kind: "command",
    name: "MODEL",
    args: "",
    error: "unknown",
  });
  // The menu filter is case-insensitive even though names are lowercase.
  assert.deepEqual(
    filterCommands("B").map((c) => c.name),
    ["built-ins"],
  );
  for (const command of COMMANDS) {
    assert.equal(command.name, command.name.toLowerCase());
  }
});

test("registry has exactly the v1 commands with descriptions", () => {
  assert.deepEqual(
    COMMANDS.map((c) => c.name),
    ["model", "built-ins", "verbose", "rename", "help"],
  );
  for (const command of COMMANDS) {
    assert.equal(typeof command.description, "string");
    assert.ok(command.description.length > 0, "description must be non-empty");
  }
});
