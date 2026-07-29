//! # Pause Guardian Cross-Contract Integration
//!
//! Provides utilities for contracts to atomically check the pause state
//! via the pause_guardian contract before executing state-mutating operations.
//!
//! ## Design
//!
//! Payment-path entry points (deposit, stake, claim_rewards, deploy_escrow, etc.)
//! must call `require_not_paused(env, guardian_address)` at the top of the function.
//! If the guardian contract reports `is_paused() == true`, the call panics with
//! `ContractPaused` and the transaction is rolled back atomically.
//!
//! ## Performance
//!
//! Each cross-contract call to `is_paused()` is a single Soroban host invocation.
//! The pause state is checked **synchronously** within the same transaction ledger,
//! guaranteeing that pause takes effect immediately without requiring separate
//! ledger rounds.

use soroban_sdk::{Address, Env, Symbol};

/// Error returned when a contract is in a paused state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractPaused;

impl ContractPaused {
    pub fn msg() -> &'static str {
        "Contract is paused"
    }
}

/// Check the pause state atomically via cross-contract call.
///
/// # Arguments
///
/// * `env` — The Soroban contract environment.
/// * `guardian_address` — The address of the pause_guardian contract.
///
/// # Returns
///
/// `true` if the guardian reports `is_paused()`.
///
/// # Panics
///
/// Panics if the cross-contract call fails (e.g., guardian not found, invalid return type).
pub fn is_paused(env: &Env, guardian_address: &Address) -> bool {
    env.invoke_contract(
        guardian_address,
        &Symbol::new(env, "is_paused"),
        soroban_sdk::Vec::<soroban_sdk::Val>::new(env),
    )
}

/// Assert that the contract is not paused.
///
/// Calls `is_paused(env, guardian_address)` and panics with "Contract is paused"
/// if the result is `true`. Otherwise returns normally.
///
/// # Usage
///
/// ```ignore
/// pub fn deposit(env: Env, from: Address, token: Address, amount: i128) {
///     let guardian = get_pause_guardian(&env);
///     require_not_paused(&env, &guardian);
///     // ... rest of deposit logic
/// }
/// ```
pub fn require_not_paused(env: &Env, guardian_address: &Address) {
    if is_paused(env, guardian_address) {
        panic!("{}", ContractPaused::msg());
    }
}
