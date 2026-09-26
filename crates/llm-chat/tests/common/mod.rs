//! Fixture bindings shared by the Chat Completions tests. No network or credential source.
// Each integration test binary compiles this module and uses only part of it.
#![allow(dead_code)]
use llm_core::{AuthKind, BillingKind, Capabilities, Id, Protocol};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};

pub fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

pub fn capable() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["low".to_owned(), "high".to_owned()],
        context_window: 32_768,
        max_output_tokens: 4_096,
    }
}

/// An explicitly anonymous, self-hosted binding: the shape a local vLLM deployment declares.
pub fn binding_at(protocol: Protocol, capabilities: Capabilities, base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("self-hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Anonymous,
            billing_kind: BillingKind::SelfHosted,
            secret_reference_id: None,
            api_key_header: None,
        },
        Endpoint {
            id: id("local-vllm"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture endpoint"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("Qwen/Qwen3-8B"),
        },
        ServingModel {
            id: id("local-small"),
            endpoint_id: id("local-vllm"),
            model_id: id("small"),
            protocol,
            capabilities,
        },
    )
    .bind()
    .expect("fixture binding")
}

pub fn binding() -> Binding {
    binding_at(
        Protocol::ChatCompletions,
        capable(),
        "http://127.0.0.1:8000/v1",
    )
}
