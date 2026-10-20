//! Adversary pass 1, wave 2026-10-20-w09: every YAML spelling of the two streaming declarations in
//! an `llm.routes/1` mapping route is either refused or loads at the clients' values, and a claim
//! the protocol cannot carry is never dropped on the way.

use llm_core::ErrorCode;
use llm_models::routes::Routes;

const MARKER: &str = "zq-marker-7f3";

fn document(tail: &str) -> String {
    format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: http://127.0.0.1:8000/v1, protocol: chat-completions, auth: {{kind: anonymous}}}}
routes:
  r:
    model: p/m
    capabilities:
      tools: &t true
      tool_choice: false
      temperature: false
      top_p: false
      reasoning_efforts: []
      context_window: 32768
      max_output_tokens: 8192
{tail}"
    )
}

#[test]
fn adversary_w09_every_spelling_of_a_claim_is_refused_and_echoes_no_value() {
    let claims = [
        "      input_during_output: True\n",
        "      input_during_output: TRUE\n",
        "      input_during_output: !!bool true\n",
        "      input_during_output: *t\n",
        "      input_during_output: ~\n",
        "      input_during_output:\n",
        "      input_during_output: 'true'\n",
        "      input_during_output: yes\n",
        "      input_during_output: on\n",
        "      input_during_output: 1\n",
        "      input_during_output: false\n      input_during_output: true\n",
        "      input_during_output: true\n      input_during_output: false\n",
        "      output_streaming: False\n",
        "      output_streaming: 0\n",
        "      output_streaming: ~\n",
        "      output_streaming: no\n",
        "      output_streaming: true\n      output_streaming: false\n",
        "      Input_During_Output: true\n",
        "      input-during-output: true\n",
        "      inputDuringOutput: true\n",
        "      output-streaming: false\n",
        "      streaming: false\n",
        "      <<: {input_during_output: true}\n",
        "      <<: {output_streaming: false}\n",
        "      input_during_output: zq-marker-7f3\n",
        "      output_streaming: \"zq-marker-7f3\"\n",
        "      input_during_output: !zq-marker-7f3 true\n",
    ];
    let mut accepted = Vec::new();
    for tail in claims {
        match Routes::parse(&document(tail)) {
            Ok(_) => accepted.push(tail.trim().to_owned()),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::InvalidRequest, "{tail}");
                assert!(!error.message.contains(MARKER), "{tail}: {}", error.message);
            }
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
}

#[test]
fn adversary_w09_every_spelling_of_the_defaults_loads_at_the_clients_values() {
    for tail in [
        "",
        "      output_streaming: true\n",
        "      output_streaming: True\n",
        "      output_streaming: *t\n",
        "      input_during_output: false\n",
        "      input_during_output: FALSE\n",
        "      output_streaming: true\n      input_during_output: false\n",
        "      input_during_output: false\n      output_streaming: true\n",
    ] {
        let routes = Routes::parse(&document(tail)).unwrap_or_else(|error| {
            panic!("{tail}: {}", error.message);
        });
        let mut seen = 0;
        for (_, binding) in routes.catalog().bindings() {
            seen += 1;
            assert!(binding.capabilities().output_streaming, "{tail}");
            assert!(!binding.capabilities().input_during_output, "{tail}");
        }
        assert_eq!(seen, 1, "{tail}");
    }
}

#[test]
fn adversary_w09_a_capabilities_mapping_shared_by_alias_is_refused_at_its_first_route() {
    let source = "format: llm.routes/1
endpoints:
  p: {base_url: http://127.0.0.1:8000/v1, protocol: responses, auth: {kind: anonymous}}
routes:
  a: {model: p/m, capabilities: &c {tools: true, tool_choice: true, temperature: true,
      top_p: true, reasoning_efforts: [], context_window: 32768, max_output_tokens: 8192,
      input_during_output: true}}
  b: {model: p/n, capabilities: *c}
";
    let error = Routes::parse(source).map(|_| ()).unwrap_err();
    assert_eq!(
        error.message,
        "route `a`: no protocol in this workspace carries input during output"
    );
}
