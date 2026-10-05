//! The refusal that reaches the caller of `prepare_auth`, and so the error a model turn returns,
//! names the account's reference that refused (Harness names its source, C7), never where the
//! value lives or the value itself. A missing, expired or malformed credential is the caller's to
//! fix and is `Unauthorized`, which no fallback takes.
use llm_core::{BoxFuture, Cancel, Dispatch, ErrorCode};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_providers::{Binding, BindingDocument};

const BEARER: &str = r#"{"format":"llm.binding/1","provider":{"id":"my-lab","category":"private-compute"},"account":{"id":"research","provider_id":"my-lab","auth_kind":"bearer","billing_kind":"metered","secret_reference_id":"lab-login"},"endpoint":{"id":"gpu-7","account_id":"research","base_url":"http://127.0.0.1:8000/proxy/v1"},"model":{"id":"code","upstream_name":"custom-org/CustomModel-27B:revision-4"},"serving":{"id":"local-code","endpoint_id":"gpu-7","model_id":"code","protocol":"chat-completions","capabilities":{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":[],"context_window":32768,"max_output_tokens":8192}}}"#;

/// Where the value would live and what it would be; neither may reach a diagnostic.
const LOCATION_CANARY: &str = "/run/llm-secrets/location-canary";
const VALUE_CANARY: &str = "llm-fixture-private-marker";

fn binding() -> Binding {
    serde_json::from_str::<BindingDocument>(BEARER)
        .unwrap()
        .bind()
        .unwrap()
}

/// Refuses every reference with one kind; its own state holds both canaries.
#[derive(Debug)]
struct Refusing {
    error: SecretError,
    location: &'static str,
    value: &'static str,
}
impl SecretResolver for Refusing {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move { Err(self.error) })
    }
}

/// Every variant, classified without a wildcard: a variant added later does not compile here
/// until somebody decides whether it is the caller's to fix.
const fn class(error: SecretError) -> ErrorCode {
    match error {
        SecretError::Missing | SecretError::Expired | SecretError::Malformed => {
            ErrorCode::Unauthorized
        }
        SecretError::InvalidReference
        | SecretError::Unavailable
        | SecretError::RefreshUnsupported
        | SecretError::RefreshRejected
        | SecretError::RefreshUncertain
        | SecretError::TooLarge
        | SecretError::TooManyReferences
        | SecretError::UnsafeSource
        | SecretError::UnsupportedPlatform => ErrorCode::Unavailable,
    }
}

#[tokio::test]
async fn every_resolver_refusal_names_the_reference_and_nothing_else() {
    for error in [
        SecretError::InvalidReference,
        SecretError::Unavailable,
        SecretError::Missing,
        SecretError::Expired,
        SecretError::RefreshUnsupported,
        SecretError::RefreshRejected,
        SecretError::RefreshUncertain,
        SecretError::TooLarge,
        SecretError::TooManyReferences,
        SecretError::UnsafeSource,
        SecretError::UnsupportedPlatform,
        SecretError::Malformed,
    ] {
        let source = Refusing {
            error,
            location: LOCATION_CANARY,
            value: VALUE_CANARY,
        };
        let refusal = binding()
            .prepare_auth(&source, &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(refusal.code, class(error), "{error:?}");
        assert_eq!(refusal.dispatch, Dispatch::NotSent, "{error:?}");
        assert_eq!(
            refusal.message,
            format!("secret reference `lab-login` refused: {error}")
        );
        let rendered = format!("{refusal} {refusal:?}");
        assert!(!rendered.contains(source.location), "{rendered}");
        assert!(!rendered.contains(source.value), "{rendered}");
    }
}
