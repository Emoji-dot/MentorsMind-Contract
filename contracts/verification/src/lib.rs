#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env};

/// Default grace period: 7 days in seconds
const DEFAULT_GRACE_PERIOD_SECS: u64 = 7 * 24 * 60 * 60;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract-isolated storage namespace root (#826).
    NamespaceRoot,
    Admin,
    Verification(Address),
    Tier(Address),
    GracePeriod,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationRecord {
    pub credential_hash: BytesN<32>,
    pub verified_at: u64,
    pub expiry: u64,
    pub is_active: bool,
    /// Grace period in seconds — allows verification to remain valid after expiry
    pub grace_period_secs: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationStatus {
    pub is_verified: bool,
    pub is_grace: bool,
    pub expires_at: u64,
    pub grace_expires_at: u64,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationRenewedEventData {
    pub mentor: Address,
    pub new_expiry: u64,
    pub renewed_at: u64,
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
        
        let grace_period = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::GracePeriod)
            .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);
        
        let rec = VerificationRecord {
            credential_hash,
            verified_at: now,
            expiry,
            is_active: true,
            grace_period_secs: grace_period,
        };
        let key = DataKey::Verification(mentor.clone());
        env.storage().persistent().set(&key, &rec);
        let tkey = DataKey::Tier(mentor.clone());
        if !env.storage().persistent().has(&tkey) {
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
            Some(r) => {
                if !r.is_active {
                    return false;
                }
                let now = env.ledger().timestamp();
                // Within expiry window → verified
                if now <= r.expiry {
                    return true;
                }
                // Within grace period window → verified (with grace flag)
                let grace_expires = r.expiry.checked_add(r.grace_period_secs).unwrap_or(u64::MAX);
                now <= grace_expires
            }
        }
    }

    /// Check if mentor is verified, ignoring grace period.
    /// Used for gating new sessions where credentials must not be expired.
    pub fn is_verified_strict(env: Env, mentor: Address) -> bool {
        let key = DataKey::Verification(mentor);
        let rec: Option<VerificationRecord> = env.storage().persistent().get(&key);
        match rec {
            None => false,
            Some(r) => r.is_active && env.ledger().timestamp() <= r.expiry,
        }
    }

    /// Get detailed verification status including grace period info.
    pub fn get_verification_status(env: Env, mentor: Address) -> VerificationStatus {
        let key = DataKey::Verification(mentor);
        let rec: Option<VerificationRecord> = env.storage().persistent().get(&key);
        match rec {
            None => VerificationStatus {
                is_verified: false,
                is_grace: false,
                expires_at: 0,
                grace_expires_at: 0,
            },
            Some(r) => {
                let now = env.ledger().timestamp();
                let grace_expires = r.expiry.checked_add(r.grace_period_secs).unwrap_or(u64::MAX);
                let is_verified = r.is_active && now <= grace_expires;
                let is_grace = r.is_active && now > r.expiry && now <= grace_expires;
                VerificationStatus {
                    is_verified,
                    is_grace,
                    expires_at: r.expiry,
                    grace_expires_at: grace_expires,
                }
            }
        }
    }

    /// Renew a mentor's verification by setting a new expiry (admin only).
    ///
    /// Auth: Only the admin can renew verifications.
    /// Resets the grace period counter for the new expiry window.
    ///
    /// Panics if:
    /// - Contract is not initialized
    /// - Caller is not the admin
    /// - Mentor is not verified
    pub fn renew_verification(env: Env, mentor: Address, new_expiry: u64) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let key = DataKey::Verification(mentor.clone());
        let mut rec: VerificationRecord = env.storage().persistent().get(&key).expect("Not verified");
        
        rec.expiry = new_expiry;
        env.storage().persistent().set(&key, &rec);

        env.events().publish(
            (
                symbol_short!("Verify"),
                symbol_short!("Renew"),
                mentor.clone(),
            ),
            VerificationRenewedEventData {
                mentor,
                new_expiry,
                renewed_at: env.ledger().timestamp(),
            },
        );
    }

    /// Set the global grace period in seconds (admin only).
    /// Default: 7 days (604_800 seconds).
    pub fn set_grace_period(env: Env, grace_period_secs: u64) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::GracePeriod, &grace_period_secs);
    }

    /// Get the current global grace period in seconds.
    pub fn get_grace_period(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::GracePeriod)
            .unwrap_or(DEFAULT_GRACE_PERIOD_SECS)
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::Env;

    struct TestFixture {
        env: Env,
        admin: Address,
        mentor: Address,
        contract_id: Address,
    }

    impl TestFixture {
        fn setup() -> Self {
            let env = Env::default();
            env.mock_all_auths();

            let admin = Address::generate(&env);
            let mentor = Address::generate(&env);
            let contract_id = env.register_contract(None, VerificationContract);

            let client = VerificationContractClient::new(&env, &contract_id);
            client.initialize(&admin);

            TestFixture {
                env,
                admin,
                mentor,
                contract_id,
            }
        }

        fn client(&self) -> VerificationContractClient {
            VerificationContractClient::new(&self.env, &self.contract_id)
        }
    }

    #[test]
    fn test_initialize() {
        let f = TestFixture::setup();
        let client = f.client();
        // Should not panic on initialization
        assert_eq!(client.get_grace_period(), DEFAULT_GRACE_PERIOD_SECS);
    }

    #[test]
    fn test_verify_mentor_creates_record() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(
            &f.env,
            &[0u8; 32],
        );

        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        let status = client.get_verification_status(&f.mentor);
        assert!(status.is_verified);
        assert!(!status.is_grace);
        assert_eq!(status.expires_at, 5000);
    }

    #[test]
    fn test_is_verified_within_expiry() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        // At timestamp 1000, expiry is 5000 → verified
        assert!(client.is_verified(&f.mentor));
        assert!(client.is_verified_strict(&f.mentor));
    }

    #[test]
    fn test_is_verified_grace_period() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        // Set grace period to 1000 seconds
        client.set_grace_period(&1000u64);

        // At timestamp 5100 (within grace: 5000 + 1000)
        f.env.ledger().set_timestamp(5100);
        assert!(client.is_verified(&f.mentor)); // grace window
        assert!(!client.is_verified_strict(&f.mentor)); // strict check fails
    }

    #[test]
    fn test_is_verified_after_grace_period_expires() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        // Set grace period to 1000 seconds
        client.set_grace_period(&1000u64);

        // At timestamp 6001 (beyond grace: 5000 + 1000)
        f.env.ledger().set_timestamp(6001);
        assert!(!client.is_verified(&f.mentor)); // grace expired
        assert!(!client.is_verified_strict(&f.mentor)); // strict check fails
    }

    #[test]
    fn test_is_verified_strict_ignores_grace() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        // At timestamp 5500 (beyond expiry but within default grace period)
        f.env.ledger().set_timestamp(5500);
        assert!(client.is_verified(&f.mentor)); // grace active
        assert!(!client.is_verified_strict(&f.mentor)); // strict rejects
    }

    #[test]
    fn test_revoke_verification() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        assert!(client.is_verified(&f.mentor));

        client.revoke_verification(&f.mentor);

        assert!(!client.is_verified(&f.mentor));
        assert!(!client.is_verified_strict(&f.mentor));
    }

    #[test]
    fn test_renew_verification() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        // Set grace period to 100 seconds
        client.set_grace_period(&100u64);

        // At timestamp 5050 (within grace)
        f.env.ledger().set_timestamp(5050);
        assert!(client.is_verified(&f.mentor));

        // Renew to timestamp 10000
        client.renew_verification(&f.mentor, &10000u64);

        // Now at timestamp 5050, new expiry is 10000 → verified
        assert!(client.is_verified_strict(&f.mentor));
    }

    #[test]
    fn test_credential_expires_during_session() {
        let f = TestFixture::setup();
        let client = f.client();

        // Session starts at timestamp 1000
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        // Credential expires at 2000, session ends at 3000
        client.verify_mentor(&f.mentor, &cred_hash, &2000u64);

        // Set grace period to 2000 seconds (covers gap until session end)
        client.set_grace_period(&2000u64);

        // At session end (3000), credential is expired but within grace
        f.env.ledger().set_timestamp(3000);
        assert!(client.is_verified(&f.mentor)); // in-flight session allowed
        assert!(!client.is_verified_strict(&f.mentor)); // but cannot start new session
    }

    #[test]
    fn test_get_verification_status_detailed() {
        let f = TestFixture::setup();
        let client = f.client();
        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);

        let status = client.get_verification_status(&f.mentor);
        assert!(status.is_verified);
        assert!(!status.is_grace);
        assert_eq!(status.expires_at, 5000);
        assert_eq!(status.grace_expires_at, 5000 + DEFAULT_GRACE_PERIOD_SECS);
    }

    #[test]
    fn test_grace_period_update() {
        let f = TestFixture::setup();
        let client = f.client();

        assert_eq!(client.get_grace_period(), DEFAULT_GRACE_PERIOD_SECS);

        client.set_grace_period(&1000u64);
        assert_eq!(client.get_grace_period(), 1000u64);

        client.set_grace_period(&5000u64);
        assert_eq!(client.get_grace_period(), 5000u64);
    }

    #[test]
    fn test_multiple_mentors_independent_grace() {
        let f = TestFixture::setup();
        let client = f.client();
        let mentor2 = Address::generate(&f.env);

        f.env.ledger().set_timestamp(1000);

        let cred_hash = soroban_sdk::BytesN::<32>::from_array(&f.env, &[0u8; 32]);
        
        // Mentor 1 expires at 5000
        client.verify_mentor(&f.mentor, &cred_hash, &5000u64);
        // Mentor 2 expires at 6000
        client.verify_mentor(&mentor2, &cred_hash, &6000u64);

        client.set_grace_period(&100u64);

        // At 5050: mentor1 in grace, mentor2 verified
        f.env.ledger().set_timestamp(5050);
        let status1 = client.get_verification_status(&f.mentor);
        let status2 = client.get_verification_status(&mentor2);

        assert!(status1.is_grace);
        assert!(!status2.is_grace);
        assert!(status2.is_verified);
    }
}
