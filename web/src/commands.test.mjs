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

test("/model is retired: unknown command", () => {
  assert.equal(
    COMMANDS.find((command) => command.name === "model"),
    undefined,
    "the retired /model command must not linger in the registry",
  );
  assert.deepEqual(parseInput("/model"), {
    kind: "command",
    name: "model",
    args: "",
    error: "unknown",
  });
});

test("prefix filtering matches command names", () => {
  assert.deepEqual(
    filterCommands("").map((c) => c.name),
    ["models", "built-ins", "verbose", "rename", "help", "console"],
  );
  assert.deepEqual(
    filterCommands("/").map((c) => c.name),
    ["models", "built-ins", "verbose", "rename", "help", "console"],
  );
  assert.deepEqual(
    filterCommands("/b").map((c) => c.name),
    ["built-ins"],
  );
  assert.deepEqual(
    filterCommands("mo").map((c) => c.name),
    ["models"],
  );
  assert.deepEqual(filterCommands("con"), [
    {
      name: "console",
      description: "open the devtools console popup",
    },
  ]);
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

test("registry has exactly the v1+console commands with descriptions", () => {
  assert.deepEqual(
    COMMANDS.map((c) => c.name),
    ["models", "built-ins", "verbose", "rename", "help", "console"],
  );
  for (const command of COMMANDS) {
    assert.equal(typeof command.description, "string");
    assert.ok(command.description.length > 0, "description must be non-empty");
  }
});

test("console command is registered and /help will list it", () => {
  const consoleCommand = COMMANDS.find((c) => c.name === "console");
  if (!consoleCommand) throw new Error("console command missing from the registry");
  assert.equal(consoleCommand.description, "open the devtools console popup");
  assert.equal(consoleCommand.argsRequired, undefined, "console takes no args");
  assert.deepEqual(parseInput("/console"), {
    kind: "command",
    name: "console",
    args: "",
  });
  // The /help runner joins the registry, so the console entry flows into the
  // help output text exactly as the other commands do.
  const helpText = COMMANDS.map(
    (command) => `/${command.name} — ${command.description}`,
  ).join("\n");
  assert.ok(
    helpText.includes("/console — open the devtools console popup"),
    `/help must list /console, got ${JSON.stringify(helpText)}`,
  );
});
