//! Provider descriptions: the shipped Runpod description, instance and template refusals, and the
//! document rules of `llm.provider-description/1`. No I/O is involved.

use llm_core::{AuthKind, ErrorCode, Protocol};
use llm_providers::{ProviderDescription, ProviderDescriptionDocument, descriptions};

const DIGEST: &str = "9500a8989878d53d8731f27bf8dbbd57801b328c760bdb32c38ba36d5cb580db";

/// A complete document whose inference table and control plane the caller supplies.
fn document(inference: &str, control_plane: &str) -> String {
    format!(
        "format = \"llm.provider-description/1\"\n\n[provider]\nid = \"my-cloud\"\ncategory = \"gpu-cloud\"\n\n[inference]\n{inference}\n{control_plane}"
    )
}

fn inference(template: &str) -> String {
    format!(
        "base_url_template = \"{template}\"\nprotocols = [\"chat-completions\"]\nauth_kind = \"bearer\""
    )
}

fn control_plane(
    openapi_url: &str,
    digest: &str,
    server_url: &str,
    operations: [&str; 4],
) -> String {
    let [create, list, get, delete] = operations;
    format!(
        "[control_plane]\nopenapi_url = \"{openapi_url}\"\ndocument_sha256 = \"{digest}\"\nserver_url = \"{server_url}\"\nauth_kind = \"bearer\"\n\n[control_plane.operations]\ncreate_instance = \"{create}\"\nlist_instances = \"{list}\"\nget_instance = \"{get}\"\ndelete_instance = \"{delete}\"\n"
    )
}

fn plane() -> String {
    control_plane(
        "https://api.example.test/openapi.json",
        DIGEST,
        "https://api.example.test/v1",
        ["Create", "List", "Get", "Delete"],
    )
}

fn refused(source: &str, message: &str) {
    let error = ProviderDescription::parse(source).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.message, message);
}

const TEMPLATE_REFUSAL: &str = "base URL template must hold {instance} exactly once, inside the host's first label, and no other brace";
const DOCUMENT_REFUSAL: &str = "invalid llm.provider-description/1 TOML document";

#[test]
fn the_shipped_runpod_description_is_accepted_with_every_declared_value() {
    let runpod = descriptions::runpod();
    assert_eq!(runpod.provider().id.as_str(), "runpod");
    assert_eq!(runpod.provider().category.as_str(), "gpu-cloud");
    let inference = runpod.inference();
    assert_eq!(
        inference.base_url_template(),
        "https://{instance}-8000.proxy.runpod.net/v1/"
    );
    assert_eq!(
        inference.protocols(),
        [
            Protocol::ChatCompletions,
            Protocol::Messages,
            Protocol::Responses
        ]
    );
    assert_eq!(inference.auth_kind(), AuthKind::Bearer);
    assert!(inference.api_key_header().is_none());
    let plane = runpod.control_plane().unwrap();
    assert_eq!(
        plane.openapi_url(),
        "https://rest.runpod.io/v1/openapi.json"
    );
    assert_eq!(plane.document_sha256(), DIGEST);
    assert_eq!(plane.server_url(), "https://rest.runpod.io/v1");
    assert_eq!(plane.auth_kind(), AuthKind::Bearer);
    assert_eq!(
        plane.operations().roles(),
        [
            ("create_instance", "CreatePod"),
            ("list_instances", "ListPods"),
            ("get_instance", "GetPod"),
            ("delete_instance", "DeletePod"),
        ]
    );
    assert!(descriptions::by_name("runpod").is_some());
    assert!(descriptions::by_name("elsewhere").is_none());
}

#[test]
fn a_runpod_instance_resolves_to_its_proxy_url() {
    let url = descriptions::runpod()
        .inference_base_url("abc123xyz")
        .unwrap();
    assert_eq!(url.as_str(), "https://abc123xyz-8000.proxy.runpod.net/v1/");
    let longest = "a".repeat(48);
    assert_eq!(
        descriptions::runpod()
            .inference_base_url(&longest)
            .unwrap()
            .as_str(),
        format!("https://{longest}-8000.proxy.runpod.net/v1/")
    );
}

#[test]
fn an_instance_that_could_change_the_url_is_refused_before_substitution() {
    let runpod = descriptions::runpod();
    let oversized = "a".repeat(49);
    for instance in [
        "abc.evil",
        "abc/evil",
        "Abc",
        "",
        oversized.as_str(),
        "abc-def",
        "abc@evil",
    ] {
        let error = runpod.inference_base_url(instance).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "instance must be 1-48 bytes of lower-case ASCII letters and digits"
        );
    }
}

#[test]
fn a_template_without_the_placeholder_twice_or_outside_the_host_is_refused() {
    for template in [
        "https://pod-8000.proxy.example.test/v1/",
        "https://{instance}-{instance}.proxy.example.test/v1/",
        "https://proxy.example.test/{instance}/v1/",
        "https://pods.{instance}.example.test/v1/",
        "{instance}://proxy.example.test/v1/",
        "https://{instance}-8000.proxy.example.test/{v1}/",
        "https://{pod}-8000.proxy.example.test/v1/",
    ] {
        refused(&document(&inference(template), ""), TEMPLATE_REFUSAL);
    }
}

#[test]
fn a_template_whose_host_is_only_the_instance_is_refused() {
    for template in [
        "https://{instance}/v1/",
        "https://{instance}:8000/v1/",
        "https://pod-{instance}",
    ] {
        refused(
            &document(&inference(template), ""),
            "base URL template must keep a fixed domain after the instance's label",
        );
    }
}

#[test]
fn a_template_that_forms_no_endpoint_url_is_refused() {
    refused(
        &document(&inference("ftp://{instance}.example.test/v1/"), ""),
        "base URL template does not form a valid endpoint URL",
    );
}

#[test]
fn a_description_without_a_control_plane_is_accepted() {
    let description = ProviderDescription::parse(&document(
        &inference("https://{instance}.example.test/v1"),
        "",
    ))
    .unwrap();
    assert!(description.control_plane().is_none());
    assert_eq!(
        description.inference_base_url("pod7").unwrap().as_str(),
        "https://pod7.example.test/v1/"
    );
}

#[test]
fn no_protocol_or_a_repeated_protocol_is_refused() {
    let template = "https://{instance}.example.test/v1/";
    refused(
        &document(
            &format!("base_url_template = \"{template}\"\nprotocols = []\nauth_kind = \"bearer\""),
            "",
        ),
        "provider description must name at least one protocol",
    );
    refused(
        &document(
            &format!(
                "base_url_template = \"{template}\"\nprotocols = [\"messages\", \"messages\"]\nauth_kind = \"bearer\""
            ),
            "",
        ),
        "provider description names a protocol twice",
    );
}

#[test]
fn api_key_without_a_header_and_subscription_oauth_are_refused() {
    let template = "https://{instance}.example.test/v1/";
    refused(
        &document(
            &format!(
                "base_url_template = \"{template}\"\nprotocols = [\"messages\"]\nauth_kind = \"api-key\""
            ),
            "",
        ),
        "exactly API-key authentication requires an API-key header",
    );
    refused(
        &document(
            &format!(
                "base_url_template = \"{template}\"\nprotocols = [\"messages\"]\nauth_kind = \"subscription-oauth\""
            ),
            "",
        ),
        "provider descriptions describe API access, not subscription OAuth",
    );
    let accepted = ProviderDescription::parse(&document(
        &format!(
            "base_url_template = \"{template}\"\nprotocols = [\"messages\"]\nauth_kind = \"api-key\"\napi_key_header = \"x-api-key\""
        ),
        "",
    ))
    .unwrap();
    assert_eq!(
        accepted.inference().api_key_header().unwrap().as_str(),
        "x-api-key"
    );
}

#[test]
fn control_plane_digest_scheme_and_operations_are_checked() {
    let inference = inference("https://{instance}.example.test/v1/");
    assert!(ProviderDescription::parse(&document(&inference, &plane())).is_ok());
    for digest in [DIGEST.to_uppercase().as_str(), &DIGEST[..63], "sha256:abc"] {
        refused(
            &document(
                &inference,
                &control_plane(
                    "https://api.example.test/openapi.json",
                    digest,
                    "https://api.example.test/v1",
                    ["Create", "List", "Get", "Delete"],
                ),
            ),
            "OpenAPI digest must be 64 lower-case hex digits",
        );
    }
    refused(
        &document(
            &inference,
            &control_plane(
                "http://api.example.test/openapi.json",
                DIGEST,
                "https://api.example.test/v1",
                ["Create", "List", "Get", "Delete"],
            ),
        ),
        "OpenAPI URL must be HTTPS with a host and no userinfo, query or fragment",
    );
    refused(
        &document(
            &inference,
            &control_plane(
                "https://api.example.test/openapi.json",
                DIGEST,
                "http://api.example.test/v1",
                ["Create", "List", "Get", "Delete"],
            ),
        ),
        "control-plane server must be HTTPS with a host and no userinfo, query or fragment",
    );
    refused(
        &document(
            &inference,
            &control_plane(
                "https://api.example.test/openapi.json",
                DIGEST,
                "https://api.example.test/v1",
                ["Create", "Get", "Get", "Delete"],
            ),
        ),
        "two control-plane roles name the same operation",
    );
    refused(
        &document(
            &inference,
            &control_plane(
                "https://api.example.test/openapi.json",
                DIGEST,
                "https://api.example.test/v1",
                ["Create pod", "List", "Get", "Delete"],
            ),
        ),
        "operation ID must be 1-128 bytes of ASCII letters, digits, '_', '.' or '-'",
    );
}

#[test]
fn an_unknown_field_an_unknown_format_or_an_oversized_document_is_not_a_description() {
    let template = inference("https://{instance}.example.test/v1/");
    let unknown_field = document(&format!("{template}\nregion = \"eu\""), "");
    assert_eq!(
        ProviderDescriptionDocument::parse(&unknown_field)
            .unwrap_err()
            .message,
        DOCUMENT_REFUSAL
    );
    let unknown_format =
        document(&template, "").replace("llm.provider-description/1", "llm.provider-description/2");
    refused(&unknown_format, DOCUMENT_REFUSAL);
    let oversized = format!("{}\n#{}", document(&template, ""), "x".repeat(64 * 1024));
    refused(&oversized, "provider description exceeds its byte bound");
}
