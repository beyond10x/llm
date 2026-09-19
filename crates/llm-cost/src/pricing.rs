use crate::{
    AccountingInput, Amount, AttemptUsage, ChargeKind, ComputeUsage, CostError, Currency,
    Observation, PriceBook, Rate,
};
use llm_core::{BillingKind, Id};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Basis {
    MeteredEstimate,
    ComputeEstimate,
    ReferenceUsage,
    RecordedMetered,
    RecordedCompute,
    SubscriptionCharge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    InputToken,
    CachedInputToken,
    CacheCreationInputToken,
    OutputToken,
    ResourceMillisecond,
    RecordedCharge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownReason {
    QuantityUnknown,
    RateUnknown,
    PriceUnknown,
    BindingMismatch,
    ModelUnknown,
    ModelMismatch,
    ChargeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub unit: Unit,
    pub quantity: Option<u64>,
    pub amount: Option<Amount>,
    pub unknown: Option<UnknownReason>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Total {
    pub known_subtotal: Amount,
    pub unknown_lines: u64,
    pub complete_total: Option<Amount>,
}
impl Total {
    fn add(&mut self, line: &Line) -> Result<(), CostError> {
        if let Some(amount) = line.amount {
            self.known_subtotal = self.known_subtotal.add(amount)?;
        } else {
            self.unknown_lines = self
                .unknown_lines
                .checked_add(1)
                .ok_or(CostError::Overflow)?;
        }
        self.complete_total = (self.unknown_lines == 0).then_some(self.known_subtotal);
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    pub observation: Observation,
    pub basis: Basis,
    pub lines: Vec<Line>,
    pub total: Total,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
enum OutputFormat {
    #[serde(rename = "llm.cost/1")]
    V1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostReport {
    format: OutputFormat,
    pub currency: Currency,
    pub price_revision: Id,
    pub price_source: String,
    pub price_as_of_unix_ms: u64,
    pub price_digest: Id,
    pub records: Vec<Record>,
    /// Separate bases intentionally have no combined grand total: estimates,
    /// reference valuations and recorded charges can describe the same work.
    pub totals: BTreeMap<Basis, Total>,
}

impl PriceBook {
    /// # Errors
    /// Refuses invalid usage, duplicate observations, mixed currencies, bad periods, bounds and
    /// arithmetic overflow. Missing facts produce attributed unknown lines, never implicit zero.
    pub fn quote(&self, input: &AccountingInput) -> Result<CostReport, CostError> {
        input.validate(&self.document.currency)?;
        let mut records = Vec::with_capacity(input.observations.len());
        let mut totals: BTreeMap<Basis, Total> = BTreeMap::new();
        for observation in &input.observations {
            let (basis, lines) = match observation {
                Observation::Attempt(v) => (
                    match v.billing {
                        BillingKind::Metered => Basis::MeteredEstimate,
                        BillingKind::Subscription | BillingKind::SelfHosted => {
                            Basis::ReferenceUsage
                        }
                    },
                    self.attempt(v)?,
                ),
                Observation::Compute(v) => (Basis::ComputeEstimate, vec![self.compute_line(v)?]),
                Observation::Recorded(v) => (
                    match v.charge_kind {
                        ChargeKind::Metered => Basis::RecordedMetered,
                        ChargeKind::Compute => Basis::RecordedCompute,
                        ChargeKind::Subscription => Basis::SubscriptionCharge,
                    },
                    vec![Line {
                        unit: Unit::RecordedCharge,
                        quantity: Some(1),
                        amount: v.amount,
                        unknown: v.amount.is_none().then_some(UnknownReason::ChargeUnknown),
                    }],
                ),
            };
            let mut total = Total::default();
            for line in &lines {
                total.add(line)?;
                totals.entry(basis).or_default().add(line)?;
            }
            records.push(Record {
                observation: observation.clone(),
                basis,
                lines,
                total,
            });
        }
        Ok(CostReport {
            format: OutputFormat::V1,
            currency: self.document.currency.clone(),
            price_revision: self.document.revision.clone(),
            price_source: self.document.source.clone(),
            price_as_of_unix_ms: self.document.as_of_unix_ms,
            price_digest: self.digest.clone(),
            records,
            totals,
        })
    }

    fn attempt(&self, usage: &AttemptUsage) -> Result<Vec<Line>, CostError> {
        let price = self.model(&usage.price_id);
        let context = match price {
            None => Some(UnknownReason::PriceUnknown),
            Some(p)
                if p.serving_model_id != usage.serving_model_id
                    || p.binding_revision != usage.binding.binding_revision =>
            {
                Some(UnknownReason::BindingMismatch)
            }
            Some(_) if usage.upstream_model.is_none() => Some(UnknownReason::ModelUnknown),
            Some(p) if usage.upstream_model.as_ref() != Some(&p.upstream_model) => {
                Some(UnknownReason::ModelMismatch)
            }
            Some(_) => None,
        };
        let counts = &usage.usage;
        // Cache partitions are validated before pricing. None is never subtracted as zero.
        let uncached = match (
            counts.input_tokens,
            counts.cached_input_tokens,
            counts.cache_creation_input_tokens,
        ) {
            (Some(total), Some(read), Some(created)) => Some(
                total
                    .checked_sub(read)
                    .and_then(|n| n.checked_sub(created))
                    .ok_or(CostError::InvalidUsage)?,
            ),
            _ => None,
        };
        [
            (
                Unit::InputToken,
                uncached,
                price.and_then(|p| p.input.as_ref()),
            ),
            (
                Unit::CachedInputToken,
                counts.cached_input_tokens,
                price.and_then(|p| p.cached_input.as_ref()),
            ),
            (
                Unit::CacheCreationInputToken,
                counts.cache_creation_input_tokens,
                price.and_then(|p| p.cache_creation_input.as_ref()),
            ),
            // Reasoning is already included in output; no second charge is added.
            (
                Unit::OutputToken,
                counts.output_tokens,
                price.and_then(|p| p.output.as_ref()),
            ),
        ]
        .into_iter()
        .map(|(unit, quantity, rate)| line(unit, quantity, rate, context))
        .collect()
    }

    fn compute_line(&self, usage: &ComputeUsage) -> Result<Line, CostError> {
        let price = self.compute(&usage.price_id);
        let context = match price {
            None => Some(UnknownReason::PriceUnknown),
            Some(p)
                if p.hosting_provider != usage.hosting_provider
                    || p.account != usage.account
                    || p.resource_class != usage.resource_class =>
            {
                Some(UnknownReason::BindingMismatch)
            }
            Some(_) => None,
        };
        line(
            Unit::ResourceMillisecond,
            usage.duration_ms,
            price.and_then(|p| p.rate.as_ref()),
            context,
        )
    }
}

fn line(
    unit: Unit,
    quantity: Option<u64>,
    rate: Option<&Rate>,
    context: Option<UnknownReason>,
) -> Result<Line, CostError> {
    let (amount, unknown) = match (context, quantity, rate) {
        (Some(reason), _, _) => (None, Some(reason)),
        (_, None, _) => (None, Some(UnknownReason::QuantityUnknown)),
        (_, Some(0), _) => (Some(Amount::ZERO), None),
        (_, Some(_), None) => (None, Some(UnknownReason::RateUnknown)),
        (_, Some(count), Some(rate)) => (Some(rate.price(count)?), None),
    };
    Ok(Line {
        unit,
        quantity,
        amount,
        unknown,
    })
}
