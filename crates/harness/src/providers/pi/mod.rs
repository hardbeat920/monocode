//! Port of src/integrations/harness/providers/pi and providers/omp (they share
//! piFamily.ts), plus src/features/sessions/model/ompInterjections.ts.
//!
//! omp is a fork of Pi that speaks the same `--mode rpc` JSONL protocol, so
//! both providers run on one core. `PiFlavor` holds what differs.

mod deps;
#[cfg(test)]
mod family_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod testing;

pub mod adapter;
pub mod catalog;
pub mod client;
pub mod family;
pub mod flavor;
pub mod interjections;
pub mod omp;
pub mod protocol;
pub mod skills;
pub mod subagents;
pub mod text;
pub mod title;

pub use adapter::{PiFamilyAdapter, register_pi};
pub use flavor::{OMP_FLAVOR, PI_FLAVOR, PiFlavor};
pub use omp::register_omp;

use crate::core::register::HarnessContext;

/// Register the Pi and omp adapters. Idempotent.
pub fn register(ctx: &HarnessContext) {
    register_pi(ctx);
    register_omp(ctx);
}
