//! The one integration-test binary of `llm-http`: each module is a former `tests/*.rs` file.
//!
//! Two modules start this executable again as a child and name the case to run by its module
//! path (`ambient_proxy`, `adversary_w25_form_exchange`): renaming a module moves that name.

mod adversary_untyped;
mod adversary_w10_refusal_bounds;
mod adversary_w11_pass2_json_body;
mod adversary_w11_stream_queue;
mod adversary_w25_form_exchange;
mod ambient_proxy;
mod byte_stream;
mod cancellation;
mod connect_and_idle;
mod form_exchange;
mod framing;
mod json_exchange;
mod no_tokio_runtime;
mod retry_classes;
mod security_w25_form_exchange;
mod transport;
