#![forbid(unsafe_code)]

//! Embeddings over an embed route (red skeleton).

use llm_calls::{CallIdentity, Recording};
use llm_core::{BoxFuture, Cancel, EmbedRequest, Embedder, Embeddings, Error};
use llm_credentials::SecretResolver;
use llm_http::HttpClient;
use llm_providers::Binding;
use llm_routing::Catalog;
use std::sync::Arc;

pub struct OpenAiEmbedder {
    _binding: Binding,
    _http: HttpClient,
    _resolver: Arc<dyn SecretResolver>,
}

impl OpenAiEmbedder {
    /// # Errors
    /// Not yet.
    pub fn for_route(
        catalog: &Catalog,
        alias: &str,
        http: HttpClient,
        resolver: Arc<dyn SecretResolver>,
    ) -> Result<Self, Error> {
        let selection = catalog.resolve_embed(alias)?;
        Ok(Self {
            _binding: selection.binding.clone(),
            _http: http,
            _resolver: resolver,
        })
    }

    /// # Errors
    /// Not yet.
    pub async fn embed_recorded(
        &self,
        _request: &EmbedRequest,
        _cancel: &Cancel,
        _identity: CallIdentity,
        _recording: Recording<'_>,
    ) -> Result<Embeddings, Error> {
        Err(Error::unsupported("not yet"))
    }
}

impl Embedder for OpenAiEmbedder {
    fn embed<'a>(
        &'a self,
        _request: &'a EmbedRequest,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<Embeddings, Error>> {
        Box::pin(async { Err(Error::unsupported("not yet")) })
    }
}
