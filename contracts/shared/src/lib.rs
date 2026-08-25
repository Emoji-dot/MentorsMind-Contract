#![no_std]
#![allow(deprecated)] // Temporarily allow deprecated Events::publish until we migrate to #[contractevent]

use soroban_sdk::contracterror;

/// Shared contract primitives reused across multiple Soroban modules.
///
/// Centralizing these definitions keeps authorization and state-transition
/// behavior aligned across contracts that make the same safety assumptions.
pub mod admin;
pub mod atomic_state;
pub mod cartel_detection;
pub mod community_protection;
pub mod cross_contract_auth;
pub mod disaster_recovery;
pub mod emergency;
pub mod emergency_rollback;
pub mod escrow;
pub mod events;
pub mod failure_tracking;
pub mod gas_estimation;
pub mod governance_voting;
pub mod interface_id;
pub mod justice_protection;
pub mod learner_protection;
pub mod outcome_authenticity;
pub mod pause_guard;
pub mod pricing_protection;
pub mod privacy_protection;
pub mod reentrancy_guard;
pub mod reputation;
pub mod safe_math;
pub mod scalability_protection;
pub mod sig_validation;
pub mod staking;
pub mod state_machine;
pub mod storage;
pub mod storage_compatibility;
pub mod threat_intelligence;
pub mod ttl_utils;
pub mod validation;

pub use admin::{
    AdminChangeProposal, AdminTransfer, ADMIN_COOLING_OFF_SECS, MIN_ADMIN_TIMELOCK_SECS,
};
pub use atomic_state::{
    all_checkpoints_passed, compute_transition_proof_hash, is_transition_expired,
    AtomicStateValidator, CrossContractStateCheck, InvalidStateRecord, PostConditionCheck,
    PreConditionCheck, StateTransitionContext, StateTransitionProof, MAX_CHECKPOINT_COUNT,
    STATE_TRANSITION_LOCK_TTL, STATE_TRANSITION_TIMEOUT_SECS,
};
pub use cartel_detection::{
    AvailabilityChange, CartelActivityRecord, CartelDetection, CartelDetectionError,
    CartelDetectionResult, CoordinationPattern, TimeSlotFairnessAnalysis, TimeSlotInfo,
};
pub use community_protection::{
    compute_community_intervention, detect_coordination, detect_coordination_ring,
    evaluate_fair_access, is_restoration_eligible, validate_network_authenticity,
    verify_social_proof, CommunityInterventionRecord, CoordinationFlag, FairAccessDecision,
    NetworkEffectScore, SocialProofRecord, COMMUNITY_INTERVENTION_THRESHOLD,
    COORDINATION_MIN_INTERACTIONS, COORDINATION_RISK_THRESHOLD, COORDINATION_TIGHT_WINDOW_SECS,
    NETWORK_DISTINCT_SOURCE_MIN_BPS, NETWORK_SUSPICIOUS_GROWTH_PER_DAY,
    SOCIAL_PROOF_BURST_WINDOW_SECS, SOCIAL_PROOF_MIN_DISTINCT_BPS,
};
pub use cross_contract_auth::{ContractRegistry, CrossContractAuth, InterfaceRegistryLookup};
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
pub use escrow::{EscrowRecord, EscrowStatus, EscrowTransitionLog};
pub use failure_tracking::{
    calculate_backoff_delay, calculate_next_retry, classify_failure, compute_failure_hash,
    ExponentialBackoff, FailureClassification, RecoveryState, ReleaseFailure,
    MANUAL_RECOVERY_THRESHOLD, MAX_AUTO_RELEASE_ATTEMPTS,
};
pub use gas_estimation::GasEstimate;
pub use governance_voting::{
    calculate_voting_weight, compute_commitment_hash, compute_random_deadline_extension,
    detect_vote_manipulation, get_vote_phase, validate_minimum_holding_period, ManipulationFlag,
    RevealedVote, VoteCommitment, VotePhase, COMMIT_PHASE_BPS, MAX_RANDOM_EXTENSION_SECS,
    MIN_HOLDING_PERIOD_SECS,
};
pub use justice_protection::{
    compute_justice_intervention, ensure_dispute_independence, is_justice_restoration_eligible,
    protect_arbitration_fairness, validate_evidence_authenticity, ArbitrationBiasFlag,
    DisputeIndependenceFlag, EvidenceAuthenticity, JusticeInterventionRecord,
    ARBITRATION_BIAS_RATIO_BPS_THRESHOLD, ARBITRATION_BIAS_RISK_THRESHOLD,
    ARBITRATION_MIN_RULINGS_FOR_BIAS, DISPUTE_COORDINATION_WINDOW_SECS,
    DISPUTE_INDEPENDENCE_RISK_THRESHOLD, EVIDENCE_DUPLICATE_WINDOW_SECS,
    EVIDENCE_TAMPER_RISK_THRESHOLD, JUSTICE_INTERVENTION_THRESHOLD,
    JUSTICE_RESTORATION_COOLDOWN_SECS,
};
pub use learner_protection::{
    assess_vulnerability, compute_emergency_intervention, compute_learner_protection_intervention,
    compute_welfare_status, detect_predatory_behavior, enforce_learner_fair_pricing,
    identify_exploitation_patterns, is_protection_restoration_eligible, EmergencyIntervention,
    ExploitationPattern, LearnerProtectionRecord, PredatoryBehaviorDetection,
    VulnerabilityAssessment, WelfareStatus, AFFORDABILITY_DEVIATION_BPS,
    EMERGENCY_PATTERN_THRESHOLD, EMERGENCY_SUSPENSION_COOLDOWN_SECS, FINANCIAL_PROTECTION_CAP_BPS,
    LEARNER_PROTECTION_COOLDOWN_SECS, PREDATORY_COMPLAINT_RATIO_BPS,
    PREDATORY_LOW_QUALITY_THRESHOLD, PREDATORY_RISK_THRESHOLD,
    VULNERABILITY_HIGH_RECURRENCE_THRESHOLD, VULNERABILITY_RISK_THRESHOLD,
    VULNERABILITY_SESSION_WINDOW,
};
pub use outcome_authenticity::{
    authenticate_learning_outcomes, compute_outcome_intervention, is_outcome_restoration_eligible,
    protect_success_metrics, validate_assessment_criteria, AssessmentValidation,
    OutcomeAuthenticity, OutcomeInterventionRecord, SuccessMetricProtection,
    ASSESSMENT_COORDINATION_WINDOW_SECS, ASSESSMENT_RISK_THRESHOLD, METRIC_GAMING_DEVIATION_BPS,
    OUTCOME_BURST_WINDOW_SECS, OUTCOME_INTERVENTION_THRESHOLD, OUTCOME_MIN_DISTINCT_BPS,
    OUTCOME_RESTORATION_COOLDOWN_SECS, OUTCOME_RISK_THRESHOLD,
};
pub use pause_guard::{is_paused, require_not_paused, ContractPaused};
pub use pricing_protection::{
    compute_pricing_intervention, detect_price_coordination, enforce_fair_pricing,
    validate_market_rate, verify_demand_authenticity, DemandAuthenticity, FairPricingResult,
    MarketRateValidation, PriceCoordinationFlag, PricingInterventionRecord,
    DEFAULT_MAX_MARKET_DEVIATION_BPS, DEMAND_BURST_WINDOW_SECS, DEMAND_MIN_DISTINCT_BPS,
    MAX_MARKET_DEVIATION_CEILING_BPS, PRICE_COORDINATION_WINDOW_SECS, PRICE_MATCH_TOLERANCE_BPS,
    PRICING_RISK_THRESHOLD,
};
pub use privacy_protection::{
    check_access, compute_privacy_intervention, detect_exploitation, minimize_to_need_to_know,
    AccessDecision, ConsentRecord, PrivacyInterventionRecord, PrivacyMonitoringResult,
    ACCESS_MONITORING_WINDOW_SECS, ALL_FIELDS, FIELD_CAREER_DATA, FIELD_CONTACT, FIELD_IDENTITY,
    FIELD_LEARNING_HISTORY, FIELD_PAYMENT, MAX_ACCESSES_PER_WINDOW, MINIMAL_SESSION_FIELDS,
    PRIVACY_RISK_THRESHOLD,
};
pub use reentrancy_guard::{
    validate_amount_limits, validate_caller_is_authorized, AtomicBatch, BatchOp,
    ReentrancyAttemptLog, ReentrancyGuard, StateSnapshot,
};
pub use reputation::{
    analyze_review_pattern, detect_sybil, interaction_commitment, BehavioralAnalysis,
    ReputationProof, SybilDetection,
};
pub use safe_math::SafeMath;
pub use scalability_protection::{
    compute_scalability_intervention, detect_resource_competition, distribute_resources_fairly,
    is_performance_restoration_eligible, validate_load_pattern, FairResourceAllocation,
    LoadValidationResult, PerformanceInterventionRecord, ResourceCompetitionFlag,
    FAIR_ALLOCATION_MAX_SHARE_BPS, LOAD_SUSPICIOUS_RATE_PER_MINUTE,
    PERFORMANCE_INTERVENTION_THRESHOLD, PERFORMANCE_RESTORATION_COOLDOWN_SECS,
    RESOURCE_BURST_WINDOW_SECS, RESOURCE_COMPETITION_RISK_THRESHOLD, RESOURCE_MIN_DISTINCT_BPS,
};
pub use sig_validation::{
    current_nonce, is_deadline_valid, validate_and_consume_nonce, validate_deadline, MetaTxAction,
    MetaTxPayload, SigError, EXPIRY_TOLERANCE_SECS, MAX_DEADLINE_SECS,
};
pub use staking::{
    action_claim, action_stake, action_unstake, apply_bps_multiplier,
    compute_early_unstake_penalty, compute_reward_multiplier_bps, detect_suspicious_pattern,
    PenaltyCalculation, RewardLockup, StakeRecord, StakedEventData, StakingActionRecord,
    StakingSnapshot, SuspiciousPatternFlag, BASIS_POINTS, EARLY_UNSTAKE_PENALTY_MAX_BPS,
    EARLY_UNSTAKE_PENALTY_MIN_BPS, MAX_SCALING_DURATION_SECS, MIN_STAKING_DURATION_SECS,
    PATTERN_DETECTION_WINDOW, REWARD_LOCKUP_SECS, REWARD_MULTIPLIER_MAX_BPS,
    REWARD_MULTIPLIER_MIN_BPS, SUSPICIOUS_CYCLE_THRESHOLD_SECS,
};
pub use state_machine::StateMachine;
pub use storage::{
    CollisionDetector, CollisionDetector as CollisionDetection, SecureStorageAccess,
    StorageAccessControl, StorageIntegrity, StorageIntegrityRecord, StorageKeyDerivation,
    StorageKeyFingerprint, StorageNamespace, StorageSecurityError, STORAGE_DERIVE_CTX,
};
pub use storage::{EternalStorage, InstanceKey, PersistentKey, StorageType, TempKey};
pub use storage_compatibility::{
    CompatibilityError, CompatibilityReport, CompatibilityValidator, GradualMigrationStatus,
    MigrationScript, StorageField, StorageFieldType, StorageLayoutSchema, StorageVersion,
};
pub use threat_intelligence::{
    assess_delegation_concentration, assess_review_quality, assess_token_velocity,
    correlate_attack_vectors, DelegationConcentrationReport, EconomicVelocityReport,
    MultiVectorThreatReport, ReviewQualityReport, DEFAULT_DELEGATION_CAP_BPS,
    ECONOMIC_VELOCITY_WARN_BPS, GOVERNANCE_CONCENTRATION_WARN_BPS, MULTI_VECTOR_RESPONSE_THRESHOLD,
    REVIEW_MANIPULATION_THRESHOLD,
};
pub use ttl_utils::{
    next_bump_interval, should_bump_ttl, AlertLevel, DataBackupRecord, DataDependencyTracker,
    DependencyItem, ExpirationMonitor, TTLAlert, TTLManager, TTLRecoveryManager,
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, ONE_DAY_LEDGERS, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD, SAFETY_MARGIN_LEDGERS, SEVEN_DAYS_LEDGERS,
    TEMPORARY_BUMP_AMOUNT, TEMPORARY_LIFETIME_THRESHOLD, THIRTY_DAYS_LEDGERS,
    WARNING_THRESHOLD_LEDGERS,
};
pub use validation::{require_auth_and_validate, ValidationError, Validator};

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
