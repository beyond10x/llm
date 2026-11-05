# Request fixtures written by this repository

Unlike `../data/`, whose files are copied from public repositories, the files here are written
by this repository and record the exact bytes an encoder sends. A test that compares against one
fails on any change to them; regenerate a file only on purpose.

## `structured-output-converse-request.json`

The Converse body for a turn with instructions, one user text and a named JSON Schema output
format (story `structured-output`). The shape follows the published Bedrock Runtime API
reference (`OutputConfig`, `OutputFormat`, `OutputFormatStructure`, `JsonSchemaDefinition`, read
2026-11-04): `outputConfig.textFormat` is `{"type": "json_schema", "structure": {"jsonSchema":
{...}}}`, and `jsonSchema.schema` is the schema as a JSON string, not an object. It was not
recorded against an account.

## `image-converse-request.json`

The Converse body for a turn with instructions, one user text and an inline PNG (the eight bytes
of the PNG signature; story `image-input`). The shape follows the published Bedrock Runtime API
reference as the story cites it (`ContentBlock`, `ImageBlock`, `ImageSource`): `{"image": {"format":
"png", "source": {"bytes": "<base64>"}}}`, the blob base64 as the JSON protocol writes one, in the
user message beside the text. It was not recorded against an account.
