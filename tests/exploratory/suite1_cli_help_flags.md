# Suite 1: CLI Help Flags

**Objective:** Verify that `-h/--help` flags work correctly for all demo binaries with subcommands.

## Demos to Test

From README.md:
1. `axoner-web` (web UI demo with web feature) - has serve subcommand
2. Note: `axoner` (minimal CLI demo) does not support --help as it runs directly without subcommands

## Test Cases

### Case 1.1: axoner-web --help
```bash
cargo run --example axoner-web --release -- --help
```
**Expected:** 
- Help text displays without error
- Shows usage, options, and description
- Lists `serve` subcommand
- Exit code 0

### Case 1.2: axoner-web -h
```bash
cargo run --example axoner-web --release -- -h
```
**Expected:**
- Same output as `--help`
- Exit code 0

### Case 1.3: axoner-web serve --help (with web feature)
```bash
cargo run --example axoner-web --features web --release -- serve --help
```
**Expected:**
- Shows serve subcommand options: `--host`, `--port`, `--web-root`
- Shows verbose flag: `-v`
- Exit code 0

### Case 1.4: axoner-web serve -h (with web feature)
```bash
cargo run --example axoner-web --features web --release -- serve -h
```
**Expected:**
- Same output as serve `--help`
- Exit code 0

## Execution Log Template

```
Test Date: YYYY-MM-DD HH:MM:SS UTC
Tester: [name]

Case 1.1: axoner-web --help
Status: [PASS/FAIL]
Output:
[paste output here]

Case 1.2: axoner-web -h
Status: [PASS/FAIL]
Output:
[paste output here]

Case 1.3: axoner-web serve --help
Status: [PASS/FAIL]
Output:
[paste output here]

Case 1.4: axoner-web serve -h
Status: [PASS/FAIL]
Output:
[paste output here]

## Summary
Total: 4
Passed: X
Failed: X
Notes: [any observations]
```
