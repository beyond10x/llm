#![forbid(unsafe_code)]

//! Builds the single-attempt port a catalog serving model declares.
//!
//! The binding's protocol selects the Chat Completions, Responses or Messages client; the
//! binding's account selects the credential resolver through a lookup the caller supplies. An
//! anonymous account needs no resolver. A credentialed account without one is refused as
//! `unauthorized` before any I/O. Construction sends nothing and resolves no secret: resolution
//! stays at turn time, inside the client.
//!
//! A routes document's `bedrock` endpoint has no binding: [`routes::Routes::port`] builds
//! `llm-bedrock`'s `BedrockClient` for its routes, signing with the endpoint's AWS keys, with
//! the container credentials endpoint the caller supplies, with the credentials STS answers for
//! the web identity role the caller supplies ([`WebIdentityRole`]), or with the keys the
//! endpoint's `aws-process` command prints. That command runs only in a build with feature
//! `process` and for a caller that allows it
//! ([`routes::RouteResolvers::allow_credential_process`]).

pub mod routes;

use llm_bedrock::{
    BedrockClient, BedrockTarget, ContainerCredentials, CredentialSource, StaticCredentials,
    WebIdentityCredentials,
};
use llm_chat::ChatClient;
use llm_core::{
    AuthKind, BoxFuture, Cancel, Capabilities, Error, ErrorCode, Id, Model, Protocol, Provenance,
    StreamSink, TurnOutcome, TurnRequest,
};
use llm_credentials::{AnonymousResolver, SecretRef, SecretResolver};
use llm_http::HttpClient;
use llm_messages::MessagesClient;
use llm_providers::Binding;
use llm_responses::ResponsesClient;
use llm_routing::{Catalog, Models};
use std::{collections::BTreeMap, fmt, sync::Arc};

/// The role an `aws-web-identity` Bedrock endpoint assumes with STS `AssumeRoleWithWebIdentity`,
/// as the caller supplies it: on IAM roles for service accounts (IRSA), the role ARN is the value
/// of `AWS_ROLE_ARN`, and the STS endpoint the caller's regional one, such as
/// `https://sts.eu-central-1.amazonaws.com`. None of the three is a secret; the token is read
/// from its file at each fetch, never kept here.
///
/// Nothing is checked or sent when the role is made: building the endpoint's port checks the
/// three as `llm_bedrock::WebIdentityCredentials::new` does, and refuses as `unauthorized`. The
/// `Debug` form shows the role ARN and the session name, never the endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct WebIdentityRole {
    role_arn: String,
    session_name: String,
    sts_endpoint: String,
}

impl WebIdentityRole {
    /// The role `role_arn`, assumed as `session_name` at the STS `sts_endpoint`.
    pub fn new(
        role_arn: impl Into<String>,
        session_name: impl Into<String>,
        sts_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            role_arn: role_arn.into(),
            session_name: session_name.into(),
            sts_endpoint: sts_endpoint.into(),
        }
    }

    pub fn role_arn(&self) -> &str {
        &self.role_arn
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    /// The STS endpoint, as the caller wrote it.
    pub fn sts_endpoint(&self) -> &str {
        &self.sts_endpoint
    }
}

impl fmt::Debug for WebIdentityRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebIdentityRole")
            .field("role_arn", &self.role_arn)
            .field("session_name", &self.session_name)
            .finish_non_exhaustive()
    }
}

/// The caller's credential resolvers, looked up by account id.
///
/// Implemented for a map from account id to resolver and for any closure of that shape.
pub trait Resolvers {
    /// The resolver for this account, or `None` when the caller has none for it.
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>>;

    /// The container credentials endpoint (`AWS_CONTAINER_CREDENTIALS_FULL_URI`) an
    /// `aws-container` Bedrock endpoint fetches its credentials from, when the caller supplies
    /// one. None by default.
    fn container_credentials_uri(&self) -> Option<&str> {
        None
    }

    /// The role an `aws-web-identity` Bedrock endpoint assumes, when the caller supplies one.
    /// None by default.
    fn web_identity_role(&self) -> Option<&WebIdentityRole> {
        None
    }

    /// Why this account has no resolver, when the caller knows: a port for it is refused
    /// `unauthorized` with this message instead of the generic one. It must echo no secret
    /// reference's variable, path or entry. None by default.
    fn unavailable(&self, _account_id: &Id) -> Option<&str> {
        None
    }

    /// Whether an `aws-process` Bedrock endpoint may run the command its routes document
    /// names. False by default: such a port is refused `unauthorized` and nothing runs.
    fn credential_process_allowed(&self) -> bool {
        false
    }
}

impl Resolvers for BTreeMap<Id, Arc<dyn SecretResolver>> {
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self.get(account_id).cloned()
    }
}

impl<F> Resolvers for F
where
    F: Fn(&Id) -> Option<Arc<dyn SecretResolver>>,
{
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self(account_id)
    }
}

enum Client {
    Chat(ChatClient),
    Responses(ResponsesClient),
    Messages(MessagesClient),
    Bedrock(BedrockClient),
}

/// The secret references a Bedrock endpoint's credentials resolve through, under its account.
pub(crate) enum AwsReferences {
    Keys {
        account: Id,
        access_key_id: SecretRef,
        secret_access_key: SecretRef,
        session_token: Option<SecretRef>,
    },
    Container {
        account: Id,
        token: SecretRef,
    },
    WebIdentity {
        account: Id,
        token: SecretRef,
    },
    /// No reference: the command prints the keys. `command` is the program, then its arguments.
    Process {
        endpoint: Id,
        command: Vec<String>,
    },
}

/// An `aws-process` endpoint's credentials: the command's, with every failure naming the
/// endpoint before the program.
#[cfg(feature = "process")]
struct EndpointProcess {
    endpoint: Id,
    source: llm_bedrock::ProcessCredentials,
}

#[cfg(feature = "process")]
impl CredentialSource for EndpointProcess {
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<llm_bedrock::AwsCredentials, Error>> {
        Box::pin(async move {
            self.source.credentials(cancel).await.map_err(|mut error| {
                if error.code != ErrorCode::Cancelled {
                    error.message = format!("endpoint `{}`: {}", self.endpoint, error.message);
                }
                error
            })
        })
    }
}

/// The credential source of an `aws-process` endpoint, when the build and the caller allow its
/// command to run.
fn process_credentials(
    endpoint: &Id,
    command: Vec<String>,
    resolvers: &dyn Resolvers,
) -> Result<Arc<dyn CredentialSource>, Error> {
    if !resolvers.credential_process_allowed() {
        return Err(Error::new(
            ErrorCode::Unauthorized,
            format!(
                "endpoint `{endpoint}`: an aws-process endpoint runs a command, and the caller \
                 does not allow credential processes"
            ),
        ));
    }
    #[cfg(feature = "process")]
    {
        let mut parts = command.into_iter();
        let program = parts.next().unwrap_or_default();
        let source =
            llm_bedrock::ProcessCredentials::new(program, parts).map_err(|mut error| {
                error.message = format!("endpoint `{endpoint}`: {}", error.message);
                error
            })?;
        Ok(Arc::new(EndpointProcess {
            endpoint: endpoint.clone(),
            source,
        }))
    }
    #[cfg(not(feature = "process"))]
    {
        drop(command);
        Err(Error::new(
            ErrorCode::Unauthorized,
            format!(
                "endpoint `{endpoint}`: an aws-process endpoint needs feature `process` of \
                 llm-models, which this build does not enable"
            ),
        ))
    }
}

fn unresolved(resolvers: &dyn Resolvers, account: &Id) -> Error {
    let message = resolvers.unavailable(account).map_or_else(
        || format!("no credential resolver is supplied for account `{account}`"),
        ToOwned::to_owned,
    );
    Error::new(ErrorCode::Unauthorized, message)
}

/// The single-attempt port one serving model declares: one client over its binding's protocol.
pub struct Port {
    client: Client,
    resolver: Option<Arc<dyn SecretResolver>>,
}

impl Port {
    /// The protocol whose client this port is.
    pub fn protocol(&self) -> Protocol {
        match &self.client {
            Client::Chat(_) => Protocol::ChatCompletions,
            Client::Responses(_) => Protocol::Responses,
            Client::Messages(_) => Protocol::Messages,
            Client::Bedrock(client) => client.provenance().protocol,
        }
    }

    /// The port of one Bedrock Runtime target, signing with credentials from the caller's
    /// resolver for its account: the keys themselves (`aws-keys`), the container endpoint's
    /// token (`aws-container`), whose URI the caller supplies too, or the web identity token
    /// (`aws-web-identity`), whose role, session name and STS endpoint the caller supplies too;
    /// or with the keys the endpoint's command prints (`aws-process`), when the caller allows it
    /// to run.
    pub(crate) fn bedrock(
        target: BedrockTarget,
        references: AwsReferences,
        http: HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Self, Error> {
        let (resolver, credentials): (_, Arc<dyn CredentialSource>) = match references {
            AwsReferences::Keys {
                account,
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                let resolver = resolvers
                    .resolver(&account)
                    .ok_or_else(|| unresolved(resolvers, &account))?;
                let credentials = StaticCredentials::new(
                    resolver.clone(),
                    access_key_id,
                    secret_access_key,
                    session_token,
                );
                (Some(resolver), Arc::new(credentials))
            }
            AwsReferences::Container { account, token } => {
                let resolver = resolvers
                    .resolver(&account)
                    .ok_or_else(|| unresolved(resolvers, &account))?;
                let uri = resolvers.container_credentials_uri().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unauthorized,
                        format!(
                            "no container credentials endpoint is supplied for account `{account}`"
                        ),
                    )
                })?;
                let credentials =
                    ContainerCredentials::new(http.clone(), uri, resolver.clone(), token)?;
                (Some(resolver), Arc::new(credentials))
            }
            AwsReferences::WebIdentity { account, token } => {
                let resolver = resolvers
                    .resolver(&account)
                    .ok_or_else(|| unresolved(resolvers, &account))?;
                let role = resolvers.web_identity_role().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unauthorized,
                        format!("no web identity role is supplied for account `{account}`"),
                    )
                })?;
                let credentials = WebIdentityCredentials::new(
                    http.clone(),
                    role.sts_endpoint(),
                    role.role_arn(),
                    role.session_name(),
                    resolver.clone(),
                    token,
                )?;
                (Some(resolver), Arc::new(credentials))
            }
            AwsReferences::Process { endpoint, command } => {
                (None, process_credentials(&endpoint, command, resolvers)?)
            }
        };
        Ok(Self {
            client: Client::Bedrock(BedrockClient::new(target, http, credentials)?),
            resolver,
        })
    }

    /// The caller's resolver this port presents credentials from; `None` for an anonymous
    /// account.
    pub fn resolver(&self) -> Option<&Arc<dyn SecretResolver>> {
        self.resolver.as_ref()
    }

    fn model(&self) -> &dyn Model {
        match &self.client {
            Client::Chat(client) => client,
            Client::Responses(client) => client,
            Client::Messages(client) => client,
            Client::Bedrock(client) => client,
        }
    }

    fn from_binding(
        binding: Binding,
        http: HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Self, Error> {
        let account = &binding.declaration().account;
        let resolver = match account.auth_kind {
            AuthKind::Anonymous => None,
            AuthKind::Bearer | AuthKind::ApiKey | AuthKind::SubscriptionOauth => Some(
                resolvers
                    .resolver(&account.id)
                    .ok_or_else(|| unresolved(resolvers, &account.id))?,
            ),
        };
        let attached = resolver
            .clone()
            .unwrap_or_else(|| Arc::new(AnonymousResolver) as Arc<dyn SecretResolver>);
        let client = match binding.provenance().protocol {
            Protocol::ChatCompletions => Client::Chat(ChatClient::new(binding, http, attached)),
            Protocol::Responses => {
                Client::Responses(ResponsesClient::new(binding, http, attached)?)
            }
            Protocol::Messages => Client::Messages(MessagesClient::new(binding, http, attached)?),
            // A binding never carries it (`BindingDocument::bind` refuses it); kept as a refusal
            // so a binding built some other way still cannot reach a client here.
            Protocol::BedrockConverse => {
                return Err(Error::unsupported(
                    "bedrock-converse is served by llm-bedrock's BedrockClient, not by a binding",
                ));
            }
        };
        Ok(Self { client, resolver })
    }
}

impl Model for Port {
    fn provenance(&self) -> &Provenance {
        self.model().provenance()
    }
    fn capabilities(&self) -> &Capabilities {
        self.model().capabilities()
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        self.model().turn(request, sink, cancel)
    }
}

/// Builds the port the catalog declares for one serving model.
///
/// # Errors
/// Refuses, before any I/O, a serving model id the catalog does not declare
/// (`invalid-request`) and a credentialed account the caller has no resolver for
/// (`unauthorized`).
pub fn port(
    catalog: &Catalog,
    serving_model_id: &Id,
    http: HttpClient,
    resolvers: &dyn Resolvers,
) -> Result<Port, Error> {
    let binding = catalog
        .binding(serving_model_id)
        .ok_or_else(|| Error::invalid("no such serving model in the catalog"))?;
    Port::from_binding(binding.clone(), http, resolvers)
}

/// A port for every serving model a catalog declares, served to ordered fallback as
/// [`Models`].
pub struct CatalogModels {
    ports: BTreeMap<Id, Port>,
}

impl CatalogModels {
    /// Builds every serving model's port on the rules of [`port`].
    ///
    /// # Errors
    /// Refuses as a whole, before any I/O, on the first serving model (in id order) whose port
    /// cannot be built.
    pub fn build(
        catalog: &Catalog,
        http: &HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Self, Error> {
        let ports = catalog
            .bindings()
            .map(|(id, binding)| {
                Port::from_binding(binding.clone(), http.clone(), resolvers)
                    .map(|port| (id.clone(), port))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { ports })
    }

    /// The port built for this serving model.
    pub fn get(&self, serving_model_id: &Id) -> Option<&Port> {
        self.ports.get(serving_model_id)
    }

    /// Every built port, keyed by serving-model id, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &Port)> {
        self.ports.iter()
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }
}

impl Models for CatalogModels {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.ports
            .get(serving_model_id)
            .map(|port| port as &dyn Model)
    }
}
