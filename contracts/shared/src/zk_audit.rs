//! Minimal audit-commitment primitive (first increment toward #872).
//!
//! This is deliberately narrow in scope: it provides a cryptographic
//! commitment to an audit entry's data (a SHA-256 hash) so the entry's
//! existence and integrity can be recorded and later verified on-chain
//! without storing the underlying sensitive data itself. It is a real,
//! usable building block for selective disclosure (the pre-image is
//! revealed only to parties who need it, off-chain or via a separate
//! authorized call) — it is NOT a zero-knowledge proof system, threshold
//! decryption scheme, or automated compliance reporting pipeline. Those
//! remain future work; this establishes the commitment primitive they
//! would build on.

use soroban_sdk::{contracttype, Bytes, BytesN, Env};

/// A commitment to an audit entry: a hash of its data plus the timestamp
/// it was recorded, so the commitment itself reveals nothing about the
/// underlying entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCommitment {
    /// SHA-256 hash of the audit entry's serialized data.
    pub commitment: BytesN<32>,
    /// Ledger timestamp the commitment was created.
    pub committed_at: u64,
}

/// Commit to an audit entry's data without revealing it on-chain.
pub fn commit_audit_entry(env: &Env, data: &Bytes) -> AuditCommitment {
    AuditCommitment {
        commitment: env.crypto().sha256(data).into(),
        committed_at: env.ledger().timestamp(),
    }
}

/// Verify that `data` is the pre-image of a previously recorded `commitment`
/// — the selective-disclosure check a party would run once given access to
/// the underlying data out-of-band.
pub fn verify_audit_commitment(env: &Env, commitment: &AuditCommitment, data: &Bytes) -> bool {
    let recomputed: BytesN<32> = env.crypto().sha256(data).into();
    recomputed == commitment.commitment
}
