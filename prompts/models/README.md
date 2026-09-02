System prompt overrides for specific provider+model pairs.

File naming: `<provider>--<model>.patch`, where any `/` in the model id is
replaced with `_` (e.g. model `openai/gpt-oss-20b` → `openai_gpt-oss-20b`).

Patch format (plain text):

    # PREPEND
    <lines prepended before the base prompt>
    # APPEND
    <lines appended after the base prompt>
    # REPLACE
    <if this section is present (even empty), it replaces the base entirely>

Sections are optional. Section markers must be exactly `# PREPEND`, `# APPEND`,
`# REPLACE` on their own line; comments outside sections are not allowed.

Run `make prompts` to compose these into `prompts/generated/` after editing.
