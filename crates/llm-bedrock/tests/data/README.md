# Test data and where it comes from

Every file here is copied unchanged from a public repository; none is written by hand except this
page and `frames/uris.txt`. No file was recorded against an account of this project.

| Directory | Source | Revision | Licence |
| --- | --- | --- | --- |
| `sigv4/` | [awslabs/aws-c-auth](https://github.com/awslabs/aws-c-auth), `tests/aws-signing-test-suite/v4/` (the AWS SigV4 test suite) | `84a26ea184d9ccd322c8164759510b3604ab13fd` | Apache-2.0 (`sigv4/LICENSE`, `sigv4/NOTICE`) |
| `eventstream/` | [smithy-lang/smithy-rs](https://github.com/smithy-lang/smithy-rs), `rust-runtime/aws-smithy-eventstream/test_data/` | `4d4380068a04858da53e9980e75ff4c8ba0f0de6` | Apache-2.0 (`eventstream/LICENSE`) |
| `frames/pydantic-*` | [pydantic/pydantic-ai](https://github.com/pydantic/pydantic-ai), `tests/models/cassettes/test_bedrock/` | `f30a0b5699ed2f8a8c1a6ef7493fc641012a68b0` | MIT (`frames/LICENSE-pydantic-ai`) |
| `frames/ddtrace-*` | [DataDog/dd-trace-py](https://github.com/DataDog/dd-trace-py), `tests/contrib/botocore/bedrock_cassettes/` | `1cb9f945a5d9550f9b1f72622780f747e1d810b7` | Apache-2.0 (`frames/LICENSE-dd-trace-py`) |

## `sigv4/`

Only the header-signing files of each case are kept (`context.json`, `request.txt`,
`header-canonical-request.txt`, `header-string-to-sign.txt`, `header-signature.txt`,
`header-signed-request.txt`); the query-signing files and the SigV4a suite are left out, because
this crate signs headers only.

## `frames/`

Each cassette is a recorded HTTP exchange with Amazon Bedrock Runtime in `us-east-1`. The
response body of each interaction is stored as it was recorded: `<cassette>[-<n>].bin` is an
`application/vnd.amazon.eventstream` body, `<cassette>.json` a JSON body, and
`<cassette>[-<n>].request.json` the JSON request body that produced it. `uris.txt` lists each
cassette's request URIs in interaction order; they name the operation (`converse`,
`converse-stream`, `invoke`, `invoke-with-response-stream`) and the model each body belongs to.
`pydantic-test_bedrock_streaming_error` was answered HTTP 400 with
`{"message":"The provided model identifier is invalid."}` and has no response body file.
