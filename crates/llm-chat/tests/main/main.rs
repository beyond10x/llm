//! The integration tests of `llm-chat`, linked as one test binary: one module per subject, and
//! the fixture bindings they share in `common`.

mod adversary;
mod adversary_pass_2;
mod adversary_tool_result;
mod adversary_tool_result_2;
mod adversary_toolname;
mod adversary_toolname_2;
mod adversary_w08_fields;
mod adversary_w10_cut_count;
mod adversary_w11_llama_timings;
mod adversary_w12_timings;
mod adversary_w2026_11_09_block_boundaries_pass2;
mod adversary_w21_unpublished;
mod adversary_w24_refusal;
mod adversary_w25_structured_output;
mod common;
mod every_output_delta;
mod image_input;
mod incoming;
mod ingress;
mod local_endpoint;
mod outgoing;
mod retry_class;
mod retry_signal;
mod structured_output;
mod usage_reader;
