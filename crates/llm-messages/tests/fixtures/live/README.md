# Live Messages capture, 2026-10-21

Two turns recorded from `https://api.anthropic.com/v1/messages` on 2026-10-21,
model `claude-haiku-4-5` (served as `claude-haiku-4-5-20251001`), through `MessagesClient` under a
`subscription-oauth` account. The token was the active Claude login's, read in the capture process
only (approval-record:live-capture-claude-login-2026-10-21). The capture is the transport's
response tap: status, response headers and the event stream. No request header was written, so no
credential was recorded. `tests/live_capture.rs` replays all four files.

Both turns sent the same request: the 400-line prompt `tests/live_capture.rs` builds, no system
instruction, `max_tokens` 64. The first turn wrote the prompt prefix to the cache, and the second
read it back.

| File | Content | Cache figures (`message_delta.usage`) |
|---|---|---|
| `messages-cache-write-head.txt`, `messages-cache-write.sse` | turn 1 | `cache_creation_input_tokens` 13027, `cache_read_input_tokens` 0, `input_tokens` 3, `output_tokens` 4 |
| `messages-cache-read-head.txt`, `messages-cache-read.sse` | turn 2 | `cache_creation_input_tokens` 0, `cache_read_input_tokens` 13027, `input_tokens` 3, `output_tokens` 4 |

`message_start.message.usage` also carries `cache_creation.ephemeral_5m_input_tokens` (13027, then
0), `ephemeral_1h_input_tokens` 0, `service_tier` `standard`, `inference_geo` `not_available` and
`speed` `standard`.

## Scrubbed

| Where | Field | Replaced with |
|---|---|---|
| body | message id `msg_…` | `msg_000…001`, `msg_000…002` |
| head | `request-id` | `req_000…001`, `req_000…002` |
| head | `anthropic-organization-id` | `00000000-0000-4000-8000-000000000000` |
| head | `anthropic-workspace-id` | `wrkspc_scrubbed` |
| head | `traceresponse` | all-zero trace and span ids |
| head | `cf-ray` | `0000000000000000-XXX` |
| head | `date` | `Wed, 21 Oct 2026 00:00:00 GMT` |
| head | all 13 `anthropic-ratelimit-unified-*` headers (usage window, reset, overage) | `[scrubbed]` |
| head | `transfer-encoding`, `connection` | dropped; the replay adds its own framing |

No email address, account name or organization name appeared in either file.
