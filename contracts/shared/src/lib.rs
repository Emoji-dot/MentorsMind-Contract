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
pub mod cross_chain_sync;
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
pub mod key_management;
pub mod learner_protection;
pub mod outcome_authenticity;
pub mod pagination;
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
pub mod transaction_guard;
pub mod ttl_utils;
pub mod validator_accountability;
pub mod validation;

use soroban_sdk::{contracttype, symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env, Symbol, Vec};

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
pub use pagination::{
    BoundedIteration, BudgetExceeded, OperationBudget, Pagination, MAX_PAGE_SIZE,
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
    BatchValidationError, ReentrancyAttemptLog, ReentrancyGuard, StateSnapshot, MAX_BATCH_SIZE,
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
pub use validation::{require_auth_and_validate, ValidationError, Validator};

// ---------------------------------------------------------------------------
// #866 — Cross-Chain State Synchronization
// ---------------------------------------------------------------------------
pub use cross_chain_sync::{
    acknowledge_prepare, begin_atomic_xchain_op, compute_state_merkle_root, confirm_commit,
    confirm_rollback, expire_xchain_op, get_chain_isolation, get_inconsistency,
    get_xchain_op, initiate_rollback, is_chain_isolated, is_reorg_safe, isolate_chain,
    lift_chain_isolation, record_inconsistency, record_reorg_event, require_finality,
    validate_state_proof, AtomicXChainOp, ChainFinalityConfig, ChainIsolationRecord,
    CrossChainInconsistency, CrossChainStateProof, FinalityTier, XChainPhase,
    XChainSyncError, MAX_PARTICIPATING_CHAINS, MIN_FINALITY_CONFIRMATIONS,
    REORG_SAFE_DEPTH, XCHAIN_OP_TIMEOUT_SECS,
};

// ---------------------------------------------------------------------------
// #867 — Social Engineering / Transaction-Intent Protection
// ---------------------------------------------------------------------------
pub use transaction_guard::{
    add_multisig_approval, create_multisig_requirement, evaluate_transaction_intent,
    get_protection_state, is_multisig_satisfied, record_suspicious_pattern,
    require_account_not_blocked, require_cooling_off_elapsed, unblock_account,
    AccountProtectionState, MultiSigRequirement, RiskLevel, SuspiciousPattern,
    TransactionIntent, AUTO_BLOCK_SCORE_THRESHOLD, COOLING_OFF_PERIOD_SECS,
    EMERGENCY_COOLING_OFF_SECS, HIGH_VALUE_THRESHOLD_BPS, MAX_OPS_PER_WINDOW,
};

// ---------------------------------------------------------------------------
// #868 — Advanced Cryptographic Key Management
// ---------------------------------------------------------------------------
pub use key_management::{
    approve_social_recovery, derive_child_key_commitment, emergency_revoke_key,
    execute_key_rotation, execute_social_recovery, get_current_key, get_guardians,
    initiate_social_recovery, is_key_revoked, is_registered_guardian,
    is_reinstate_eligible, is_rotation_due, is_scheme_supported,
    is_threshold_met, propose_key_rotation, register_guardian,
    register_key, register_threshold_share, submit_threshold_share,
    KeyRecord, KeyRevocationRecord, KeyRotationProposal, KeyScheme,
    SocialRecoverySession, ThresholdKeyShare, DEFAULT_THRESHOLD_K, DEFAULT_THRESHOLD_N,
    KEY_ROTATION_OVERLAP_SECS, KEY_ROTATION_PERIOD_SECS, MAX_GUARDIANS,
    MAX_THRESHOLD_SHARES, REVOCATION_COOLDOWN_SECS, SOCIAL_RECOVERY_QUORUM,
};

// ---------------------------------------------------------------------------
// #869 — Validator Accountability / Consensus Attack Resistance
// ---------------------------------------------------------------------------
pub use validator_accountability::{
    activate_emergency_consensus, apply_slash, assess_incentive_alignment,
    compute_network_anomaly_score, deactivate_emergency_consensus, detect_consensus_attack,
    get_emergency_state, get_validator_record, graduated_slash_bps, is_emergency_active,
    is_validator_ejected, record_epoch_participation, record_missed_epoch,
    readmit_validator, register_validator, select_healthy_validators,
    ConsensusAnomalyRecord, EmergencyConsensusState, IncentiveAlignmentScore,
    SlashingEvent, ValidatorRecord, ViolationType,
    ATTACK_EJECTION_THRESHOLD, EJECTION_COOLDOWN_SECS, EMERGENCY_TRIGGER_SCORE,
    INITIAL_REPUTATION_SCORE, MAX_REPUTATION_SCORE, MIN_REPUTATION_SCORE,
    REPUTATION_PENALTY_ATTACK, REPUTATION_PENALTY_EQUIVOCATION,
    REPUTATION_PENALTY_MISSED, SLASH_CRITICAL_BPS, SLASH_MAJOR_BPS, SLASH_MINOR_BPS,
};

/// Layer-2 and state-channel integration metadata tracked by contracts that
/// need to defer L1 commitment until the applicable challenge window ends.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct L2Integration {
    pub network_id: u32,
    pub finality_delay_secs: u64,
    pub challenge_period_secs: u64,
    pub last_l2_block: u64,
    pub last_l1_commitment: u64,
    pub emergency_shutdown: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateChannelRecord {
    pub channel_id: BytesN<32>,
    pub party_a: Address,
    pub party_b: Address,
    pub opened_at: u64,
    pub dispute_deadline: u64,
    pub force_closed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossLayerAtomicityRecord {
    pub operation_id: BytesN<32>,
    pub l1_started: bool,
    pub l2_started: bool,
    pub committed: bool,
    pub rolled_back: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FraudProof {
    pub l2_network_id: u32,
    pub state_root: BytesN<32>,
    pub invalid_transition_hash: BytesN<32>,
    pub challenger: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossLayerAuditLog {
    pub operation_id: BytesN<32>,
    pub contract_id: Address,
    pub source_layer: Symbol,
    pub target_layer: Symbol,
    pub synchronized: bool,
    pub observed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameTheoryState {
    pub collusion_score_bps: u32,
    pub audit_sample_rate_bps: u32,
    pub penalty_multiplier_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveCompatibilityResult {
    pub strategy_proof: bool,
    pub honest_nash_equilibrium: bool,
    pub confidence_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollusionDetection {
    pub suspicious: bool,
    pub coordination_score_bps: u32,
    pub evidence_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZKProof {
    pub scheme: Symbol,
    pub circuit_hash: BytesN<32>,
    pub proof_hash: BytesN<32>,
    pub nullifier: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivacyAudit {
    pub linkability_risk_bps: u32,
    pub anonymity_breach: bool,
    pub audited_at: u64,
}

pub fn l2_finality_reached(env: &Env, integration: &L2Integration, l2_block_age_secs: u64) -> bool {
    !integration.emergency_shutdown
        && l2_block_age_secs >= integration.finality_delay_secs + integration.challenge_period_secs
        && env.ledger().timestamp() >= integration.last_l1_commitment
}

pub fn verify_l2_fraud_proof(proof: &FraudProof, expected_state_root: &BytesN<32>) -> bool {
    &proof.state_root == expected_state_root
}

pub fn record_cross_layer_audit(
    env: &Env,
    contract_id: &Address,
    operation_id: &BytesN<32>,
    source_layer: Symbol,
    target_layer: Symbol,
    synchronized: bool,
) -> CrossLayerAuditLog {
    let log = CrossLayerAuditLog {
        operation_id: operation_id.clone(),
        contract_id: contract_id.clone(),
        source_layer,
        target_layer,
        synchronized,
        observed_at: env.ledger().timestamp(),
    };
    env.events().publish(
        (symbol_short!("xl"), symbol_short!("audit")),
        (
            log.operation_id.clone(),
            log.contract_id.clone(),
            log.source_layer.clone(),
            log.target_layer.clone(),
            log.synchronized,
            log.observed_at,
        ),
    );
    log
}

pub fn compute_nullifier(
    env: &Env,
    address: &Address,
    namespace: &str,
    secret: &Bytes,
    blinding: &BytesN<32>,
) -> BytesN<32> {
    let mut payload = Bytes::new(env);
    payload.append(&address.to_xdr(env));
    payload.append(&Bytes::from_slice(env, namespace.as_bytes()));
    payload.append(secret);
    let blind = Bytes::from_slice(env, &blinding.to_array());
    payload.append(&blind);
    env.crypto().sha256(&payload).into()
}

pub fn audit_privacy(proof: &ZKProof, signal_strength_bps: u32, audited_at: u64) -> PrivacyAudit {
    let _ = proof;
    let linkability_risk_bps = u32::min(10_000, signal_strength_bps.saturating_add(750));
    PrivacyAudit {
        linkability_risk_bps,
        anonymity_breach: linkability_risk_bps > 7_500,
        audited_at,
    }
}

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
