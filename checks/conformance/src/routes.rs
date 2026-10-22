//! Routes document conformance observations.
//!
//! This module parses the authored `llm.routes/1` document with the real
//! `llm_models::routes::Routes`, answers each probed name, and reports the expanded catalog. For
//! an authentication it writes the scenario's file secrets into a private fixture home, builds
//! the document's resolvers over that home, takes the named route's binding and prepares its
//! credential as a turn would before it sends anything. It reads no suite and branches on no
//! scenario name, and it changes no environment variable.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path},
    sync::Arc,
};

use ess_conformance::target::TargetError;
use llm_core::{BoxFuture, Cancel, Capabilities, Error, Id};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_models::{
    Resolvers,
    routes::{Routes, SecretKind, SecretSources},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.routes.LastLoad", "llm.routes.LastAuthentication"];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.routes.Load" => Some(load(input)),
        "llm.routes.Authenticate" => Some(authenticate(input)),
        _ => None,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("routes observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadInput {
    routes_yaml: String,
    probe_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthenticateInput {
    routes_yaml: String,
    name: String,
    file_secrets: Vec<String>,
}

/// The kebab-case wire label a vocabulary value serializes as.
fn label(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".to_owned())
}

fn refuse(facts: &mut Value, error: &Error) {
    facts["error_code"] = json!(error.code);
    facts["error_message"] = json!(error.message);
}

fn capabilities(route: &Id, capabilities: &Capabilities) -> String {
    format!(
        "{route}=tools:{},tool_choice:{},temperature:{},top_p:{},efforts:{},context:{},output:{},\
output_streaming:{},input_during_output:{}",
        capabilities.tools,
        capabilities.tool_choice,
        capabilities.temperature,
        capabilities.top_p,
        capabilities.reasoning_efforts.join("|"),
        capabilities.context_window,
        capabilities.max_output_tokens,
        capabilities.output_streaming,
        capabilities.input_during_output,
    )
}

fn load(input: &Value) -> Result<Observed, TargetError> {
    let input: LoadInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "loaded": false, "error_code": null, "error_message": null, "routes": [],
        "endpoints": [], "answers": [], "catalog_serving_models": [], "capabilities": [],
        "bedrock_endpoints": [], "aliases": []
    });
    match Routes::parse(&input.routes_yaml) {
        Ok(routes) => {
            facts["loaded"] = json!(true);
            facts["routes"] = json!(
                routes
                    .routes()
                    .map(|route| format!(
                        "{}={}/{}",
                        route.name(),
                        route.endpoint(),
                        route.upstream_model()
                    ))
                    .collect::<Vec<_>>()
            );
            facts["endpoints"] = json!(
                routes
                    .endpoints()
                    .map(|endpoint| format!(
                        "{}:{}:{}:{}",
                        endpoint.name(),
                        label(endpoint.protocol()),
                        label(endpoint.auth_kind()),
                        endpoint.secret_kind().map_or("none", SecretKind::as_str)
                    ))
                    .collect::<Vec<_>>()
            );
            facts["answers"] = json!(
                input
                    .probe_names
                    .iter()
                    .map(|name| match routes.target(name) {
                        Ok(target) => format!(
                            "{name}={}:{}:{}",
                            target.endpoint,
                            label(target.protocol),
                            target.upstream_model
                        ),
                        Err(error) => format!("{name}!{}", label(error.code)),
                    })
                    .collect::<Vec<_>>()
            );
            facts["catalog_serving_models"] = json!(
                routes
                    .catalog()
                    .bindings()
                    .map(|(id, _)| id.as_str())
                    .collect::<Vec<_>>()
            );
            facts["capabilities"] = json!(
                routes
                    .catalog()
                    .bindings()
                    .map(|(id, binding)| capabilities(id, binding.capabilities()))
                    .collect::<Vec<_>>()
            );
            // `<endpoint>:<protocol>:<region>:<aws-keys or aws-container>`, in name order.
            facts["bedrock_endpoints"] = json!(
                routes
                    .bedrock_endpoints()
                    .map(|endpoint| format!(
                        "{}:{}:{}:{}",
                        endpoint.name(),
                        label(endpoint.protocol()),
                        endpoint.region(),
                        endpoint.credential_kind().as_str()
                    ))
                    .collect::<Vec<_>>()
            );
            // `<alias>=<route>`, routes in name order and each route's aliases in document order.
            facts["aliases"] = json!(
                routes
                    .routes()
                    .flat_map(|route| route
                        .aliases()
                        .iter()
                        .map(move |alias| format!("{alias}={}", route.name())))
                    .collect::<Vec<_>>()
            );
        }
        Err(error) => refuse(&mut facts, &error),
    }
    Ok(Observed {
        facts,
        view: "llm.routes.LastLoad",
        event: "llm.routes.Loaded",
        field: "loaded",
    })
}

/// Handed to an anonymous binding, which never asks for a secret.
struct NoSecret;

impl SecretResolver for NoSecret {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::InvalidReference) })
    }
}

fn private_directory(path: &Path) -> Result<(), TargetError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(unavailable)
}

/// Writes each `<relative path>=<content>` below `home`, directories 0700 and files 0600, as a
/// protected secret file is kept.
fn write_secrets(home: &Path, secrets: &[String]) -> Result<(), TargetError> {
    for secret in secrets {
        let (relative, content) = secret
            .split_once('=')
            .ok_or_else(|| unavailable("a file secret must read <relative path>=<content>"))?;
        let relative = Path::new(relative);
        if !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(unavailable("a file secret path must stay below the home"));
        }
        let mut directory = home.to_path_buf();
        if let Some(parent) = relative.parent() {
            for component in parent.components() {
                directory.push(component);
                if !directory.is_dir() {
                    fs::create_dir(&directory).map_err(unavailable)?;
                }
                private_directory(&directory)?;
            }
        }
        let path = home.join(relative);
        fs::write(&path, content).map_err(unavailable)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(unavailable)?;
    }
    Ok(())
}

fn authenticate(input: &Value) -> Result<Observed, TargetError> {
    let input: AuthenticateInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "prepared": false, "error_code": null, "error_message": null, "header": null
    });
    let scratch = Path::new("target/conformance");
    fs::create_dir_all(scratch).map_err(unavailable)?;
    let fixture = tempfile::tempdir_in(scratch).map_err(unavailable)?;
    let home = fixture.path().canonicalize().map_err(unavailable)?;
    private_directory(&home)?;
    write_secrets(&home, &input.file_secrets)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(unavailable)?;
    let result = runtime.block_on(async {
        let routes = Routes::parse(&input.routes_yaml)?;
        let resolvers = routes.resolvers(&SecretSources::new().with_home(&home))?;
        let binding = routes.binding(&input.name)?;
        let resolver = resolvers
            .resolver(&binding.declaration().account.id)
            .unwrap_or_else(|| Arc::new(NoSecret));
        binding
            .prepare_auth(resolver.as_ref(), &Cancel::new())
            .await
    });
    match result {
        Ok(prepared) => {
            facts["prepared"] = json!(true);
            let (headers, _) = prepared.into_parts();
            // A prepared credential travels in exactly one header; anonymous access in none.
            if headers.len() > 1 {
                return Err(unavailable(
                    "a prepared credential carries more than one header",
                ));
            }
            facts["header"] = json!(headers.keys().next().map(AsRef::<str>::as_ref));
        }
        Err(error) => refuse(&mut facts, &error),
    }
    drop(fixture);
    Ok(Observed {
        facts,
        view: "llm.routes.LastAuthentication",
        event: "llm.routes.Authenticated",
        field: "prepared",
    })
}
