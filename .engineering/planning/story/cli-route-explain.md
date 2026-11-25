---
format: aep.planning-md/3
id: story:cli-route-explain
kind: story
status: active
title: llm route explain shows which target selection chooses and why
tags:
- from-review
relations:
- decomposes: epic:inference
- depends_on: story:output-and-image-capabilities
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: README.md
- confidence: inferred
  path: crates/llm-cli/Cargo.toml
- confidence: cited
  path: crates/llm-cli/src/lib.rs
- confidence: inferred
  path: crates/llm-cli/tests/command.rs
- confidence: inferred
  path: crates/llm-cli/tests/credential_process.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/guides/configure-model-routes.md
- confidence: inferred
  path: website/docs/guides/explain-a-route.md
- confidence: inferred
  path: website/docs/guides/use-the-llm-command.md
- confidence: inferred
  path: website/docs/reference/crates.md
- confidence: inferred
  path: website/docs/status.mdx
- confidence: inferred
  path: website/docs/status/limitations.md
revision: 21
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:42Z", actor: "human:operator", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:42Z", actor: "human:operator", revision: 4}
---
## Context

`story:output-and-image-capabilities` (wave 2026-11-05-w26) asked that `llm route explain` name a
selection rejection. `llm-cli` has only `model list` and `infer`; the rejection is named in
`Catalog::explain`'s JSON and in the "route has no compatible target" refusal, which a library caller
reads. A command-line user cannot see why a route chose or rejected a target without running a turn.

## Outcome

A command-line user sees, for a route and a turn shape, which target selection would choose and why
each other target was rejected, without calling a provider.

## Acceptance

- `llm route explain <route>` over `Routes::catalog().explain` prints every candidate target with its
  rejection names (for example `structured-output`, `image-input`), in selection order, and the chosen
  target; it makes no network call and prints no secret.
- Flags state the turn's needs (structured output, image input, tools); a test pins the output.

## Scope

Derived 2026-11-25 by `story-scoper`, read at `main` 831c21d95. Every line is **cited** (the source is in brackets: `story` when the story's text names it, `tree` when it was read in the tree at the line given) or **inferred** (a reading that could be wrong). The story names one crate (`llm-cli`), two symbols (`Catalog::explain`, `Routes::catalog()`) and no file path, so no path below is cited by the story.

- **Primary surface:** `crates/llm-cli`, one new subcommand and its tests — cited (story)
- **No library change:** `llm-routing` and `llm-models` already give what the first acceptance line asks, and `llm-cli` depends on both (`crates/llm-cli/Cargo.toml:29-30`) — inferred
- **Exists, `explain`:** `crates/llm-routing/src/selection.rs:178-225`, `pub fn explain(&self, request: &TurnRequest, input_tokens: Option<u64>) -> Result<RouteExplanation, Error>` on `Catalog`. It validates the request (`:183`), reads `request.model` as the route alias (`:184-188`), refuses an embed route (`:189-191`) and does no I/O — cited (story, tree)
- **Exists, what it returns:** `RouteExplanation` (`selection.rs:116-125`) holds `route_id`, `alias`, `config_digest`, `input_tokens`, `selected_target_id: Option<Id>` and `targets`. `TargetExplanation` (`:103-114`) holds `target_id`, `serving_model_id`, `position`, `provenance`, `auth_kind`, `billing_kind`, `capabilities` and `rejections: Vec<Rejection>`. Both derive `Serialize` — cited (tree)
- **Exists, order and choice:** `targets` has one entry per target of the route, rejected ones included, in position order (`crates/llm-routing/src/catalog.rs:172`). The chosen target is the first entry without a rejection (`selection.rs:203-205`) — cited (tree)
- **Exists, rejection names:** `Rejection` (`selection.rs:61-82`) has 13 variants, is `#[non_exhaustive]` and serializes kebab-case; `Rejection::label()` (`:84-100`) gives the same names: `tools`, `tool-choice`, `temperature`, `top-p`, `reasoning-effort`, `output-limit`, `structured-output`, `image-input`, `input-tokens-unknown`, `context-window`, `opaque-state`, `refusal-text`, `fallback-disabled`. Within one target they come in the fixed order of `capability_rejections` (`:275-360`), then `fallback-disabled` (`:200-202`) — cited (tree)
- **Exists, callers of `explain`:** `Catalog::resolve` (`selection.rs:235`), `Catalog::run_turn` (`crates/llm-routing/src/fallback.rs:319`), `llm_calls::replay` (`crates/llm-calls/src/replay.rs:928`), the shadow reservation (`crates/llm-calls/src/shadow.rs:457`), `llm_decide::judge` (`crates/llm-decide/src/judge.rs:258`), the conformance target (`checks/conformance/src/target.rs:88`) and the example `crates/llm-routing/examples/explain.rs:15`. `llm-cli` does not call it, and `llm infer` runs no selection (`crates/llm-cli/src/lib.rs:811-820`: `Routes::port`, then the port's turn) — cited (tree)
- **Exists, `Routes::catalog()`:** `crates/llm-models/src/routes.rs:1658` returns the `llm.catalog/1` catalog the document expands into — cited (story, tree)
- **Gap 1, one candidate per route:** `expand` (`routes.rs:1467-1528`) gives every route one target at position 0 with `fallback_enabled: false` (`:1495-1506`, module doc `:25-28`). Over an `llm.toml` the command always prints one candidate; "each other target" and `fallback-disabled` cannot occur — cited (tree)
- **Gap 2, Bedrock routes:** a Bedrock route has no catalog entry (`routes.rs:1633-1642`, `:1657`), so `explain` answers `invalid-request`, "unknown route alias" (`selection.rs:185-188`). The story does not say what the command prints for one — cited (tree)
- **Gap 3, aliases and direct names:** the catalog's alias is the route's name (`routes.rs:1497`); a document's `aliases` live in `Routes` only (`:1424`, `Routes::route` `:1685`). The command has to map `<route>` through `Routes::route(name)` to `name()`, as `replayed` does (`crates/llm-cli/src/lib.rs:433-438`). A direct `<endpoint>/<model-id>` name has only a private one-entry catalog (`direct_catalog`, `routes.rs:1718`) that `Routes::catalog()` does not hold — cited (tree)
- **Gap 4, the input-token bound:** with `input_tokens` `None` every target is rejected `input-tokens-unknown` (`selection.rs:328-329`) and nothing is chosen — cited (tree)
- **Gap 4, what follows:** "the chosen target" needs a bound, for example a flag `--input-tokens <N>`, which the acceptance's list of flags does not name — inferred
- **Command, declared:** `crates/llm-cli/src/lib.rs:67-76` `Cli` (clap derive, global `--config`), `:78-89` `enum Command` (`Model { command: ModelCommand }`, `Infer(Infer)`, `Replay(Replay)`), `:112-119` `enum ModelCommand { List { format } }`, `:121-125` `Format { Text, Json }` — cited (tree)
- **Command, dispatched:** `run` (`lib.rs:326-341`) parses; `execute` (`:348-358`) calls `locate` and `load`, then matches the command with no wildcard arm — cited (tree)
- **Command, the model to copy:** `llm model list`, `list(routes: &Routes, format: Format)` (`lib.rs:610-650`) with `rows` (`:562-585`) and `table` (`:652-705`). It receives the parsed document and no `Environment`, so it builds no resolver, no HTTP client and no runtime; a closed stdout is exit 0 (`:643-648`) — cited (tree)
- **Command, the new shape:** a `Route { command: RouteCommand }` variant with `Explain { route, flags, format }`, one arm in `execute`, one function that takes `&Routes` only — inferred
- **Flags onto `TurnRequest`:** `<route>` becomes `TurnRequest::new(<route name>, vec![Item::user(..)])` (`crates/llm-core/src/turn.rs:96`); no prompt flag is needed, selection reads no text — inferred
- **Flag, structured output:** `controls.output_format = Some(OutputFormat::json_schema(<a schema object>))` (`crates/llm-core/src/controls.rs:343`); selection reads only that it is set (`selection.rs:312`) — inferred
- **Flag, image input:** one `Item::user_image(ImageSource::bytes(ImageMediaType::Png, <one byte or more>))` (`crates/llm-core/src/item.rs:133`, `crates/llm-core/src/image.rs:93`, `:173-180`); selection reads only that such an item exists (`selection.rs:316-320`) — inferred
- **Flag, tools:** one `ToolSpec` whose `input_schema` is an object (`turn.rs:14-18`, `:170-195`); selection reads `!request.tools.is_empty()` (`selection.rs:283`) — inferred
- **Needs selection reads that the acceptance does not name:** `tool_choice` (`selection.rs:285`), `temperature` (`:289`), `top_p` (`:293`), `reasoning_effort` (`:297-301`), `max_output_tokens` (`:305-307`, `:331`). `llm infer` already has `--max-output-tokens` (`lib.rs:136-137`) — cited (tree)
- **What the explanation is not:** `explain` calls `validate()` and not `validate_for`, so a target it chooses can still be refused by its wire before I/O (a Responses or Chat format without a name, a Converse image by URL, `docs/design.md:84-88`) — cited (tree)
- **Output pinned today:** `crates/llm-cli/tests/command.rs` pins `model list` text by column (`:402-455`) and JSON by value (`:458-487`, `:1221-1242`); `crates/llm-cli/tests/credential_process.rs:263-286` pins the JSON once more. All run the built binary through `llm()` (`command.rs:319-338`, `env_clear`). No test reads `--help` — cited (tree)
- **Output, what a pinned test has to fix:** `config_digest` and `binding_revision` hash the endpoint URL, so a test that pins the whole output needs a fixed `base_url`, as the `model list` tests use `http://127.0.0.1:9` (`command.rs:403`) — inferred
- **No network call, helpers:** `Fixture` (`command.rs:50-78`) is a loopback server that keeps every request it read. `refused_before_sending` (`:1013-1024`) asserts `fixture.received().is_empty()`, but also exit 2 and an empty stdout, so the explain test uses `Fixture` and `llm()` directly — cited (tree)
- **No secret, fakes:** `KEYS` (`command.rs:396-399`) sets both fixture variables to `API_KEY` (`:25`); `SECRET_ACCESS_KEY` and `CONTAINER_TOKEN` (`:23-24`); a `keyring_core::mock::Store` through `llm_cli::Environment.keychain` (`:1111-1123`); the `aws-process` run counter `runs()` (`credential_process.rs:152`, asserted 0 for `model list` at `:285`). Absence is asserted at `command.rs:1008` and `:1347` — cited (tree)
- **No network call and no secret, the test:** routes at a `Fixture` origin, `llm route explain` run with `KEYS` set; assert exit 0, the pinned stdout, `API_KEY` in neither stream and `fixture.received().is_empty()`. A fourth case in `credential_process.rs` (`CASES`, `:30-43`) asserts `runs() == 0` for an `aws-process` document — inferred
- **ESS, the command:** no domain describes the `llm` command. `spec/domains/` has 17 files and none names `llm-cli`, a subcommand or the binary; the conformance target calls library functions. `model list`, `infer` and `replay` as commands are held by Rust tests alone — cited (tree)
- **ESS, route explanation:** types `llm.catalog.RouteRejection` (the 13 variants), `TargetExplanation` and `RouteExplanation` (`spec/domains/catalog.yaml:53-80`); command `llm.routing.Evaluate` (`spec/domains/routing.yaml:126`) with entity `llm.routing.Evaluation` and view `llm.routing.LastEvaluation`, answered through `Catalog::explain` (`checks/conformance/src/target.rs:88`), rejections observed as `<target>:<name>` (`:90-101`) — cited (tree)
- **ESS, scenarios today:** 23 of the 91 files in `contracts/routing/scenarios/` name `rejections`. Occurrences per name there: `fallback-disabled` 13, `context-window` 7, `opaque-state` 7, `input-tokens-unknown` 5, `temperature` 5, `structured-output` 3, `image-input` 3, `output-limit` 3, and 1 each for `tools`, `tool-choice`, `top-p`, `reasoning-effort`. `refusal-text` is in no scenario; `crates/llm-routing/tests/catalog.rs:85-98` and `crates/llm-models/tests/adversary_w24_refusal_route.rs` hold it — cited (tree)
- **ESS, not covered:** no scenario runs selection over the catalog a routes document expands into. `llm.routes.Load` (`spec/domains/routes.yaml:92`) observes names, endpoints and capabilities, and no file under `contracts/routes/` declares `structured_output` or `image_input`; `crates/llm-models/tests/feature_declarations.rs:105-133` holds it — cited (tree)
- **ESS, reading:** the story needs no scenario. It adds no library behaviour, selection is already answered by `llm.routing`, and no domain or adapter starts the binary. `contracts/baseline.json` stays 1090 and `AGENTS.md:80` is not touched — inferred
- **ESS, if a scenario is wanted anyway:** it needs a new domain, an adapter in `checks/conformance/src/` that starts the binary, files under `contracts/schema/` and a floor move. That changes the shape of the conformance target and is not in the acceptance — inferred
- **Documents, the command's guide:** `website/docs/guides/use-the-llm-command.md`: `:12` "has three subcommands" with the list at `:14-18`; a new section between "List the models" (`:94`) and "Run a turn" (`:111`); `:203` "`llm infer` and `llm model list` both refuse the file" (the check runs in `load`, before every subcommand); the exit-code rows `:288` and `:290`. The heading id `#exit-codes` stays: `ux/scenarios/task-exit-codes/truth.json` reads it — inferred
- **Documents, limitations page:** `website/docs/status/limitations.md:32-37`, "The operator command line has three commands … The command has no other subcommand" — inferred
- **Documents, routes guide:** `website/docs/guides/configure-model-routes.md:164-168` says selection over the route's catalog rejects a route that declares a feature `false`; one sentence naming the command — inferred
- **Documents, optional links:** `website/docs/guides/explain-a-route.md` (the library example, `:11-13`), `website/docs/getting-started.md:60-68`, `website/docs/index.md:75-76` and `website/docs/concepts/routing.md:81-88` describe the explanation through the cargo example — inferred
- **Documents, design:** `docs/design.md:416-419` says what the `llm` command is as a caller; `:1335` ("Listing or explaining routes must not resolve secrets") is in the section that moved to the gateway repository; `:70-92` (the declarations) stays — inferred
- **Documents, status record:** a row `cli-route-explain` in "Providers and routing" of `docs/implementation-status.md` (`:137-153`), beside `output-and-image-capabilities` (`:144`) and `llm-command` (`:148`); then `website/data/status.json` and `website/docs/status.mdx` regenerated with `task docs-generate`. The record is not prose for the gate's path list (`AGENTS.md:125-127`), so the change runs the whole gate — inferred
- **Documents, generated reference:** `llm-docs generate` writes no page for the command. It writes `website/docs/reference/crates.md` from each package's `description`; `:22` is the `llm-cli` row, from `crates/llm-cli/Cargo.toml:9`, which lists what the command does — cited (tree)
- **Documents, root files:** `README.md:78` (the `llm-cli` row), `AGENTS.md:25` ("`model list`, `infer` and `replay`"), `CHANGELOG.md:7` (one line under Unreleased, under a new `### Added` heading; today only `### Fixed` at `:9`), and the crate documentation `crates/llm-cli/src/lib.rs:3-31` — inferred
- **Documents, invariants table:** the acceptance asks for no row. A row "explaining a route resolves no secret and sends nothing" would sit beside `AGENTS.md:61` or `:63` and cite a test of `crates/llm-cli/tests/` — inferred
- **Documents, site checks:** `.github/workflows/docs-ux.yml:5-8` runs on changes under `website/` and `docs/`; the guide has a ratchet entry at `ux/baseline.json:726`. No page is added — cited (tree)
- **The story's Context is stale in one place:** it says `llm-cli` has only `model list` and `infer`; `replay` is there (`crates/llm-cli/src/lib.rs:88`) — cited (story, tree)
- **`depends_on: story:output-and-image-capabilities` (implemented, 0.22.0):** the story uses `Rejection::StructuredOutput` and `Rejection::ImageInput` with their labels (`selection.rs:71-76`, `:92-93`), the rule that only a declared `false` rejects (`:310-322`), the optional `Capabilities.structured_output` and `image_input` (`crates/llm-core/src/vocabulary.rs:109-125`), and that a routes document's declaration reaches the expanded catalog (`feature_declarations.rs:105-133`). All of it is on `main`. That story's acceptance was amended on 2026-11-05 to hand the command to this one — cited (story, tree)
- **Overlap with `story:test-binaries-one-per-crate`, tests:** its part 5 moves `crates/llm-cli/tests` and edits the `[[test]]` entry (`crates/llm-cli/Cargo.toml:46-48`). A case added to `tests/command.rs` conflicts with that move unless the move is a pure rename. A new `tests/route_explain.rs` merges cleanly, but is one more binary and cannot use the helpers of `command.rs`. The description at `Cargo.toml:9` is another hunk. This story touches no file under `crates/llm-routing/tests` or `crates/llm-models/tests` — inferred
- **Overlap with `story:test-binaries-one-per-crate`, documents:** `AGENTS.md` (this story `:25`, that one the table), `CHANGELOG.md` Unreleased, `website/docs/guides/use-the-llm-command.md` (this story the body, that one the `source:` line 6) — inferred
- **Overlap with `story:responses-carries-refusal-text`:** it changes when `refusal-text` fires (`selection.rs:349-358`, today every wire but Chat Completions), not the set of names: the variant stays for a Messages target. This story has no flag that puts refusal text into the probe turn, so its pinned output cannot hold `refusal-text`, and it edits no file of `llm-routing`. The two meet only if this story's guide lists all 13 names with their meaning; a link to `website/docs/concepts/routing.md` avoids that. Shared file: `CHANGELOG.md` Unreleased — inferred
- **Overlap with `story:replay-stops-at-the-last-case-have-scenarios` and `story:tool-arguments-with-a-repeated-key`:** with no scenario added, this story shares no file under `contracts/` with them and does not touch `AGENTS.md:80`. All three add lines at `CHANGELOG.md:7-9`; this story and the replay story would each add the `### Added` heading there, a small conflict for whichever merges second. `AGENTS.md:25` and `docs/design.md:416` are other hunks than the ones those stories' scopes name — inferred
- **Confidence:** high for where the code and the test land, because the subcommand enum, the dispatch, the model subcommand and every helper were read at their lines; medium for the flags and the output, because the acceptance leaves the four gaps above open — cited (tree)
- **Would collide with:** any unit that edits `enum Command` or `execute` in `crates/llm-cli/src/lib.rs`; any unit that edits or moves `crates/llm-cli/tests/command.rs` or `crates/llm-cli/tests/credential_process.rs`; any unit that edits `website/docs/guides/use-the-llm-command.md`, `website/docs/status/limitations.md`, the "Providers and routing" rows of `docs/implementation-status.md` (and so the two generated status files), `README.md:78`, `AGENTS.md:25` or the Unreleased block of `CHANGELOG.md`; and, only if a scenario is added, every unit that moves `contracts/suite.json`, `contracts/baseline.json` and `AGENTS.md:80` — inferred
- **Safety fact:** the command can be written to receive the parsed `Routes` and nothing else, as `list` does (`lib.rs:610`). `Routes::parse` does no I/O and refuses a literal secret (`routes.rs:1530-1543`); `explain` reads only the catalog's routes and bindings (`selection.rs:178-225`); `TargetExplanation` has no field for a secret reference, a base URL or a header (`:103-114`). So no credential source, client or runtime is in reach of the code path. Step 2 for the whole, step 3 for `explain` and its return type (read line by line), unproven — inferred
- **Safety fact, second:** `execute` runs `locate` and `load` before every subcommand (`lib.rs:349-350`). For a document with an `aws-process` endpoint, `load` runs the owner and mode check (`:311-321`) and starts no program, so the new subcommand refuses such a file exactly as `model list` does. Step 2, unproven — inferred
- **Safety fact, where a symbol search stops:** JSON output would make the command a second reader of the serde shape of `RouteExplanation`, which `spec/domains/catalog.yaml:60-80` and `contracts/schema/schema/types/llm.catalog.RouteExplanation.schema.json` also describe. A later field on `TargetExplanation` then changes the command's pinned output. Step 1 — inferred
