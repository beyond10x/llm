use llm_core::{
    Dispatch, Error, ErrorCode, Id, Item, OutcomeDocument, Protocol, Provenance, StopReason,
    TurnObservation, TurnOutcome, TurnRequest, Usage,
};
use serde_json::json;

fn binding() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: Id::new("lab").unwrap(),
        account: Id::new("account").unwrap(),
        endpoint: Id::new("endpoint").unwrap(),
        model: Id::new("internal-model").unwrap(),
        binding_revision: Id::new("binding-r1").unwrap(),
    }
}

#[test]
fn success_requires_terminal_bound_evidence_but_does_not_invent_unknown_fields() {
    let request = TurnRequest::new("internal-model", vec![Item::user("hello")]);
    let mut outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("hello")],
        observation: TurnObservation::new(binding()),
    };
    assert_eq!(
        outcome.validate_for(&request, &binding()).unwrap_err().code,
        ErrorCode::Protocol
    );
    outcome.observation.final_usage = true;
    outcome.validate_for(&request, &binding()).unwrap();
    let document = OutcomeDocument::new(outcome);
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(value["format"], "llm.outcome/4");
    for missing in ["upstream_model", "response_id", "usage"] {
        assert!(value["outcome"]["observation"].get(missing).is_none());
    }
    assert_eq!(
        serde_json::from_value::<OutcomeDocument>(value.clone()).unwrap(),
        document
    );
    for version in [
        "llm.outcome/1",
        "llm.outcome/2",
        "llm.outcome/3",
        "llm.outcome/5",
    ] {
        let mut old = value.clone();
        old["format"] = json!(version);
        assert!(serde_json::from_value::<OutcomeDocument>(old).is_err());
    }
    for field in ["observation", "observation.final_usage"] {
        let mut missing = value.clone();
        if field == "observation" {
            missing["outcome"].as_object_mut().unwrap().remove(field);
        } else {
            missing["outcome"]["observation"]
                .as_object_mut()
                .unwrap()
                .remove("final_usage");
        }
        assert!(serde_json::from_value::<OutcomeDocument>(missing).is_err());
    }
}

#[test]
fn failed_attempt_preserves_partial_or_final_usage_and_the_actual_model() {
    for final_usage in [false, true] {
        let evidence = TurnObservation {
            upstream_model: Some(Id::new("wire-model-revision").unwrap()),
            response_id: Some(Id::new("response-1").unwrap()),
            usage: Some(Usage {
                input_tokens: Some(u64::MAX),
                output_tokens: Some(0),
                ..Usage::default()
            }),
            final_usage,
            ..TurnObservation::new(binding())
        };
        let error = Error::cancelled()
            .with_dispatch(Dispatch::Accepted)
            .with_observation(evidence.clone());
        error.validate_for(&binding()).unwrap();
        let persisted = serde_json::to_string(&error).unwrap();
        assert_eq!(serde_json::from_str::<Error>(&persisted).unwrap(), error);
        assert_eq!(error.observation.as_deref(), Some(&evidence));
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(error.dispatch, Dispatch::Accepted);
        assert!(
            error
                .clone()
                .with_dispatch(Dispatch::NotSent)
                .validate_for(&binding())
                .is_err()
        );
        for coordinate in 0..6 {
            let mut foreign = binding();
            match coordinate {
                0 => foreign.protocol = Protocol::Responses,
                1 => foreign.provider = Id::new("other").unwrap(),
                2 => foreign.account = Id::new("other").unwrap(),
                3 => foreign.endpoint = Id::new("other").unwrap(),
                4 => foreign.model = Id::new("other").unwrap(),
                _ => foreign.binding_revision = Id::new("other").unwrap(),
            }
            assert!(error.validate_for(&foreign).is_err());
        }
    }
    Error::cancelled().validate_for(&binding()).unwrap();
    let mut evidence = TurnObservation::new(binding());
    evidence.usage = Some(Usage {
        input_tokens: Some(1),
        cached_input_tokens: Some(2),
        ..Usage::default()
    });
    assert!(
        Error::protocol("safe diagnostic")
            .with_dispatch(Dispatch::Accepted)
            .with_observation(evidence)
            .validate_for(&binding())
            .is_err()
    );
}
