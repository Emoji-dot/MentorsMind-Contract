//! Minimal proof-of-personhood attestation primitive (first increment
//! toward #873).
//!
//! This is not biometric verification or ML-based sybil detection —
//! it's the simplest honest building block those would sit on top of:
//! a time-bounded attestation issued by a trusted attester, which
//! reputation/governance logic can require before granting full
//! voting weight. Replacing the attester with a decentralized
//! biometric/social-graph pipeline is future work.

use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonhoodAttestation {
    /// The account this attestation vouches for.
    pub subject: Address,
    /// The trusted party that issued the attestation.
    pub attester: Address,
    /// Ledger timestamp the attestation was issued.
    pub issued_at: u64,
    /// Ledger timestamp after which the attestation must be renewed.
    pub expires_at: u64,
}

/// Whether an attestation is currently valid (not expired) as of `env`'s
/// current ledger timestamp.
pub fn is_attestation_valid(env: &Env, attestation: &PersonhoodAttestation) -> bool {
    env.ledger().timestamp() < attestation.expires_at
}
