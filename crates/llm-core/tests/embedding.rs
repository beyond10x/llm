use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol,
    Provenance, StopReason, StreamEvent, StreamSink, ToolCall, ToolName, ToolSpec, TurnDocument,
    TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn target() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: Id::new("local-provider").unwrap(),
        account: Id::new("anonymous").unwrap(),
        endpoint: Id::new("development").unwrap(),
        model: Id::new("test-model").unwrap(),
        binding_revision: Id::new("fake-model-v1").unwrap(),
    }
}
fn observation(usage: Option<Usage>) -> TurnObservation {
    TurnObservation {
        usage,
        final_usage: true,
        ..TurnObservation::new(target())
    }
}
fn capabilities() -> Capabilities {
    let mut value = Capabilities::text(8192, 1024);
    value.tools = true;
    value.tool_choice = true;
    value
}
fn request() -> TurnRequest {
    let mut value = TurnRequest::new("test-model", vec![Item::user("What is the answer?")]);
    value.tools.push(ToolSpec {
        name: ToolName::new("lookup.answer").unwrap(),
        description: "Look up an answer".into(),
        input_schema: json!({"type":"object"}),
    });
    value
}

struct FakeModel {
    provenance: Provenance,
    capabilities: Capabilities,
}
impl Model for FakeModel {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            request.validate_for(&self.provenance, &self.capabilities)?;
            tokio::select! {
                biased;
                () = cancel.cancelled() => Err(Error::cancelled()),
                result = async {
                    let Some(Item::ToolResult { output, .. }) = request.items.last() else {
                        let call = ToolCall { call_id: CallId::new("call-1").unwrap(), name: ToolName::new("lookup.answer").unwrap(), arguments: json!({}) };
                        sink.emit(StreamEvent::ToolCallStarted { call_id: call.call_id.clone(), name: call.name.clone() }).await?;
                        sink.emit(StreamEvent::ToolArgumentsDelta { call_id: call.call_id.clone(), delta: "{}".into() }).await?;
                        return Ok(TurnOutcome { stop_reason: StopReason::ToolCalls, items: vec![
                            Item::Opaque { provenance: self.provenance.clone(), payload: json!({"continuation":"retain-verbatim"}) },
                            Item::ToolCall(call)], observation: observation(None) });
                    };
                    sink.emit(StreamEvent::TextDelta { text: "Answer: ".into() }).await?;
                    sink.emit(StreamEvent::TextDelta { text: output.to_string() }).await?;
                    Ok(TurnOutcome { stop_reason: StopReason::EndTurn, items: vec![Item::assistant(format!("Answer: {output}"))],
                        observation: observation(Some(Usage { output_tokens: Some(3), ..Usage::default() })) })
                } => result.map_err(|error: Error| error.with_dispatch(Dispatch::Accepted)),
            }
        })
    }
}

#[tokio::test]
async fn an_embedded_caller_completes_a_streamed_tool_round_trip_without_a_gateway() {
    let model: Box<dyn Model> = Box::new(FakeModel {
        provenance: target(),
        capabilities: capabilities(),
    });
    let mut request = request();
    let mut sink = VecSink::new(32, 4096);
    let cancel = Cancel::new();
    let first = model.turn(&request, &mut sink, &cancel).await.unwrap();
    first.validate_for(&request, model.provenance()).unwrap();
    assert_eq!(first.stop_reason, StopReason::ToolCalls);
    assert!(first.observation.usage.is_none());
    assert_eq!(first.tool_calls().count(), 1);
    request.items.extend(first.items);
    // Only the embedding application supplies the result; LLM has no tool execution port.
    request.items.push(Item::ToolResult {
        call_id: CallId::new("call-1").unwrap(),
        output: json!(42),
        failed: false,
    });
    let second = model.turn(&request, &mut sink, &cancel).await.unwrap();
    second.validate_for(&request, model.provenance()).unwrap();
    assert_eq!(second.stop_reason, StopReason::EndTurn);
    assert_eq!(sink.text(), "Answer: 42");
    let usage = second.observation.usage.unwrap();
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.cached_input_tokens, None);
}

struct PendingSink {
    entered: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}
struct DropNotice(Arc<AtomicBool>);
impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl StreamSink for PendingSink {
    fn emit(&mut self, _: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            let _notice = DropNotice(self.dropped.clone());
            self.entered.store(true, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancellation_wakes_a_backpressured_turn_and_drops_pending_work() {
    let model = FakeModel {
        provenance: target(),
        capabilities: capabilities(),
    };
    let entered = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut sink = PendingSink {
        entered: entered.clone(),
        dropped: dropped.clone(),
    };
    let cancel = Cancel::new();
    let cancel_from_elsewhere = cancel.clone();
    let request = request();
    let turn = model.turn(&request, &mut sink, &cancel);
    let ((), result) = tokio::join!(
        async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            cancel_from_elsewhere.cancel();
        },
        turn
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(cancel.is_cancelled());
}

#[tokio::test]
async fn bounded_sink_failure_preserves_prefix_and_prevents_success() {
    let model = FakeModel {
        provenance: target(),
        capabilities: capabilities(),
    };
    let mut request = request();
    request.items.extend([
        Item::ToolCall(ToolCall {
            call_id: CallId::new("call-1").unwrap(),
            name: ToolName::new("lookup.answer").unwrap(),
            arguments: json!({}),
        }),
        Item::ToolResult {
            call_id: CallId::new("call-1").unwrap(),
            output: json!(42),
            failed: false,
        },
    ]);
    let mut sink = VecSink::new(1, 1024);
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(sink.text(), "Answer: ");
}

#[test]
fn opaque_state_cannot_cross_any_binding_coordinate() {
    let selected = target();
    for coordinate in 0..6 {
        let mut origin = selected.clone();
        match coordinate {
            0 => origin.protocol = Protocol::Messages,
            1 => origin.provider = Id::new("another").unwrap(),
            2 => origin.account = Id::new("another").unwrap(),
            3 => origin.endpoint = Id::new("another").unwrap(),
            4 => origin.model = Id::new("another").unwrap(),
            _ => origin.binding_revision = Id::new("another").unwrap(),
        }
        let mut request = request();
        request.items.push(Item::Opaque {
            provenance: origin,
            payload: json!({"x":1}),
        });
        assert_eq!(
            request
                .validate_for(&selected, &capabilities())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn persisted_version_and_unrecognized_authority_fields_refuse() {
    let document = TurnDocument::new(request());
    let mut encoded = serde_json::to_value(&document).unwrap();
    assert_eq!(encoded["format"], "llm.turn/3");
    assert!(encoded["request"].get("sampling").is_none());
    assert_eq!(
        serde_json::from_value::<TurnDocument>(encoded.clone()).unwrap(),
        document
    );
    // `llm.turn/2` predates the unattributed variant: a reader of that version must not be
    // handed one silently, so the older envelope is refused by name rather than read.
    for old in ["llm.turn/1", "llm.turn/2"] {
        encoded["format"] = json!(old);
        let error = serde_json::from_value::<TurnDocument>(encoded.clone()).unwrap_err();
        assert!(error.to_string().contains(old), "{error}");
    }
    let mut future = serde_json::to_value(&document).unwrap();
    future["format"] = json!("llm.turn/4");
    assert!(serde_json::from_value::<TurnDocument>(future).is_err());
    for field in ["approval", "envelope"] {
        let mut encoded = serde_json::to_value(&document).unwrap();
        encoded["request"]["tools"][0][field] = json!({});
        assert!(serde_json::from_value::<TurnDocument>(encoded).is_err());
    }
}

#[test]
fn unknown_usage_stays_distinct_from_zero_and_invalid_subsets_refuse() {
    assert_eq!(serde_json::to_value(Usage::default()).unwrap(), json!({}));
    let usage: Usage = serde_json::from_value(json!({"output_tokens":0})).unwrap();
    assert_eq!(usage.output_tokens, Some(0));
    assert_eq!(usage.input_tokens, None);
    for usage in [
        Usage {
            input_tokens: Some(1),
            cached_input_tokens: Some(2),
            ..Usage::default()
        },
        Usage {
            cached_input_tokens: Some(u64::MAX),
            cache_creation_input_tokens: Some(1),
            ..Usage::default()
        },
        Usage {
            output_tokens: Some(1),
            reasoning_output_tokens: Some(2),
            ..Usage::default()
        },
    ] {
        assert_eq!(usage.validate().unwrap_err().code, ErrorCode::Protocol);
    }
}

#[test]
fn provider_output_must_name_published_tools_and_match_its_terminal_reason() {
    let request = request();
    let call = ToolCall {
        call_id: CallId::new("new-call").unwrap(),
        name: ToolName::new("not-published").unwrap(),
        arguments: json!({}),
    };
    let mut outcome = TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items: vec![Item::ToolCall(call)],
        observation: observation(None),
    };
    assert_eq!(
        outcome.validate_for(&request, &target()).unwrap_err().code,
        ErrorCode::Protocol
    );
    outcome.items.clear();
    assert_eq!(
        outcome.validate_for(&request, &target()).unwrap_err().code,
        ErrorCode::Protocol
    );
    outcome.stop_reason = StopReason::EndTurn;
    outcome
        .items
        .push(Item::user("a model cannot inject a user message"));
    assert_eq!(
        outcome.validate_for(&request, &target()).unwrap_err().code,
        ErrorCode::Protocol
    );
}

#[test]
fn output_document_versions_and_unknown_fields_are_rejected() {
    let document = llm_core::OutcomeDocument::new(TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("text")],
        observation: observation(None),
    });
    let mut value = serde_json::to_value(&document).unwrap();
    assert_eq!(value["format"], "llm.outcome/4");
    assert_eq!(
        serde_json::from_value::<llm_core::OutcomeDocument>(value.clone()).unwrap(),
        document
    );
    for old in ["llm.outcome/1", "llm.outcome/3"] {
        value["format"] = json!(old);
        let error = serde_json::from_value::<llm_core::OutcomeDocument>(value.clone()).unwrap_err();
        assert!(error.to_string().contains(old), "{error}");
    }
    let mut future = serde_json::to_value(&document).unwrap();
    future["format"] = json!("llm.outcome/5");
    assert!(serde_json::from_value::<llm_core::OutcomeDocument>(future).is_err());
    let mut value = serde_json::to_value(&document).unwrap();
    value["outcome"]["unknown"] = json!(true);
    assert!(serde_json::from_value::<llm_core::OutcomeDocument>(value).is_err());
}

fn unattributed() -> Item {
    Item::UnattributedOpaque {
        protocol: Protocol::Responses,
        payload: json!({"type":"reasoning","id":"rs_1","encrypted_content":"AAAA","summary":[]}),
    }
}

/// An ingress surface may carry state it could not attribute; carrying it is not sending it.
#[test]
fn unattributed_opaque_state_is_carried_but_refused_by_name_until_a_caller_binds_it() {
    let mut request = request();
    request.items.push(unattributed());
    // Structurally valid: it is a well-formed request a gateway may hold.
    request.validate().unwrap();
    let error = request
        .validate_for(&target(), &capabilities())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);

    // The explicit caller decision, and nothing else, makes it native state for one target.
    let bound = request.bind_unattributed(&target()).unwrap();
    assert_eq!(bound, 1);
    assert_eq!(
        request.items.last(),
        Some(&Item::Opaque {
            provenance: target(),
            payload: json!({"type":"reasoning","id":"rs_1","encrypted_content":"AAAA","summary":[]}),
        })
    );
    request.validate_for(&target(), &capabilities()).unwrap();
}

/// Binding is a decision about one protocol's bytes; it cannot move them to another wire.
#[test]
fn binding_refuses_a_target_of_another_protocol_and_leaves_the_request_unchanged() {
    let mut request = request();
    request.items.push(unattributed());
    let before = request.clone();
    let mut other = target();
    other.protocol = Protocol::Messages;
    let error = request.bind_unattributed(&other).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(request, before);
    assert_eq!(
        unattributed().bind_unattributed(&other).unwrap_err().code,
        ErrorCode::Unsupported
    );
    // Every other item is already what it is; binding leaves it alone.
    assert_eq!(
        Item::user("hi").bind_unattributed(&target()).unwrap(),
        Item::user("hi")
    );
}

#[test]
fn unattributed_state_round_trips_through_the_versioned_envelope() {
    let mut request = request();
    request.items.push(unattributed());
    let encoded = serde_json::to_value(TurnDocument::new(request.clone())).unwrap();
    assert_eq!(encoded["format"], "llm.turn/3");
    assert_eq!(
        serde_json::to_string(&request.items[1]).unwrap(),
        r#"{"kind":"unattributed-opaque","protocol":"responses","payload":{"encrypted_content":"AAAA","id":"rs_1","summary":[],"type":"reasoning"}}"#
    );
    assert_eq!(
        serde_json::from_value::<TurnDocument>(encoded)
            .unwrap()
            .request,
        request
    );
}

/// A decoder watched its own binding produce what it decoded, so model output is always bound.
#[test]
fn model_output_carrying_unattributed_state_is_refused() {
    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![unattributed(), Item::assistant("done")],
        observation: observation(None),
    };
    assert_eq!(
        outcome
            .validate_for(&request(), &target())
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
}
