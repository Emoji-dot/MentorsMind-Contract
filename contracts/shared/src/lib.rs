#![no_std]
#![allow(deprecated)] // Temporarily allow deprecated Events::publish until we migrate to #[contractevent]

use soroban_sdk::contracterror;

/// Shared contract primitives reused across multiple Soroban modules.
///
/// Centralizing these definitions keeps authorization and state-transition
/// behavior aligned across contracts that make the same safety assumptions.
pub mod cross_contract_auth;
pub mod disaster_recovery;
pub mod escrow;
pub mod events;
pub mod gas_estimation;
pub mod governance_voting;
pub mod pause_guard;
pub mod reentrancy_guard;
pub mod sig_validation;
pub mod state_machine;
pub mod staking;
pub mod storage;
pub mod storage_compatibility;
pub mod ttl_utils;
pub mod interface_id;
pub mod validation;
pub mod reputation;
pub mod assessment_security;
pub mod ml_security;
pub mod cartel_detection;
pub mod transfer_security;

pub use disaster_recovery::{
    compute_checksum, push_snapshot_index, RollbackApproval, RollbackProposal, SnapshotMeta,
    StateVerificationReport, EMERGENCY_SIGNERS, EMERGENCY_THRESHOLD, MAX_SNAPSHOTS,
};
pub use cross_contract_auth::{ContractRegistry, CrossContractAuth, InterfaceRegistryLookup};
pub use escrow::{EscrowRecord, EscrowStatus, EscrowTransitionLog};
pub use gas_estimation::GasEstimate;
pub use governance_voting::{
    calculate_voting_weight, compute_commitment_hash, compute_random_deadline_extension,
    detect_vote_manipulation, get_vote_phase, validate_minimum_holding_period, ManipulationFlag,
    MAX_RANDOM_EXTENSION_SECS, MIN_HOLDING_PERIOD_SECS, COMMIT_PHASE_BPS, RevealedVote, VoteCommitment,
    VotePhase,
};
pub use pause_guard::{ContractPaused, is_paused, require_not_paused};
pub use reentrancy_guard::{
    AtomicBatch, BatchOp, ReentrancyAttemptLog, ReentrancyGuard, StateSnapshot,
    validate_amount_limits, validate_caller_is_authorized,
};
pub use sig_validation::{
    current_nonce, is_deadline_valid, validate_and_consume_nonce, validate_deadline,
    MetaTxAction, MetaTxPayload, SigError, EXPIRY_TOLERANCE_SECS, MAX_DEADLINE_SECS,
};
pub use state_machine::StateMachine;
pub use staking::{
    StakeRecord, StakedEventData, StakingSnapshot, RewardLockup, PenaltyCalculation,
    SuspiciousPatternFlag, StakingActionRecord, compute_reward_multiplier_bps,
    compute_early_unstake_penalty, detect_suspicious_pattern, apply_bps_multiplier,
    action_stake, action_unstake, action_claim,
    MIN_STAKING_DURATION_SECS, REWARD_LOCKUP_SECS, MAX_SCALING_DURATION_SECS,
    REWARD_MULTIPLIER_MIN_BPS, REWARD_MULTIPLIER_MAX_BPS,
    EARLY_UNSTAKE_PENALTY_MIN_BPS, EARLY_UNSTAKE_PENALTY_MAX_BPS,
    BASIS_POINTS, PATTERN_DETECTION_WINDOW, SUSPICIOUS_CYCLE_THRESHOLD_SECS,
};
pub use storage::{EternalStorage, StorageType, InstanceKey, PersistentKey, TempKey};
pub use storage_compatibility::{
    CompatibilityError, CompatibilityReport, CompatibilityValidator, GradualMigrationStatus,
    MigrationScript, StorageField, StorageFieldType, StorageLayoutSchema, StorageVersion,
};
pub use ttl_utils::{
    next_bump_interval, should_bump_ttl, AlertLevel, DataBackupRecord, DataDependencyTracker,
    DependencyItem, ExpirationMonitor, TTLAlert, TTLManager, TTLRecoveryManager,
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, ONE_DAY_LEDGERS, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD, SAFETY_MARGIN_LEDGERS, SEVEN_DAYS_LEDGERS,
    TEMPORARY_BUMP_AMOUNT, TEMPORARY_LIFETIME_THRESHOLD, THIRTY_DAYS_LEDGERS,
    WARNING_THRESHOLD_LEDGERS,
};
pub use validation::{Validator, ValidationError, require_auth_and_validate};
pub use reputation::{
    analyze_review_pattern, detect_sybil, interaction_commitment, BehavioralAnalysis,
    ReputationProof, SybilDetection,
};
pub use assessment_security::{
    AssessmentSecurity, AssessmentSecurityError, GamingDetectionResult,
    ProgressAuthenticityRecord, ManipulationRecord, GamingFlag,
};
pub use ml_security::{
    MLSecurity, MLSecurityError, AttackDetectionResult, ModelRobustnessReport,
    AIPerformanceMetrics, TrainingDataIntegrityRecord, PoisoningRecord, AdversarialAttackType,
};
pub use cartel_detection::{
    CartelDetection, CartelDetectionError, CartelDetectionResult, TimeSlotFairnessAnalysis,
    CartelActivityRecord, CartelSeverity,
};
pub use transfer_security::{
    TransferSecurity, TransferSecurityError, FraudDetectionResult, TransferIntegrityResult,
    CredentialAuthenticityProof, CrossPlatformVerification, CredentialFraudType,
};

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
    /// Input validation failed (see `ValidationError` for field details).
    ValidationError = 11,
    /// A cross-contract caller failed interface-registry verification.
    UnauthorizedContract = 12,
}
