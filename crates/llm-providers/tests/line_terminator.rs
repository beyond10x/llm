//! A token file written by an editor or `echo` ends in a newline (Harness trims it,
//! `harness-credential/src/oauth.rs:114`). llm keeps the bytes raw up to `prepare_auth`, which
//! presents the material without one trailing line terminator and refuses everything else
//! outside printable US-ASCII.
use http::header::AUTHORIZATION;
use llm_core::{BoxFuture, Cancel, Dispatch, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::{Binding, BindingDocument};

const BEARER: &str = r#"{"format":"llm.binding/1","provider":{"id":"my-lab","category":"private-compute"},"account":{"id":"research","provider_id":"my-lab","auth_kind":"bearer","billing_kind":"metered","secret_reference_id":"my-opaque-reference"},"endpoint":{"id":"gpu-7","account_id":"research","base_url":"http://127.0.0.1:8000/proxy/v1"},"model":{"id":"code","upstream_name":"custom-org/CustomModel-27B:revision-4"},"serving":{"id":"local-code","endpoint_id":"gpu-7","model_id":"code","protocol":"chat-completions","capabilities":{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":[],"context_window":32768,"max_output_tokens":8192}}}"#;

fn binding(json: &str) -> Binding {
    serde_json::from_str::<BindingDocument>(json)
        .unwrap()
        .bind()
        .unwrap()
}

struct Material(Vec<u8>);
impl SecretResolver for Material {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(self.0.clone())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

#[tokio::test]
async fn a_token_ending_in_one_line_terminator_is_presented_without_it() {
    for material in [&b"llm-fixture-token\n"[..], b"llm-fixture-token\r\n"] {
        let (headers, generation) = binding(BEARER)
            .prepare_auth(&Material(material.to_vec()), &Cancel::new())
            .await
            .unwrap()
            .into_parts();
        let value = headers.get(AUTHORIZATION).unwrap();
        assert_eq!(value.as_bytes(), b"Bearer llm-fixture-token");
        assert!(value.is_sensitive());
        assert!(generation.is_some());
    }
    let api_key = BEARER.replace(
        r#""auth_kind":"bearer","billing_kind":"metered","secret_reference_id":"my-opaque-reference""#,
        r#""auth_kind":"api-key","billing_kind":"metered","secret_reference_id":"my-opaque-reference","api_key_header":"X-Lab-Key""#,
    );
    let (headers, _) = binding(&api_key)
        .prepare_auth(&Material(b"llm-fixture-token\n".to_vec()), &Cancel::new())
        .await
        .unwrap()
        .into_parts();
    assert_eq!(
        headers.get("x-lab-key").unwrap().as_bytes(),
        b"llm-fixture-token"
    );
}

// Negative half: removing more than one terminator, or whitespace generally, would accept these.
#[tokio::test]
async fn only_one_trailing_line_terminator_is_removed() {
    for material in [
        &b"llm-fixture-token\n\n"[..],
        b"llm-fixture-token\r\n\r\n",
        b"llm-fixture-token\r",
        b"llm-fixture-token \n",
        b"llm-fixture-token\n ",
        b"\nllm-fixture-token",
        b"llm-fixture\ntoken",
        b"\n",
        b"\r\n",
    ] {
        let error = binding(BEARER)
            .prepare_auth(&Material(material.to_vec()), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{material:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent);
        assert_eq!(error.message, "credential is not a bounded HTTP token");
    }
}
