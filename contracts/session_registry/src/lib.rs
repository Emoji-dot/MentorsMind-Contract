#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Vec, String};
use shared::{
    ContentProtection, ContentType, AccessLevel, ProtectedContent, AccessLog,
    IPVerification, IPUsageRecord,
    UsageRightsManager, License, LicenseType, ViolationRecord,
    SharedError,
};

// ── Storage keys ─────────────────────────────────────────────────────────────
const BACKEND: Symbol = symbol_short!("BACKEND");
const TTL_THRESHOLD: u32 = 500_000;
const TTL_BUMP: u32 = 1_000_000;

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
    pub protected_content: Vec<Symbol>, // Content IDs associated with this session
    pub content_licenses: Vec<Symbol>,  // License IDs for session content
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Session(Symbol),
    MentorSessions(Address),
    LearnerSessions(Address),
    SessionOracle,
    SessionContent(Symbol), // Protected content for session
    ContentAccess(Symbol, Address), // Access logs for content
    ContentLicense(Symbol), // License for content
    UsageTracking(Symbol, Address), // Usage tracking per user per content
    ViolationRecord(Symbol), // Violation records
}

// ── Errors ────────────────────────────────────────────────────────────────────
// Errors are surfaced via panics to keep compatibility with SDK 21 contractimpl.
// Error codes are documented here for reference:
// NotInitialized = 1, Unauthorized = 2, SessionNotFound = 3, DuplicateSession = 4

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
            protected_content: Vec::new(&env),
            content_licenses: Vec::new(&env),
        };

        env.storage().persistent().set(&session_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&session_key, TTL_THRESHOLD, TTL_BUMP);

        // Index by mentor
        let mentor_key = DataKey::MentorSessions(mentor.clone());
        let mut mentor_sessions: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&mentor_key)
            .unwrap_or(Vec::new(&env));
        mentor_sessions.push_back(session_id.clone());
        env.storage()
            .persistent()
            .set(&mentor_key, &mentor_sessions);
        env.storage()
            .persistent()
            .extend_ttl(&mentor_key, TTL_THRESHOLD, TTL_BUMP);

        // Index by learner
        let learner_key = DataKey::LearnerSessions(learner.clone());
        let mut learner_sessions: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&learner_key)
            .unwrap_or(Vec::new(&env));
        learner_sessions.push_back(session_id.clone());
        env.storage()
            .persistent()
            .set(&learner_key, &learner_sessions);
        env.storage()
            .persistent()
            .extend_ttl(&learner_key, TTL_THRESHOLD, TTL_BUMP);

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

    /// Get all session IDs for a mentor.
    pub fn get_sessions_by_mentor(env: Env, mentor: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::MentorSessions(mentor))
            .unwrap_or(Vec::new(&env))
    }

    /// Get all session IDs for a learner.
    pub fn get_sessions_by_learner(env: Env, learner: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::LearnerSessions(learner))
            .unwrap_or(Vec::new(&env))
    }

    fn require_backend(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&BACKEND)
            .expect("Not initialized")
    }


    pub fn update_session_metadata(env: Env, session_id: u64, tags: Vec<String>) {
        let key = (symbol_short!("SessMeta"), session_id);
        env.storage().persistent().set(&key, &tags);
    }
    
    pub fn get_sessions_by_participant(env: Env, _participant: Address) -> Vec<u64> {
        Vec::new(&env)
    }

    /// Manage session content with protection and IP verification
    pub fn manage_session_content(
        env: Env,
        session_id: Symbol,
        content_id: Symbol,
        content_type: ContentType,
        access_level: AccessLevel,
        mentor: Address,
    ) -> Result<(), SharedError> {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        // Get session record
        let session_key = DataKey::Session(session_id.clone());
        let mut session: SessionRecord = env
            .storage()
            .persistent()
            .get(&session_key)
            .ok_or(SharedError::NotFound)?;

        // Verify mentor is the owner of this session
        if session.mentor != mentor {
            return Err(SharedError::Unauthorized);
        }

        // Create protected content
        let protected_content = ContentProtection::create_protected_content(
            &env,
            content_id.clone(),
            mentor.clone(),
            content_type,
            access_level,
        )?;

        // Store protected content
        let content_key = DataKey::SessionContent(content_id.clone());
        env.storage().persistent().set(&content_key, &protected_content);
        env.storage()
            .persistent()
            .extend_ttl(&content_key, TTL_THRESHOLD, TTL_BUMP);

        // Add content to session record
        session.protected_content.push_back(content_id.clone());
        env.storage().persistent().set(&session_key, &session);

        // Emit event
        env.events().publish(
            (
                symbol_short!("content"),
                Symbol::new(&env, "content_protected"),
                content_id,
            ),
            (mentor, session_id),
        );

        Ok(())
    }

    /// Track content usage during sessions
    pub fn track_content_usage(
        env: Env,
        session_id: Symbol,
        content_id: Symbol,
        user: Address,
        usage_type: Symbol,
    ) -> Result<(), SharedError> {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        // Get session record
        let session_key = DataKey::Session(session_id.clone());
        let session: SessionRecord = env
            .storage()
            .persistent()
            .get(&session_key)
            .ok_or(SharedError::NotFound)?;

        // Verify user is participant in session
        if session.mentor != user && session.learner != user {
            return Err(SharedError::Unauthorized);
        }

        // Get protected content
        let content_key = DataKey::SessionContent(content_id.clone());
        let mut protected_content: ProtectedContent = env
            .storage()
            .persistent()
            .get(&content_key)
            .ok_or(SharedError::NotFound)?;

        // Check access permissions
        let encryption_key = ContentProtection::generate_encryption_key(
            &env,
            Symbol::new(&env, "temp_key"),
            protected_content.access_level.clone(),
            env.ledger().timestamp() + 3600, // 1 hour validity
        )?;

        let has_access = ContentProtection::verify_access(
            &env,
            &protected_content,
            &user,
            &encryption_key,
        )?;

        if !has_access {
            return Err(SharedError::ContentAccessDenied);
        }

        // Create usage tracking record
        let usage_record = IPVerification::track_usage(
            &env,
            content_id.clone(),
            user.clone(),
            usage_type.clone(),
            session_id.clone(),
            has_access,
        );

        // Store usage record
        let usage_key = DataKey::UsageTracking(content_id.clone(), user.clone());
        env.storage().persistent().set(&usage_key, &usage_record);

        // Update content access statistics
        ContentProtection::update_access_stats(&env, &mut protected_content);
        env.storage().persistent().set(&content_key, &protected_content);

        // Log access
        let access_log = ContentProtection::log_access(
            &env,
            content_id.clone(),
            user.clone(),
            usage_type,
            true,
            None, // IP hash not available in this context
        );

        let log_key = DataKey::ContentAccess(content_id, user);
        env.storage().persistent().set(&log_key, &access_log);

        Ok(())
    }

    /// Enforce IP rights and detect violations
    pub fn enforce_ip_rights(
        env: Env,
        content_id: Symbol,
        alleged_violator: Address,
        evidence_hash: BytesN<32>,
        reporter: Address,
    ) -> Result<Symbol, SharedError> {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        // Get protected content
        let content_key = DataKey::SessionContent(content_id.clone());
        let protected_content: ProtectedContent = env
            .storage()
            .persistent()
            .get(&content_key)
            .ok_or(SharedError::NotFound)?;

        // Only content owner or authorized users can report violations
        if protected_content.owner != reporter && 
           !protected_content.authorized_viewers.contains(&reporter) {
            return Err(SharedError::Unauthorized);
        }

        // Create infringement record
        let violation_id = Symbol::new(&env, "violation");
        let infringement = IPVerification::report_infringement(
            &env,
            violation_id.clone(),
            content_id.clone(),
            alleged_violator.clone(),
            evidence_hash,
            reporter.clone(),
        );

        // Store infringement record
        let violation_key = DataKey::ViolationRecord(violation_id.clone());
        env.storage().persistent().set(&violation_key, &infringement);

        // Emit violation event
        env.events().publish(
            (
                symbol_short!("violation"),
                Symbol::new(&env, "ip_violation_reported"),
                violation_id.clone(),
            ),
            (content_id, alleged_violator, reporter),
        );

        Ok(violation_id)
    }

    /// Create and manage content licenses for sessions
    pub fn create_content_license(
        env: Env,
        session_id: Symbol,
        content_id: Symbol,
        licensee: Address,
        license_types: Vec<LicenseType>,
        expires_at: Option<u64>,
        max_usage_count: Option<u32>,
    ) -> Result<Symbol, SharedError> {
        let backend = Self::require_backend(&env);
        backend.require_auth();

        // Get session record
        let session_key = DataKey::Session(session_id.clone());
        let mut session: SessionRecord = env
            .storage()
            .persistent()
            .get(&session_key)
            .ok_or(SharedError::NotFound)?;

        // Get protected content
        let content_key = DataKey::SessionContent(content_id.clone());
        let protected_content: ProtectedContent = env
            .storage()
            .persistent()
            .get(&content_key)
            .ok_or(SharedError::NotFound)?;

        // Only content owner (mentor) can create licenses
        if protected_content.owner != session.mentor {
            return Err(SharedError::Unauthorized);
        }

        // Create license
        let license_id = Symbol::new(&env, "license");
        let license = UsageRightsManager::create_license(
            &env,
            license_id.clone(),
            licensee.clone(),
            protected_content.owner.clone(),
            content_id.clone(),
            license_types,
            expires_at,
            max_usage_count,
            None, // No payment required for session content
            None, // No payment token
        )?;

        // Store license
        let license_key = DataKey::ContentLicense(license_id.clone());
        env.storage().persistent().set(&license_key, &license);

        // Add license to session record
        session.content_licenses.push_back(license_id.clone());
        env.storage().persistent().set(&session_key, &session);

        // Emit license creation event
        env.events().publish(
            (
                symbol_short!("license"),
                Symbol::new(&env, "content_license_created"),
                license_id.clone(),
            ),
            (content_id, licensee, session_id),
        );

        Ok(license_id)
    }

    /// Validate content access based on licenses
    pub fn validate_content_access(
        env: Env,
        content_id: Symbol,
        user: Address,
        usage_type: LicenseType,
    ) -> Result<bool, SharedError> {
        // Get protected content
        let content_key = DataKey::SessionContent(content_id.clone());
        let protected_content: ProtectedContent = env
            .storage()
            .persistent()
            .get(&content_key)
            .ok_or(SharedError::NotFound)?;

        // Check if user is content owner
        if protected_content.owner == user {
            return Ok(true);
        }

        // Check if user has appropriate license
        // This is a simplified check - in practice, you'd iterate through all licenses
        // for this content and check if any grant the required permission to this user
        
        // For now, check if user is in authorized viewers list
        if protected_content.authorized_viewers.contains(&user) {
            return Ok(true);
        }

        // Check public access
        if protected_content.access_level == AccessLevel::Public && 
           usage_type == LicenseType::View {
            return Ok(true);
        }

        Ok(false)
    }

    /// Get session content information
    pub fn get_session_content(env: Env, session_id: Symbol) -> Vec<Symbol> {
        let session_key = DataKey::Session(session_id);
        let session: Option<SessionRecord> = env.storage().persistent().get(&session_key);
        
        match session {
            Some(s) => s.protected_content,
            None => Vec::new(&env),
        }
    }

    /// Get content access logs
    pub fn get_content_access_log(
        env: Env,
        content_id: Symbol,
        user: Address,
    ) -> Option<AccessLog> {
        let log_key = DataKey::ContentAccess(content_id, user);
        env.storage().persistent().get(&log_key)
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
        assert_eq!(record.protected_content.len(), 0);
        assert_eq!(record.content_licenses.len(), 0);
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
            client.register_session(
                &sid,
                &mentor,
                &learner,
                &2_000_000u64,
                &60u32,
                &100i128,
                &token,
            );
        }

        let mentor_sessions = client.get_sessions_by_mentor(&mentor);
        assert_eq!(mentor_sessions.len(), 3);

        let learner_sessions = client.get_sessions_by_learner(&learner);
        assert_eq!(learner_sessions.len(), 3);
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
}
