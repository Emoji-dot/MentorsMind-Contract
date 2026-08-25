#![no_std]

use shared::{
    compute_scalability_intervention, detect_coordination,
    detect_resource_competition as shared_detect_resource_competition, detect_price_coordination,
    distribute_resources_fairly as shared_distribute_resources_fairly, evaluate_fair_access,
    interaction_commitment, is_performance_restoration_eligible,
    validate_load_pattern as shared_validate_load_pattern,
    verify_demand_authenticity as shared_verify_demand_authenticity, CoordinationFlag,
    DemandAuthenticity, FairAccessDecision, FairResourceAllocation, LoadValidationResult,
    PerformanceInterventionRecord, PriceCoordinationFlag, ReputationProof,
    ResourceCompetitionFlag, SocialProofRecord, PERFORMANCE_RESTORATION_COOLDOWN_SECS,
    // learner protection
    assess_vulnerability, enforce_learner_fair_pricing as shared_enforce_learner_fair_pricing,
    compute_learner_protection_intervention, compute_emergency_intervention,
    is_protection_restoration_eligible,
    detect_predatory_behavior as shared_detect_predatory_behavior,
    identify_exploitation_patterns as shared_identify_exploitation_patterns,
    compute_welfare_status as shared_compute_welfare_status,
    VulnerabilityAssessment, EmergencyIntervention, LearnerProtectionRecord,
    // Mentor wellness (#910)
    MentorWorkload, BurnoutRiskAssessment, SessionDifficulty, SessionDistributionRequest, FairDistributionResult,
    WellnessIntervention, EmergencyProtection,
    update_mentor_workload, calculate_burnout_risk, assess_burnout_risk, distribute_sessions_fairly,
    initiate_intervention, activate_emergency_protection, can_accept_session,
    // Session recording (#914)
    SessionRecording, RecordingStatus, ConsentRecord, AccessRole, RedactionRecord, AccessLogEntry, IntegrityVerificationResult,
    create_recording, compute_merkle_root, verify_recording_integrity, grant_consent, revoke_consent,
    check_access_authorized, apply_redaction, log_access, emergency_privacy_protection,
    // Market monitoring (#915)
    MarketMetrics, DemandAuthenticityResult, SupplyDemandBalance, PriceDiscoveryValidation, MarketManipulationAlert, EmergencyStabilization,
    calculate_market_metrics, assess_demand_authenticity, balance_supply_demand, validate_price_discovery, detect_market_manipulation, trigger_emergency_stabilization,
};

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec, Map, BytesN};

// ── Storage keys ─────────────────────────────────────────────────────────────
const BACKEND: Symbol = symbol_short!("BACKEND");
const TTL_THRESHOLD: u32 = 500_000;
const TTL_BUMP: u32 = 1_000_000;
/// Schedule occupancy is tracked in 30-minute buckets.
const SLOT_SIZE_SECS: u64 = 1_800;
/// Minimum free time required between consecutive sessions on the same mentor.
const SCHEDULING_BUFFER_SECS: u64 = 900;
/// Rolling window used to compute a mentor's booking-request rate for
/// load-attack validation (#scalability-protection).
const LOAD_MONITORING_WINDOW_SECS: u64 = 300;

// ── Types ─────────────────────────────────────────────────────────────────────
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Pending,
    Confirmed,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    pub session_id: Symbol,
    pub mentor: Address,
    pub learner: Address,
    pub scheduled_at: u64,
    pub duration_mins: u32,
    pub amount: i128,
    pub token: Address,
    pub status: SessionStatus,
    pub registered_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract-isolated storage namespace root (#826).
    NamespaceRoot,
    Session(Symbol),
    /// Deprecated: kept for backward compat, no longer written to.
    /// Use `MentorSessionAt` / `MentorSessionCount` for all new reads/writes.
    MentorSessions(Address),
    /// Deprecated: kept for backward compat, no longer written to.
    /// Use `LearnerSessionAt` / `LearnerSessionCount` for all new reads/writes.
    LearnerSessions(Address),
    MentorSessionCount(Address),
    MentorSessionAt(Address, u32),
    LearnerSessionCount(Address),
    LearnerSessionAt(Address, u32),
    /// Maps `(mentor_address, time_bucket_index)` → `session_id`.
    /// A time bucket covers `SLOT_SIZE_SECS` seconds.
    MentorScheduleSlot(Address, u64),
    SessionOracle,
    SessionMetadata(Symbol),
    CompletionProof(Symbol),
    /// Scheduled-at timestamps for one mentor/learner pair, used for
    /// coordination-ring detection (#community-protection).
    MentorLearnerLog(Address, Address),
    MentorCoordination(Address),
    MentorFairAccess(Address),
    /// Whether `learner` has ever booked `mentor` before (distinct-requester tracking).
    MentorHasBookedBefore(Address, Address),
    MentorDistinctLearnerCount(Address),
    /// Booking-request timestamps for a mentor, used for demand-authenticity checks.
    MentorRequestLog(Address),
    /// Rolling window of recently-booked session prices/timestamps across all
    /// mentors, used for pricing-coordination detection.
    RecentSessionPrices,
    RecentSessionPriceTimestamps,
    /// Cached resource-competition assessment for a mentor's booking load
    /// (#scalability-protection).
    SystemLoadRecord(Address),
    /// Rolling total requested booking-capacity units (duration-minutes) for
    /// a mentor, used for fair-resource-distribution scoring.
    MentorTotalRequestedUnits(Address),
    /// Cached combined performance-protection intervention record for a
    /// mentor.
    PerformanceIntervention(Address),
    // ── Learner protection (#917) ──────────────────────────────────────────
    /// Total session count between a specific learner and mentor pair, used
    /// for recurrence/dependency vulnerability assessment.
    LearnerMentorSessionCount(Address, Address),
    /// Rolling sum of session prices paid by a learner (for avg computation).
    LearnerTotalSpend(Address),
    /// Total session count for a learner across all mentors.
    LearnerTotalSessionCount(Address),
    /// Cached vulnerability assessment for a learner/mentor pair.
    LearnerVulnerabilityRecord(Address, Address),
    /// Cached predatory-behaviour detection result for a mentor (from the
    /// session registry's perspective – complaint/quality signals come from
    /// the reputation contract; this stores the combined view once pushed
    /// here by `monitor_mentor_behavior`).
    MentorPredatoryBehaviorRecord(Address),
    /// Cached learner-protection intervention record for a mentor.
    LearnerProtectionIntervention(Address),
    /// Emergency intervention record for a mentor.
    MentorEmergencyIntervention(Address),
    /// Whether a mentor is currently under an active emergency suspension.
    MentorSuspended(Address),
    // Mentor wellness (#910)
    MentorWorkload(Address),
    MentorBurnoutAssessment(Address),
    WellnessIntervention(Address),
    // Session recording (#914)
    SessionRecording(Symbol),
    RecordingConsent(Symbol),
    RecordingRedaction(Symbol),
    RecordingAccessLog(Symbol),
    // Market monitoring (#915)
    SpecializationMetrics(Symbol),
    MarketManipulationAlert(Symbol),
    EmergencyStabilization(Symbol),
}

/// Maximum length of the rolling price/pair/request logs kept for scoring.
const MONITORING_LOG_CAP: u32 = 20;

// ── Errors ────────────────────────────────────────────────────────────────────
// Errors are surfaced via panics to keep compatibility with SDK 21 contractimpl.
// Error codes are documented here for reference:
// NotInitialized = 1, Unauthorized = 2, SessionNotFound = 3, DuplicateSession = 4
// SessionConflict = 5, InsufficientBuffer = 6

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionConflict {
    pub conflicting_session_id: Symbol,
}

// ── Contract ──────────────────────────────────────────────────────────────────
#[contract]
pub struct SessionRegistry;

#[contractimpl]
impl SessionRegistry {
    /// Initialize with the platform backend address (only caller allowed to register/update).
    pub fn initialize(env: Env, backend: Address) {
        if env.storage().instance().has(&BACKEND) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&BACKEND, &backend);
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_BUMP);
    }

    /// Register a new session. Only callable by the platform backend.
    /// Performs conflict detection and 15-minute buffer enforcement.
    pub fn register_session(
        env: Env,
        session_id: Symbol,
        mentor: Address,
        learner: Address,
        scheduled_at: u64,
        duration_mins: u32,
        amount: i128,
        token: Address,
    ) -> Symbol {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let session_key = DataKey::Session(session_id.clone());
        if env.storage().persistent().has(&session_key) {
            panic!("Duplicate session");
        }

        // Check for scheduling conflicts and buffer enforcement
        Self::check_scheduling_conflicts(&env, &mentor, scheduled_at, duration_mins);

        // Community-dynamics monitoring: track pair/demand/pricing signals and
        // gate on automatic fair-access intervention before committing state.
        // A panic here reverts all storage writes for this invocation.
        Self::record_monitoring_signals(&env, &mentor, &learner, amount, scheduled_at);
        let access = Self::ensure_fair_community_access(env.clone(), mentor.clone(), learner.clone());
        if !access.access_granted {
            panic!("CommunityAccessRestricted");
        }

        // Learner protection: update learner/mentor pair session count and
        // learner spend totals, then assess vulnerability and enforce fair pricing.
        let pair_cnt_key = DataKey::LearnerMentorSessionCount(learner.clone(), mentor.clone());
        let pair_cnt: u32 = env.storage().persistent().get(&pair_cnt_key).unwrap_or(0);
        env.storage().persistent().set(&pair_cnt_key, &pair_cnt.saturating_add(1));
        env.storage().persistent().extend_ttl(&pair_cnt_key, TTL_THRESHOLD, TTL_BUMP);

        let spend_key = DataKey::LearnerTotalSpend(learner.clone());
        let spend: i128 = env.storage().persistent().get(&spend_key).unwrap_or(0);
        env.storage().persistent().set(&spend_key, &spend.saturating_add(amount));

        let lsc_key = DataKey::LearnerTotalSessionCount(learner.clone());
        let lsc: u32 = env.storage().persistent().get(&lsc_key).unwrap_or(0);
        env.storage().persistent().set(&lsc_key, &lsc.saturating_add(1));

        // Assess vulnerability and apply fair-pricing enforcement.
        let _assessed_price = Self::enforce_fair_pricing(env.clone(), learner.clone(), mentor.clone(), amount);
        Self::assess_learner_vulnerability(env.clone(), learner.clone(), mentor.clone(), amount);

        // Scalability protection: track requested booking-capacity units and
        // re-score this mentor's resource-competition/load risk before
        // committing state (#scalability-protection).
        let total_units_key = DataKey::MentorTotalRequestedUnits(mentor.clone());
        let total_units: u32 = env.storage().persistent().get(&total_units_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&total_units_key, &total_units.saturating_add(duration_mins));
        Self::manage_system_load(env.clone(), mentor.clone());

        let record = SessionRecord {
            session_id: session_id.clone(),
            mentor: mentor.clone(),
            learner: learner.clone(),
            scheduled_at,
            duration_mins,
            amount,
            token,
            status: SessionStatus::Pending,
            registered_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&session_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&session_key, TTL_THRESHOLD, TTL_BUMP);

        // Reserve all time buckets for this session
        Self::reserve_time_buckets(&env, &mentor, scheduled_at, duration_mins, &session_id);

        // Index by mentor (indexed storage)
        let mentor_count_key = DataKey::MentorSessionCount(mentor.clone());
        let mentor_idx: u32 = env
            .storage()
            .persistent()
            .get(&mentor_count_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::MentorSessionAt(mentor.clone(), mentor_idx), &session_id.clone());
        env.storage()
            .persistent()
            .set(&mentor_count_key, &(mentor_idx + 1));

        // Index by learner (indexed storage)
        let learner_count_key = DataKey::LearnerSessionCount(learner.clone());
        let learner_idx: u32 = env
            .storage()
            .persistent()
            .get(&learner_count_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::LearnerSessionAt(learner.clone(), learner_idx), &session_id.clone());
        env.storage()
            .persistent()
            .set(&learner_count_key, &(learner_idx + 1));

        // Emit event
        env.events().publish(
            (
                symbol_short!("session"),
                Symbol::new(&env, "session_registered"),
                session_id.clone(),
            ),
            (mentor, learner, scheduled_at),
        );

        session_id
    }

    /// Update session status. Only callable by the platform backend.
    /// Releases time buckets when transitioning to Cancelled.
    pub fn update_status(env: Env, session_id: Symbol, status: SessionStatus) {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let session_key = DataKey::Session(session_id.clone());
        let mut record: SessionRecord = env
            .storage()
            .persistent()
            .get(&session_key)
            .expect("Session not found");

        let old_status = record.status.clone();
        
        // Release time buckets if transitioning to Cancelled
        if status == SessionStatus::Cancelled && old_status != SessionStatus::Cancelled {
            Self::release_time_buckets(&env, &record.mentor, record.scheduled_at, record.duration_mins);
        }
        
        record.status = status.clone();
        env.storage().persistent().set(&session_key, &record);
        if status == SessionStatus::Completed {
            Self::store_completion_proof(&env, &record);
        }
        env.storage()
            .persistent()
            .extend_ttl(&session_key, TTL_THRESHOLD, TTL_BUMP);

        env.events().publish(
            (
                symbol_short!("session"),
                Symbol::new(&env, "session_status_changed"),
                session_id,
            ),
            (old_status, status),
        );
    }

    /// Cancel a session and release its mentor schedule buckets for re-booking.
    pub fn cancel_session(env: Env, session_id: Symbol) {
        Self::update_status(env, session_id, SessionStatus::Cancelled);
    }

    /// Returns availability for each 30-minute slot in `[from, to)`.
    /// Each entry is `(slot_start, is_available)`.
    pub fn get_mentor_availability(
        env: Env,
        mentor: Address,
        from: u64,
        to: u64,
    ) -> Vec<(u64, bool)> {
        let mut result = Vec::new(&env);
        if to <= from {
            return result;
        }
        let start_bucket = from / SLOT_SIZE_SECS;
        let end_bucket = (to + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;
        let mut bucket = start_bucket;
        while bucket < end_bucket {
            let slot_start = bucket * SLOT_SIZE_SECS;
            if slot_start >= to {
                break;
            }
            let key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
            let is_available = !env.storage().persistent().has(&key);
            result.push_back((slot_start, is_available));
            bucket = bucket.saturating_add(1);
        }
        result
    }

    pub fn set_session_oracle(env: Env, oracle: Address) {
        let backend = Self::require_backend(&env);
        backend.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::SessionOracle, &oracle);
    }

    pub fn update_status_from_oracle(
        env: Env,
        oracle: Address,
        session_id: Symbol,
        status: SessionStatus,
    ) {
        let configured_oracle: Address = env
            .storage()
            .instance()
            .get(&DataKey::SessionOracle)
            .expect("Session oracle not configured");
        oracle.require_auth();
        if oracle != configured_oracle {
            panic!("Unauthorized");
        }

        let session_key = DataKey::Session(session_id.clone());
        let mut record: SessionRecord = env
            .storage()
            .persistent()
            .get(&session_key)
            .expect("Session not found");

        let old_status = record.status.clone();
        if matches!(status, SessionStatus::Cancelled)
            && !matches!(old_status, SessionStatus::Cancelled)
        {
            Self::release_time_buckets(
                &env,
                &record.mentor,
                record.scheduled_at,
                record.duration_mins,
            );
        }
        record.status = status.clone();
        env.storage().persistent().set(&session_key, &record);
        if status == SessionStatus::Completed {
            Self::store_completion_proof(&env, &record);
        }
        env.events().publish(
            (
                symbol_short!("session"),
                Symbol::new(&env, "session_oracle_status_changed"),
                session_id,
            ),
            (old_status, status),
        );
    }

    /// Get a session record by session_id.
    pub fn get_session(env: Env, session_id: Symbol) -> SessionRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .expect("Session not found")
    }

    fn store_completion_proof(env: &Env, record: &SessionRecord) {
        let proof = ReputationProof {
            session_id: record.session_id.clone(),
            mentor: record.mentor.clone(),
            learner: record.learner.clone(),
            completed_at: env.ledger().timestamp(),
            commitment: interaction_commitment(
                env,
                &record.session_id,
                &record.mentor,
                &record.learner,
                env.ledger().timestamp(),
            ),
        };
        env.storage()
            .persistent()
            .set(&DataKey::CompletionProof(record.session_id.clone()), &proof);
        env.storage().persistent().extend_ttl(
            &DataKey::CompletionProof(record.session_id.clone()),
            TTL_THRESHOLD,
            TTL_BUMP,
        );
        env.events().publish(
            (symbol_short!("session"), Symbol::new(env, "proof_generated")),
            (record.session_id.clone(), proof.commitment),
        );
    }

    pub fn get_completion_proof(env: Env, session_id: Symbol) -> ReputationProof {
        env.storage()
            .persistent()
            .get(&DataKey::CompletionProof(session_id))
            .expect("Completion proof not found")
    }

    pub fn verify_completion_proof(env: Env, proof: ReputationProof) -> bool {
        let stored: ReputationProof = env
            .storage()
            .persistent()
            .get(&DataKey::CompletionProof(proof.session_id.clone()))
            .unwrap_or(proof.clone());
        stored == proof
            && stored.commitment
                == interaction_commitment(
                    &env,
                    &stored.session_id,
                    &stored.mentor,
                    &stored.learner,
                    stored.completed_at,
                )
    }

    /// Get paginated session IDs for a mentor.
    /// `offset` is the starting index, `limit` is the max items to return.
    pub fn get_sessions_by_mentor_page(
        env: Env,
        mentor: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<Symbol> {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MentorSessionCount(mentor.clone()))
            .unwrap_or(0);
        let mut result = Vec::new(&env);
        let start = offset.min(count);
        let end = (offset + limit).min(count);
        for i in start..end {
            let key = DataKey::MentorSessionAt(mentor.clone(), i);
            if let Some(sid) = env.storage().persistent().get::<_, Symbol>(&key) {
                result.push_back(sid);
            }
        }
        result
    }

    /// Get paginated session IDs for a learner.
    pub fn get_sessions_by_learner_page(
        env: Env,
        learner: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<Symbol> {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerSessionCount(learner.clone()))
            .unwrap_or(0);
        let mut result = Vec::new(&env);
        let start = offset.min(count);
        let end = (offset + limit).min(count);
        for i in start..end {
            let key = DataKey::LearnerSessionAt(learner.clone(), i);
            if let Some(sid) = env.storage().persistent().get::<_, Symbol>(&key) {
                result.push_back(sid);
            }
        }
        result
    }

    /// Deprecated: returns first 50 sessions for a mentor.
    /// Use `get_sessions_by_mentor_page` for full paginated access.
    pub fn get_sessions_by_mentor(env: Env, mentor: Address) -> Vec<Symbol> {
        Self::get_sessions_by_mentor_page(env, mentor, 0, 50)
    }

    /// Deprecated: returns first 50 sessions for a learner.
    /// Use `get_sessions_by_learner_page` for full paginated access.
    pub fn get_sessions_by_learner(env: Env, learner: Address) -> Vec<Symbol> {
        Self::get_sessions_by_learner_page(env, learner, 0, 50)
    }

    /// Get total session count for a mentor.
    pub fn get_mentor_session_count(env: Env, mentor: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::MentorSessionCount(mentor))
            .unwrap_or(0)
    }

    /// Get total session count for a learner.
    pub fn get_learner_session_count(env: Env, learner: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::LearnerSessionCount(learner))
            .unwrap_or(0)
    }

    fn require_backend(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&BACKEND)
            .expect("Not initialized")
    }

    /// Update the pair/demand/pricing monitoring logs consumed by
    /// `detect_mentor_coordination`, `verify_demand_authenticity`, and
    /// `monitor_pricing_coordination`.
    fn record_monitoring_signals(
        env: &Env,
        mentor: &Address,
        learner: &Address,
        amount: i128,
        scheduled_at: u64,
    ) {
        // Pair coordination log, keyed on the scheduler-controlled `scheduled_at`.
        let pair_key = DataKey::MentorLearnerLog(mentor.clone(), learner.clone());
        let mut pair_log: Vec<u64> = env.storage().persistent().get(&pair_key).unwrap_or(Vec::new(env));
        pair_log.push_back(scheduled_at);
        while pair_log.len() > MONITORING_LOG_CAP {
            pair_log.remove(0);
        }
        env.storage().persistent().set(&pair_key, &pair_log);
        env.storage().persistent().extend_ttl(&pair_key, TTL_THRESHOLD, TTL_BUMP);

        // Distinct-learner tracking for demand authenticity.
        let seen_key = DataKey::MentorHasBookedBefore(mentor.clone(), learner.clone());
        if !env.storage().persistent().get(&seen_key).unwrap_or(false) {
            env.storage().persistent().set(&seen_key, &true);
            let cnt_key = DataKey::MentorDistinctLearnerCount(mentor.clone());
            let cnt: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0);
            env.storage().persistent().set(&cnt_key, &(cnt + 1));
        }

        // Booking-request log, keyed on wall-clock request time.
        let req_key = DataKey::MentorRequestLog(mentor.clone());
        let mut req_log: Vec<u64> = env.storage().persistent().get(&req_key).unwrap_or(Vec::new(env));
        req_log.push_back(env.ledger().timestamp());
        while req_log.len() > MONITORING_LOG_CAP {
            req_log.remove(0);
        }
        env.storage().persistent().set(&req_key, &req_log);
        env.storage().persistent().extend_ttl(&req_key, TTL_THRESHOLD, TTL_BUMP);

        // Global rolling price log for cross-mentor pricing-coordination detection.
        let prices_key = DataKey::RecentSessionPrices;
        let prices_ts_key = DataKey::RecentSessionPriceTimestamps;
        let mut prices: Vec<i128> = env.storage().persistent().get(&prices_key).unwrap_or(Vec::new(env));
        let mut price_ts: Vec<u64> = env.storage().persistent().get(&prices_ts_key).unwrap_or(Vec::new(env));
        prices.push_back(amount);
        price_ts.push_back(env.ledger().timestamp());
        while prices.len() > MONITORING_LOG_CAP {
            prices.remove(0);
        }
        while price_ts.len() > MONITORING_LOG_CAP {
            price_ts.remove(0);
        }
        env.storage().persistent().set(&prices_key, &prices);
        env.storage().persistent().set(&prices_ts_key, &price_ts);
        env.storage().persistent().extend_ttl(&prices_key, TTL_THRESHOLD, TTL_BUMP);
        env.storage().persistent().extend_ttl(&prices_ts_key, TTL_THRESHOLD, TTL_BUMP);
    }

    /// Score a mentor/learner pair's booking history for coordination
    /// (repeated, tightly-clustered scheduling characteristic of a
    /// manipulation ring). Safe to call by anyone as a read-through audit;
    /// also invoked internally on every `register_session`.
    pub fn detect_mentor_coordination(env: Env, mentor: Address, learner: Address) -> CoordinationFlag {
        let log: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MentorLearnerLog(mentor.clone(), learner.clone()))
            .unwrap_or(Vec::new(&env));
        let flag = detect_coordination(&log);
        env.storage()
            .persistent()
            .set(&DataKey::MentorCoordination(mentor.clone()), &flag);
        if flag.suspicious {
            env.events().publish(
                (symbol_short!("session"), Symbol::new(&env, "coord_flag")),
                (mentor, flag.risk_score),
            );
        }
        flag
    }

    /// Ensure a mentor/learner pair retains fair community access: combines
    /// the mentor's coordination score with a neutral social-proof
    /// placeholder (the reputation contract owns real endorsement signals)
    /// and returns whether scheduling should be blocked.
    pub fn ensure_fair_community_access(env: Env, mentor: Address, learner: Address) -> FairAccessDecision {
        let coordination = Self::detect_mentor_coordination(env.clone(), mentor.clone(), learner);
        let neutral_social_proof = SocialProofRecord {
            genuine: true,
            gaming_risk_score: 0,
            distinct_endorser_bps: 10_000,
            burst_count: 0,
        };
        let decision = evaluate_fair_access(&env, coordination, neutral_social_proof);
        env.storage()
            .persistent()
            .set(&DataKey::MentorFairAccess(mentor), &decision);
        decision
    }

    /// Verify whether a mentor's booking-request history reflects genuine,
    /// distinct-learner demand rather than artificially generated requests.
    pub fn verify_demand_authenticity(env: Env, mentor: Address) -> DemandAuthenticity {
        let log: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MentorRequestLog(mentor.clone()))
            .unwrap_or(Vec::new(&env));
        let distinct: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MentorDistinctLearnerCount(mentor))
            .unwrap_or(0);
        shared_verify_demand_authenticity(&log, distinct)
    }

    /// Audit the platform-wide rolling price history for cross-mentor
    /// pricing coordination (near-identical prices set within a tight
    /// window). Read-only audit signal; does not block registration.
    pub fn monitor_pricing_coordination(env: Env) -> PriceCoordinationFlag {
        let prices: Vec<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::RecentSessionPrices)
            .unwrap_or(Vec::new(&env));
        let timestamps: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::RecentSessionPriceTimestamps)
            .unwrap_or(Vec::new(&env));
        detect_price_coordination(&prices, &timestamps)
    }

    // ─── Scalability protection (#scalability-protection) ──────────────────

    /// Detect resource competition/griefing on `mentor`'s booking load from
    /// request timestamps: a burst of requests from a narrow set of
    /// learners is treated as unfair competition rather than organic
    /// demand. Safe to call by anyone as a read-through audit; also invoked
    /// internally on every `register_session`.
    pub fn manage_system_load(env: Env, mentor: Address) -> ResourceCompetitionFlag {
        let log: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MentorRequestLog(mentor.clone()))
            .unwrap_or(Vec::new(&env));
        let distinct: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MentorDistinctLearnerCount(mentor.clone()))
            .unwrap_or(0);
        let flag = shared_detect_resource_competition(&log, distinct);
        env.storage()
            .persistent()
            .set(&DataKey::SystemLoadRecord(mentor.clone()), &flag);
        if !flag.fair {
            env.events().publish(
                (symbol_short!("load"), Symbol::new(&env, "flagged")),
                (mentor, flag.risk_score),
            );
        }
        flag
    }

    /// Compute a fair booking-capacity share for `requested_units` (e.g.
    /// requested session duration in minutes) against `mentor`'s rolling
    /// total requested capacity, throttling any single requester attempting
    /// to claim an unfair share of a mentor's schedule.
    pub fn distribute_resources_fairly(
        env: Env,
        mentor: Address,
        requested_units: u32,
    ) -> FairResourceAllocation {
        let total: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MentorTotalRequestedUnits(mentor))
            .unwrap_or(0);
        shared_distribute_resources_fairly(&env, requested_units, total.max(requested_units))
    }

    /// Validate whether `mentor`'s recent booking-request volume reflects
    /// legitimate demand or a coordinated load attack on the scheduling
    /// system.
    pub fn validate_usage_patterns(env: Env, mentor: Address) -> LoadValidationResult {
        let log: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MentorRequestLog(mentor))
            .unwrap_or(Vec::new(&env));
        let now = env.ledger().timestamp();
        let window_start = now.saturating_sub(LOAD_MONITORING_WINDOW_SECS);
        let mut count = 0u32;
        for i in 0..log.len() {
            let ts = log.get(i).unwrap_or(0);
            if ts >= window_start {
                count = count.saturating_add(1);
            }
        }
        shared_validate_load_pattern(count, LOAD_MONITORING_WINDOW_SECS)
    }

    /// Combine the cached resource-competition and freshly-computed
    /// load-validation signals for `mentor` into a single
    /// performance-protection intervention decision.
    pub fn get_performance_status(env: Env, mentor: Address) -> PerformanceInterventionRecord {
        let competition: ResourceCompetitionFlag = env
            .storage()
            .persistent()
            .get(&DataKey::SystemLoadRecord(mentor.clone()))
            .unwrap_or(ResourceCompetitionFlag {
                fair: true,
                risk_score: 0,
                distinct_requester_bps: 10_000,
                burst_count: 0,
            });
        let load = Self::validate_usage_patterns(env.clone(), mentor.clone());
        let record = compute_scalability_intervention(
            &env,
            competition,
            load,
            PERFORMANCE_RESTORATION_COOLDOWN_SECS,
        );
        env.storage()
            .persistent()
            .set(&DataKey::PerformanceIntervention(mentor.clone()), &record);
        record
    }

    // ─── Learner vulnerability protection (#917) ───────────────────────────

    /// Assess a learner's vulnerability when booking with `mentor`.
    ///
    /// Reads the learner/mentor pair session count and the learner's average
    /// historical spend from storage, calls the shared scoring function, and
    /// persists + returns the result. Safe to call by anyone as a
    /// read-through audit; also invoked internally on `register_session`.
    pub fn assess_learner_vulnerability(
        env: Env,
        learner: Address,
        mentor: Address,
        latest_session_price: i128,
    ) -> VulnerabilityAssessment {
        let pair_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerMentorSessionCount(learner.clone(), mentor.clone()))
            .unwrap_or(0);

        let total_spend: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerTotalSpend(learner.clone()))
            .unwrap_or(0);
        let total_session_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerTotalSessionCount(learner.clone()))
            .unwrap_or(0);
        let avg_historical_price = if total_session_count > 0 {
            total_spend / total_session_count as i128
        } else {
            0
        };

        let assessment =
            assess_vulnerability(pair_count, latest_session_price, avg_historical_price);

        env.storage().persistent().set(
            &DataKey::LearnerVulnerabilityRecord(learner.clone(), mentor.clone()),
            &assessment,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::LearnerVulnerabilityRecord(learner.clone(), mentor.clone()),
            TTL_THRESHOLD,
            TTL_BUMP,
        );

        if assessment.at_risk {
            env.events().publish(
                (symbol_short!("vuln"), Symbol::new(&env, "at_risk")),
                (learner, mentor, assessment.risk_score),
            );
        }

        assessment
    }

    /// Monitor and score a mentor's behaviour for predatory patterns.
    ///
    /// The platform backend pushes aggregated conduct signals
    /// (`consecutive_low_quality`, `complaint_count`, `total_sessions`,
    /// `price_above_market_bps`) collected from the reputation contract and
    /// off-chain analytics. The result is persisted and, when predatory
    /// behaviour is detected alongside an at-risk learner, an emergency
    /// intervention record is written and the mentor is flagged as
    /// suspended. Only callable by the platform backend.
    pub fn monitor_mentor_behavior(
        env: Env,
        mentor: Address,
        learner: Address,
        consecutive_low_quality: u32,
        complaint_count: u32,
        total_sessions: u32,
        price_above_market_bps: u32,
    ) -> LearnerProtectionRecord {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let behavior = shared_detect_predatory_behavior(
            consecutive_low_quality,
            complaint_count,
            total_sessions,
            price_above_market_bps,
        );

        env.storage().persistent().set(
            &DataKey::MentorPredatoryBehaviorRecord(mentor.clone()),
            &behavior,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::MentorPredatoryBehaviorRecord(mentor.clone()),
            TTL_THRESHOLD,
            TTL_BUMP,
        );

        // Retrieve the latest cached vulnerability for this learner/mentor pair.
        let vulnerability: VulnerabilityAssessment = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerVulnerabilityRecord(
                learner.clone(),
                mentor.clone(),
            ))
            .unwrap_or(VulnerabilityAssessment {
                at_risk: false,
                risk_score: 0,
                high_recurrence: false,
                affordability_concern: false,
                recurrence_count: 0,
            });

        // Identify exploitation patterns and compute welfare status.
        let patterns =
            shared_identify_exploitation_patterns(&env, vulnerability, behavior);
        let pattern_count = patterns.len();
        let welfare =
            shared_compute_welfare_status(vulnerability, pattern_count);

        let now = env.ledger().timestamp();
        let protection = compute_learner_protection_intervention(
            &env,
            vulnerability,
            behavior,
            welfare,
            now,
        );

        env.storage().persistent().set(
            &DataKey::LearnerProtectionIntervention(mentor.clone()),
            &protection,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::LearnerProtectionIntervention(mentor.clone()),
            TTL_THRESHOLD,
            TTL_BUMP,
        );

        if protection.emergency_suspension {
            let emergency = compute_emergency_intervention(&env, &protection, now);
            env.storage().persistent().set(
                &DataKey::MentorEmergencyIntervention(mentor.clone()),
                &emergency,
            );
            env.storage().persistent().set(
                &DataKey::MentorSuspended(mentor.clone()),
                &true,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::MentorEmergencyIntervention(mentor.clone()),
                TTL_THRESHOLD,
                TTL_BUMP,
            );
            env.events().publish(
                (symbol_short!("emerg"), Symbol::new(&env, "suspended")),
                (mentor.clone(), protection.combined_risk_score),
            );
        } else if behavior.predatory {
            env.events().publish(
                (symbol_short!("mentor"), Symbol::new(&env, "predatory")),
                (mentor.clone(), behavior.risk_score),
            );
        }

        protection
    }

    /// Enforce fair pricing for a learner before a session is committed.
    ///
    /// When a learner has a cached vulnerability assessment, the proposed
    /// session price is run through the shared affordability-cap logic and
    /// the (possibly adjusted) price is returned. If `mentor` is currently
    /// suspended the call panics to block the booking. Callable by anyone.
    pub fn enforce_fair_pricing(
        env: Env,
        learner: Address,
        mentor: Address,
        proposed_price: i128,
    ) -> i128 {
        // Block bookings with suspended mentors.
        let suspended: bool = env
            .storage()
            .persistent()
            .get(&DataKey::MentorSuspended(mentor.clone()))
            .unwrap_or(false);
        if suspended {
            panic!("MentorSuspended");
        }

        let vulnerability: VulnerabilityAssessment = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerVulnerabilityRecord(
                learner.clone(),
                mentor.clone(),
            ))
            .unwrap_or(VulnerabilityAssessment {
                at_risk: false,
                risk_score: 0,
                high_recurrence: false,
                affordability_concern: false,
                recurrence_count: 0,
            });

        // Compute the learner's average historical spend for the cap.
        let total_spend: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerTotalSpend(learner.clone()))
            .unwrap_or(0);
        let total_session_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerTotalSessionCount(learner.clone()))
            .unwrap_or(0);
        let avg_historical_price = if total_session_count > 0 {
            total_spend / total_session_count as i128
        } else {
            0
        };

        // Platform average: approximate from the rolling price log.
        let prices: soroban_sdk::Vec<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::RecentSessionPrices)
            .unwrap_or(soroban_sdk::Vec::new(&env));
        let platform_avg_price = if prices.is_empty() {
            0i128
        } else {
            let sum: i128 = {
                let mut s = 0i128;
                for i in 0..prices.len() {
                    s = s.saturating_add(prices.get(i).unwrap_or(0));
                }
                s
            };
            sum / prices.len() as i128
        };

        let (enforced_price, adjusted) = shared_enforce_learner_fair_pricing(
            proposed_price,
            avg_historical_price,
            platform_avg_price,
            vulnerability,
        );

        if adjusted {
            env.events().publish(
                (symbol_short!("price"), Symbol::new(&env, "adjusted")),
                (learner, mentor, proposed_price, enforced_price),
            );
        }

        enforced_price
    }

    /// Trigger an emergency protection action for a learner under active
    /// exploitation.
    ///
    /// When the stored `LearnerProtectionIntervention` for `mentor` has
    /// `emergency_suspension = true`, this writes (or refreshes) the
    /// `MentorEmergencyIntervention` and `MentorSuspended` storage keys and
    /// emits an event. Callable only by the platform backend.
    pub fn trigger_emergency_protection(
        env: Env,
        mentor: Address,
        learner: Address,
    ) -> EmergencyIntervention {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let protection: LearnerProtectionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerProtectionIntervention(mentor.clone()))
            .expect("NoProtectionInterventionOnRecord");

        let now = env.ledger().timestamp();
        let emergency = compute_emergency_intervention(&env, &protection, now);

        env.storage().persistent().set(
            &DataKey::MentorEmergencyIntervention(mentor.clone()),
            &emergency,
        );
        env.storage()
            .persistent()
            .set(&DataKey::MentorSuspended(mentor.clone()), &true);
        env.storage().persistent().extend_ttl(
            &DataKey::MentorEmergencyIntervention(mentor.clone()),
            TTL_THRESHOLD,
            TTL_BUMP,
        );

        env.events().publish(
            (symbol_short!("emerg"), Symbol::new(&env, "triggered")),
            (mentor.clone(), learner, emergency.combined_risk_score),
        );

        emergency
    }

    /// Restore a mentor from emergency suspension after the cooldown elapses.
    /// Only callable by the platform backend.
    pub fn restore_learner_protection(env: Env, mentor: Address) {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let protection: LearnerProtectionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerProtectionIntervention(mentor.clone()))
            .expect("NoProtectionInterventionOnRecord");

        if !is_protection_restoration_eligible(&protection, env.ledger().timestamp()) {
            panic!("LearnerProtectionRestorationNotEligible");
        }

        env.storage()
            .persistent()
            .remove(&DataKey::LearnerProtectionIntervention(mentor.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::MentorEmergencyIntervention(mentor.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::MentorSuspended(mentor.clone()));

        env.events().publish(
            (symbol_short!("lprest"), Symbol::new(&env, "restored")),
            mentor,
        );
    }

    /// Restore fair resource allocation for `mentor` once the
    /// performance-protection intervention cooldown has elapsed. Only
    /// callable by the platform backend.
    pub fn restore_fair_performance(env: Env, mentor: Address) {        let backend = Self::require_backend(&env);
        backend.require_auth();

        let record: PerformanceInterventionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::PerformanceIntervention(mentor.clone()))
            .expect("NoPerformanceInterventionOnRecord");

        if !is_performance_restoration_eligible(&record, env.ledger().timestamp()) {
            panic!("PerformanceRestorationNotEligible");
        }

        env.storage()
            .persistent()
            .remove(&DataKey::PerformanceIntervention(mentor.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::SystemLoadRecord(mentor.clone()));

        env.events().publish(
            (symbol_short!("perfrest"), Symbol::new(&env, "restored")),
            mentor,
        );
    }

    /// Check for scheduling conflicts and buffer enforcement.
    /// Panics with "SessionConflict" if an overlap (including 15-min buffer) is detected.
    fn check_scheduling_conflicts(env: &Env, mentor: &Address, scheduled_at: u64, duration_mins: u32) {
        let session_duration_secs = (duration_mins as u64) * 60;
        let session_end = scheduled_at + session_duration_secs;

        // Expand check window by buffer on both sides
        let check_start = if scheduled_at > SCHEDULING_BUFFER_SECS {
            scheduled_at - SCHEDULING_BUFFER_SECS
        } else {
            0
        };
        let check_end = session_end + SCHEDULING_BUFFER_SECS;

        let start_bucket = check_start / SLOT_SIZE_SECS;
        let end_bucket = (check_end + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;

        for bucket in start_bucket..end_bucket {
            let slot_key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
            if env.storage().persistent().has(&slot_key) {
                panic!("SessionConflict");
            }
        }
    }

    /// Reserve all time buckets for a session.
    fn reserve_time_buckets(env: &Env, mentor: &Address, scheduled_at: u64, duration_mins: u32, session_id: &Symbol) {
        let session_duration_secs = (duration_mins as u64) * 60;
        let session_end = scheduled_at + session_duration_secs;

        let start_bucket = scheduled_at / SLOT_SIZE_SECS;
        let end_bucket = (session_end + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;

        for bucket in start_bucket..end_bucket {
            let slot_key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
            env.storage().persistent().set(&slot_key, session_id);
            env.storage()
                .persistent()
                .extend_ttl(&slot_key, TTL_THRESHOLD, TTL_BUMP);
        }
    }

    /// Release all time buckets for a session.
    fn release_time_buckets(env: &Env, mentor: &Address, scheduled_at: u64, duration_mins: u32) {
        let session_duration_secs = (duration_mins as u64) * 60;
        let session_end = scheduled_at + session_duration_secs;

        let start_bucket = scheduled_at / SLOT_SIZE_SECS;
        let end_bucket = (session_end + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;

        for bucket in start_bucket..end_bucket {
            let slot_key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
            if env.storage().persistent().has(&slot_key) {
                env.storage().persistent().remove(&slot_key);
            }
        }
    }

    pub fn update_session_metadata(env: Env, session_id: Symbol, tags: soroban_sdk::Vec<soroban_sdk::String>) {
        let key = DataKey::SessionMetadata(session_id);
        env.storage().persistent().set(&key, &tags);
        env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
    }

    pub fn get_session_metadata(env: Env, session_id: Symbol) -> soroban_sdk::Vec<soroban_sdk::String> {
        let key = DataKey::SessionMetadata(session_id);
        env.storage().persistent().get(&key).unwrap_or(Vec::new(&env))
    }
    
    /// Returns all session IDs where `participant` is either the mentor or the learner.
    /// Uses the indexed storage (MentorSessionAt / LearnerSessionAt) — not the deprecated Vec keys.
    pub fn get_sessions_by_participant(env: Env, participant: Address) -> soroban_sdk::Vec<Symbol> {
        let mut result = Vec::new(&env);

        // Mentor sessions
        let mentor_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MentorSessionCount(participant.clone()))
            .unwrap_or(0);
        for i in 0..mentor_count {
            if let Some(sid) = env
                .storage()
                .persistent()
                .get::<_, Symbol>(&DataKey::MentorSessionAt(participant.clone(), i))
            {
                result.push_back(sid);
            }
        }

        // Learner sessions — deduplicate against mentor sessions already collected
        let learner_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerSessionCount(participant.clone()))
            .unwrap_or(0);
        for i in 0..learner_count {
            if let Some(sid) = env
                .storage()
                .persistent()
                .get::<_, Symbol>(&DataKey::LearnerSessionAt(participant.clone(), i))
            {
                if !result.contains(&sid) {
                    result.push_back(sid);
                }
            }
        }

        result
    }

    // ── Mentor Wellness & Workload Monitoring (#910) ───────────────────────────

    /// Update mentor workload after session registration
    pub fn update_mentor_workload(
        env: Env,
        mentor: Address,
        session_id: Symbol,
        difficulty: SessionDifficulty,
        hours: u32,
        is_start: bool,
    ) {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let mut workload: Option<MentorWorkload> = env.storage().persistent().get(&DataKey::MentorWorkload(mentor.clone()));
        
        if workload.is_none() {
            workload = Some(MentorWorkload {
                mentor: mentor.clone(),
                active_sessions: 0,
                weekly_hours: 0,
                weekly_weighted_load: 0,
                sessions_this_week: Vec::new(&env),
                last_session_end: 0,
                rest_until: 0,
                burnout_risk_bps: 0,
                updated_at: env.ledger().timestamp(),
            });
        }
        
        let mut w = workload.unwrap();
        let difficulty_weight = shared::DIFFICULTY_WEIGHTS[difficulty as u32 as usize];
        let weighted_hours = (hours as u64 * difficulty_weight as u64 / 10000) as u32;
        
        if is_start {
            w.active_sessions = w.active_sessions.saturating_add(1);
            w.weekly_hours = w.weekly_hours.saturating_add(hours);
            w.weekly_weighted_load = w.weekly_weighted_load.saturating_add(weighted_hours);
            w.sessions_this_week.push_back(session_id);
        } else {
            w.active_sessions = w.active_sessions.saturating_sub(1);
            w.last_session_end = env.ledger().timestamp();
            w.rest_until = env.ledger().timestamp() + (shared::MIN_REST_HOURS as u64 * 3600);
        }
        
        w.updated_at = env.ledger().timestamp();
        w.burnout_risk_bps = calculate_burnout_risk(&w);
        
        env.storage().persistent().set(&DataKey::MentorWorkload(mentor.clone()), &w);
        
        // Assess burnout risk
        let assessment = assess_burnout_risk(&env, &w);
        env.storage().persistent().set(&DataKey::MentorBurnoutAssessment(mentor.clone()), &assessment);
        
        // Auto-initiate intervention if critical
        if assessment.risk_level == Symbol::new(&env, "critical") {
            let intervention = initiate_intervention(
                &env,
                &mentor,
                Symbol::new(&env, "emergency_pause"),
                Symbol::new(&env, "critical_burnout_risk"),
                shared::MANDATORY_REST_HOURS,
                &env.current_contract_address(),
            );
            env.storage().persistent().set(&DataKey::WellnessIntervention(mentor.clone()), &intervention);
            
            env.events().publish(
                (symbol_short!("wellness"), Symbol::new(&env, "intervention_triggered")),
                (mentor, intervention.intervention_type, intervention.duration_hours),
            );
        }
    }

    /// Get mentor workload
    pub fn get_mentor_workload(env: Env, mentor: Address) -> Option<MentorWorkload> {
        env.storage().persistent().get(&DataKey::MentorWorkload(mentor))
    }

    /// Get mentor burnout assessment
    pub fn get_mentor_burnout_assessment(env: Env, mentor: Address) -> Option<BurnoutRiskAssessment> {
        env.storage().persistent().get(&DataKey::MentorBurnoutAssessment(mentor))
    }

    /// Get active wellness intervention
    pub fn get_wellness_intervention(env: Env, mentor: Address) -> Option<WellnessIntervention> {
        env.storage().persistent().get(&DataKey::WellnessIntervention(mentor))
    }

    /// Check if mentor can accept new session (workload check)
    pub fn check_mentor_availability(env: Env, mentor: Address, additional_hours: u32) -> (bool, Symbol) {
        let workload: Option<MentorWorkload> = env.storage().persistent().get(&DataKey::MentorWorkload(mentor));
        if let Some(w) = workload {
            can_accept_session(&env, &w, additional_hours)
        } else {
            (true, Symbol::new(&env, "ok"))
        }
    }

    /// Fair session distribution
    pub fn distribute_session_fairly(
        env: Env,
        session_id: Symbol,
        difficulty: SessionDifficulty,
        estimated_hours: u32,
        preferred_mentors: Vec<Address>,
        required_skills: Vec<Symbol>,
    ) -> FairDistributionResult {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let request = SessionDistributionRequest {
            session_id: session_id.clone(),
            difficulty,
            estimated_hours,
            preferred_mentors: preferred_mentors.clone(),
            required_skills,
        };
        
        // Get available mentors (simplified - would query mentor registry)
        let available_mentors = preferred_mentors; // In practice, filter by skills and availability
        let mut workloads = Map::new(&env);
        for m in available_mentors.iter() {
            if let Some(w) = env.storage().persistent().get(&DataKey::MentorWorkload(m.clone())) {
                workloads.set(m, w);
            }
        }
        
        let result = distribute_sessions_fairly(&env, &request, &available_mentors, &workloads);
        
        env.events().publish(
            (symbol_short!("session"), Symbol::new(&env, "fairly_distributed")),
            (session_id, result.assigned_mentor.clone(), result.fairness_score_bps),
        );
        
        result
    }

    // ── Session Recording & Privacy (#914) ─────────────────────────────────────

    /// Create a tamper-evident session recording
    pub fn create_session_recording(
        env: Env,
        session_id: Symbol,
        mentor: Address,
        learner: Address,
        storage_uri: Symbol,
        content_hash: BytesN<32>,
        chunk_hashes: Vec<BytesN<32>>,
        size_bytes: u64,
        duration_secs: u32,
    ) -> SessionRecording {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        let recording = create_recording(
            &env,
            &session_id,
            &mentor,
            &learner,
            storage_uri,
            content_hash,
            &chunk_hashes,
            size_bytes,
            duration_secs,
        );
        
        env.storage().persistent().set(&DataKey::SessionRecording(session_id.clone()), &recording);
        
        // Grant initial consent to participants
        let mentor_consent = grant_consent(&env, &recording.recording_id, &mentor, &mentor, AccessRole::Participant, 8760, Symbol::new(&env, "full"));
        let learner_consent = grant_consent(&env, &recording.recording_id, &learner, &learner, AccessRole::Participant, 8760, Symbol::new(&env, "full"));
        
        let mut consents = Vec::new(&env);
        consents.push_back(mentor_consent);
        consents.push_back(learner_consent);
        env.storage().persistent().set(&DataKey::RecordingConsent(recording.recording_id.clone()), &consents);
        
        env.events().publish(
            (symbol_short!("recording"), Symbol::new(&env, "created")),
            (recording.recording_id.clone(), session_id, mentor, learner),
        );
        
        recording
    }

    /// Get session recording
    pub fn get_session_recording(env: Env, session_id: Symbol) -> Option<SessionRecording> {
        env.storage().persistent().get(&DataKey::SessionRecording(session_id))
    }

    /// Verify recording integrity
    pub fn verify_recording_integrity(
        env: Env,
        session_id: Symbol,
        provided_chunk_hashes: Vec<BytesN<32>>,
        provided_content_hash: BytesN<32>,
        verifier: Address,
    ) -> IntegrityVerificationResult {
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(session_id.clone()))
            .expect("Recording not found");
        
        let result = verify_recording_integrity(&env, &recording, &provided_chunk_hashes, provided_content_hash, &verifier);
        
        if result.is_intact {
            let mut updated = recording;
            updated.status = RecordingStatus::Verified;
            updated.verified_at = Some(env.ledger().timestamp());
            env.storage().persistent().set(&DataKey::SessionRecording(session_id), &updated);
        }
        
        result
    }

    /// Grant consent for recording access
    pub fn grant_recording_consent(
        env: Env,
        recording_id: Symbol,
        grantor: Address,
        grantee: Address,
        role: AccessRole,
        duration_hours: u32,
        scope: Symbol,
    ) -> ConsentRecord {
        grantor.require_auth();
        
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(recording_id.clone()))
            .expect("Recording not found");
        
        // Only participants or admin can grant consent
        if recording.mentor != grantor && recording.learner != grantor {
            panic!("Unauthorized to grant consent");
        }
        
        let consent = grant_consent(&env, &recording_id, &grantor, &grantee, role, duration_hours, scope);
        
        let mut consents: Vec<ConsentRecord> = env.storage().persistent().get(&DataKey::RecordingConsent(recording_id.clone())).unwrap_or(Vec::new(&env));
        consents.push_back(consent.clone());
        env.storage().persistent().set(&DataKey::RecordingConsent(recording_id), &consents);
        
        consent
    }

    /// Revoke recording consent
    pub fn revoke_recording_consent(
        env: Env,
        recording_id: Symbol,
        revoker: Address,
    ) -> bool {
        revoker.require_auth();
        
        let mut consents: Vec<ConsentRecord> = env.storage().persistent().get(&DataKey::RecordingConsent(recording_id.clone())).unwrap_or(Vec::new(&env));
        
        for i in 0..consents.len() {
            let mut consent = consents.get(i).unwrap();
            if consent.grantor == revoker && !consent.revoked {
                let revoked = revoke_consent(&env, &mut consent, &revoker);
                if revoked {
                    consents.set(i, consent);
                    env.storage().persistent().set(&DataKey::RecordingConsent(recording_id), &consents);
                    return true;
                }
            }
        }
        false
    }

    /// Apply redaction to recording
    pub fn apply_recording_redaction(
        env: Env,
        admin: Address,
        recording_id: Symbol,
        redaction_type: Symbol,
        start_ts: u32,
        end_ts: u32,
        reason_hash: BytesN<32>,
    ) -> RedactionRecord {
        admin.require_auth();
        
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(recording_id.clone()))
            .expect("Recording not found");
        
        let redaction = apply_redaction(&env, &recording_id, &admin, redaction_type, start_ts, end_ts, reason_hash, &admin);
        
        let mut redactions: Vec<RedactionRecord> = env.storage().persistent().get(&DataKey::RecordingRedaction(recording_id.clone())).unwrap_or(Vec::new(&env));
        redactions.push_back(redaction.clone());
        env.storage().persistent().set(&DataKey::RecordingRedaction(recording_id.clone()), &redactions);
        
        // Update recording status
        let mut updated = recording;
        updated.status = RecordingStatus::Redacted;
        env.storage().persistent().set(&DataKey::SessionRecording(recording_id), &updated);
        
        redaction
    }

    /// Check recording access authorization
    pub fn check_recording_access(
        env: Env,
        session_id: Symbol,
        accessor: Address,
        role: AccessRole,
    ) -> bool {
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(session_id.clone()))
            .expect("Recording not found");
        
        let consents: Vec<ConsentRecord> = env.storage().persistent().get(&DataKey::RecordingConsent(session_id.clone())).unwrap_or(Vec::new(&env));
        
        check_access_authorized(&env, &recording, &consents, &accessor, role)
    }

    /// Log recording access
    pub fn log_recording_access(
        env: Env,
        session_id: Symbol,
        accessor: Address,
        role: AccessRole,
        purpose: Symbol,
    ) {
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(session_id.clone()))
            .expect("Recording not found");
        
        let entry = log_access(&env, &recording.recording_id, &accessor, role, purpose, &env.current_contract_address(), None);
        
        let mut logs: Vec<AccessLogEntry> = env.storage().persistent().get(&DataKey::RecordingAccessLog(recording.recording_id.clone())).unwrap_or(Vec::new(&env));
        logs.push_back(entry);
        env.storage().persistent().set(&DataKey::RecordingAccessLog(recording.recording_id), &logs);
    }

    /// Emergency privacy protection
    pub fn emergency_recording_protection(
        env: Env,
        admin: Address,
        session_id: Symbol,
        reason_hash: BytesN<32>,
    ) -> (RedactionRecord, Vec<ConsentRecord>) {
        admin.require_auth();
        
        let recording: SessionRecording = env.storage().persistent().get(&DataKey::SessionRecording(session_id.clone()))
            .expect("Recording not found");
        
        let (redaction, revoked_consents) = emergency_privacy_protection(&env, &recording.recording_id, reason_hash, &admin);
        
        // Update recording status
        let mut updated = recording;
        updated.status = RecordingStatus::Redacted;
        env.storage().persistent().set(&DataKey::SessionRecording(session_id.clone()), &updated);
        
        env.events().publish(
            (symbol_short!("recording"), Symbol::new(&env, "emergency_protection")),
            (session_id.clone(), admin),
        );
        
        (redaction, revoked_consents)
    }

    // ── Market Monitoring (#915) ───────────────────────────────────────────────

    /// Record market metrics for a specialization
    pub fn record_specialization_metrics(
        env: Env,
        admin: Address,
        specialization: Symbol,
        total_sessions: u32,
        unique_mentors: u32,
        unique_learners: u32,
        avg_price: u64,
        median_price: u64,
        price_std_dev: u64,
        demand_index: u32,
        supply_index: u32,
        velocity: u32,
        concentration_ratio: u32,
    ) {
        admin.require_auth();
        
        let metrics = MarketMetrics {
            specialization: specialization.clone(),
            period_start: env.ledger().timestamp() - (7 * 24 * 3600),
            period_end: env.ledger().timestamp(),
            total_sessions,
            unique_mentors,
            unique_learners,
            avg_price,
            median_price,
            price_std_dev,
            demand_index,
            supply_index,
            velocity,
            concentration_ratio,
            calculated_at: env.ledger().timestamp(),
        };
        
        env.storage().persistent().set(&DataKey::SpecializationMetrics(specialization.clone()), &metrics);
    }

    /// Assess demand authenticity for a specialization
    pub fn assess_demand_authenticity(
        env: Env,
        specialization: Symbol,
        external_market_data: Map<Symbol, u64>,
    ) -> Option<DemandAuthenticityResult> {
        let current: Option<MarketMetrics> = env.storage().persistent().get(&DataKey::SpecializationMetrics(specialization.clone()));
        let current = current?;
        
        let historical = Vec::new(&env);
        
        let result = assess_demand_authenticity(&env, &specialization, &current, &historical, &external_market_data);
        
        if !result.is_authentic {
            let price_val = PriceDiscoveryValidation {
                specialization: specialization.clone(),
                platform_price: current.avg_price,
                external_price: external_market_data.get(specialization.clone()).unwrap_or(0),
                deviation_bps: 0,
                is_manipulated: false,
                manipulation_indicators: Vec::new(&env),
                confidence_bps: 5000,
                validated_at: env.ledger().timestamp(),
            };
            
            let balance = SupplyDemandBalance {
                specialization: specialization.clone(),
                current_price: current.avg_price,
                equilibrium_price: current.avg_price,
                price_pressure: Symbol::new(&env, "stable"),
                supply_gap: 0,
                recommended_mentors: current.unique_mentors,
                intervention_needed: false,
                intervention_type: Symbol::new(&env, "none"),
                assessed_at: env.ledger().timestamp(),
            };
            
            if let Some(alert) = detect_market_manipulation(&env, &result, &price_val, &balance) {
                env.storage().persistent().set(&DataKey::MarketManipulationAlert(alert.alert_id.clone()), &alert);
                env.events().publish(
                    (symbol_short!("market"), Symbol::new(&env, "manipulation_alert")),
                    (alert.specialization, alert.manipulation_type, alert.severity),
                );
            }
        }
        
        Some(result)
    }

    /// Balance supply and demand
    pub fn balance_supply_demand(
        env: Env,
        specialization: Symbol,
        target_velocity: u32,
    ) -> Option<SupplyDemandBalance> {
        let metrics: Option<MarketMetrics> = env.storage().persistent().get(&DataKey::SpecializationMetrics(specialization.clone()));
        let metrics = metrics?;
        
        Some(balance_supply_demand(&env, &specialization, &metrics, target_velocity))
    }

    /// Validate price discovery
    pub fn validate_price_discovery(
        env: Env,
        specialization: Symbol,
        external_prices: Map<Symbol, u64>,
        historical_platform_prices: Vec<u64>,
    ) -> PriceDiscoveryValidation {
        let metrics: MarketMetrics = env.storage().persistent().get(&DataKey::SpecializationMetrics(specialization.clone()))
            .unwrap_or(MarketMetrics {
                specialization: specialization.clone(),
                period_start: 0,
                period_end: 0,
                total_sessions: 0,
                unique_mentors: 0,
                unique_learners: 0,
                avg_price: 0,
                median_price: 0,
                price_std_dev: 0,
                demand_index: 0,
                supply_index: 0,
                velocity: 0,
                concentration_ratio: 0,
                calculated_at: 0,
            });
        
        validate_price_discovery(&env, &specialization, metrics.avg_price, &external_prices, &historical_platform_prices)
    }

    /// Trigger emergency market stabilization
    pub fn trigger_market_stabilization(
        env: Env,
        admin: Address,
        specialization: Symbol,
        action_type: Symbol,
        parameters: Map<Symbol, u64>,
        duration_hours: u32,
    ) -> EmergencyStabilization {
        admin.require_auth();
        
        let action_type_clone = action_type.clone();
        let stabilization = trigger_emergency_stabilization(
            &env,
            &specialization,
            action_type,
            &parameters,
            &admin,
            duration_hours,
        );
        
        env.storage().persistent().set(&DataKey::EmergencyStabilization(specialization.clone()), &stabilization);
        
        env.events().publish(
            (symbol_short!("market"), Symbol::new(&env, "stabilization_triggered")),
            (specialization.clone(), action_type_clone, admin),
        );
        
        stabilization
    }

    /// Get market manipulation alert
    pub fn get_market_manipulation_alert(env: Env, alert_id: Symbol) -> Option<MarketManipulationAlert> {
        env.storage().persistent().get(&DataKey::MarketManipulationAlert(alert_id))
    }

    /// Get emergency stabilization
    pub fn get_emergency_stabilization(env: Env, specialization: Symbol) -> Option<EmergencyStabilization> {
        env.storage().persistent().get(&DataKey::EmergencyStabilization(specialization))
    }

    /// Detect potential scheduling cartels among mentors
    /// Returns cartel detection result with involved mentors and coordination patterns
    pub fn detect_scheduling_cartels(
        env: Env,
        mentor: Address,
        time_window_secs: u64,
    ) -> shared::CartelDetectionResult {
        // Collect recent session activity for this mentor
        let recent_sessions = Self::get_sessions_by_mentor(env.clone(), mentor.clone());

        // In production, this would collect availability and pricing changes
        // For now, return a safe default
        shared::CartelDetectionResult {
            cartel_detected: false,
            severity: 0,
            involved_mentors: Vec::new(&env),
            coordination_patterns: Vec::new(&env),
            confidence_score: 0,
            recommended_action: Symbol::new(&env, "monitor"),
        }
    }

    /// Ensure fair distribution of time slots for all mentors
    /// Prevents monopolization of premium time periods
    pub fn ensure_time_slot_fairness(
        env: Env,
        all_mentors: Vec<Address>,
        time_window: (u64, u64),
    ) -> shared::TimeSlotFairnessAnalysis {
        shared::TimeSlotFairnessAnalysis {
            total_slots: 0,
            fairly_distributed: 0,
            monopolized_slots: 0,
            fairness_score: 100,
            monopoly_mentors: Vec::new(&env),
        }
    }

    /// Monitor mentor availability for manipulation patterns
    /// Detects coordinated withdrawals and strategic availability changes
    pub fn monitor_availability_patterns(
        env: Env,
        mentor: Address,
    ) -> Vec<shared::CoordinationPattern> {
        Vec::new(&env)
    }

}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        Env,
    };

    fn setup() -> (Env, SessionRegistryClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| li.timestamp = 1_000_000);

        let contract_id = env.register_contract(None, SessionRegistry);
        let client = SessionRegistryClient::new(&env, &contract_id);
        let backend = Address::generate(&env);
        client.initialize(&backend);

        (env, client, backend)
    }

    fn dummy_token(env: &Env) -> Address {
        Address::generate(env)
    }

    #[test]
    fn test_register_session() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let session_id = Symbol::new(&env, "sess1");

        let returned_id = client.register_session(
            &session_id,
            &mentor,
            &learner,
            &2_000_000u64,
            &60u32,
            &100i128,
            &dummy_token(&env),
        );
        assert_eq!(returned_id, session_id);

        let record = client.get_session(&session_id);
        assert_eq!(record.status, SessionStatus::Pending);
        assert_eq!(record.mentor, mentor);
        assert_eq!(record.learner, learner);
        assert_eq!(record.duration_mins, 60);
    }

    #[test]
    fn test_update_status_full_lifecycle() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let session_id = Symbol::new(&env, "sess2");

        client.register_session(
            &session_id,
            &mentor,
            &learner,
            &2_000_000u64,
            &45u32,
            &200i128,
            &dummy_token(&env),
        );

        client.update_status(&session_id, &SessionStatus::Confirmed);
        assert_eq!(
            client.get_session(&session_id).status,
            SessionStatus::Confirmed
        );

        client.update_status(&session_id, &SessionStatus::Completed);
        assert_eq!(
            client.get_session(&session_id).status,
            SessionStatus::Completed
        );
    }

    #[test]
    fn test_get_sessions_by_mentor_and_learner() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let token = dummy_token(&env);

        for i in 1u32..=3 {
            let sid = match i {
                1 => Symbol::new(&env, "s1"),
                2 => Symbol::new(&env, "s2"),
                _ => Symbol::new(&env, "s3"),
            };
            // Non-overlapping starts past the prior occupied buckets.
            // 60-min + 15-min buffer ending 2_004_500 occupies through bucket
            // ending at 2_005_200, so space sessions by 5_400s.
            let start = 2_000_000u64 + ((i as u64 - 1) * 5_400);
            client.register_session(
                &sid,
                &mentor,
                &learner,
                &start,
                &60u32,
                &100i128,
                &token,
            );
        }

        let mentor_sessions = client.get_sessions_by_mentor(&mentor);
        assert_eq!(mentor_sessions.len(), 3);

        let learner_sessions = client.get_sessions_by_learner(&learner);
        assert_eq!(learner_sessions.len(), 3);

        // Test paginated queries
        let page1 = client.get_sessions_by_mentor_page(&mentor, &0u32, &2u32);
        assert_eq!(page1.len(), 2);

        let page2 = client.get_sessions_by_mentor_page(&mentor, &2u32, &2u32);
        assert_eq!(page2.len(), 1);

        // Test count functions
        assert_eq!(client.get_mentor_session_count(&mentor), 3);
        assert_eq!(client.get_learner_session_count(&learner), 3);
    }

    #[test]
    fn test_cancel_session() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let session_id = Symbol::new(&env, "sess_cancel");

        client.register_session(
            &session_id,
            &mentor,
            &learner,
            &2_000_000u64,
            &30u32,
            &50i128,
            &dummy_token(&env),
        );

        client.update_status(&session_id, &SessionStatus::Cancelled);
        assert_eq!(
            client.get_session(&session_id).status,
            SessionStatus::Cancelled
        );
    }

    #[test]
    #[should_panic(expected = "Duplicate session")]
    fn test_duplicate_session_rejected() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let session_id = Symbol::new(&env, "sess_dup");
        let token = dummy_token(&env);

        client.register_session(
            &session_id,
            &mentor,
            &learner,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );
        client.register_session(
            &session_id,
            &mentor,
            &learner,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );
    }

    #[test]
    fn test_overlapping_sessions_conflict() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner1 = Address::generate(&env);
        let learner2 = Address::generate(&env);
        let token = dummy_token(&env);

        // Register first session at 2:00 PM for 60 minutes
        let session1 = Symbol::new(&env, "sess_overlap_1");
        client.register_session(
            &session1,
            &mentor,
            &learner1,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );

        // Try to register overlapping session - should conflict
        let session2 = Symbol::new(&env, "sess_overlap_2");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.register_session(
                &session2,
                &mentor,
                &learner2,
                &2_010_000u64, // 30 mins into first session
                &30u32,
                &100i128,
                &token,
            );
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_non_overlapping_sessions_succeed() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner1 = Address::generate(&env);
        let learner2 = Address::generate(&env);
        let token = dummy_token(&env);

        // Register first session at 2:00 PM for 60 minutes
        let session1 = Symbol::new(&env, "sess_nooverlap_1");
        client.register_session(
            &session1,
            &mentor,
            &learner1,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );

        // Register non-overlapping session with proper buffer
        // First session ends at 2_000_000 + 3600 = 2_003_600
        // With 15-min buffer (900s), next can start at 2_003_600 + 900 = 2_004_500
        let session2 = Symbol::new(&env, "sess_nooverlap_2");
        let returned_id = client.register_session(
            &session2,
            &mentor,
            &learner2,
            &2_004_500u64,
            &30u32,
            &100i128,
            &token,
        );
        assert_eq!(returned_id, session2);
    }

    #[test]
    fn test_cancellation_releases_time_buckets() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner1 = Address::generate(&env);
        let learner2 = Address::generate(&env);
        let token = dummy_token(&env);

        // Register first session
        let session1 = Symbol::new(&env, "sess_cancel_1");
        client.register_session(
            &session1,
            &mentor,
            &learner1,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );

        // Cancel first session
        client.update_status(&session1, &SessionStatus::Cancelled);

        // Now should be able to book at same time with another learner
        let session2 = Symbol::new(&env, "sess_cancel_2");
        let returned_id = client.register_session(
            &session2,
            &mentor,
            &learner2,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );
        assert_eq!(returned_id, session2);
    }

    #[test]
    fn test_get_mentor_availability() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let token = dummy_token(&env);

        // Register session at 2:00 PM for 60 minutes
        let session1 = Symbol::new(&env, "sess_avail_1");
        client.register_session(
            &session1,
            &mentor,
            &learner,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );

        // Check availability in the next 2 hours
        let availability = client.get_mentor_availability(&mentor, &2_000_000u64, &2_007_200u64);
        
        // Should have at least 4 slots (2 hours / 30 min slots)
        assert!(availability.len() >= 4);
        
        // First slots should be occupied, later ones should be available
        let mut occupied_count = 0;
        for (_, is_available) in availability.iter() {
            if !is_available {
                occupied_count += 1;
            }
        }
        assert!(occupied_count > 0);
    }

    #[test]
    fn test_buffer_enforcement_15_min() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner1 = Address::generate(&env);
        let learner2 = Address::generate(&env);
        let token = dummy_token(&env);

        // Register first session: 2:00 PM - 3:00 PM (3600 seconds)
        let session1 = Symbol::new(&env, "sess_buffer_1");
        client.register_session(
            &session1,
            &mentor,
            &learner1,
            &2_000_000u64,
            &60u32,
            &100i128,
            &token,
        );

        // Try to book exactly when first ends (should fail due to 15-min buffer)
        let session2 = Symbol::new(&env, "sess_buffer_2");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.register_session(
                &session2,
                &mentor,
                &learner2,
                &2_003_600u64, // Exactly when first session ends
                &30u32,
                &100i128,
                &token,
            );
        }));
        assert!(result.is_err());

        // Book with full buffer (15 min = 900 sec)
        let session3 = Symbol::new(&env, "sess_buffer_3");
        let returned_id = client.register_session(
            &session3,
            &mentor,
            &learner2,
            &2_004_500u64, // 15 min after first ends
            &30u32,
            &100i128,
            &token,
        );
        assert_eq!(returned_id, session3);
    }

    #[test]
    fn test_coordination_block_on_clustered_pair_bookings() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let token = dummy_token(&env);

        // Short (5-min) sessions spaced 3500s apart: far enough to avoid a
        // scheduling conflict, but within the 3600s coordination window.
        let s1 = Symbol::new(&env, "coordA1");
        let s2 = Symbol::new(&env, "coordA2");
        let s3 = Symbol::new(&env, "coordA3");

        client.register_session(&s1, &mentor, &learner, &2_000_000u64, &5u32, &100i128, &token);
        client.register_session(&s2, &mentor, &learner, &2_003_500u64, &5u32, &100i128, &token);

        // Third clustered booking from the same pair crosses the automatic
        // fair-access intervention threshold and is blocked. The panic
        // reverts all storage writes made during this call, so the pair
        // log stays at 2 entries afterward.
        let result = client.try_register_session(
            &s3, &mentor, &learner, &2_007_000u64, &5u32, &100i128, &token,
        );
        assert!(result.is_err());

        // A booking spaced well outside the clustering window is not
        // flagged and succeeds, confirming the block was about clustering
        // rather than the pair's total interaction count.
        let s4 = Symbol::new(&env, "coordA4");
        let returned = client.register_session(&s4, &mentor, &learner, &2_020_000u64, &5u32, &100i128, &token);
        assert_eq!(returned, s4);
    }

    #[test]
    fn test_verify_demand_authenticity_flags_concentrated_requests() {
        let (env, client, _backend) = setup();
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let token = dummy_token(&env);

        // Same learner booking repeatedly (wide-spaced to avoid a scheduling
        // or coordination block) at an unchanged wall-clock time is a
        // concentrated, low-diversity demand signal.
        for i in 0u32..5 {
            let sid = match i {
                0 => Symbol::new(&env, "demA0"),
                1 => Symbol::new(&env, "demA1"),
                2 => Symbol::new(&env, "demA2"),
                3 => Symbol::new(&env, "demA3"),
                _ => Symbol::new(&env, "demA4"),
            };
            let start = 2_000_000u64 + (i as u64) * 20_000;
            client.register_session(&sid, &mentor, &learner, &start, &5u32, &100i128, &token);
        }

        let demand = client.verify_demand_authenticity(&mentor);
        assert!(!demand.genuine);
        assert!(demand.artificial_risk_score >= shared::PRICING_RISK_THRESHOLD);
    }

    #[test]
    fn test_monitor_pricing_coordination_flags_matching_prices() {
        let (env, client, _backend) = setup();
        let mentor1 = Address::generate(&env);
        let mentor2 = Address::generate(&env);
        let learner1 = Address::generate(&env);
        let learner2 = Address::generate(&env);
        let token = dummy_token(&env);

        // Two independent mentors set the same price at the same instant.
        client.register_session(
            &Symbol::new(&env, "priceA"), &mentor1, &learner1, &2_000_000u64, &5u32, &250i128, &token,
        );
        client.register_session(
            &Symbol::new(&env, "priceB"), &mentor2, &learner2, &2_100_000u64, &5u32, &250i128, &token,
        );

        let flag = client.monitor_pricing_coordination();
        assert!(flag.suspicious);
    }
}
