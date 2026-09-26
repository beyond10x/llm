use crate::{Amount, CostError, Currency, MAX_DOCUMENT_BYTES, MAX_ENTRIES, Rate};
use llm_core::{BillingKind, Dispatch, Id, Provenance, Usage, exceeds};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PriceFormat {
    #[serde(rename = "llm.prices/1")]
    V1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum UsageFormat {
    #[serde(rename = "llm.usage/2")]
    V2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPrice {
    pub id: Id,
    pub serving_model_id: Id,
    pub binding_revision: Id,
    pub upstream_model: Id,
    pub input: Option<Rate>,
    pub cached_input: Option<Rate>,
    pub cache_creation_input: Option<Rate>,
    pub output: Option<Rate>,
}
impl ModelPrice {
    fn rates(&self) -> impl Iterator<Item = &Rate> {
        [
            &self.input,
            &self.cached_input,
            &self.cache_creation_input,
            &self.output,
        ]
        .into_iter()
        .filter_map(Option::as_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputePrice {
    pub id: Id,
    pub hosting_provider: Id,
    pub account: Id,
    pub resource_class: Id,
    /// Milliseconds of one explicitly priced resource, e.g. a whole pod, not inferred GPU time.
    pub rate: Option<Rate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceDocument {
    format: PriceFormat,
    pub revision: Id,
    pub source: String,
    pub as_of_unix_ms: u64,
    pub currency: Currency,
    #[serde(default)]
    pub models: Vec<ModelPrice>,
    #[serde(default)]
    pub compute: Vec<ComputePrice>,
}

/// Immutable validated prices. A source/revision change also changes its content identity.
#[derive(Debug, Clone)]
pub struct PriceBook {
    pub(crate) document: PriceDocument,
    pub(crate) digest: Id,
    models: BTreeMap<Id, usize>,
    compute: BTreeMap<Id, usize>,
}
impl PriceBook {
    /// # Errors
    /// Refuses unknown fields/versions, non-string amounts and invalid or excessive prices.
    pub fn parse_json(input: &str) -> Result<Self, CostError> {
        if input.len() > MAX_DOCUMENT_BYTES {
            return Err(CostError::TooLarge);
        }
        Self::from_document(serde_json::from_str(input).map_err(|_| CostError::InvalidDocument)?)
    }
    /// # Errors
    /// Applies the same contract as JSON. TOML amounts must be quoted decimal strings.
    pub fn parse_toml(input: &str) -> Result<Self, CostError> {
        if input.len() > MAX_DOCUMENT_BYTES {
            return Err(CostError::TooLarge);
        }
        Self::from_document(toml::from_str(input).map_err(|_| CostError::InvalidDocument)?)
    }
    /// # Errors
    /// Refuses duplicate IDs (also across model/compute rows), zero denominators and size bounds.
    pub fn from_document(mut document: PriceDocument) -> Result<Self, CostError> {
        if document.models.len().saturating_add(document.compute.len()) > MAX_ENTRIES
            || exceeds(&document, MAX_DOCUMENT_BYTES)
        {
            return Err(CostError::TooLarge);
        }
        source(&document.source)?;
        let mut ids = BTreeSet::new();
        for id in document
            .models
            .iter()
            .map(|r| &r.id)
            .chain(document.compute.iter().map(|r| &r.id))
        {
            if !ids.insert(id) {
                return Err(CostError::DuplicatePrice);
            }
        }
        for rate in document
            .models
            .iter()
            .flat_map(ModelPrice::rates)
            .chain(document.compute.iter().filter_map(|r| r.rate.as_ref()))
        {
            if rate.per_units == 0 {
                return Err(CostError::InvalidDenominator);
            }
        }
        document.models.sort_by(|a, b| a.id.cmp(&b.id));
        document.compute.sort_by(|a, b| a.id.cmp(&b.id));
        let mut digest = String::from("prices-sha256:");
        for byte in
            Sha256::digest(serde_json::to_vec(&document).map_err(|_| CostError::InvalidDocument)?)
        {
            write!(digest, "{byte:02x}").map_err(|_| CostError::InvalidDocument)?;
        }
        let models = document
            .models
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        let compute = document
            .compute
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        Ok(Self {
            document,
            digest: Id::new(digest).map_err(|_| CostError::InvalidDocument)?,
            models,
            compute,
        })
    }
    pub fn document(&self) -> &PriceDocument {
        &self.document
    }
    pub fn digest(&self) -> &Id {
        &self.digest
    }
    pub(crate) fn model(&self, id: &Id) -> Option<&ModelPrice> {
        self.models.get(id).map(|i| &self.document.models[*i])
    }
    pub(crate) fn compute(&self, id: &Id) -> Option<&ComputePrice> {
        self.compute.get(id).map(|i| &self.document.compute[*i])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptUsage {
    pub id: Id,
    pub usage_record_id: Id,
    pub request_id: Id,
    pub route_target_id: Id,
    pub serving_model_id: Id,
    pub price_id: Id,
    pub binding: Provenance,
    /// Model observed upstream, not the caller's requested route alias.
    pub upstream_model: Option<Id>,
    pub billing: BillingKind,
    pub dispatch: Dispatch,
    pub failed: bool,
    /// False retains partial counts as a lower bound; success is independent of usage finality.
    pub final_usage: bool,
    pub usage: Usage,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeUsage {
    pub id: Id,
    pub price_id: Id,
    pub hosting_provider: Id,
    pub account: Id,
    pub resource_class: Id,
    pub resource_id: Id,
    pub duration_ms: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChargeKind {
    Metered,
    Compute,
    Subscription,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedCharge {
    pub id: Id,
    pub source: String,
    pub provider: Id,
    pub account: Id,
    pub subject: Id,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    pub currency: Currency,
    pub amount: Option<Amount>,
    pub charge_kind: ChargeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Observation {
    Attempt(AttemptUsage),
    Compute(ComputeUsage),
    Recorded(RecordedCharge),
}
impl Observation {
    pub fn id(&self) -> &Id {
        match self {
            Self::Attempt(v) => &v.id,
            Self::Compute(v) => &v.id,
            Self::Recorded(v) => &v.id,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountingInput {
    format: UsageFormat,
    pub observations: Vec<Observation>,
}
impl AccountingInput {
    pub fn new(observations: Vec<Observation>) -> Self {
        Self {
            format: UsageFormat::V2,
            observations,
        }
    }
    /// # Errors
    /// Refuses unknown fields/versions, malformed values and excess input bytes. Quote validates
    /// usage, currencies, intervals and uniqueness, including programmatically built inputs.
    pub fn parse_json(input: &str) -> Result<Self, CostError> {
        if input.len() > MAX_DOCUMENT_BYTES {
            return Err(CostError::TooLarge);
        }
        serde_json::from_str(input).map_err(|_| CostError::InvalidDocument)
    }
    pub(crate) fn validate(&self, currency: &Currency) -> Result<(), CostError> {
        if self.observations.len() > MAX_ENTRIES || exceeds(self, MAX_DOCUMENT_BYTES) {
            return Err(CostError::TooLarge);
        }
        let mut ids = BTreeSet::new();
        for observation in &self.observations {
            if !ids.insert(observation.id()) {
                return Err(CostError::DuplicateObservation);
            }
            match observation {
                Observation::Attempt(v) => {
                    v.usage.validate().map_err(|_| CostError::InvalidUsage)?;
                }
                Observation::Compute(_) => (),
                Observation::Recorded(v) => {
                    source(&v.source)?;
                    if &v.currency != currency {
                        return Err(CostError::CurrencyMismatch);
                    }
                    if v.period_start_unix_ms >= v.period_end_unix_ms {
                        return Err(CostError::InvalidPeriod);
                    }
                }
            }
        }
        Ok(())
    }
}

fn source(value: &str) -> Result<(), CostError> {
    if value.trim().is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(CostError::InvalidDocument);
    }
    Ok(())
}
