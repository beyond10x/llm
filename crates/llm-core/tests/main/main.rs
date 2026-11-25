//! The one integration-test binary of `llm-core`: each module is a former `tests/*.rs` file.

mod adversary_w08_controls;
mod adversary_w11_interrupt_pattern;
mod adversary_w2026_11_09_block_boundaries_pass2;
mod adversary_w21_unpublished;
mod adversary_w26_image_input;
mod adversary_w26_non_exhaustive;
mod adversary_wave3_opaque;
mod capabilities;
mod controls;
mod dependency_boundary;
mod embedding;
mod image_input;
mod item_kind;
mod observations;
mod retry_class;
mod structured_output;
mod unpublished_calls;
