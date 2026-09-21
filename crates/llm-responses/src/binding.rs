use llm_core::{Error, Id, Protocol, Provenance};

/// The protocol this crate projects. A binding declaring anything else is refused.
pub const PROTOCOL: Protocol = Protocol::Responses;

/// The path this wire serves, appended to an endpoint's base URL.
pub const PATH: &str = "/responses";

/// One selected serving binding, plus the upstream model name that binding is configured to send.
///
/// The upstream name is separate on purpose. A neutral [`llm_core::TurnRequest`] carries the
/// operator's own model identifier, which is the one [`Provenance::model`] repeats; the name the
/// endpoint answers to is a catalog fact about that binding and is not in the request. Keeping
/// them apart is what lets ingress refuse a body addressed to a different upstream model instead
/// of serving it, and it is why no absent wire model is ever filled in from the caller's alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    provenance: Provenance,
    upstream_model: Id,
}

impl Binding {
    /// Binds one projection to one serving model.
    pub const fn new(provenance: Provenance, upstream_model: Id) -> Self {
        Self {
            provenance,
            upstream_model,
        }
    }

    /// The six coordinates opaque continuation state must match exactly.
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The model identifier this endpoint answers to, which is not the caller's alias.
    pub const fn upstream_model(&self) -> &Id {
        &self.upstream_model
    }

    /// # Errors
    /// Refuses a binding declaring a protocol this crate does not speak.
    pub fn validate(&self) -> Result<(), Error> {
        if self.provenance.protocol != PROTOCOL {
            return Err(Error::unsupported(
                "binding does not declare the Responses protocol",
            ));
        }
        Ok(())
    }
}
