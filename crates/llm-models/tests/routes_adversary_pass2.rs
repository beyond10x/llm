//! Adversarial cases for `llm.routes/1`, second pass: the alias bound measured in memory rather
//! than in bytes, the refusal classification, and the direct-name boundary.

use llm_core::ErrorCode;
use llm_models::routes::Routes;
use llm_routing::MAX_CONFIG_BYTES;

const CHILD: &str = "LLM_ROUTES_ADVERSARY_PASS2_CHILD";
const CHILD_RAN: &str = "llm-routes-adversary-pass2-child-ran";

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

/// The pass-1 contract, held for a different alias shape: a document far below
/// `MAX_CONFIG_BYTES` must not raise peak memory past 8 x `MAX_CONFIG_BYTES` while it is parsed.
/// Here one anchored sequence of 2000 one-byte integers is aliased 2000 times inside one route
/// value. The expansion budget charges one byte per node, so 4 million nodes fit under it, but
/// the untagged route value is buffered node by node before its type is checked, and each
/// buffered node costs far more than one byte.
#[cfg(target_os = "linux")]
#[test]
fn aliases_to_a_sequence_cannot_make_a_small_document_cost_more_than_a_small_multiple_of_its_bound()
{
    let name = "aliases_to_a_sequence_cannot_make_a_small_document_cost_more_than_a_small_multiple_of_its_bound";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture", "--test-threads=1"])
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
    let anchored = vec!["0"; 2000].join(",");
    let aliases = vec!["*x"; 2000].join(",");
    let document = format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
  r: [&x [{anchored}], {aliases}]
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

/// Acceptance: an unknown endpoint field is refused as `invalid-request`. The refusal is
/// classified by searching the parser's message for an internal marker, and that message quotes
/// the unknown field's name, so a field whose name is the marker is refused as `too-large`.
#[test]
fn an_unknown_endpoint_field_is_refused_as_invalid_request_whatever_its_name() {
    let document = "format: llm.routes/1
endpoints:
  p:
    base_url: https://p.example.invalid/v1
    protocol: messages
    auth: {kind: anonymous}
    llm.routes expansion bound: 1
routes:
  r: p/m
";
    let error = Routes::parse(document).unwrap_err();
    assert_eq!(
        (error.code, error.message.contains("expansion bound")),
        (ErrorCode::InvalidRequest, false),
        "{}",
        error.message
    );
}

/// The module doc: a name that is not a route but reads `<declared endpoint>/<model-id>`
/// resolves directly, and the direct name is itself one `Id` of at most 256 bytes. A model id of
/// 255 bytes is valid (a route to it loads), but the direct name `p/<that id>` is 257 bytes and
/// is refused as neither a route nor a direct name; one of exactly 256 bytes resolves.
#[test]
fn a_direct_name_longer_than_one_id_is_refused_though_a_route_reaches_its_model() {
    let model = "m".repeat(255);
    let document = format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
  r: p/{model}
"
    );
    let routes = Routes::parse(&document).unwrap();
    assert_eq!(
        routes.target("r").unwrap().upstream_model.as_str(),
        model.as_str()
    );
    let direct = routes
        .target(&format!("p/{model}"))
        .map(|target| target.upstream_model.as_str().to_owned())
        .map_err(|error| (error.code, error.message));
    assert_eq!(
        direct,
        Err((
            ErrorCode::InvalidRequest,
            "the name is neither a route nor <declared endpoint>/<model-id>".to_owned()
        )),
        "story:routes-document decided that a direct name is one Id, at most 256 bytes; a model \
         id longer than 254 bytes is reached through a route"
    );
    let longest = "m".repeat(254);
    let direct = routes
        .target(&format!("p/{longest}"))
        .map(|target| {
            (
                target.endpoint.as_str().to_owned(),
                target.upstream_model.as_str().to_owned(),
            )
        })
        .map_err(|error| (error.code, error.message));
    assert_eq!(
        direct,
        Ok(("p".to_owned(), longest)),
        "story:routes-document: a direct name is one Id, at most 256 bytes, and one of exactly \
         256 bytes resolves"
    );
}
