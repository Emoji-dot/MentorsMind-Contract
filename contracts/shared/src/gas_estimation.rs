//! Shared gas-estimation type for on-chain view functions.
//!
//! `env.budget()` (soroban-sdk's instruction/memory metering handle) is only
//! available under the `testutils` feature, so it can't be used inside a
//! deployed contract's estimate function to predict the cost of an
//! operation that hasn't run yet. Estimate functions therefore use a
//! heuristic derived from current storage state and the number of
//! cross-contract calls the real operation performs; tests compare the
//! heuristic against `env.budget()` readings taken around the real
//! operation.
use soroban_sdk::contracttype;

/// A heuristic cost estimate for a protocol operation, returned by
/// `estimate_*` view functions (#761). Never mutates state.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GasEstimate {
    /// Estimated CPU instructions the operation will consume.
    pub base_instructions: u64,
    /// Estimated number of storage reads.
    pub storage_reads: u32,
    /// Estimated number of storage writes.
    pub storage_writes: u32,
    /// Estimated number of cross-contract invocations (token transfers,
    /// oracle/reputation/snapshot lookups, etc.).
    pub cross_contract_calls: u32,
}
