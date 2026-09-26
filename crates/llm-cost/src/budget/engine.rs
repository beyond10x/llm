use super::{
    BudgetCommand, BudgetError, BudgetPolicy, BudgetReceipt, BudgetTotals, BudgetView,
    DispatchPermit, Operation, Phase, PostedCharge, Reservation, ReservationRequest,
};
use crate::{AccountingInput, Amount, MAX_DOCUMENT_BYTES, Observation};
use llm_core::{Id, exceeds};
use std::collections::BTreeMap;

/// Deterministic simulation engine. Mutable access serializes one instance; this
/// type supplies no storage or protection between separately constructed engines.
#[derive(Debug, Clone)]
pub struct BudgetEngine {
    policy: BudgetPolicy,
    now_ms: u64,
    reservations: BTreeMap<Id, Reservation>,
    charges: BTreeMap<Id, PostedCharge>,
}
impl BudgetEngine {
    /// # Errors
    /// Refuses invalid policy bounds.
    pub fn new(policy: BudgetPolicy) -> Result<Self, BudgetError> {
        policy.validate()?;
        Ok(Self {
            policy,
            now_ms: 0,
            reservations: BTreeMap::new(),
            charges: BTreeMap::new(),
        })
    }
    pub fn policy(&self) -> &BudgetPolicy {
        &self.policy
    }
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }
    pub fn reservation(&self, id: &Id) -> Option<&Reservation> {
        self.reservations.get(id)
    }

    /// Elapsed obligations are processed even if the subsequent command refuses.
    /// A simulation receipt is not a durable permission; use `SqliteLedger` at an I/O edge.
    /// # Errors
    /// Refuses invalid transitions, unknown spend, cap/concurrency violations or invalid input.
    pub fn apply(
        &mut self,
        now_ms: u64,
        command: BudgetCommand,
    ) -> Result<BudgetReceipt, BudgetError> {
        if exceeds(&command, MAX_DOCUMENT_BYTES) {
            return Err(BudgetError::TooLarge);
        }
        self.advance(now_ms)?;
        let started = self.command(command)?;
        Ok(BudgetReceipt {
            permit: started.map(|reservation| DispatchPermit {
                ledger: self.policy.id.clone(),
                reservation,
            }),
        })
    }

    /// Recovery never reissues starts. The persistent owner calls this only after
    /// excluding the previous writer, on every open-existing (including clean reopen).
    #[cfg(feature = "sqlite")]
    pub(crate) fn recover(&mut self) {
        for record in self.reservations.values_mut() {
            if record.phase == Phase::Started {
                record.phase = if matches!(record.request.operation, Operation::Compute { .. }) {
                    Phase::StopRequired
                } else {
                    Phase::Uncertain
                };
            }
        }
    }

    fn advance(&mut self, now_ms: u64) -> Result<(), BudgetError> {
        if now_ms < self.now_ms {
            return Err(BudgetError::ClockReversed);
        }
        self.now_ms = now_ms;
        for record in self.reservations.values_mut() {
            if record.request.expires_at_ms > now_ms {
                continue;
            }
            record.phase = match record.phase {
                Phase::Reserved => Phase::Cancelled,
                Phase::Started if matches!(record.request.operation, Operation::Compute { .. }) => {
                    Phase::StopRequired
                }
                Phase::Started => Phase::Uncertain,
                phase => phase,
            };
        }
        Ok(())
    }

    fn command(&mut self, command: BudgetCommand) -> Result<Option<Id>, BudgetError> {
        match command {
            BudgetCommand::Tick => (),
            BudgetCommand::Reserve { request } => self.reserve(request)?,
            BudgetCommand::Begin { id } => {
                let record = self.get(&id)?;
                if record.phase != Phase::Reserved {
                    return Err(BudgetError::WrongPhase);
                }
                self.admit(Amount::ZERO, false)?;
                self.get_mut(&id)?.phase = Phase::Started;
                return Ok(Some(id));
            }
            BudgetCommand::Cancel { id } => {
                let record = self.get_mut(&id)?;
                if record.phase != Phase::Reserved {
                    return Err(BudgetError::WrongPhase);
                }
                record.phase = Phase::Cancelled;
            }
            BudgetCommand::Uncertain { id, evidence } => {
                let record = self.get_mut(&id)?;
                if !matches!(record.phase, Phase::Started | Phase::Uncertain) {
                    return Err(BudgetError::WrongPhase);
                }
                record.phase = if matches!(record.request.operation, Operation::Compute { .. }) {
                    Phase::StopRequired
                } else {
                    Phase::Uncertain
                };
                record.evidence = Some(evidence);
            }
            BudgetCommand::Settle { id, settlement } => self.settle(&id, settlement)?,
            BudgetCommand::RequireStop { id, evidence } => {
                let record = self.get_mut(&id)?;
                if !matches!(record.request.operation, Operation::Compute { .. })
                    || !matches!(record.phase, Phase::Started | Phase::StopRequired)
                {
                    return Err(BudgetError::WrongPhase);
                }
                record.phase = Phase::StopRequired;
                record.evidence = Some(evidence);
            }
            BudgetCommand::ConfirmStopped { id, evidence } => {
                let record = self.get_mut(&id)?;
                if !matches!(record.request.operation, Operation::Compute { .. })
                    || !matches!(record.phase, Phase::Started | Phase::StopRequired)
                {
                    return Err(BudgetError::WrongPhase);
                }
                record.phase = Phase::Stopped;
                record.evidence = Some(evidence);
            }
            BudgetCommand::Renew {
                id,
                additional,
                expires_at_ms,
                assumption,
            } => {
                let result = self.renew(&id, additional, expires_at_ms, assumption.clone());
                if result.is_err() {
                    if let Some(record) = self.reservations.get_mut(&id)
                        && matches!(record.request.operation, Operation::Compute { .. })
                        && record.phase == Phase::Started
                    {
                        record.phase = Phase::StopRequired;
                        record.evidence = Some(assumption);
                    }
                    result?;
                }
            }
            BudgetCommand::PostCharge { charge } => {
                self.post_charge(charge)?;
            }
            BudgetCommand::ReconcileCharge {
                id,
                amount,
                evidence,
            } => {
                let charge = self.charges.get_mut(&id).ok_or(BudgetError::Missing)?;
                if charge.observation.amount.is_some() {
                    return Err(BudgetError::WrongPhase);
                }
                charge.observation.amount = Some(amount);
                charge.reconciliation = Some(evidence);
            }
        }
        Ok(None)
    }

    fn post_charge(&mut self, charge: crate::RecordedCharge) -> Result<(), BudgetError> {
        if self.reservations.contains_key(&charge.id) || self.charges.contains_key(&charge.id) {
            return Err(BudgetError::Duplicate);
        }
        AccountingInput::new(vec![Observation::Recorded(charge.clone())])
            .validate(&self.policy.currency)
            .map_err(|_| BudgetError::InvalidCharge)?;
        // Observed charges are retained even when already over cap or uncertain.
        self.charges.insert(
            charge.id.clone(),
            PostedCharge {
                observation: charge,
                reconciliation: None,
            },
        );
        Ok(())
    }

    fn settle(&mut self, id: &Id, settlement: super::Settlement) -> Result<(), BudgetError> {
        if settlement.currency != self.policy.currency {
            return Err(BudgetError::InvalidCharge);
        }
        let record = self.get_mut(id)?;
        let compute = matches!(record.request.operation, Operation::Compute { .. });
        if (compute && record.phase != Phase::Stopped)
            || (!compute && !matches!(record.phase, Phase::Started | Phase::Uncertain))
        {
            return Err(BudgetError::WrongPhase);
        }
        // Reconciliation cannot erase a previously known lower bound.
        if record
            .settlement
            .as_ref()
            .is_some_and(|old| settlement.known < old.known)
        {
            return Err(BudgetError::InvalidCharge);
        }
        record.phase = if settlement.complete {
            Phase::Settled
        } else if compute {
            Phase::Stopped
        } else {
            Phase::Uncertain
        };
        record.settlement = Some(settlement);
        Ok(())
    }

    fn reserve(&mut self, request: ReservationRequest) -> Result<(), BudgetError> {
        if self.reservations.contains_key(&request.id) || self.charges.contains_key(&request.id) {
            return Err(BudgetError::Duplicate);
        }
        if request.expires_at_ms <= self.now_ms {
            return Err(BudgetError::InvalidExpiry);
        }
        if matches!(request.operation, Operation::Compute { .. })
            && self
                .reservations
                .values()
                .any(|r| !r.phase.terminal() && r.request.operation == request.operation)
        {
            return Err(BudgetError::ResourceBusy);
        }
        self.admit(request.reserved, true)?;
        self.reservations.insert(
            request.id.clone(),
            Reservation {
                request,
                phase: Phase::Reserved,
                settlement: None,
                evidence: None,
            },
        );
        Ok(())
    }
    fn renew(
        &mut self,
        id: &Id,
        additional: Amount,
        expires_at_ms: u64,
        assumption: Id,
    ) -> Result<(), BudgetError> {
        let record = self.get(id)?;
        if !matches!(record.request.operation, Operation::Compute { .. })
            || record.phase != Phase::Started
        {
            return Err(BudgetError::WrongPhase);
        }
        if expires_at_ms <= record.request.expires_at_ms {
            return Err(BudgetError::InvalidExpiry);
        }
        let reserved = record
            .request
            .reserved
            .add(additional)
            .map_err(|_| BudgetError::Arithmetic)?;
        self.admit(additional, false)?;
        let record = self.get_mut(id)?;
        record.request.reserved = reserved;
        record.request.expires_at_ms = expires_at_ms;
        record.request.assumption = assumption;
        Ok(())
    }
    fn get(&self, id: &Id) -> Result<&Reservation, BudgetError> {
        self.reservations.get(id).ok_or(BudgetError::Missing)
    }
    fn get_mut(&mut self, id: &Id) -> Result<&mut Reservation, BudgetError> {
        self.reservations.get_mut(id).ok_or(BudgetError::Missing)
    }
    fn admit(&self, additional: Amount, new: bool) -> Result<(), BudgetError> {
        let totals = self.calculate()?;
        if totals.uncertain != 0 {
            return Err(BudgetError::UncertainSpend);
        }
        if totals
            .exposure()?
            .checked_add(u128::from(additional.nanos()))
            .ok_or(BudgetError::Arithmetic)?
            > u128::from(self.policy.limit.nanos())
        {
            return Err(BudgetError::LimitExceeded);
        }
        if new && totals.active >= self.policy.max_active as usize {
            return Err(BudgetError::ConcurrencyExceeded);
        }
        Ok(())
    }
    fn calculate(&self) -> Result<Calculation, BudgetError> {
        let mut result = Calculation::default();
        for record in self.reservations.values() {
            let known = record.settlement.as_ref().map_or(0, |s| s.known.nanos());
            result.settled = result
                .settled
                .checked_add(u128::from(known))
                .ok_or(BudgetError::Arithmetic)?;
            if !record.phase.terminal() {
                result.active += 1;
                let held =
                    u128::from(record.request.reserved.nanos()).saturating_sub(u128::from(known));
                result.held = result
                    .held
                    .checked_add(held)
                    .ok_or(BudgetError::Arithmetic)?;
            }
            if matches!(
                record.phase,
                Phase::Uncertain | Phase::StopRequired | Phase::Stopped
            ) {
                result.uncertain += 1;
            }
            if record.phase == Phase::StopRequired {
                result.stop_required.push(record.request.id.clone());
            }
        }
        for charge in self.charges.values() {
            if let Some(amount) = charge.observation.amount {
                result.settled = result
                    .settled
                    .checked_add(u128::from(amount.nanos()))
                    .ok_or(BudgetError::Arithmetic)?;
            } else {
                result.uncertain += 1;
            }
        }
        Ok(result)
    }
    /// # Errors
    /// Refuses aggregate arithmetic overflow; individual evidence remains retained.
    pub fn view(&self) -> Result<BudgetView, BudgetError> {
        let t = self.calculate()?;
        let exposure = t.exposure()?;
        Ok(BudgetView {
            storage_failed: false,
            policy: self.policy.clone(),
            now_ms: self.now_ms,
            reservations: self.reservations.values().cloned().collect(),
            charges: self.charges.values().cloned().collect(),
            totals: BudgetTotals {
                settled_nanos: t.settled.to_string(),
                held_nanos: t.held.to_string(),
                exposure_nanos: exposure.to_string(),
                uncertain_count: t.uncertain,
                active_count: t.active,
                above_limit: exposure > u128::from(self.policy.limit.nanos()),
                stop_required: t.stop_required,
            },
        })
    }
}
#[derive(Default)]
struct Calculation {
    settled: u128,
    held: u128,
    uncertain: usize,
    active: usize,
    stop_required: Vec<Id>,
}
impl Calculation {
    fn exposure(&self) -> Result<u128, BudgetError> {
        self.settled
            .checked_add(self.held)
            .ok_or(BudgetError::Arithmetic)
    }
}
