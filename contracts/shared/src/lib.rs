#![no_std]
#![allow(deprecated)] // Temporarily allow deprecated Events::publish until we migrate to #[contractevent]

use soroban_sdk::contracterror;

/// Shared contract primitives reused across multiple Soroban modules.
///
/// Centralizing these definitions keeps authorization and state-transition
/// behavior aligned across contracts that make the same safety assumptions.
pub mod admin;
pub mod cross_contract_auth;
pub mod disaster_recovery;
pub mod emergency;
pub mod emergency_rollback;
pub mod escrow;
pub mod events;
pub mod gas_estimation;
pub mod governance_voting;
pub mod pause_guard;
pub mod reentrancy_guard;
pub mod safe_math;
pub mod sig_validation;
pub mod state_machine;
pub mod staking;
pub mod storage;
pub mod storage_compatibility;
pub mod ttl_utils;
pub mod interface_id;
pub mod validation;
pub mod reputation;
pub mod failure_tracking;
pub mod atomic_state;
pub mod community_protection;
pub mod pricing_protection;
pub mod privacy_protection;
pub mod network_authenticity;
pub mod competency_validation;
pub mod benchmark_integrity;
pub mod emergency_response_protection;

pub use admin::{
    AdminChangeProposal, AdminTransfer, ADMIN_COOLING_OFF_SECS, MIN_ADMIN_TIMELOCK_SECS,
};
pub use disaster_recovery::{
    compute_checksum, push_snapshot_index, RollbackApproval, RollbackProposal, SnapshotMeta,
    StateVerificationReport, EMERGENCY_SIGNERS, EMERGENCY_THRESHOLD, MAX_SNAPSHOTS,
};
pub use emergency::{
    EmergencyAction, EmergencyAdminRole, EmergencyAuditRecord, EmergencyCircuitBreaker,
    EmergencyMultisig, MultisigValidation, EMERGENCY_ADMIN_TTL_SECS, EMERGENCY_CIRCUIT_WINDOW_SECS,
    EMERGENCY_MSIG_SIGNERS, EMERGENCY_MSIG_THRESHOLD, EMERGENCY_RELEASE_CAP_BPS,
    EMERGENCY_TIMELOCK_SECS,
};
pub use emergency_rollback::{
    EmergencyRollback, ImmutableRollbackAuditRecord, RollbackAuthorization, RollbackJustification,
    RollbackScope, ROLLBACK_COMMUNITY_REVIEW_SECS, ROLLBACK_GOVERNANCE_QUORUM_BPS,
    ROLLBACK_MAX_WINDOW_SECS,
};
pub use safe_math::SafeMath;
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
pub use storage::{
    CollisionDetector, CollisionDetector as CollisionDetection, SecureStorageAccess,
    StorageAccessControl, StorageIntegrity, StorageKeyDerivation, StorageKeyFingerprint,
    StorageIntegrityRecord, StorageNamespace, StorageSecurityError, STORAGE_DERIVE_CTX,
};
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
pub use failure_tracking::{
    ReleaseFailure, FailureClassification, ExponentialBackoff, RecoveryState,
    calculate_backoff_delay, classify_failure, calculate_next_retry, compute_failure_hash,
    MAX_AUTO_RELEASE_ATTEMPTS, MANUAL_RECOVERY_THRESHOLD,
};
pub use atomic_state::{
    StateTransitionContext, PreConditionCheck, PostConditionCheck, CrossContractStateCheck,
    StateTransitionProof, InvalidStateRecord, AtomicStateValidator, compute_transition_proof_hash,
    all_checkpoints_passed, is_transition_expired, STATE_TRANSITION_TIMEOUT_SECS,
    STATE_TRANSITION_LOCK_TTL, MAX_CHECKPOINT_COUNT,
};
pub use community_protection::{
    detect_coordination, detect_coordination_ring, validate_network_authenticity,
    verify_social_proof, evaluate_fair_access, compute_community_intervention,
    is_restoration_eligible, CoordinationFlag, NetworkEffectScore, SocialProofRecord,
    FairAccessDecision, CommunityInterventionRecord, COORDINATION_MIN_INTERACTIONS,
    COORDINATION_TIGHT_WINDOW_SECS, COORDINATION_RISK_THRESHOLD,
    NETWORK_DISTINCT_SOURCE_MIN_BPS, NETWORK_SUSPICIOUS_GROWTH_PER_DAY,
    SOCIAL_PROOF_BURST_WINDOW_SECS, SOCIAL_PROOF_MIN_DISTINCT_BPS,
    COMMUNITY_INTERVENTION_THRESHOLD,
};
pub use pricing_protection::{
    detect_price_coordination, validate_market_rate, enforce_fair_pricing,
    verify_demand_authenticity, compute_pricing_intervention, PriceCoordinationFlag,
    MarketRateValidation, FairPricingResult, DemandAuthenticity, PricingInterventionRecord,
    PRICE_COORDINATION_WINDOW_SECS, PRICE_MATCH_TOLERANCE_BPS, PRICING_RISK_THRESHOLD,
    DEFAULT_MAX_MARKET_DEVIATION_BPS, MAX_MARKET_DEVIATION_CEILING_BPS,
    DEMAND_BURST_WINDOW_SECS, DEMAND_MIN_DISTINCT_BPS,
};
pub use privacy_protection::{
    check_access, minimize_to_need_to_know, detect_exploitation, compute_privacy_intervention,
    ConsentRecord, AccessDecision, PrivacyMonitoringResult, PrivacyInterventionRecord,
    FIELD_IDENTITY, FIELD_CONTACT, FIELD_LEARNING_HISTORY, FIELD_CAREER_DATA, FIELD_PAYMENT,
    MINIMAL_SESSION_FIELDS, ALL_FIELDS, ACCESS_MONITORING_WINDOW_SECS,
    MAX_ACCESSES_PER_WINDOW, PRIVACY_RISK_THRESHOLD,
};

/// Economic sanity ceiling for a single financial amount (token smallest units).
pub const MAX_FINANCIAL_AMOUNT: i128 = 1_000_000_000_000_000;
/// Absolute upper bound for fee basis-points helpers in shared validation.
pub const MAX_FEE_BPS: u32 = 10_000;

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
