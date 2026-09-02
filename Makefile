JTD_CODEGEN ?= mise exec -- jtd-codegen
TSC         ?= tsc
SCHEMA_DIR  := schemas
OUT_DIR     := web/generated
SCHEMAS     := $(wildcard $(SCHEMA_DIR)/*.jdt.json)
VALIDATORS  := $(patsubst $(SCHEMA_DIR)/%.jdt.json,$(OUT_DIR)/%.mjs,$(SCHEMAS))

.PHONY: validators clean-validators check-types prompts init check build-server serve-up serve-down serve-status serve-logs evals

# Compose prompts/generated/*.txt from prompts/base.txt + prompts/models/*.patch.
# Must run before `cargo build`: src/prompt.rs embeds
# prompts/generated/default.txt at compile time via include_str!.
prompts:
	node prompts/build.mjs

check-types:
	$(TSC) --noEmit

validators: $(VALIDATORS) $(OUT_DIR)/validators.mjs

$(OUT_DIR)/%.mjs: $(SCHEMA_DIR)/%.jdt.json
	@mkdir -p $(OUT_DIR)
	$(JTD_CODEGEN) --target js $< > $@

# Barrel: one differentiated re-export per generated module.
# BSD sed has no \U, so capitalize via awk.
$(OUT_DIR)/validators.mjs: $(VALIDATORS)
	@mkdir -p $(OUT_DIR)
	@rm -f $@
	@for f in $(VALIDATORS); do \
	  stem=$$(basename "$$f" .mjs); \
	  cap=$$(printf '%s' "$$stem" | awk '{ print toupper(substr($$0,1,1)) substr($$0,2) }'); \
	  echo "export { validate as validate$$cap } from \"./$$stem.mjs\";" >> $@; \
	done

clean-validators:
	rm -rf $(OUT_DIR)

init:
	@command -v luarocks >/dev/null 2>&1 || { echo "ERROR: install luarocks (brew install luarocks)"; exit 1; }
	@luarocks --lua-version=5.1 list tl 2>/dev/null | grep -q "^tl$$" \
		|| luarocks --lua-version=5.1 install tl
	@echo "tl (Teal, LuaJIT 5.1 ABI): OK"

check:
	@eval "$$(luarocks --lua-version=5.1 path)" && tl check tooling/lib/*.tl

build-server:
	cargo build --release --features web --example axoner-web

serve-up:
	@tooling/serve.lua up $(PROVIDER) $(MODEL) $(PORT)

serve-down:
	@tooling/serve.lua down $(PROVIDER) $(MODEL) $(PORT)

serve-status:
	@tooling/serve.lua status

serve-logs:
	@tooling/serve.lua logs $(PROVIDER) $(MODEL) $(PORT)

AGT_SERVER := target/release/examples/axoner-web

# Run the promptfoo eval matrix: build + compose prompts, start all 7
# provider--model servers on ports 9501-9507, run evals/promptfooconfig.yaml,
# stop the servers, and print a per-provider summary. Exits non-zero if any
# config has a failing test.
evals: build-server prompts
	@test -x $(AGT_SERVER) || { echo "ERROR: server binary missing: $(AGT_SERVER)"; exit 1; }
	@mkdir -p .tmp/evals
	@tooling/serve.lua up groq qwen/qwen3.8-27b 9501
	@tooling/serve.lua up groq openai/gpt-oss-20b 9502
	@tooling/serve.lua up groq openai/gpt-oss-120b 9503
	@tooling/serve.lua up mistral zai-glm-5-2 9504
	@tooling/serve.lua up mistral mistral-medium-latest 9505
	@tooling/serve.lua up opencode-zen glm-5.2 9506
	@tooling/serve.lua up opencode-go glm-5.2 9507
	@promptfoo eval -c evals/promptfooconfig.yaml --no-share -j 1 --output .tmp/evals/results.json || true
	@tooling/serve.lua down all
	@node evals/summarize.mjs .tmp/evals/results.json
