use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol};
use shared::{
    IPVerification, IPRecord, IPType, OwnershipProof,
    ContentProtection, ProtectedContent, ContentType, AccessLevel,
    SharedError,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Verification(Address),
    Tier(Address),
    IPRecord(Symbol),           // IP records by IP ID
    ContentOwnership(Symbol),   // Content ownership by content ID
    IPUsage(Symbol, Address),   // IP usage by IP ID and user
    InfringementCase(Symbol),   // Infringement cases by case ID
    TakedownRequest(Symbol),    // Takedown requests by request ID
    RecoveryAction(Symbol),     // Recovery actions by action ID
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationRecord {
    pub credential_hash: BytesN<32>,
    pub verified_at: u64,
    pub expiry: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TakedownRequest {
    pub request_id: Symbol,
    pub content_id: Symbol,
    pub ip_id: Symbol,
    pub requester: Address,
    pub target_platform: Symbol,
    pub reason: Symbol,
    pub evidence_hash: BytesN<32>,
    pub requested_at: u64,
    pub status: Symbol, // "pending", "processing", "completed", "rejected"
    pub processed_by: Option<Address>,
    pub processed_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RecoveryAction {
    pub action_id: Symbol,
    pub ip_id: Symbol,
    pub recovery_type: Symbol, // "takedown", "dmca", "legal", "platform_report"
    pub target: Address,
    pub initiated_by: Address,
    pub initiated_at: u64,
    pub completed_at: Option<u64>,
    pub status: Symbol, // "initiated", "in_progress", "completed", "failed"
    pub outcome: Option<Symbol>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorVerifiedEventData {
    pub credential_hash: BytesN<32>,
    pub verified_at: u64,
    pub expiry: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationRevokedEventData {
    pub revoked: bool,
}

#[contract]
pub struct VerificationContract;

#[contractimpl]
impl VerificationContract {
    /// Initialize the verification contract with an admin.
    ///
    /// Auth: No authorization required for initialization.
    /// Can only be called once.
    ///
    /// Panics if:
    /// - Contract is already initialized
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
    }

    /// Verify a mentor with credentials (admin only).
    ///
    /// Auth: Only the admin can verify mentors.
    /// The admin address is retrieved from persistent storage.
    ///
    /// Panics if:
    /// - Contract is not initialized
    /// - Caller is not the admin
    /// - Caller fails authorization check
    pub fn verify_mentor(env: Env, mentor: Address, credential_hash: BytesN<32>, expiry: u64) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        let now = env.ledger().timestamp();
        let rec = VerificationRecord {
            credential_hash,
            verified_at: now,
            expiry,
            is_active: true,
        };
        let key = DataKey::Verification(mentor.clone());
        env.storage().persistent().set(&key, &rec);
        let tkey = DataKey::Tier(mentor.clone());
        if !env.storage().persistent().has(&tkey) {
            // New mentors start from the base tier until a separate promotion
            // path raises their score.
            env.storage().persistent().set(&tkey, &0i32);
        }
        env.events().publish(
            (
                symbol_short!("Verify"),
                symbol_short!("VrfyOk"),
                mentor.clone(),
            ),
            MentorVerifiedEventData {
                credential_hash: rec.credential_hash.clone(),
                verified_at: rec.verified_at,
                expiry: rec.expiry,
            },
        );
    }

    /// Revoke a mentor's verification (admin only).
    ///
    /// Auth: Only the admin can revoke verifications.
    /// The admin address is retrieved from persistent storage.
    ///
    /// Panics if:
    /// - Contract is not initialized
    /// - Caller is not the admin
    /// - Caller fails authorization check
    /// - Mentor is not verified
    pub fn revoke_verification(env: Env, mentor: Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        let key = DataKey::Verification(mentor.clone());
        let mut rec: VerificationRecord =
            env.storage().persistent().get(&key).expect("Not verified");
        rec.is_active = false;
        env.storage().persistent().set(&key, &rec);
        env.events().publish(
            (
                symbol_short!("Verify"),
                symbol_short!("Revoke"),
                mentor.clone(),
            ),
            VerificationRevokedEventData { revoked: true },
        );
    }

    pub fn is_verified(env: Env, mentor: Address) -> bool {
        let key = DataKey::Verification(mentor);
        let rec: Option<VerificationRecord> = env.storage().persistent().get(&key);
        match rec {
            None => false,
            // Verification is only valid while the record is active and the
            // recorded expiry has not been reached yet.
            Some(r) => r.is_active && env.ledger().timestamp() <= r.expiry,
        }
    }

    pub fn get_verification(env: Env, mentor: Address) -> VerificationRecord {
        let key = DataKey::Verification(mentor);
        env.storage().persistent().get(&key).expect("Not verified")
    }

    /// Verify content ownership and create IP record
    pub fn verify_content_ownership(
        env: Env,
        ip_id: Symbol,
        owner: Address,
        ip_type: IPType,
        title: Symbol,
        description_hash: BytesN<32>,
        ownership_proof: OwnershipProof,
    ) -> Result<(), SharedError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(SharedError::NotInitialized)?;
        admin.require_auth();

        let ip_record = IPVerification::create_ip_record(
            &env,
            ip_id.clone(),
            owner.clone(),
            ip_type.clone(),
            title,
            description_hash,
            ownership_proof,
        )?;

        // Store IP record
        let ip_key = DataKey::IPRecord(ip_id.clone());
        env.storage().persistent().set(&ip_key, &ip_record);

        // Emit ownership verification event
        env.events().publish(
            (
                symbol_short!("Verify"),
                Symbol::new(&env, "IPOwnershipVerified"),
                ip_id,
            ),
            (owner, ip_type),
        );

        Ok(())
    }

    /// Validate IP claims for a specific content
    pub fn validate_ip_claims(
        env: Env,
        ip_id: Symbol,
        claimant: Address,
    ) -> Result<bool, SharedError> {
        // Get IP record
        let ip_key = DataKey::IPRecord(ip_id);
        let ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        // Validate claims using IP verification utilities
        IPVerification::validate_ip_claims(&env, &ip_record, &claimant)
    }

    /// Register content ownership for a mentor
    pub fn register_content_ownership(
        env: Env,
        content_id: Symbol,
        mentor: Address,
        ip_id: Symbol,
        _license_terms_hash: Option<BytesN<32>>,
    ) -> Result<(), SharedError> {
        // Verify mentor is verified
        if !Self::is_verified(env.clone(), mentor.clone()) {
            return Err(SharedError::Unauthorized);
        }

        mentor.require_auth();

        // Get IP record to verify ownership
        let ip_key = DataKey::IPRecord(ip_id.clone());
        let ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        // Verify mentor owns the IP
        if ip_record.owner != mentor {
            return Err(SharedError::Unauthorized);
        }

        // Create protected content record
        let protected_content = ContentProtection::create_protected_content(
            &env,
            content_id.clone(),
            mentor.clone(),
            ContentType::Curriculum, // Default to curriculum
            AccessLevel::Licensed,
        )?;

        // Store content ownership record
        let ownership_key = DataKey::ContentOwnership(content_id.clone());
        env.storage().persistent().set(&ownership_key, &protected_content);

        // Emit content registration event
        env.events().publish(
            (
                symbol_short!("Content"),
                Symbol::new(&env, "ContentOwnershipRegistered"),
                content_id,
            ),
            (mentor, ip_id),
        );

        Ok(())
    }

    /// Track IP usage by users
    pub fn track_ip_usage(
        env: Env,
        ip_id: Symbol,
        user: Address,
        usage_type: Symbol,
        context: Symbol,
    ) -> Result<(), SharedError> {
        // Get IP record
        let ip_key = DataKey::IPRecord(ip_id.clone());
        let mut ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        // Check if user is authorized
        let authorized = IPVerification::is_authorized_user(&ip_record, &user);

        // Create usage record
        let usage_record = IPVerification::track_usage(
            &env,
            ip_id.clone(),
            user.clone(),
            usage_type.clone(),
            context,
            authorized,
        );

        // Store usage record
        let usage_key = DataKey::IPUsage(ip_id.clone(), user.clone());
        env.storage().persistent().set(&usage_key, &usage_record);

        // Update IP statistics
        IPVerification::update_usage_stats(&env, &mut ip_record);
        env.storage().persistent().set(&ip_key, &ip_record);

        // Emit usage tracking event
        env.events().publish(
            (
                symbol_short!("Usage"),
                Symbol::new(&env, "IPUsageTracked"),
                ip_id,
            ),
            (user, usage_type, authorized),
        );

        Ok(())
    }

    /// Report IP infringement
    pub fn report_infringement(
        env: Env,
        infringement_id: Symbol,
        ip_id: Symbol,
        alleged_infringer: Address,
        evidence_hash: BytesN<32>,
        reporter: Address,
    ) -> Result<(), SharedError> {
        // Verify reporter has standing to report (IP owner or authorized user)
        let ip_key = DataKey::IPRecord(ip_id.clone());
        let ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        if ip_record.owner != reporter && !IPVerification::is_authorized_user(&ip_record, &reporter) {
            return Err(SharedError::Unauthorized);
        }

        reporter.require_auth();

        // Create infringement record
        let infringement = IPVerification::report_infringement(
            &env,
            infringement_id.clone(),
            ip_id.clone(),
            alleged_infringer.clone(),
            evidence_hash,
            reporter.clone(),
        );

        // Store infringement record
        let infringement_key = DataKey::InfringementCase(infringement_id.clone());
        env.storage().persistent().set(&infringement_key, &infringement);

        // Emit infringement report event
        env.events().publish(
            (
                symbol_short!("Infring"),
                Symbol::new(&env, "InfringementReported"),
                infringement_id,
            ),
            (ip_id, alleged_infringer, reporter),
        );

        Ok(())
    }

    /// Initiate takedown request
    pub fn initiate_takedown(
        env: Env,
        request_id: Symbol,
        content_id: Symbol,
        ip_id: Symbol,
        target_platform: Symbol,
        reason: Symbol,
        evidence_hash: BytesN<32>,
        requester: Address,
    ) -> Result<(), SharedError> {
        // Verify requester owns the IP
        let ip_key = DataKey::IPRecord(ip_id.clone());
        let ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        if ip_record.owner != requester {
            return Err(SharedError::Unauthorized);
        }

        requester.require_auth();

        // Create takedown request
        let takedown_request = TakedownRequest {
            request_id: request_id.clone(),
            content_id,
            ip_id: ip_id.clone(),
            requester: requester.clone(),
            target_platform,
            reason,
            evidence_hash,
            requested_at: env.ledger().timestamp(),
            status: Symbol::new(&env, "pending"),
            processed_by: None,
            processed_at: None,
        };

        // Store takedown request
        let takedown_key = DataKey::TakedownRequest(request_id.clone());
        env.storage().persistent().set(&takedown_key, &takedown_request);

        // Emit takedown request event
        env.events().publish(
            (
                symbol_short!("Takedown"),
                Symbol::new(&env, "TakedownRequested"),
                request_id,
            ),
            (ip_id, requester),
        );

        Ok(())
    }

    /// Process takedown request (admin only)
    pub fn process_takedown(
        env: Env,
        request_id: Symbol,
        processor: Address,
        approved: bool,
    ) -> Result<(), SharedError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(SharedError::NotInitialized)?;
        
        if processor != admin {
            return Err(SharedError::Unauthorized);
        }
        
        processor.require_auth();

        // Get takedown request
        let takedown_key = DataKey::TakedownRequest(request_id.clone());
        let mut takedown_request: TakedownRequest = env
            .storage()
            .persistent()
            .get(&takedown_key)
            .ok_or(SharedError::NotFound)?;

        // Update request status
        let now = env.ledger().timestamp();
        takedown_request.status = if approved {
            Symbol::new(&env, "approved")
        } else {
            Symbol::new(&env, "rejected")
        };
        takedown_request.processed_by = Some(processor.clone());
        takedown_request.processed_at = Some(now);

        // Store updated request
        env.storage().persistent().set(&takedown_key, &takedown_request);

        // Emit processing event
        env.events().publish(
            (
                symbol_short!("Takedown"),
                Symbol::new(&env, "TakedownProcessed"),
                request_id,
            ),
            (processor, approved),
        );

        Ok(())
    }

    /// Initiate IP recovery action
    pub fn initiate_recovery_action(
        env: Env,
        action_id: Symbol,
        ip_id: Symbol,
        recovery_type: Symbol,
        target: Address,
        initiator: Address,
    ) -> Result<(), SharedError> {
        // Verify initiator owns the IP
        let ip_key = DataKey::IPRecord(ip_id.clone());
        let ip_record: IPRecord = env
            .storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)?;

        if ip_record.owner != initiator {
            return Err(SharedError::Unauthorized);
        }

        initiator.require_auth();

        // Create recovery action
        let recovery_action = RecoveryAction {
            action_id: action_id.clone(),
            ip_id: ip_id.clone(),
            recovery_type: recovery_type.clone(),
            target,
            initiated_by: initiator.clone(),
            initiated_at: env.ledger().timestamp(),
            completed_at: None,
            status: Symbol::new(&env, "initiated"),
            outcome: None,
        };

        // Store recovery action
        let recovery_key = DataKey::RecoveryAction(action_id.clone());
        env.storage().persistent().set(&recovery_key, &recovery_action);

        // Emit recovery action event
        env.events().publish(
            (
                symbol_short!("Recovery"),
                Symbol::new(&env, "RecoveryActionInitiated"),
                action_id,
            ),
            (ip_id, recovery_type, initiator),
        );

        Ok(())
    }

    /// Complete IP recovery action
    pub fn complete_recovery_action(
        env: Env,
        action_id: Symbol,
        outcome: Symbol,
        completer: Address,
    ) -> Result<(), SharedError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(SharedError::NotInitialized)?;
        
        if completer != admin {
            return Err(SharedError::Unauthorized);
        }
        
        completer.require_auth();

        // Get recovery action
        let recovery_key = DataKey::RecoveryAction(action_id.clone());
        let mut recovery_action: RecoveryAction = env
            .storage()
            .persistent()
            .get(&recovery_key)
            .ok_or(SharedError::NotFound)?;

        // Update action
        let now = env.ledger().timestamp();
        recovery_action.completed_at = Some(now);
        recovery_action.status = Symbol::new(&env, "completed");
        recovery_action.outcome = Some(outcome.clone());

        // Store updated action
        env.storage().persistent().set(&recovery_key, &recovery_action);

        // Emit completion event
        env.events().publish(
            (
                symbol_short!("Recovery"),
                Symbol::new(&env, "RecoveryActionCompleted"),
                action_id,
            ),
            (outcome, completer),
        );

        Ok(())
    }

    /// Get IP record
    pub fn get_ip_record(env: Env, ip_id: Symbol) -> Result<IPRecord, SharedError> {
        let ip_key = DataKey::IPRecord(ip_id);
        env.storage()
            .persistent()
            .get(&ip_key)
            .ok_or(SharedError::NotFound)
    }

    /// Get content ownership record
    pub fn get_content_ownership(env: Env, content_id: Symbol) -> Result<ProtectedContent, SharedError> {
        let ownership_key = DataKey::ContentOwnership(content_id);
        env.storage()
            .persistent()
            .get(&ownership_key)
            .ok_or(SharedError::NotFound)
    }

    /// Get takedown request
    pub fn get_takedown_request(env: Env, request_id: Symbol) -> Result<TakedownRequest, SharedError> {
        let takedown_key = DataKey::TakedownRequest(request_id);
        env.storage()
            .persistent()
            .get(&takedown_key)
            .ok_or(SharedError::NotFound)
    }

    /// Get recovery action
    pub fn get_recovery_action(env: Env, action_id: Symbol) -> Result<RecoveryAction, SharedError> {
        let recovery_key = DataKey::RecoveryAction(action_id);
        env.storage()
            .persistent()
            .get(&recovery_key)
            .ok_or(SharedError::NotFound)
    }
}

#[cfg(test)]
mod test {}
