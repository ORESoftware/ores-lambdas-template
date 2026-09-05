//! __PREFIX__-lambdas — provider-neutral function core.
//!
//! `runtime` is the only event-validation and dispatch authority. Everything under
//! `adapters` is a thin translation from one provider's invocation shape into a
//! [`runtime::Invocation`] and back; adapters never contain business rules.
#![forbid(unsafe_code)]

pub mod runtime;
pub mod adapters;

pub use runtime::{dispatch, Invocation, Operation, Provider, Receipt, SCHEMA_VERSION, MAX_INVOCATION_BYTES};
