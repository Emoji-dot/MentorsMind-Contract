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
pub mod objective_assessment;
pub mod grade_inflation;
pub mod assessment_authenticity;
pub mod mentor_wellness;
pub mod recording_integrity;
pub mod market_monitoring;
pub mod community_protection;
pub mod pricing_protection;
pub mod privacy_protection;
pub mod justice_protection;
pub mod outcome_authenticity;
pub mod scalability_protection;
pub mod learner_protection;

// Security integrity system modules (added on main; module declarations
// restored here — main shipped the files without registering them).
pub mod assessment_security;
pub mod transfer_security;
pub mod ml_security;
pub mod cartel_detection;

// Fraud/security protection modules covering skill verification (#891),
// escrow payment integrity (#886), scheduling integrity (#884), and
// cross-session data isolation (#899).
pub mod skill_verification;
pub mod payment_integrity;
pub mod scheduling_integrity;
pub mod session_privacy;

// IP Protection and Content Security modules
pub mod content_authentication;
pub mod ip_protection;
pub mod content_licensing;
pub mod plagiarism_detection;
pub mod platform_interoperability;
pub mod identity_verification;
pub mod availability_pricing_protection;

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
pub use objective_assessment::{
    AssessmentCriteria, GradingRubric, ObjectiveAssessment, PeerReviewValidation, AssessmentCalibration,
    validate_rubric_weights, calculate_weighted_score, validate_peer_review, calibrate_rubric, apply_calibration,
    MAX_CRITERIA_COUNT, MIN_PEER_REVIEWERS, CONSENSUS_THRESHOLD_BPS, GRADE_STD_DEV_THRESHOLD,
};
pub use grade_inflation::{
    GradeDistributionStats, InflationDetectionResult, MentorScoringAdjustment, GradeCorrectionRecord,
    calculate_grade_distribution, detect_grade_inflation, apply_inflation_adjustment, record_grade_correction,
    MIN_SESSIONS_FOR_ANALYSIS, OUTLIER_ZSCORE_THRESHOLD, INFLATION_WINDOW, MAX_INFLATION_RATE_BPS, INFLATION_PENALTY_BPS_PER_DETECTION,
};
pub use assessment_authenticity::{
    ValidationSource, ValidationResult, AuthenticityVerification, ConsensusRecord,
    submit_validation, verify_assessment_authenticity, perform_consensus_validation, check_source_diversity, verify_blockchain_attestation,
    MIN_VALIDATION_SOURCES, VALIDATION_CONSENSUS_BPS, MAX_VALIDATION_AGE_SECS,
};
pub use mentor_wellness::{
    SessionDifficulty, MentorWorkload, BurnoutRiskAssessment, SessionDistributionRequest, FairDistributionResult,
    WellnessIntervention, EmergencyProtection,
    update_mentor_workload, calculate_burnout_risk, assess_burnout_risk, distribute_sessions_fairly,
    initiate_intervention, activate_emergency_protection, can_accept_session,
    MAX_CONCURRENT_SESSIONS, MAX_WEEKLY_HOURS, MIN_REST_HOURS, BURNOUT_RISK_THRESHOLD_BPS, MANDATORY_REST_HOURS, DIFFICULTY_WEIGHTS,
};
pub use recording_integrity::{
    RecordingStatus, AccessRole, SessionRecording, ConsentRecord, RedactionRecord, AccessLogEntry, IntegrityVerificationResult,
    create_recording, compute_merkle_root, verify_recording_integrity, grant_consent, revoke_consent,
    check_access_authorized, apply_redaction, log_access, emergency_privacy_protection,
    MAX_RECORDING_SIZE_MB, MIN_CONSENT_DURATION_HOURS, DEFAULT_RETENTION_DAYS,
};
pub use market_monitoring::{
    MarketMetrics, DemandAuthenticityResult, SupplyDemandBalance, PriceDiscoveryValidation, MarketManipulationAlert, EmergencyStabilization,
    calculate_market_metrics, assess_demand_authenticity, balance_supply_demand, validate_price_discovery, detect_market_manipulation, trigger_emergency_stabilization,
    MIN_MARKET_DATA_POINTS, MAX_PRICE_DEVIATION_BPS, ARTIFICIAL_DEMAND_THRESHOLD_BPS, SUPPLY_RESTRICTION_THRESHOLD_BPS, STABILIZATION_THRESHOLD_BPS,
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
    ConsentRecord as PrivacyConsentRecord, AccessDecision, PrivacyMonitoringResult, PrivacyInterventionRecord,
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
pub use assessment_security::{
    AssessmentRecord, AssessmentSecurity, AssessmentSecurityError, GamingDetectionResult,
    GamingFlag, ManipulationRecord, ProgressAuthenticityRecord,
};
pub use ml_security::{AIPerformanceMetrics, MLSecurity, ModelRobustnessReport};
pub use cartel_detection::{CoordinationPattern, CartelDetectionResult, TimeSlotFairnessAnalysis};
pub use transfer_security::{
    CredentialAuthenticityProof, CredentialFraudType, CredentialTransfer,
    CreditInflationRecord, CrossPlatformVerification, FraudDetectionResult,
    TransferIntegrityResult, TransferSecurity, TransferSecurityError,
};
pub use skill_verification::{
    score_practical_assessment, validate_peer_consensus, authenticate_external_credential,
    evaluate_domain_governance, detect_skill_fraud, compute_recertification_due,
    PracticalAssessment, PeerValidationRecord, ExpertiseAuthenticationRecord,
    SpecializationGovernanceRecord, SkillFraudFlag, RecertificationSchedule,
    MIN_PEER_VALIDATORS, PASSING_ASSESSMENT_SCORE_BPS, RECERTIFICATION_PERIOD_SECS,
    SKILL_FRAUD_RISK_THRESHOLD, MIN_SESSIONS_FOR_EXPERTISE_TRACKING,
    EXPERTISE_UNDERPERFORMANCE_THRESHOLD_BPS,
};
pub use payment_integrity::{
    validate_evidence_sufficiency, verify_completion_criteria,
    detect_payment_timing_manipulation, check_multisig_threshold, compute_emergency_isolation,
    EvidenceSufficiency, PaymentTimingCheck, EscrowMultisigApproval, EmergencyFundLock,
    PaymentAuditEntry, MIN_EVIDENCE_ITEMS, MIN_DISPUTE_COOLDOWN_SECS,
    ESCROW_MULTISIG_THRESHOLD, PAYMENT_TIMING_RISK_THRESHOLD, RAPID_ACTION_WINDOW_SECS,
};
pub use scheduling_integrity::{
    compute_availability_commitment, verify_availability_commitment, validate_conflict_proof,
    compute_random_tiebreak, assign_fair_slot, detect_availability_gaming,
    AvailabilityCommitment, ConflictProof, FairSchedulingDecision, SchedulingAuditRecord,
    AvailabilityGamingFlag, MIN_COMMITMENT_LEAD_SECS, MAX_CONFLICT_PROOF_AGE_SECS,
    GAMING_RISK_THRESHOLD, RAPID_AVAILABILITY_CHANGE_WINDOW_SECS,
};
pub use session_privacy::{
    enforce_session_boundary, detect_cross_session_leak, contain_data_breach,
    SessionAccessBoundary, CrossSessionLeakResult, DataBreachContainment,
    CROSS_SESSION_MONITORING_WINDOW_SECS, LEAK_DISTINCT_SESSION_THRESHOLD,
    BREACH_RISK_THRESHOLD,
};

// IP Protection and Content Security re-exports
pub use content_authentication::{
    create_watermark, verify_watermark, register_ownership, verify_ownership,
    DigitalWatermark, AuthenticationResult, OwnershipRecord,
    WATERMARK_VERIFICATION_THRESHOLD_BPS, MAX_WATERMARK_AGE_SECS,
};
pub use ip_protection::{
    detect_unauthorized_distribution, record_usage, initiate_enforcement,
    verify_usage_authorization, calculate_usage_statistics, determine_protection_level,
    UnauthorizedDistributionRecord, UsageTrackingEntry, IPEnforcementRecord, UsageReport,
    ProtectionLevel, MAX_UNAUTHORIZED_BEFORE_ESCALATION, MAX_DISTRIBUTIONS_BEFORE_LOCKDOWN,
};
pub use content_licensing::{
    create_license, has_permission, record_license_usage, calculate_revenue_shares,
    configure_revenue_share, grant_permission, validate_license, check_usage_limit,
    License, LicenseType, Permission, RevenueShare, LicenseUsageRecord, PermissionGrant,
    MIN_REVENUE_SHARE_BPS, MAX_REVENUE_SHARE_BPS, DEFAULT_CREATOR_SHARE_BPS,
};
pub use plagiarism_detection::{
    create_fingerprint, compare_fingerprints, analyze_content_segments,
    detect_plagiarism, create_plagiarism_report, segment_content, assess_plagiarism_risk,
    ContentFingerprint, PlagiarismDetectionResult, ContentSegment, PlagiarismReport,
    PLAGIARISM_CONFIDENCE_THRESHOLD_BPS, DEFAULT_SEGMENT_SIZE,
};
pub use platform_interoperability::{
    create_standardized_export, detect_lock_in, record_dependency, assess_vendor_neutrality,
    verify_compliance, define_data_portability, request_data_export, restore_competitive_choice,
    StandardizedDataExport, DependencyRelationship, LockInDetectionResult,
    VendorNeutralityAssessment, InteroperabilityCompliance, DataPortability,
    LOCK_IN_THRESHOLD_BPS, VENDOR_NEUTRALITY_THRESHOLD_BPS,
};
pub use identity_verification::{
    register_biometric, create_behavioral_fingerprint, detect_account_correlation,
    detect_multi_account_abuse, propagate_penalty, verify_unique_identity,
    generate_device_signature, assess_multi_account_risk,
    BiometricData, BehavioralFingerprint, AccountCorrelationResult,
    MultiAccountDetectionResult, PenaltyPropagation, DeviceSignature,
    BIOMETRIC_CONFIDENCE_THRESHOLD_BPS, ACCOUNT_CORRELATION_THRESHOLD_BPS,
};
pub use availability_pricing_protection::{
    create_availability_commitment, verify_availability_commitment,
    detect_price_coordination, validate_dynamic_pricing, analyze_availability_pattern,
    detect_artificial_scarcity, calculate_fair_availability_requirement,
    enforce_fair_pricing, trigger_availability_intervention,
    AvailabilityCommitment, PriceCoordinationFlag, DynamicPricingValidation,
    AvailabilityPattern, BASE_HOURS_PER_WEEK, MAX_PRICE_DEVIATION_BPS,
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
