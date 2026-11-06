//! An ordered chain of classifiers for one label set under one deadline.

use crate::classify::{
    Ask, Classification, Classifier, Decision, LabelSet, Ports, SetupError, hold,
};
use llm_core::Id;
use std::{collections::BTreeSet, time::Duration};

/// Classifiers in order for one label set and one deadline.
pub struct Chain {
    id: Id,
    label_set: LabelSet,
    deadline: Duration,
    classifiers: Vec<Box<dyn Classifier>>,
}

/// Every attempt of one chain run, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainOutcome {
    pub chain_id: Id,
    pub label_set_id: Id,
    pub label_set_version: String,
    pub attempts: Vec<Classification>,
}

impl ChainOutcome {
    /// The chain's answer: the deciding attempt, or the final abstention.
    pub fn result(&self) -> Option<&Classification> {
        self.attempts.last()
    }
    pub fn decision(&self) -> Option<&Decision> {
        match self.result()? {
            Classification::Decided(decision) => Some(decision),
            Classification::Abstained(_) => None,
        }
    }
}

impl Chain {
    /// # Errors
    /// Refuses a chain with no classifier, a repeated classifier id or a zero deadline.
    pub fn new(
        id: Id,
        label_set: LabelSet,
        deadline: Duration,
        classifiers: Vec<Box<dyn Classifier>>,
    ) -> Result<Self, SetupError> {
        if classifiers.is_empty() {
            return Err(SetupError::NoClassifier);
        }
        let mut seen = BTreeSet::new();
        for classifier in &classifiers {
            if !seen.insert(classifier.id()) {
                return Err(SetupError::DuplicateClassifier(classifier.id().clone()));
            }
        }
        if deadline.is_zero() {
            return Err(SetupError::ZeroDeadline);
        }
        Ok(Self {
            id,
            label_set,
            deadline,
            classifiers,
        })
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }
    pub const fn label_set(&self) -> &LabelSet {
        &self.label_set
    }
    pub const fn deadline(&self) -> Duration {
        self.deadline
    }
    pub fn classifiers(&self) -> impl Iterator<Item = &dyn Classifier> {
        self.classifiers.iter().map(AsRef::as_ref)
    }

    /// Asks each classifier in order until one decides, and reports every attempt.
    ///
    /// The deadline runs from the start on the recording clock; each classifier is told the
    /// time left. Every result is held to the chain's label set and the classifier's floor, so a
    /// decision outside them abstains `OffLabel`, whoever wrote the classifier. Only the last
    /// abstention of a chain where none decided is final. `request_id`
    /// names this classification: a model call's record id is `<request id>/<classifier id>`.
    /// `input_tokens` is the caller's estimate of a model turn's input, which routing admits each
    /// target against (see [`Ask::input_tokens`]).
    pub async fn classify(
        &self,
        request_id: &Id,
        input: &str,
        input_tokens: Option<u64>,
        ports: Ports<'_>,
    ) -> ChainOutcome {
        let clock = ports.recording.clock;
        let start = clock.monotonic();
        let mut attempts = Vec::with_capacity(self.classifiers.len());
        for classifier in &self.classifiers {
            let elapsed = clock.monotonic().saturating_sub(start);
            let ask = Ask {
                label_set: &self.label_set,
                input,
                remaining: self.deadline.saturating_sub(elapsed),
                request_id,
                input_tokens,
                ports,
            };
            // Held again here: a classifier is any implementation of a public trait.
            let attempt = hold(
                classifier.as_ref(),
                &self.label_set,
                classifier.classify(ask).await,
            );
            let decided = matches!(attempt, Classification::Decided(_));
            attempts.push(attempt);
            if decided {
                break;
            }
        }
        if let Some(Classification::Abstained(last)) = attempts.last_mut() {
            last.is_final = true;
        }
        ChainOutcome {
            chain_id: self.id.clone(),
            label_set_id: self.label_set.id().clone(),
            label_set_version: self.label_set.version().to_owned(),
            attempts,
        }
    }
}
