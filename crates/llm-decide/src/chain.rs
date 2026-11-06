//! An ordered chain of classifiers for one label set under one deadline.

use crate::classify::{Classification, Classifier, Decision, LabelSet, Ports, SetupError};
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

    /// Not yet: the chain runs no classifier.
    pub async fn classify(&self, _request_id: &Id, _input: &str, _ports: Ports<'_>) -> ChainOutcome {
        let _ = (self.deadline, self.classifiers.len());
        ChainOutcome {
            chain_id: self.id.clone(),
            label_set_id: self.label_set.id().clone(),
            label_set_version: self.label_set.version().to_owned(),
            attempts: Vec::new(),
        }
    }
}
