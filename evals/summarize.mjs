// Prints a per-provider PASS/FAIL summary from a promptfoo JSON results file.
// Usage: node evals/summarize.mjs .tmp/evals/results.json
// Exits non-zero if any provider has a failing test or the file is unreadable.
import { readFileSync } from "node:fs";

const file = process.argv[2] ?? ".tmp/evals/results.json";

let parsed;
try {
  parsed = JSON.parse(readFileSync(file, "utf8"));
} catch (err) {
  console.error(`ERROR: cannot read results file ${file}: ${err.message}`);
  process.exit(1);
}

// promptfoo nests rows at results.results for --output JSON exports; older
// shapes may have results as a plain array.
const rows = Array.isArray(parsed)
  ? parsed
  : Array.isArray(parsed?.results?.results)
    ? parsed.results.results
    : Array.isArray(parsed?.results)
      ? parsed.results
      : [];
if (!Array.isArray(rows) || rows.length === 0) {
  console.error(`ERROR: no result rows in ${file}`);
  process.exit(1);
}

const byLabel = new Map();
for (const row of rows) {
  const label = row.provider?.label ?? row.provider?.id ?? "unknown";
  if (!byLabel.has(label)) {
    byLabel.set(label, { pass: 0, fail: 0, failures: [] });
  }
  const agg = byLabel.get(label);
  const desc = row.testCase?.description ?? row.description ?? "?";
  if (row.success) {
    agg.pass++;
  } else {
    agg.fail++;
    agg.failures.push(desc);
  }
}

let failedConfigs = 0;
console.log("provider--model".padEnd(34) + "passed  result");
for (const [label, agg] of [...byLabel.entries()].sort(([a], [b]) => a.localeCompare(b))) {
  const ok = agg.fail === 0;
  if (!ok) failedConfigs++;
  const note = ok ? "" : `  (failed: ${agg.failures.join(", ")})`;
  console.log(
    label.padEnd(34) +
      `${agg.pass}/${agg.pass + agg.fail}`.padEnd(8) +
      (ok ? "PASS" : "FAIL") +
      note,
  );
}

console.log(`\n${byLabel.size - failedConfigs}/${byLabel.size} configs fully passed`);
process.exit(failedConfigs > 0 ? 1 : 0);
