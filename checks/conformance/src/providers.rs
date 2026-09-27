//! Provider account and binding conformance observations.
//!
//! This module parses the authored `BindingDocument` with the real `b10x-llm-providers` types,
//! binds it, and calls `Binding::prepare_auth` with an injected in-process resolver whose answer
//! the fixture chooses. It reports what the crate returned and never the credential value: only
//! whether the header is the declared presentation of the fixture material. It reads no suite
//! and branches on no scenario name.

use std::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use ess_conformance::target::TargetError;
use llm_core::{BoxFuture, Cancel, Error};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::{ApiKeyHeader, BaseUrl, BindingDocument};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::{Observed, token_bound};

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.providers.LastPreparation"];

const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
/// The largest fixture material this adapter will build.
const MAX_FIXTURE_SECRET_BYTES: u64 = 1024 * 1024;
/// A byte that is printable US-ASCII and cannot occur in any diagnostic this crate writes.
const FILL: u8 = b'Q';

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    if command != "llm.providers.Prepare" {
        return None;
    }
    Some(exercise(input))
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("providers observation", error.to_string())
}

#[derive(Deserialize, Clone, Copy)]
enum SecretFailure {
    InvalidReference,
    Unavailable,
    Missing,
    Expired,
    RefreshUnsupported,
    RefreshRejected,
    RefreshUncertain,
    TooLarge,
    TooManyReferences,
    UnsafeSource,
    UnsupportedPlatform,
}

impl SecretFailure {
    const fn error(self) -> SecretError {
        match self {
            Self::InvalidReference => SecretError::InvalidReference,
            Self::Unavailable => SecretError::Unavailable,
            Self::Missing => SecretError::Missing,
            Self::Expired => SecretError::Expired,
            Self::RefreshUnsupported => SecretError::RefreshUnsupported,
            Self::RefreshRejected => SecretError::RefreshRejected,
            Self::RefreshUncertain => SecretError::RefreshUncertain,
            Self::TooLarge => SecretError::TooLarge,
            Self::TooManyReferences => SecretError::TooManyReferences,
            Self::UnsafeSource => SecretError::UnsafeSource,
            Self::UnsupportedPlatform => SecretError::UnsupportedPlatform,
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
enum CancelPoint {
    Never,
    BeforeCall,
    DuringResolve,
    AfterResolve,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    binding_json: String,
    #[serde(default)]
    secret_error: Option<SecretFailure>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default, deserialize_with = "token_bound")]
    secret_length: Option<u64>,
    cancel: CancelPoint,
}

/// Answers exactly what the fixture chose, counting every call. It never refreshes anything.
struct Scripted {
    answer: Result<Vec<u8>, SecretError>,
    cancel: Cancel,
    point: CancelPoint,
    resolves: AtomicUsize,
    refreshes: AtomicUsize,
}

impl SecretResolver for Scripted {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.resolves.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match self.point {
                CancelPoint::DuringResolve => {
                    self.cancel.cancel();
                    std::future::pending::<()>().await;
                }
                CancelPoint::AfterResolve => self.cancel.cancel(),
                CancelPoint::Never | CancelPoint::BeforeCall => {}
            }
            let material = self.answer.clone()?;
            Ok(ResolvedSecret {
                secret: Secret::new(material)?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }

    fn refresh<'a>(
        &'a self,
        _reference: &'a SecretRef,
        _rejected: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(SecretError::RefreshUnsupported) })
    }
}

fn exercise(input: &Value) -> Result<Observed, TargetError> {
    let request: Prepare = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: run(&request),
        view: "llm.providers.LastPreparation",
        event: "llm.providers.Prepared",
        field: "diagnostics_safe",
    })
}

fn code(error: &Error) -> Value {
    serde_json::to_value(error.code).unwrap_or(Value::Null)
}

/// Each validated field of a document that did not parse, through the crate's own constructor.
fn field_errors(binding_json: &str) -> Vec<String> {
    let Ok(document) = serde_json::from_str::<Value>(binding_json) else {
        return Vec::new();
    };
    let mut errors = Vec::new();
    if let Some(url) = document["endpoint"]["base_url"].as_str()
        && let Err(error) = BaseUrl::new(url)
    {
        errors.push(format!("endpoint.base_url: {}", error.message));
    }
    if let Some(header) = document["account"]["api_key_header"].as_str()
        && let Err(error) = ApiKeyHeader::new(header)
    {
        errors.push(format!("account.api_key_header: {}", error.message));
    }
    errors
}

fn material(request: &Prepare) -> Result<Vec<u8>, SecretError> {
    if let Some(failure) = request.secret_error {
        return Err(failure.error());
    }
    if let Some(secret) = &request.secret {
        return Ok(secret.as_bytes().to_vec());
    }
    let length = request
        .secret_length
        .unwrap_or(0)
        .min(MAX_FIXTURE_SECRET_BYTES);
    Ok(vec![FILL; usize::try_from(length).unwrap_or(0)])
}

fn run(request: &Prepare) -> Value {
    let mut facts = json!({
        "bound": false, "error_code": null, "error_message": null, "field_errors": [],
        "request_url": null, "provenance": null, "upstream_model": null, "prepared": false,
        "header_count": null, "header_name": null, "header_scheme": null,
        "header_matches": null, "header_sensitive": null, "generation_present": null,
        "resolver_calls": 0, "refresh_calls": 0, "diagnostics_safe": true
    });
    let answer = material(request);
    let cancel = Cancel::new();
    if request.cancel == CancelPoint::BeforeCall {
        cancel.cancel();
    }
    let resolver = Scripted {
        answer: answer.clone(),
        cancel: cancel.clone(),
        point: request.cancel,
        resolves: AtomicUsize::new(0),
        refreshes: AtomicUsize::new(0),
    };
    let mut diagnostics = String::new();
    prepare(request, &resolver, &cancel, &mut facts, &mut diagnostics);
    facts["resolver_calls"] = json!(resolver.resolves.load(Ordering::SeqCst));
    facts["refresh_calls"] = json!(resolver.refreshes.load(Ordering::SeqCst));
    // Fixture material shorter than four bytes cannot be told apart from ordinary text.
    let exposed = answer
        .as_ref()
        .ok()
        .filter(|material| material.len() >= 4)
        .is_some_and(|material| {
            diagnostics
                .as_bytes()
                .windows(material.len())
                .any(|window| window == material.as_slice())
        });
    facts["diagnostics_safe"] = json!(!exposed);
    facts
}

fn prepare(
    request: &Prepare,
    resolver: &Scripted,
    cancel: &Cancel,
    facts: &mut Value,
    diagnostics: &mut String,
) {
    if request.binding_json.len() > MAX_DOCUMENT_BYTES {
        facts["error_code"] = json!("invalid-document");
        return;
    }
    let Ok(document) = serde_json::from_str::<BindingDocument>(&request.binding_json) else {
        facts["error_code"] = json!("invalid-document");
        facts["field_errors"] = json!(field_errors(&request.binding_json));
        return;
    };
    let binding = match document.bind() {
        Ok(binding) => binding,
        Err(error) => {
            facts["error_code"] = code(&error);
            facts["error_message"] = json!(error.message);
            return;
        }
    };
    facts["bound"] = json!(true);
    facts["request_url"] = json!(binding.request_url());
    let provenance = binding.provenance();
    facts["provenance"] = json!(format!(
        "{}/{}/{}/{}/{}",
        serde_json::to_value(provenance.protocol)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        provenance.provider.as_str(),
        provenance.account.as_str(),
        provenance.endpoint.as_str(),
        provenance.model.as_str()
    ));
    facts["upstream_model"] = json!(binding.upstream_model());
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        facts["error_code"] = json!("fixture:no-runtime");
        return;
    };
    let prepared = runtime.block_on(binding.prepare_auth(resolver, cancel));
    *diagnostics = format!("{prepared:?}");
    match prepared {
        Err(error) => {
            diagnostics.push_str(&error.to_string());
            facts["error_code"] = code(&error);
            facts["error_message"] = json!(error.message);
        }
        Ok(prepared) => {
            facts["prepared"] = json!(true);
            let (headers, generation) = prepared.into_parts();
            facts["header_count"] = json!(headers.len());
            facts["generation_present"] = json!(generation.is_some());
            let _rendered = write!(diagnostics, "{headers:?}");
            if let Some((name, value)) = headers.iter().next() {
                let bytes = value.as_bytes();
                let bearer = bytes.starts_with(b"Bearer ");
                let presented = if bearer { &bytes[7..] } else { bytes };
                facts["header_name"] = json!(name.as_str());
                facts["header_scheme"] = json!(if bearer { "Bearer" } else { "raw" });
                facts["header_matches"] = json!(
                    resolver
                        .answer
                        .as_deref()
                        .is_ok_and(|material| material == presented)
                );
                facts["header_sensitive"] = json!(value.is_sensitive());
            }
        }
    }
}
