# LLM

Composable model inference, provider integration, routing, provisioning and a gateway.

**Status: repository and planning foundation only.** The fourteen crate boundaries compile, but
export no runtime API. There is no usable gateway, provider client, release or deployment yet.
The implementation backlog is under `.engineering/planning/`; [the design](docs/design.md)
records the agreed target and [the domain](spec/system.yaml) gives its nouns typed homes.

The first milestone builds the full foundation here. Harness and Metaharness adopt a released
contract afterwards; the existing `llmgw` remains operational until a qualified reversible cutover.

## Checks

Run `task check` with Rust 1.98, AEP 0.55.0 and ESS 0.26.0. `task rust` checks the scaffold alone.
A passing scaffold check is not inference or provider-conformance evidence.

## License

LicenseRef-B10x-Proprietary. Source publication does not grant an open-source license.
