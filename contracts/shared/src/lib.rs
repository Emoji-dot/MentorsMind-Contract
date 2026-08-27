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
pub mod reputation;
pub mod failure_tracking;
pub mod atomic_state;
pub mod community_protection;
pub mod pricing_protection;
pub mod privacy_protection;
pub mod justice_protection;
pub mod outcome_authenticity;
pub mod scalability_protection;
pub mod learner_protection;
pub mod mev_protection;
pub mod resource_management;
pub mod platform_authenticity;
pub mod dynamic_fees;

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
    detect_layout_tampering, CompatibilityError, CompatibilityReport, CompatibilityValidator,
    GradualMigrationStatus, MigrationScript, StorageField, StorageFieldType, StorageLayoutSchema,
    StorageVersion,
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
pub use justice_protection::{
    ensure_dispute_independence, validate_evidence_authenticity, protect_arbitration_fairness,
    compute_justice_intervention, is_justice_restoration_eligible,
    DisputeIndependenceFlag, EvidenceAuthenticity, ArbitrationBiasFlag, JusticeInterventionRecord,
    DISPUTE_COORDINATION_WINDOW_SECS, DISPUTE_INDEPENDENCE_RISK_THRESHOLD,
    EVIDENCE_DUPLICATE_WINDOW_SECS, EVIDENCE_TAMPER_RISK_THRESHOLD,
    ARBITRATION_MIN_RULINGS_FOR_BIAS, ARBITRATION_BIAS_RATIO_BPS_THRESHOLD,
    ARBITRATION_BIAS_RISK_THRESHOLD, JUSTICE_INTERVENTION_THRESHOLD,
    JUSTICE_RESTORATION_COOLDOWN_SECS,
};
pub use outcome_authenticity::{
    authenticate_learning_outcomes, protect_success_metrics, validate_assessment_criteria,
    compute_outcome_intervention, is_outcome_restoration_eligible,
    OutcomeAuthenticity, SuccessMetricProtection, AssessmentValidation, OutcomeInterventionRecord,
    OUTCOME_BURST_WINDOW_SECS, OUTCOME_MIN_DISTINCT_BPS, OUTCOME_RISK_THRESHOLD,
    METRIC_GAMING_DEVIATION_BPS, ASSESSMENT_COORDINATION_WINDOW_SECS, ASSESSMENT_RISK_THRESHOLD,
    OUTCOME_INTERVENTION_THRESHOLD, OUTCOME_RESTORATION_COOLDOWN_SECS,
};
pub use scalability_protection::{
    detect_resource_competition, validate_load_pattern, distribute_resources_fairly,
    compute_scalability_intervention, is_performance_restoration_eligible,
    ResourceCompetitionFlag, LoadValidationResult, FairResourceAllocation,
    PerformanceInterventionRecord,
    RESOURCE_BURST_WINDOW_SECS, RESOURCE_MIN_DISTINCT_BPS, RESOURCE_COMPETITION_RISK_THRESHOLD,
    LOAD_SUSPICIOUS_RATE_PER_MINUTE, FAIR_ALLOCATION_MAX_SHARE_BPS,
    PERFORMANCE_INTERVENTION_THRESHOLD, PERFORMANCE_RESTORATION_COOLDOWN_SECS,
};
pub use learner_protection::{
    assess_vulnerability, detect_predatory_behavior, enforce_learner_fair_pricing,
    identify_exploitation_patterns, compute_welfare_status,
    compute_learner_protection_intervention, compute_emergency_intervention,
    is_protection_restoration_eligible,
    VulnerabilityAssessment, PredatoryBehaviorDetection, ExploitationPattern,
    WelfareStatus, EmergencyIntervention, LearnerProtectionRecord,
    VULNERABILITY_SESSION_WINDOW, VULNERABILITY_HIGH_RECURRENCE_THRESHOLD,
    VULNERABILITY_RISK_THRESHOLD, AFFORDABILITY_DEVIATION_BPS,
    FINANCIAL_PROTECTION_CAP_BPS, PREDATORY_LOW_QUALITY_THRESHOLD,
    PREDATORY_COMPLAINT_RATIO_BPS, PREDATORY_RISK_THRESHOLD,
    EMERGENCY_PATTERN_THRESHOLD, EMERGENCY_SUSPENSION_COOLDOWN_SECS,
    LEARNER_PROTECTION_COOLDOWN_SECS,
};
pub use mev_protection::{
    detect_atomic_arbitrage, enforce_protocol_isolation, compute_mev_redistribution, record_mev_monitoring,
    MevProtectionFlag, FairValueExtractionRecord, MevMonitoringRecord,
    MEV_ARBITRAGE_RISK_THRESHOLD, DEFAULT_MEV_PENALTY_BPS, MAX_MEV_PENALTY_BPS,
};
pub use resource_management::{
    allocate_system_resources, manage_session_load, detect_abuse_patterns, check_emergency_trigger,
    RateLimitStatus, ResourceAllocation, AbuseDetectionResult,
    DEFAULT_MAX_REQUESTS_PER_MINUTE, ABUSE_PATTERN_THRESHOLD_BPS, EMERGENCY_THROTTLE_RATE, RESOURCE_QUOTA_MAX_SESSIONS,
};
pub use platform_authenticity::{
    verify_session_authenticity, detect_platform_bypass, detect_fee_evasion,
    AuthenticityResult, CollusionResult, EconomicAuditResult, PenaltyTier,
    MAX_LOW_FEE_SESSIONS_PER_PAIR, LOW_FEE_THRESHOLD, REQUIRED_INTERACTION_MINUTES, FEE_EVASION_TOLERANCE_BPS,
};
pub use dynamic_fees::{
    calculate_dynamic_fee, detect_fee_gaming,
    DynamicFeeResult, FeeEvasionResult,
    BASE_FEE_BPS, MIN_FEE_BPS, HIGH_LOAD_THRESHOLD,
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
