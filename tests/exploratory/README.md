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

Tests assume the repository root as working directory. When running agents that use `WriteFile` tool, always execute from the dated folder to keep the source tree clean:

```bash
# Create dated test folder
TEST_DIR=".tmp/tests/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

# Run demo from dated folder (paths must be relative to repo root)
cargo run --example axoner-web --features web --release -- -v serve --port 9090

# In another terminal, cd to repo root and run agent/curl tests against it
```

When `WriteFile` creates files during tests, they will be isolated in the dated folder, not the source tree.

## Test Suites

- [Suite 1: CLI Help Flags](./suite1_cli_help_flags.md) - Verify all `-h/--help` flags work for all demos

## Adding New Tests

1. Create `suiteN_snake_case_title.md` with test steps and expected outcomes
2. Document setup, commands, and verification steps
3. Note any environment variables needed
4. Document results in `YYYYMMDD_hhmmss/suiteN_snake_case_title.log`
