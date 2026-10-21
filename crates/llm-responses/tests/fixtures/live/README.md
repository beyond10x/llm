# Live Codex capture, 2026-10-21

One text turn recorded from the Codex backend (`https://chatgpt.com/backend-api/codex`) at
08:58 UTC on 2026-10-21, model `gpt-6-astra`, through `ResponsesClient` with the operator's Codex
login. A loopback recorder sat between the client and the backend and wrote the response status,
headers and body. It never wrote a request header, so no credential was recorded.
`tests/live_capture.rs` replays both files.

The request: instructions `Answer in one short sentence.`; one user message, `Name the two wire
protocols a portable LLM client most often speaks, in one sentence.`

| File | Content |
|---|---|
| `codex-text-turn-head.txt` | status line and response headers, LF-separated |
| `codex-text-turn.sse` | the response body after chunked decoding, 35 events |

## Scrubbed

Body, byte-level replacements; framing, field order and the `obfuscation` padding are as received:

| Field | Replaced with |
|---|---|
| `safety_identifier` (`user-…`, 3 occurrences) | `user-scrubbed0000000000000001` |
| response id `resp_…` | `resp_000…001` |
| 3 message ids `msg_…` | `msg_000…001` to `msg_000…003` |
| reasoning item id `rs_…` | `rs_000…001` |
| `prompt_cache_key` | `00000000-0000-7000-8000-000000000001` |
| reasoning `encrypted_content` | `gAAAAA-scrubbed-reasoning` |

Head:

| Header | Treatment |
|---|---|
| `Transfer-Encoding`, `Connection` | dropped; the replay adds its own framing |
| `Set-Cookie` (3), `Report-To`, `Nel` | dropped |
| `Date` | `Wed, 21 Oct 2026 00:00:00 GMT` |
| `x-oai-request-id` | `00000000-0000-7000-8000-000000000001` |
| `CF-RAY` | `0000000000000000-XXX` |
| `x-codex-plan-type`, `x-codex-credits-has-credits`, `x-codex-turn-state`, `x-models-etag` | `[scrubbed]` |
| `x-codex-primary-used-percent`, `x-codex-primary-reset-after-seconds`, `x-codex-primary-reset-at`, `x-codex-credits-balance` | `0` |

No email address, account id or organization name appeared in either file.
