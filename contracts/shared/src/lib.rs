#![no_std]
#![allow(deprecated)] // Temporarily allow deprecated Events::publish until we migrate to #[contractevent]

use soroban_sdk::contracterror;

/// Shared contract primitives reused across multiple Soroban modules.
///
/// Centralizing these definitions keeps authorization and state-transition
/// behavior aligned across contracts that make the same safety assumptions.
pub mod content_protection;
pub mod escrow;
pub mod events;
pub mod ip_verification;
pub mod reentrancy_guard;
pub mod sig_validation;
pub mod state_machine;
pub mod storage;
pub mod staking;
pub mod ttl_utils;
pub mod usage_rights_management;

pub use content_protection::{ContentProtection, ContentType, EncryptionKey, AccessLevel, ProtectedContent, AccessLog};
pub use escrow::{EscrowRecord, EscrowStatus};
pub use ip_verification::{IPVerification, IPRecord, OwnershipProof, IPUsageRecord, InfringementRecord, IPType, IPStatus};
pub use reentrancy_guard::ReentrancyGuard;
pub use sig_validation::{
    current_nonce, is_deadline_valid, validate_and_consume_nonce, validate_deadline,
    MetaTxAction, MetaTxPayload, SigError, EXPIRY_TOLERANCE_SECS, MAX_DEADLINE_SECS,
};
pub use state_machine::StateMachine;
pub use staking::{StakeRecord, StakedEventData};
pub use storage::{EternalStorage, StorageType, InstanceKey, PersistentKey, TempKey};
pub use ttl_utils::{next_bump_interval, should_bump_ttl};
pub use usage_rights_management::{UsageRightsManager, LicenseType, ViolationPenalty, License, ViolationRecord};

/// Common error codes shared across all MentorsMind contracts.
///
/// Contracts may re-export or extend this enum; the numeric codes are stable
/// and used in off-chain tooling to distinguish error categories.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SharedError {
    /// `initialize` was called more than once on the contract.
    AlreadyInitialized = 1,
    /// A function requiring initialization was called before `initialize`.
    NotInitialized = 2,
    /// The caller lacks the required role (admin, mentor, learner, etc.).
    Unauthorized = 3,
    /// The requested record (escrow, user, token, etc.) does not exist.
    NotFound = 4,
    /// The supplied amount is zero, negative, or exceeds an allowed range.
    InvalidAmount = 5,
    /// The operation is not valid for the entity's current state.
    InvalidState = 6,
    /// An attempt was made to insert a record that already exists.
    DuplicateEntry = 7,
    /// The operation is not supported in the current contract configuration.
    UnsupportedOperation = 8,
    /// An arithmetic operation would overflow the integer bounds.
    Overflow = 9,
    /// An arithmetic operation would underflow below zero.
    Underflow = 10,
    /// Content access is denied due to insufficient permissions or invalid license.
    ContentAccessDenied = 11,
    /// Intellectual property ownership cannot be verified or is disputed.
    IPOwnershipInvalid = 12,
    /// Usage rights have been violated or exceeded allowed limits.
    UsageRightsViolation = 13,
    /// Content encryption/decryption failed.
    EncryptionError = 14,
    /// Content piracy or unauthorized distribution detected.
    PiracyDetected = 15,
}
