//! Adversarial cases for `llm.routes/1`: the byte bound a routes document is held to.

use llm_core::ErrorCode;
use llm_models::routes::Routes;
use llm_routing::MAX_CONFIG_BYTES;
use std::fmt::Write as _;

const CHILD: &str = "LLM_ROUTES_ADVERSARY_CHILD";
const CHILD_RAN: &str = "llm-routes-adversary-child-ran";

/// The process's peak resident set size in bytes, from `/proc/self/status`.
#[cfg(target_os = "linux")]
fn peak_resident_bytes() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .unwrap();
    let kib: usize = line
        .trim_start_matches("VmHWM:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .unwrap();
    kib * 1024
}

/// A document far below `MAX_CONFIG_BYTES` uses YAML anchors and aliases to repeat one 64 KiB
/// reasoning-effort name 2048 times. The byte bound exists to bound what a document costs to
/// read; parsing it must not hold more than a small multiple of that bound, whether it is then
/// refused or accepted. Measured in a child run of this binary so the peak is this parse's own.
#[cfg(target_os = "linux")]
#[test]
fn yaml_aliases_cannot_expand_a_document_past_its_byte_bound_while_it_is_parsed() {
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "yaml_aliases_cannot_expand_a_document_past_its_byte_bound_while_it_is_parsed",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains(CHILD_RAN), "{stdout}");
        return;
    }
    let anchored = "a".repeat(64 * 1024);
    let aliases = vec!["*e"; 2048].join(", ");
    let document = format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
  r:
    model: p/m
    capabilities: {{tools: true, tool_choice: true, temperature: true, top_p: true,
                   context_window: 8192, max_output_tokens: 1024,
                   reasoning_efforts: [&e {anchored}, {aliases}]}}
"
    );
    assert!(document.len() < MAX_CONFIG_BYTES / 16, "{}", document.len());
    let before = peak_resident_bytes();
    let outcome = Routes::parse(&document)
        .map(|routes| routes.routes().count())
        .map_err(|error| (error.code, error.message));
    let grown = peak_resident_bytes().saturating_sub(before);
    assert!(
        grown <= 8 * MAX_CONFIG_BYTES,
        "parsing a {} byte document raised peak RSS by {grown} bytes ({} MiB); outcome {outcome:?}",
        document.len(),
        grown / (1024 * 1024)
    );
    println!("{CHILD_RAN}");
}

/// A document of 3500 plain routes with 32-byte names is a fraction of `MAX_CONFIG_BYTES`, but
/// its expansion (a served model, serving model, route and target per route) exceeds the
/// catalog's own byte bound. The bound is kept, and the refusal names the document rather than
/// the catalog it expands into.
#[test]
fn a_document_whose_expansion_exceeds_the_catalog_bound_is_refused_as_too_large_by_name() {
    let mut document = String::from(
        "format: llm.routes/1
endpoints:
  p: {base_url: https://p.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}
routes:
",
    );
    for index in 0..3500 {
        writeln!(document, "  route-{index:0>26}: p/m").unwrap();
    }
    assert!(document.len() < MAX_CONFIG_BYTES / 8, "{}", document.len());
    let outcome = Routes::parse(&document)
        .map(|routes| routes.routes().count())
        .map_err(|error| (error.code, error.message));
    assert_eq!(
        outcome,
        Err((
            ErrorCode::TooLarge,
            "llm.routes/1 document expands beyond the catalog's bound".to_owned()
        )),
        "story:routes-document keeps the catalog's expansion bound for a document of 3500 routes; \
         if the expansion bound is raised, assert acceptance instead"
    );
}
