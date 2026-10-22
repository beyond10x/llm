//! The Anthropic `InvokeModel` wire: `/invoke` and `/invoke-with-response-stream`.
//!
//! The body is the Messages body `llm-messages` encodes, with `anthropic_version` set to
//! [`BEDROCK_ANTHROPIC_VERSION`] and without `model` (the path names the model) or `stream` (the
//! operation says whether it streams). A streamed answer is a sequence of event-stream `chunk`
//! frames, each carrying one Messages event as `{"bytes": base64(<event JSON>)}`; the events are
//! read by the `llm-messages` stream decoder, so this wire decodes Messages exactly as the
//! Messages client does. A whole answer is one Messages message, read by
//! [`llm_messages::decode_message`].

use crate::{client::BedrockTarget, errors, eventstream::Message};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use llm_core::{
    Cancel, Dispatch, Error, Item, MAX_REQUEST_BYTES, Provenance, StreamEvent, StreamSink,
    TurnOutcome, TurnRequest, exceeds,
};
use llm_http::SseEvent;
use serde_json::{Map, Value, json};
use std::time::Instant;

/// The Messages API version Bedrock requires, written in the body where the Messages route takes
/// it as a header.
pub(crate) const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// The field Bedrock adds to the terminal `message_stop` event.
const METRICS: &str = "amazon-bedrock-invocationMetrics";

/// Every field Bedrock's invocation metrics may carry. The latencies are accepted and never read.
const METRIC_FIELDS: [&str; 6] = [
    "inputTokenCount",
    "outputTokenCount",
    "invocationLatency",
    "firstByteLatency",
    "cacheReadInputTokenCount",
    "cacheWriteInputTokenCount",
];

/// Each Messages usage counter, and the invocation metric that counts the same tokens.
const COUNTERS: [(&str, &str); 4] = [
    ("input_tokens", "inputTokenCount"),
    ("output_tokens", "outputTokenCount"),
    ("cache_read_input_tokens", "cacheReadInputTokenCount"),
    ("cache_creation_input_tokens", "cacheWriteInputTokenCount"),
];

/// Where `cache_creation_input_tokens` sits in [`COUNTERS`]: a `cache_creation` breakdown
/// reports it too.
const CREATED: usize = 3;

/// Encodes the `InvokeModel` body for `request` on `target`.
///
/// # Errors
/// Refuses what the Messages encoder refuses (a provenance that is not Messages, unsupported
/// semantics, the request bound) before any I/O.
pub(crate) fn encode_request(
    request: &TurnRequest,
    target: &BedrockTarget,
) -> Result<Vec<u8>, Error> {
    let mut body = llm_messages::encode_body(
        request,
        &target.provenance,
        &target.capabilities,
        &target.model_id,
    )?;
    let fields = body
        .as_object_mut()
        .ok_or_else(|| Error::invalid("Bedrock InvokeModel body is not an object"))?;
    fields.remove("model");
    fields.remove("stream");
    fields.insert(
        "anthropic_version".to_owned(),
        Value::from(BEDROCK_ANTHROPIC_VERSION),
    );
    if exceeds(&body, MAX_REQUEST_BYTES) {
        return Err(Error::too_large(
            "Bedrock InvokeModel encoded request exceeds bound",
        ));
    }
    serde_json::to_vec(&body)
        .map_err(|_| Error::invalid("Bedrock InvokeModel request encoding failed"))
}

/// Accumulates one `invoke-with-response-stream` answer into a bound neutral outcome.
///
/// Every Messages event is handed to [`llm_messages::StreamDecoder`] under its own `type` as the
/// event name, so text, tool calls, reasoning, the stop reason, usage and every refusal are the
/// Messages decoder's. What this adds is the envelope and Bedrock's invocation metrics.
///
/// # Invocation metrics
///
/// Bedrock adds `amazon-bedrock-invocationMetrics` to `message_stop`. It is removed before the
/// event is handed on, because the Messages decoder refuses a field outside its subset. Its token
/// counts are used **only for a counter the Messages usage never reported** in this stream
/// (`message_start.message.usage` or `message_delta.usage`): Messages usage is the route's own
/// report and stays authoritative, and a metric that disagrees with it does not replace it. A
/// counter the metrics fill is handed to the Messages decoder as one more cumulative usage
/// report, a `message_delta` without a stop reason, just before `message_stop`, so it passes the
/// same checks (regression, subsets within totals) as any other report. Each metric is read as
/// the Messages counter it names: `inputTokenCount` as the uncached `input_tokens`,
/// `cacheReadInputTokenCount` and `cacheWriteInputTokenCount` as the cache counters. A cache
/// counter neither source reports stays unknown, and so does the input total that needs it.
pub(crate) struct StreamDecoder {
    inner: llm_messages::StreamDecoder,
    /// Per [`COUNTERS`] entry, whether the Messages usage reported it in this stream.
    reported: [bool; 4],
}

impl StreamDecoder {
    pub(crate) fn new(target: Provenance) -> Self {
        Self {
            inner: llm_messages::StreamDecoder::new(target),
            reported: [false; 4],
        }
    }

    /// Bound every remaining wait in this decode by one absolute instant.
    #[must_use]
    pub(crate) fn with_deadline(mut self, deadline: Instant) -> Self {
        self.inner = self.inner.with_deadline(deadline);
        self
    }

    pub(crate) const fn is_complete(&self) -> bool {
        self.inner.is_complete()
    }

    /// Whether any event other than a keep-alive or the message header was read, shown to the
    /// caller or not. A failure after it is final.
    pub(crate) const fn has_answered(&self) -> bool {
        self.inner.has_answered()
    }

    /// Retains dispatch evidence and the last valid usage snapshot on a failure.
    pub(crate) fn attach(&self, error: Error) -> Error {
        self.inner.attach(error)
    }

    /// Apply one event-stream frame, emitting the neutral deltas a caller watches.
    ///
    /// An exception frame ends the turn with the class [`errors::exception`] gives it, retriable
    /// only while nothing has answered.
    pub(crate) async fn apply(
        &mut self,
        message: &Message,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        match message.string_header(":message-type") {
            Some("event") => {}
            Some("exception" | "error") => {
                return Err(errors::exception(message, self.inner.has_answered()));
            }
            _ => {
                return Err(Error::protocol(
                    "Bedrock frame carries no known message type",
                ));
            }
        }
        if message.string_header(":event-type") != Some("chunk") {
            return Err(Error::protocol("Bedrock InvokeModel event is not a chunk"));
        }
        let mut event = chunk(&message.payload)?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::protocol("Bedrock chunk event names no type"))?
            .to_owned();
        self.note(&kind, &event);
        if kind == "message_stop" {
            let metrics = event
                .as_object_mut()
                .and_then(|fields| fields.remove(METRICS));
            if let Some(usage) = self.supplement(metrics.as_ref())? {
                let report = SseEvent::Payload {
                    event: Some("message_delta".to_owned()),
                    data: json!({"type": "message_delta", "delta": {}, "usage": usage}),
                };
                self.inner.apply(&report, sink, cancel).await?;
            }
        }
        let event = SseEvent::Payload {
            event: Some(kind),
            data: event,
        };
        self.inner.apply(&event, sink, cancel).await
    }

    /// Records which Messages usage counters `event` reports.
    fn note(&mut self, kind: &str, event: &Value) {
        let usage = match kind {
            "message_start" => event
                .get("message")
                .and_then(|message| message.get("usage")),
            "message_delta" => event.get("usage"),
            _ => None,
        };
        let Some(usage) = usage.filter(|usage| !usage.is_null()) else {
            return;
        };
        for (reported, (field, _)) in self.reported.iter_mut().zip(COUNTERS) {
            *reported |= usage.get(field).is_some_and(|found| !found.is_null());
        }
        // A breakdown is the Messages decoder's own evidence of the write count; a metric must
        // not be set beside it.
        self.reported[CREATED] |= usage
            .get("cache_creation")
            .is_some_and(|found| !found.is_null());
    }

    /// The Messages usage report the invocation metrics add: the counters Messages usage left
    /// unreported, or `None` when there is none to add.
    fn supplement(&self, metrics: Option<&Value>) -> Result<Option<Value>, Error> {
        let Some(metrics) = metrics.filter(|metrics| !metrics.is_null()) else {
            return Ok(None);
        };
        let fields = metrics
            .as_object()
            .ok_or_else(|| Error::protocol("Bedrock invocation metrics are not an object"))?;
        if fields
            .keys()
            .any(|key| !METRIC_FIELDS.contains(&key.as_str()))
        {
            return Err(Error::unsupported(
                "Bedrock invocation metrics carry a field outside the declared subset",
            ));
        }
        let mut usage = Map::new();
        for (reported, (name, wire)) in self.reported.iter().zip(COUNTERS) {
            let Some(found) = fields.get(wire).filter(|found| !found.is_null()) else {
                continue;
            };
            let count = found.as_u64().ok_or_else(|| {
                Error::protocol("Bedrock invocation metric is not an unsigned integer")
            })?;
            if !reported {
                usage.insert(name.to_owned(), Value::from(count));
            }
        }
        if usage.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Value::Object(usage)))
        }
    }

    /// The terminal outcome.
    ///
    /// # Errors
    /// Refuses a stream that never reached its terminal event, retaining the last snapshot.
    pub(crate) fn finish(self, request: &TurnRequest) -> Result<TurnOutcome, Error> {
        self.inner.finish(request)
    }
}

/// The Messages event one `chunk` payload carries.
///
/// The payload is `{"bytes": "<base64>"}`; Bedrock may add `p`, padding that hides the event's
/// length, which is never read. Any other field is refused.
fn chunk(payload: &[u8]) -> Result<Value, Error> {
    let envelope: Value = serde_json::from_slice(payload)
        .map_err(|_| Error::protocol("Bedrock chunk payload is not JSON"))?;
    let fields = envelope
        .as_object()
        .ok_or_else(|| Error::protocol("Bedrock chunk payload is not an object"))?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "bytes" | "p"))
    {
        return Err(Error::unsupported(
            "Bedrock chunk carries a field outside the declared subset",
        ));
    }
    let encoded = fields
        .get("bytes")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::protocol("Bedrock chunk carries no bytes"))?;
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| Error::protocol("Bedrock chunk bytes are not base64"))?;
    let event: Value = serde_json::from_slice(&decoded)
        .map_err(|_| Error::protocol("Bedrock chunk does not carry a JSON event"))?;
    if !event.is_object() {
        return Err(Error::protocol("Bedrock chunk event is not an object"));
    }
    Ok(event)
}

/// Decodes a whole `/invoke` answer, then shows the caller what it carries.
///
/// Nothing is shown for an answer that is refused: the events are emitted only once the message
/// has decoded, in item order, so the turn's text is exactly the text its deltas showed.
///
/// # Errors
/// Refuses a body that is not a Messages message and everything [`llm_messages::decode_message`]
/// refuses, after dispatch; and a sink or cancellation failure while the caller is told.
pub(crate) async fn decode_response(
    bytes: &[u8],
    request: &TurnRequest,
    provenance: &Provenance,
    sink: &mut dyn StreamSink,
    cancel: &Cancel,
) -> Result<TurnOutcome, Error> {
    let message: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::protocol("Bedrock InvokeModel answer is not JSON").with_dispatch(Dispatch::Accepted)
    })?;
    let outcome = llm_messages::decode_message(&message, request, provenance)?;
    for item in &outcome.items {
        for event in shown(item) {
            show(sink, event, cancel).await?;
        }
    }
    Ok(outcome)
}

/// The events that show one decoded item, as the Messages stream would have shown it.
fn shown(item: &Item) -> Vec<StreamEvent> {
    match item {
        Item::AssistantText { text } if !text.is_empty() => {
            vec![StreamEvent::TextDelta { text: text.clone() }]
        }
        Item::ToolCall(call) => vec![
            StreamEvent::ToolCallStarted {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: call.call_id.clone(),
                delta: call.arguments.to_string(),
            },
        ],
        Item::Opaque { payload, .. } => payload
            .get("thinking")
            .and_then(Value::as_str)
            .filter(|text| {
                !text.is_empty() && payload.get("type").and_then(Value::as_str) == Some("thinking")
            })
            .map(|text| StreamEvent::ReasoningDelta {
                text: text.to_owned(),
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Awaits the caller, ended early by cancellation. Backpressure is awaited, never dropped.
async fn show(sink: &mut dyn StreamSink, event: StreamEvent, cancel: &Cancel) -> Result<(), Error> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
        result = sink.emit(event) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::{BEDROCK_ANTHROPIC_VERSION, StreamDecoder, decode_response, encode_request};
    use crate::{
        client::{Api, BedrockTarget},
        eventstream::{HeaderValue, Message, MessageDecoder, encode_message},
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use llm_core::{
        Cancel, Capabilities, Dispatch, Error, Id, Item, Protocol, Provenance, StopReason,
        TurnOutcome, TurnRequest, Usage, VecSink,
    };
    use serde_json::{Value, json};

    const STREAM: &[u8] =
        include_bytes!("../tests/data/frames/ddtrace-anthropic_message_invoke_stream.bin");
    const ANSWER: &[u8] =
        include_bytes!("../tests/data/frames/ddtrace-anthropic_message_invoke.json");
    /// The text both recorded answers carry, read off their decoded events.
    const RECORDED_TEXT: &str =
        "Hobbits embark on epic quest to destroy powerful ring, battling dark forces.";
    const MODEL: &str = "anthropic.claude-3-sonnet-20240229-v1:0";

    fn provenance() -> Provenance {
        Provenance {
            protocol: Protocol::Messages,
            provider: Id::new("bedrock").unwrap(),
            account: Id::new("fixture-account").unwrap(),
            endpoint: Id::new("us-east-1").unwrap(),
            model: Id::new(MODEL).unwrap(),
            binding_revision: Id::new("fixture-1").unwrap(),
        }
    }

    fn capabilities() -> Capabilities {
        Capabilities {
            tools: true,
            tool_choice: true,
            temperature: true,
            top_p: true,
            reasoning_efforts: Vec::new(),
            context_window: 200_000,
            max_output_tokens: 4_096,
            output_streaming: true,
            input_during_output: false,
        }
    }

    fn target() -> BedrockTarget {
        BedrockTarget {
            provenance: provenance(),
            capabilities: capabilities(),
            region: "us-east-1".to_owned(),
            model_id: MODEL.to_owned(),
            api: Api::AnthropicInvoke,
            stream: true,
            endpoint: None,
        }
    }

    /// The turn the recorded requests sent.
    fn request() -> TurnRequest {
        let mut request = TurnRequest::new(
            MODEL,
            vec![Item::user(
                "summarize the plot to the lord of the rings in a dozen words",
            )],
        );
        request.max_output_tokens = Some(50);
        request.sampling.temperature = Some(0.0);
        request
    }

    /// Replays an event-stream body through the frame decoder and this wire's decoder, split
    /// at `split` bytes so frames cross reads.
    async fn replay(bytes: &[u8], split: usize) -> (Result<TurnOutcome, Error>, VecSink) {
        let mut frames = MessageDecoder::new();
        let mut decoder = StreamDecoder::new(provenance());
        let mut sink = VecSink::new(256, 64 * 1024);
        let cancel = Cancel::new();
        for read in bytes.chunks(split) {
            for frame in frames.push(read) {
                let applied = match frame {
                    Ok(frame) => decoder.apply(&frame, &mut sink, &cancel).await,
                    Err(error) => Err(error),
                };
                if let Err(error) = applied {
                    return (Err(decoder.attach(error)), sink);
                }
            }
        }
        if let Err(error) = frames.finish() {
            return (Err(decoder.attach(error)), sink);
        }
        assert!(decoder.is_complete());
        (decoder.finish(&request()), sink)
    }

    fn frame(headers: &[(&str, &str)], payload: Vec<u8>) -> Message {
        Message {
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), HeaderValue::String((*value).to_owned())))
                .collect(),
            payload,
        }
    }

    fn chunk_frame(envelope: &Value) -> Vec<u8> {
        encode_message(&frame(
            &[
                (":event-type", "chunk"),
                (":content-type", "application/json"),
                (":message-type", "event"),
            ],
            envelope.to_string().into_bytes(),
        ))
    }

    /// One `chunk` frame carrying `event` as Bedrock does.
    fn chunk(event: &Value) -> Vec<u8> {
        chunk_frame(&json!({"bytes": STANDARD.encode(event.to_string())}))
    }

    fn exception(kind: &str) -> Vec<u8> {
        encode_message(&frame(
            &[
                (":exception-type", kind),
                (":content-type", "application/json"),
                (":message-type", "exception"),
            ],
            br#"{"message":"fixture"}"#.to_vec(),
        ))
    }

    fn start(usage: &Value) -> Value {
        json!({"type":"message_start","message":{"id":"msg_fixture","type":"message",
            "role":"assistant","content":[],"model":"claude-fixture","stop_reason":null,
            "stop_sequence":null,"usage":usage}})
    }

    fn text_events() -> Vec<Value> {
        vec![
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
            json!({"type":"content_block_stop","index":0}),
        ]
    }

    fn stream(events: &[Value]) -> Vec<u8> {
        events.iter().flat_map(chunk).collect()
    }

    #[tokio::test]
    async fn the_recorded_stream_yields_the_turn_it_carries() {
        for split in [1, 7, 97, STREAM.len()] {
            let (outcome, sink) = replay(STREAM, split).await;
            let outcome = outcome.unwrap();
            assert_eq!(sink.text(), RECORDED_TEXT);
            assert_eq!(outcome.items, vec![Item::assistant(RECORDED_TEXT)]);
            assert_eq!(outcome.stop_reason, StopReason::EndTurn);
            let observation = &outcome.observation;
            assert!(observation.final_usage);
            assert_eq!(
                observation.response_id,
                Some(Id::new("msg_013MVoqeRpeko9EhHVcBdJvk").unwrap())
            );
            assert_eq!(
                observation.upstream_model,
                Some(Id::new("claude-3-sonnet-28k-20240229").unwrap())
            );
            // Messages reported `input_tokens` 21 and `output_tokens` 2 then 22; the metrics
            // repeat 21 and 22 and report no cache counter. The input total needs both cache
            // counters, which neither source reported, so it stays unknown and is never 21.
            assert_eq!(
                observation.usage,
                Some(Usage {
                    input_tokens: None,
                    output_tokens: Some(22),
                    cached_input_tokens: None,
                    cache_creation_input_tokens: None,
                    reasoning_output_tokens: None,
                })
            );
        }
    }

    #[tokio::test]
    async fn metrics_fill_only_the_counters_messages_usage_left_unreported() {
        let mut events = vec![start(&json!({"input_tokens": 21, "output_tokens": 2}))];
        events.extend(text_events());
        events.push(
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn",
            "stop_sequence":null},"usage":{"output_tokens":5}}),
        );
        // Input and output disagree with what Messages reported and are not used; the cache
        // counters Messages never reported are.
        events.push(
            json!({"type":"message_stop","amazon-bedrock-invocationMetrics":{
            "inputTokenCount":99,"outputTokenCount":50,"invocationLatency":10,
            "firstByteLatency":5,"cacheReadInputTokenCount":3,"cacheWriteInputTokenCount":4}}),
        );
        let (outcome, sink) = replay(&stream(&events), 64).await;
        let outcome = outcome.unwrap();
        assert_eq!(sink.text(), "Hello");
        assert_eq!(
            outcome.observation.usage,
            Some(Usage {
                input_tokens: Some(28),
                output_tokens: Some(5),
                cached_input_tokens: Some(3),
                cache_creation_input_tokens: Some(4),
                reasoning_output_tokens: None,
            })
        );
    }

    #[tokio::test]
    async fn an_output_count_only_the_metrics_report_is_used_and_unreported_stays_unknown() {
        let mut events = vec![start(&json!({"input_tokens": 21}))];
        events.extend(text_events());
        events.push(
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn",
            "stop_sequence":null}}),
        );
        events.push(
            json!({"type":"message_stop","amazon-bedrock-invocationMetrics":{
            "inputTokenCount":21,"outputTokenCount":22}}),
        );
        let (outcome, _) = replay(&stream(&events), 4096).await;
        assert_eq!(
            outcome.unwrap().observation.usage,
            Some(Usage {
                input_tokens: None,
                output_tokens: Some(22),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_output_tokens: None,
            })
        );
    }

    #[tokio::test]
    async fn a_metrics_field_outside_the_subset_is_refused_and_no_number_reads_as_a_count() {
        for metrics in [
            json!({"inputTokenCount": 21, "costInDollars": 1}),
            json!({"outputTokenCount": "22"}),
            json!([22]),
        ] {
            let mut events = vec![start(&json!({"input_tokens": 21}))];
            events.extend(text_events());
            events.push(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}));
            events.push(json!({"type":"message_stop","amazon-bedrock-invocationMetrics":metrics}));
            let (outcome, _) = replay(&stream(&events), 4096).await;
            let error = outcome.unwrap_err();
            assert_eq!(error.dispatch, Dispatch::Accepted);
            assert!(!error.retriable);
        }
    }

    #[tokio::test]
    async fn a_padding_field_is_ignored_and_any_other_envelope_field_is_refused() {
        let mut bytes = chunk_frame(&json!({
            "bytes": STANDARD.encode(start(&json!({"input_tokens": 1})).to_string()),
            "p": "abcdefghijklmnopqrstuvwxyzABCDEFGH",
        }));
        let mut events = text_events();
        events.push(
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":1}}),
        );
        events.push(json!({"type":"message_stop"}));
        bytes.extend(stream(&events));
        let (outcome, _) = replay(&bytes, 4096).await;
        assert_eq!(outcome.unwrap().items, vec![Item::assistant("Hello")]);

        let refused = chunk_frame(&json!({
            "bytes": STANDARD.encode(start(&json!({})).to_string()),
            "extra": true,
        }));
        let (outcome, _) = replay(&refused, 4096).await;
        assert!(outcome.is_err());
        let not_base64 = chunk_frame(&json!({"bytes": "not base64!"}));
        let (outcome, _) = replay(&not_base64, 4096).await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn an_exception_is_retriable_only_before_the_stream_answered() {
        let mut before = chunk(&start(&json!({"input_tokens": 1})));
        before.extend(exception("throttlingException"));
        let (outcome, sink) = replay(&before, 4096).await;
        let error = outcome.unwrap_err();
        assert_eq!(sink.events(), []);
        assert!(error.retriable);
        assert_eq!(error.dispatch, Dispatch::Accepted);

        let mut after = chunk(&start(&json!({"input_tokens": 1})));
        after.extend(chunk(&text_events()[0]));
        after.extend(chunk(&text_events()[1]));
        after.extend(exception("throttlingException"));
        let (outcome, sink) = replay(&after, 4096).await;
        let error = outcome.unwrap_err();
        assert_eq!(sink.text(), "Hello");
        assert!(!error.retriable);
        assert_eq!(error.dispatch, Dispatch::Accepted);
    }

    #[tokio::test]
    async fn a_frame_that_is_not_a_chunk_event_is_refused() {
        let other = encode_message(&frame(
            &[
                (":event-type", "metadata"),
                (":content-type", "application/json"),
                (":message-type", "event"),
            ],
            b"{}".to_vec(),
        ));
        let (outcome, _) = replay(&other, 4096).await;
        assert_eq!(outcome.unwrap_err().dispatch, Dispatch::Accepted);
    }

    #[tokio::test]
    async fn the_recorded_whole_answer_decodes_through_invoke() {
        let mut sink = VecSink::new(16, 64 * 1024);
        let outcome = decode_response(ANSWER, &request(), &provenance(), &mut sink, &Cancel::new())
            .await
            .unwrap();
        assert_eq!(sink.text(), RECORDED_TEXT);
        assert_eq!(outcome.items, vec![Item::assistant(RECORDED_TEXT)]);
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(
            outcome.observation.response_id,
            Some(Id::new("msg_01E6sPP1ksqicYCaBrkvzna8").unwrap())
        );
        // `input_tokens` 21 without either cache counter: the input total stays unknown.
        assert_eq!(
            outcome.observation.usage,
            Some(Usage {
                input_tokens: None,
                output_tokens: Some(22),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_output_tokens: None,
            })
        );
    }

    #[tokio::test]
    async fn a_whole_answer_that_is_not_json_is_refused_after_dispatch_and_shows_nothing() {
        let mut sink = VecSink::new(16, 64 * 1024);
        let error = decode_response(
            b"not json",
            &request(),
            &provenance(),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.dispatch, Dispatch::Accepted);
        assert_eq!(sink.events(), []);
    }

    #[test]
    fn the_body_carries_the_bedrock_version_and_neither_model_nor_stream() {
        let body: Value =
            serde_json::from_slice(&encode_request(&request(), &target()).unwrap()).unwrap();
        assert_eq!(body["anthropic_version"], json!(BEDROCK_ANTHROPIC_VERSION));
        assert_eq!(body["anthropic_version"], json!("bedrock-2023-05-31"));
        assert!(body.get("model").is_none());
        assert!(body.get("stream").is_none());
        // The recorded request's body, with the rolling prompt-cache breakpoint this projection
        // places on the last block.
        assert_eq!(
            body,
            json!({
                "anthropic_version": "bedrock-2023-05-31",
                "max_tokens": 50,
                "temperature": 0.0,
                "messages": [{"role": "user", "content": [{
                    "type": "text",
                    "text": "summarize the plot to the lord of the rings in a dozen words",
                    "cache_control": {"type": "ephemeral"},
                }]}],
            })
        );
    }

    #[test]
    fn a_target_that_is_not_messages_is_refused_before_io() {
        let mut target = target();
        target.provenance.protocol = Protocol::Responses;
        assert!(encode_request(&request(), &target).is_err());
    }
}
