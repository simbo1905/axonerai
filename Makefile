JTD_CODEGEN ?= mise exec -- jtd-codegen
SCHEMA_DIR  := schemas
OUT_DIR     := web/generated
SCHEMAS     := $(wildcard $(SCHEMA_DIR)/*.jdt.json)
VALIDATORS  := $(patsubst $(SCHEMA_DIR)/%.jdt.json,$(OUT_DIR)/%.mjs,$(SCHEMAS))

.PHONY: validators clean-validators

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
