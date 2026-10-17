# Captured Codex requests

`first-turn.json` and `second-turn.json` are two request bodies `codex-cli` 0.160.0 sent on
2026-10-16 to a loopback recorder standing in for a Responses endpoint. The recorder answered the
first request of the second capture with a scripted tool call and every other request with an
error. No provider was reached, no paid call was made and no credential was involved.

| file | what it is |
| --- | --- |
| `first-turn.json` | the opening request of a session: instructions, a developer message of two parts, the environment and the question |
| `second-turn.json` | the request after the scripted tool call: the same opening, the replayed reasoning item, the call and its plain-string output |

## Configuration

- model alias `code`, pointed at the recorder;
- `web_search = "disabled"`; the multi-agent and goals features off;
- reasoning effort `high`, reasoning summary `auto`.

## Sanitization

- every identifier is replaced by `00000000-0000-7000-8000-00000000000n`;
- long text values are shortened and marked `[shortened]`;
- the environment's working directory is replaced by `/workspace`.

The files are otherwise as captured and are not edited by hand.

## What a known model adds

Captured with `gpt-5.5`, a model Codex knows, the same client also sends
`text: {"verbosity": "low"}`, and may add hosted tools (`web_search`, `custom`, `tool_search`,
`namespace`). This crate's pinned subset refuses those tools by their type; the verbosity round
trip is tested on an authored copy of `first-turn.json` carrying that `text` object.
