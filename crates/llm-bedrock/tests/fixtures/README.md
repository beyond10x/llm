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
