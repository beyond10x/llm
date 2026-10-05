//! The conversation a caller may opt a client into, and the non-secret headers a request carries.

use llm_core::Id;

/// One conversation, minted by the caller, that a client serves.
///
/// Nothing here is required. A client without a conversation sends no cache key and no identity
/// header, which is what the Codex backend accepted on the 2026-10-04 probe. A client given one
/// sends, on every request, `prompt_cache_key` equal to [`Conversation::id`] in the body and the
/// identity headers [`request_headers`] lists. The route that wants them opts in; nothing in this
/// crate invents an identifier or an originator.
///
/// The identifier is the cache key on purpose: Harness measured `cached_tokens: 0` on every turn
/// with a prefix digest and 85% cached with the conversation's own id, which is what `codex`
/// sends (`harness-responses/src/project.rs:348`-`360`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    id: Id,
    originator: Option<Id>,
}

impl Conversation {
    /// A conversation with no named originator.
    pub const fn new(id: Id) -> Self {
        Self {
            id,
            originator: None,
        }
    }

    /// Names the calling application, sent as the `originator` header.
    #[must_use]
    pub fn with_originator(mut self, originator: Id) -> Self {
        self.originator = Some(originator);
        self
    }

    /// The identifier: the cache key and the `session-id` header.
    pub const fn id(&self) -> &Id {
        &self.id
    }

    /// The calling application the caller named, if any.
    pub const fn originator(&self) -> Option<&Id> {
        self.originator.as_ref()
    }
}

/// Every header a request carries other than authentication, in the order the client sets them.
///
/// `accept` and `content-type` always. With a conversation, `originator` when the caller named
/// one, `session-id`, and `x-client-request-id`: the identifier, `-`, and `request`, the number of
/// requests the client sent before this one, so each attempt is a new request of the same
/// conversation. The client sets exactly these, so a test or a fixture that pins this list pins
/// what is sent.
pub fn request_headers(
    conversation: Option<&Conversation>,
    request: u64,
) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("accept", "text/event-stream".to_owned()),
        ("content-type", "application/json".to_owned()),
    ];
    if let Some(conversation) = conversation {
        if let Some(originator) = conversation.originator() {
            headers.push(("originator", originator.as_str().to_owned()));
        }
        headers.push(("session-id", conversation.id().as_str().to_owned()));
        headers.push((
            "x-client-request-id",
            format!("{}-{request}", conversation.id().as_str()),
        ));
    }
    headers
}
