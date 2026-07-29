#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

// ── Storage keys ─────────────────────────────────────────────────────────────
const BACKEND: Symbol = symbol_short!("BACKEND");
const TTL_THRESHOLD: u32 = 500_000;
const TTL_BUMP: u32 = 1_000_000;
/// Schedule occupancy is tracked in 30-minute buckets.
const SLOT_SIZE_SECS: u64 = 1_800;
/// Minimum free time required between consecutive sessions on the same mentor.
const SCHEDULING_BUFFER_SECS: u64 = 900;

// ── Scheduling constants ──────────────────────────────────────────────────────
const SLOT_SIZE_SECS: u64 = 1800; // 30-minute slots
const SCHEDULING_BUFFER_SECS: u64 = 900; // 15-minute buffer between sessions

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
    Session(Symbol),
    /// Deprecated: kept for backward compat, no longer written to
    MentorSessions(Address),
    /// Deprecated: kept for backward compat, no longer written to
    LearnerSessions(Address),
    MentorSessionCount(Address),
    MentorSessionAt(Address, u32),
    LearnerSessionCount(Address),
    LearnerSessionAt(Address, u32),
    SessionOracle,
    SessionMetadata(Symbol),
    SessionOracle,
}

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
        let mut bucket = from / SLOT_SIZE_SECS;
        let end_bucket = (to + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;
        while bucket < end_bucket {
            let slot_start = bucket * SLOT_SIZE_SECS;
            if slot_start >= to {
                break;
            }
            if slot_start >= from {
                let key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
                let is_available = !env.storage().persistent().has(&key);
                result.push_back((slot_start, is_available));
            }
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
            Self::release_schedule_slots(
                &env,
                &record.mentor,
                record.scheduled_at,
                record.duration_mins,
            );
        }
        record.status = status.clone();
        env.storage().persistent().set(&session_key, &record);
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

    /// Get mentor availability slots.
    /// Returns a vector of (slot_start_time, is_available) tuples.
    /// Useful for UI/scheduling systems to find available time slots.
    pub fn get_mentor_availability(
        env: Env,
        mentor: Address,
        from: u64,
        to: u64,
    ) -> Vec<(u64, bool)> {
        let mut result = Vec::new(&env);
        
        let start_bucket = from / SLOT_SIZE_SECS;
        let end_bucket = (to + SLOT_SIZE_SECS - 1) / SLOT_SIZE_SECS;

        for bucket in start_bucket..end_bucket {
            let slot_start = bucket * SLOT_SIZE_SECS;
            let slot_key = DataKey::MentorScheduleSlot(mentor.clone(), bucket);
            let is_available = !env.storage().persistent().has(&slot_key);
            result.push_back((slot_start, is_available));
        }

        result
    }

    fn require_backend(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&BACKEND)
            .expect("Not initialized")
    }

    /// Check for scheduling conflicts and buffer enforcement.
    /// Panics with "SessionConflict" if an overlap is detected.
    fn check_scheduling_conflicts(env: &Env, mentor: &Address, scheduled_at: u64, duration_mins: u32) {
        let session_duration_secs = (duration_mins as u64) * 60;
        let session_end = scheduled_at + session_duration_secs;

        // Check time buckets covered by the session including buffers
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
                let conflicting_id: Symbol = env.storage().persistent().get(&slot_key).unwrap();
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
    
    pub fn get_sessions_by_participant(env: Env, participant: Address) -> soroban_sdk::Vec<Symbol> {
        let mut result = Vec::new(&env);
        let mentor_sessions: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::MentorSessions(participant.clone()))
            .unwrap_or(Vec::new(&env));
        for s in mentor_sessions.iter() {
            result.push_back(s);
        }
        let learner_sessions: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::LearnerSessions(participant.clone()))
            .unwrap_or(Vec::new(&env));
        for s in learner_sessions.iter() {
            if !result.contains(&s) {
                result.push_back(s);
            }
        }
        result
    }

}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
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
