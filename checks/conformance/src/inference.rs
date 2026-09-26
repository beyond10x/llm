//! Observe the production codec and validator, with no access to suite expectations.
use llm_core::{
    Error, Id, Item, OutcomeDocument, Protocol, Provenance, TurnObservation, TurnRequest,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    result_json: String,
    failure: bool,
}

pub fn observe(input: &Input) -> Value {
    let mut facts = json!({"accepted":false,"failed":false,"error_code":null,"dispatch":null,
        "upstream_model":null,"response_id":null,"final_usage":null,"input_tokens":null,
        "output_tokens":null,"cached_input_tokens":null,"cache_creation_input_tokens":null,
        "reasoning_output_tokens":null,"roundtrip_preserved":false});
    if let Err(error) = inspect(input, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    facts
}

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq>(value: &T) -> bool {
    serde_json::to_string(value)
        .ok()
        .and_then(|encoded| serde_json::from_str::<T>(&encoded).ok())
        .is_some_and(|decoded| decoded == *value)
}

fn inspect(input: &Input, facts: &mut Value) -> Result<(), Error> {
    let target = Provenance {
        protocol: Protocol::Messages,
        provider: Id::new("lab").map_err(|_| Error::invalid("fixture provider"))?,
        account: Id::new("account").map_err(|_| Error::invalid("fixture account"))?,
        endpoint: Id::new("endpoint").map_err(|_| Error::invalid("fixture endpoint"))?,
        model: Id::new("internal-model").map_err(|_| Error::invalid("fixture model"))?,
        binding_revision: Id::new("binding-r1").map_err(|_| Error::invalid("fixture revision"))?,
    };
    let observation = if input.failure {
        let error: Error = serde_json::from_str(&input.result_json)
            .map_err(|_| Error::invalid("invalid error document"))?;
        error.validate_for(&target)?;
        facts["failed"] = json!(true);
        facts["error_code"] = json!(error.code);
        facts["dispatch"] = json!(error.dispatch);
        facts["roundtrip_preserved"] = json!(roundtrip(&error));
        error.observation.map(|value| *value)
    } else {
        let document: OutcomeDocument = serde_json::from_str(&input.result_json)
            .map_err(|_| Error::invalid("invalid outcome document"))?;
        let request = TurnRequest::new("internal-model", vec![Item::user("hello")]);
        document.outcome.validate_for(&request, &target)?;
        facts["roundtrip_preserved"] = json!(roundtrip(&document));
        Some(document.outcome.observation)
    };
    facts["accepted"] = json!(true);
    if let Some(observation) = observation {
        report(&observation, facts);
    }
    Ok(())
}

fn report(observation: &TurnObservation, facts: &mut Value) {
    facts["upstream_model"] = json!(observation.upstream_model);
    facts["response_id"] = json!(observation.response_id);
    facts["final_usage"] = json!(observation.final_usage);
    if let Some(usage) = &observation.usage {
        facts["input_tokens"] = json!(usage.input_tokens.map(|v| v.to_string()));
        facts["output_tokens"] = json!(usage.output_tokens.map(|v| v.to_string()));
        facts["cached_input_tokens"] = json!(usage.cached_input_tokens.map(|v| v.to_string()));
        facts["cache_creation_input_tokens"] =
            json!(usage.cache_creation_input_tokens.map(|v| v.to_string()));
        facts["reasoning_output_tokens"] =
            json!(usage.reasoning_output_tokens.map(|v| v.to_string()));
    }
}
