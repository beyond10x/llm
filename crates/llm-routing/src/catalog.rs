use llm_core::{AuthKind, BillingKind, Error, Id, exceeds};
use llm_providers::{
    Account, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROUTE_TARGETS: usize = 64;
const MAX_DECLARATIONS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Format {
    #[serde(rename = "llm.catalog/1")]
    V1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub id: Id,
    pub alias: Id,
    #[serde(default)]
    pub fallback_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteTarget {
    pub id: Id,
    pub route_id: Id,
    pub serving_model_id: Id,
    pub position: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    format: Format,
    pub providers: Vec<Provider>,
    pub accounts: Vec<Account>,
    pub endpoints: Vec<Endpoint>,
    pub models: Vec<ServedModel>,
    pub serving_models: Vec<ServingModel>,
    pub routes: Vec<Route>,
    pub targets: Vec<RouteTarget>,
}

impl CatalogDocument {
    /// # Errors
    /// Refuses invalid versions/fields, size bounds and malformed TOML without echoing its values.
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_CONFIG_BYTES {
            return Err(Error::too_large("TOML catalog exceeds its byte bound"));
        }
        toml::from_str(source).map_err(|error: toml::de::Error| {
            let location = error
                .span()
                .map_or(String::new(), |span| format!(" at byte {}", span.start));
            Error::invalid(format!("invalid llm.catalog/1 TOML document{location}"))
        })
    }

    /// # Errors
    /// Refuses duplicate IDs, missing references, ambiguous aliases, invalid bindings and order.
    pub fn validate(mut self) -> Result<Catalog, Error> {
        if exceeds(&self, MAX_CONFIG_BYTES) {
            return Err(Error::too_large(
                "catalog declarations exceed their byte bound",
            ));
        }
        let providers = index(&self.providers, |v| &v.id)?;
        let accounts = index(&self.accounts, |v| &v.id)?;
        let endpoints = index(&self.endpoints, |v| &v.id)?;
        let models = index(&self.models, |v| &v.id)?;
        let serving = index(&self.serving_models, |v| &v.id)?;
        let routes = index(&self.routes, |v| &v.id)?;
        let _targets = index(&self.targets, |v| &v.id)?;
        for account in &self.accounts {
            account.validate()?;
            lookup(&providers, &account.provider_id, "account provider")?;
        }
        unshared_subscription_references(&self.accounts)?;
        for endpoint in &self.endpoints {
            lookup(&accounts, &endpoint.account_id, "endpoint account")?;
        }
        let mut bindings = BTreeMap::new();
        for model in serving.values() {
            let endpoint = lookup(&endpoints, &model.endpoint_id, "serving endpoint")?;
            let account = lookup(&accounts, &endpoint.account_id, "endpoint account")?;
            let provider = lookup(&providers, &account.provider_id, "account provider")?;
            let declared_model = lookup(&models, &model.model_id, "serving model")?;
            let binding = BindingDocument::new(
                provider.clone(),
                account.clone(),
                endpoint.clone(),
                declared_model.clone(),
                (*model).clone(),
            )
            .bind()?;
            bindings.insert(model.id.clone(), binding);
        }
        let mut groups: BTreeMap<Id, Vec<RouteTarget>> = BTreeMap::new();
        for target in &self.targets {
            lookup(&routes, &target.route_id, "target route")?;
            lookup(&serving, &target.serving_model_id, "target serving model")?;
            groups
                .entry(target.route_id.clone())
                .or_default()
                .push(target.clone());
        }
        let mut aliases = BTreeMap::new();
        for route in &self.routes {
            let mut targets = groups.remove(&route.id).unwrap_or_default();
            if targets.is_empty() || targets.len() > MAX_ROUTE_TARGETS {
                return Err(Error::invalid(
                    "each route requires between one and 64 targets",
                ));
            }
            subscription_billing_kept(route, &targets, &bindings)?;
            targets.sort_by_key(|target| target.position);
            let mut unique_bindings = BTreeSet::new();
            for (position, target) in targets.iter().enumerate() {
                if target.position != position || !unique_bindings.insert(&target.serving_model_id)
                {
                    return Err(Error::invalid(
                        "route positions must be contiguous and serving models cannot repeat",
                    ));
                }
            }
            if aliases
                .insert(
                    route.alias.clone(),
                    ResolvedRoute {
                        declaration: route.clone(),
                        targets,
                    },
                )
                .is_some()
            {
                return Err(Error::invalid("route alias is declared more than once"));
            }
        }
        let digest = self.canonical_digest()?;
        Ok(Catalog {
            bindings,
            routes: aliases,
            digest,
        })
    }

    fn canonical_digest(&mut self) -> Result<String, Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        // Canonical JSON field order is fixed by these types; entity declaration order is not
        // routing order (the explicit position field is). Comments and whitespace are irrelevant.
        self.providers.sort_by(|a, b| a.id.cmp(&b.id));
        self.accounts.sort_by(|a, b| a.id.cmp(&b.id));
        self.endpoints.sort_by(|a, b| a.id.cmp(&b.id));
        self.models.sort_by(|a, b| a.id.cmp(&b.id));
        self.serving_models.sort_by(|a, b| a.id.cmp(&b.id));
        self.routes.sort_by(|a, b| a.id.cmp(&b.id));
        self.targets.sort_by(|a, b| a.id.cmp(&b.id));
        let bytes =
            serde_json::to_vec(&self).map_err(|_| Error::invalid("catalog cannot be encoded"))?;
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            digest.push(char::from(HEX[usize::from(byte >> 4)]));
            digest.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Ok(digest)
    }
}

fn index<'a, T>(
    values: &'a [T],
    id: impl Fn(&'a T) -> &'a Id,
) -> Result<BTreeMap<Id, &'a T>, Error> {
    if values.len() > MAX_DECLARATIONS {
        return Err(Error::too_large("too many catalog declarations"));
    }
    let mut indexed = BTreeMap::new();
    for value in values {
        if indexed.insert(id(value).clone(), value).is_some() {
            return Err(Error::invalid(
                "duplicate catalog identifier within one kind",
            ));
        }
    }
    Ok(indexed)
}

/// Another account naming a subscription account's reference would present the subscription
/// token under another kind or billing, so it is refused whatever that account declares.
fn unshared_subscription_references(accounts: &[Account]) -> Result<(), Error> {
    for subscription in accounts
        .iter()
        .filter(|account| account.auth_kind == AuthKind::SubscriptionOauth)
    {
        if accounts.iter().any(|other| {
            other.id != subscription.id
                && other.secret_reference_id.is_some()
                && other.secret_reference_id == subscription.secret_reference_id
        }) {
            return Err(Error::invalid(
                "a subscription OAuth account cannot share its secret reference with another account",
            ));
        }
    }
    Ok(())
}

/// Fallback would move a turn between the targets of a route, so a subscription token's turn
/// could continue under other billing. Without fallback only the first target is ever selected.
fn subscription_billing_kept(
    route: &Route,
    targets: &[RouteTarget],
    bindings: &BTreeMap<Id, Binding>,
) -> Result<(), Error> {
    if !route.fallback_enabled {
        return Ok(());
    }
    let accounts: Vec<&Account> = targets
        .iter()
        .filter_map(|target| bindings.get(&target.serving_model_id))
        .map(|binding| &binding.declaration().account)
        .collect();
    if accounts
        .iter()
        .any(|account| account.auth_kind == AuthKind::SubscriptionOauth)
        && accounts
            .iter()
            .any(|account| account.billing_kind != BillingKind::Subscription)
    {
        return Err(Error::invalid(
            "a route with fallback cannot mix a subscription OAuth target with a target under other billing",
        ));
    }
    Ok(())
}

fn lookup<'a, T>(values: &BTreeMap<Id, &'a T>, id: &Id, field: &str) -> Result<&'a T, Error> {
    values
        .get(id)
        .copied()
        .ok_or_else(|| Error::invalid(format!("{field} references an undeclared identifier")))
}

#[derive(Debug)]
pub(crate) struct ResolvedRoute {
    pub declaration: Route,
    pub targets: Vec<RouteTarget>,
}

/// Validated catalog. It owns no credential resolver, HTTP client, runtime or cloud controller.
#[derive(Debug)]
pub struct Catalog {
    pub(crate) bindings: BTreeMap<Id, Binding>,
    pub(crate) routes: BTreeMap<Id, ResolvedRoute>,
    pub(crate) digest: String,
}

impl Catalog {
    /// # Errors
    /// Refuses malformed or inconsistent configuration before any external effects.
    pub fn parse(source: &str) -> Result<Self, Error> {
        CatalogDocument::parse(source)?.validate()
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.routes.values().map(|route| &route.declaration)
    }
    pub fn binding(&self, id: &Id) -> Option<&Binding> {
        self.bindings.get(id)
    }
    /// Every serving model's validated binding, keyed by serving-model id, in id order.
    pub fn bindings(&self) -> impl Iterator<Item = (&Id, &Binding)> {
        self.bindings.iter()
    }
}
