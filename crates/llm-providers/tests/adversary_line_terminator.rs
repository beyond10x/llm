//! Adversary cases for the one-trailing-terminator rule of `prepare_auth` (wave 2026-10-05-w22,
//! pass 1). spec/domains/providers.yaml: the declared presentation is the material without one
//! trailing `\n` or `\r\n`; a second terminator, a terminator anywhere else, or any other byte
//! outside printable US-ASCII is refused as `unauthorized`.
use http::header::AUTHORIZATION;
use llm_core::{BoxFuture, Cancel, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::{Binding, BindingDocument};

const BEARER: &str = r#"{"format":"llm.binding/1","provider":{"id":"my-lab","category":"private-compute"},"account":{"id":"research","provider_id":"my-lab","auth_kind":"bearer","billing_kind":"metered","secret_reference_id":"my-opaque-reference"},"endpoint":{"id":"gpu-7","account_id":"research","base_url":"http://127.0.0.1:8000/proxy/v1"},"model":{"id":"code","upstream_name":"custom-org/CustomModel-27B:revision-4"},"serving":{"id":"local-code","endpoint_id":"gpu-7","model_id":"code","protocol":"chat-completions","capabilities":{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":[],"context_window":32768,"max_output_tokens":8192}}}"#;

fn binding() -> Binding {
    serde_json::from_str::<BindingDocument>(BEARER)
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
async fn every_terminator_combination_but_one_trailing_is_refused() {
    for material in [
        &b"llm-fixture-token\r\n\n"[..],
        b"llm-fixture-token\n\r\n",
        b"llm-fixture-token\r\r\n",
        b"llm-fixture-token\n\r",
        b"llm-fixture-token\x0b",
        b"llm-fixture-token\x0c",
        b"llm-fixture-token\t\n",
        b"llm-fixture-token\0\n",
        b"llm-fixture-token\xc2\x85",
        b"\r\n\r\n",
        b"\n\n",
        b"\r",
        b"",
    ] {
        let error = binding()
            .prepare_auth(&Material(material.to_vec()), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{material:?}");
        assert_eq!(
            error.message, "credential is not a bounded HTTP token",
            "{material:?}"
        );
    }
}

// The 16 KiB bound is on the presented token: a 16 KiB token written with `echo` is 16 KiB + 1.
#[tokio::test]
async fn the_size_bound_applies_to_the_presented_token() {
    let mut token = vec![b'x'; 16 * 1024];
    token.push(b'\n');
    let (headers, _) = binding()
        .prepare_auth(&Material(token), &Cancel::new())
        .await
        .unwrap()
        .into_parts();
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap().len(),
        "Bearer ".len() + 16 * 1024
    );
    let mut token = vec![b'x'; 16 * 1024 + 1];
    token.extend_from_slice(b"\r\n");
    let error = binding()
        .prepare_auth(&Material(token), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unauthorized);
}
