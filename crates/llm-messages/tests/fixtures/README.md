# Messages test fixtures

## `claude-code-first-turn.json`

The body of the first request a Claude Code 2.1.293 session sends over `ANTHROPIC_BASE_URL` when
it names a model id it does not recognise (the gateway alias `code`): adaptive thinking, effort
`max`, two `system` text blocks (the second marked `cache_control: {"type": "ephemeral"}`), one
marked user text and one tool.

It is **authored from published sources, not captured** from Claude Code or any deployment. It is
copied byte for byte from the body of `crates/llm-gateway/tests/fixtures/claude-code/1-tool-turn.request.txt`
in [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway), whose header records its
derivation from <https://code.claude.com/docs/en/llm-gateway-protocol> (read 2026-10-15):

- the "Feature pass-through" table: adaptive reasoning sends `thinking: {"type": "adaptive"}` to a
  model name Claude Code does not recognise, `output_config` carries the effort, and
  `cache_control` appears on `system` blocks and `messages` entries;
- "System prompt attribution block": a block first in `system`, whose exact text is not published,
  so the first block's text here is a placeholder;
- effort `max` is one of Claude Code's five levels (`low`, `medium`, `high`, `xhigh`, `max`).

The user text names a file path as the client's own prompt; it is fixture text, not a path any
test reads.
