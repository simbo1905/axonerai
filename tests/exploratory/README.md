# Exploratory Manual Tests

Manual regression tests for AxonerAI demos. Each test should validate a specific feature or use case.

## Test Structure

Test results are organized by timestamp in `.tmp/tests/YYYYMMDD_hhmmss/`:

```
.tmp/tests/
├── 20260107_074300/
│   ├── suite1_cli_help_flags.log
│   ├── suite1_cli_help_flags.data/
│   └── notes.md
```

## Running Tests

**IMPORTANT: Always use `timeout` to prevent hanging.**

Tests assume the repository root as working directory. Wrap all commands with `timeout` to prevent Agent from waiting for processes that don't exit (e.g., servers, daemons).

**Timeout Strategy:**
- Start with `timeout 5s` for CLI operations (help, version, quick runs)
- If a command legitimately needs more time (e.g., first compilation, network calls), increase by 5s increments: `timeout 10s`, `timeout 15s`, etc.
- **Reset to `timeout 5s` for the next different command** to avoid unnecessary padding
- Document in results if commands needed higher timeouts for future reference

**Example timeout progression:**
```bash
# Help flags - 5s timeout
timeout 5s cargo run --example axoner-web --release -- --help

# First cargo build - may need more time
timeout 10s cargo run --example axoner-web --features web --release -- serve --help

# Next help test - reset to 5s
timeout 5s cargo run --example axoner-web --features web --release -- serve -h

# Server startup - may need 15-20s for first run, record this
timeout 15s cargo run --example axoner-web --features web --release -- serve --port 9090
```

When running agents that use `WriteFile` tool, always execute from the dated folder to keep the source tree clean:

```bash
# Create dated test folder
TEST_DIR=".tmp/tests/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

# Run demo from dated folder (paths must be relative to repo root)
timeout 20s cargo run --example axoner-web --features web --release -- -v serve --port 9090 &

# In another terminal, cd to repo root and run agent/curl tests against it
```

When `WriteFile` creates files during tests, they will be isolated in the dated folder, not the source tree.

## Test Suites

- [Suite 1: CLI Help Flags](./suite1_cli_help_flags.md) - Verify all `-h/--help` flags work for all demos

## Adding New Tests

1. Create `suiteN_snake_case_title.md` with test steps and expected outcomes
2. Document setup, commands, and verification steps
3. Note any environment variables needed
4. **Always wrap commands with appropriate `timeout`** - start with 5s, increase if needed, reset for new commands
5. Document results and timeout adjustments in `YYYYMMDD_hhmmss/suiteN_snake_case_title.log`
6. Record which commands needed higher timeouts for Agent reference
