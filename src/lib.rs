//! __PREFIX__-lambdas — provider-neutral function core.
//!
//! `runtime` is the only event-validation and dispatch authority. Everything under
//! `adapters` is a thin translation from one provider's invocation shape into a
//! [`runtime::Invocation`] and back; adapters never contain business rules.
#![forbid(unsafe_code)]

pub mod adapters;
pub mod runtime;

pub use runtime::{
    dispatch, Invocation, Operation, Provider, Receipt, MAX_INVOCATION_BYTES, SCHEMA_VERSION,
};
