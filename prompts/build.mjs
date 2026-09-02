// Zero-dependency prompt composer.
//
// Reads prompts/base.txt and every prompts/models/<provider>--<model>.patch,
// composing each into prompts/generated/<provider>--<model>.txt, and always
// writes prompts/generated/default.txt (base verbatim).
//
// Patch format (plain text):
//   # PREPEND   <- lines prepended before the base (optional)
//   # APPEND    <- lines appended after the base (optional)
//   # REPLACE   <- if present (even empty), replaces the base entirely (optional)
// Section markers must be exactly "# PREPEND", "# APPEND", "# REPLACE" on their
// own line. Comments outside sections are not allowed. Model ids containing "/"
// use "_" in patch filenames instead.
//
// Run: node prompts/build.mjs
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const PROMPTS_DIR = fileURLToPath(new URL(".", import.meta.url));
const MODELS_DIR = join(PROMPTS_DIR, "models");
const GENERATED_DIR = join(PROMPTS_DIR, "generated");

const SECTION_MARKERS = ["PREPEND", "APPEND", "REPLACE"];

/** Parse a patch's text into { sections: {PREPEND,APPEND,REPLACE: string[]}, seen: Set }. */
export function parsePatch(patchText) {
  const sections = { PREPEND: [], APPEND: [], REPLACE: [] };
  const seen = new Set();
  let current = null;
  for (const line of patchText.split("\n")) {
    const match = line.match(/^# (PREPEND|APPEND|REPLACE)$/);
    if (match) {
      current = match[1];
      seen.add(current);
      continue;
    }
    if (current !== null) {
      sections[current].push(line);
    }
  }
  // A patch file typically ends with a newline; drop only the trailing blank
  // lines of each section (blank lines inside a section are kept).
  for (const name of SECTION_MARKERS) {
    while (sections[name].length > 0 && sections[name][sections[name].length - 1] === "") {
      sections[name].pop();
    }
  }
  return { sections, seen };
}

/** Compose the base prompt with a patch's text ("" for no patch). */
export function compose(base, patchText) {
  const { sections, seen } = parsePatch(patchText ?? "");
  if (seen.has("REPLACE")) {
    return sections.REPLACE.join("\n");
  }
  const parts = [];
  if (seen.has("PREPEND")) {
    parts.push(...sections.PREPEND);
  }
  parts.push(base);
  if (seen.has("APPEND")) {
    parts.push(...sections.APPEND);
  }
  return parts.join("\n");
}

function withTrailingNewline(text) {
  return text.endsWith("\n") ? text : `${text}\n`;
}

function main() {
  const base = readFileSync(join(PROMPTS_DIR, "base.txt"), "utf8");
  mkdirSync(GENERATED_DIR, { recursive: true });

  writeFileSync(join(GENERATED_DIR, "default.txt"), withTrailingNewline(base));

  const patches = existsSync(MODELS_DIR) ? readdirSync(MODELS_DIR).filter((f) => f.endsWith(".patch")) : [];
  for (const file of patches) {
    const stem = file.replace(/\.patch$/, "");
    const composed = compose(base, readFileSync(join(MODELS_DIR, file), "utf8"));
    writeFileSync(join(GENERATED_DIR, `${stem}.txt`), withTrailingNewline(composed));
    console.log(`composed prompts/generated/${stem}.txt`);
  }
  console.log(`wrote prompts/generated/default.txt`);
}

const isMain =
  process.argv[1] !== undefined &&
  pathToFileURL(realpathSync(process.argv[1])).href ===
    pathToFileURL(realpathSync(fileURLToPath(import.meta.url))).href;

if (isMain) {
  main();
}
