#![no_std]
use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, symbol_short, vec, Address, Env, Symbol,
    Vec,
};

const MIN_CERT_RATING: u64 = 400; // 4.0/5.0 * 100
const MIN_SESSIONS_COMPLETED: u32 = 3;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateRecord {
    pub id: u64,
    pub learner: Address,
    pub mentor: Address,
    pub skill: Symbol,
    pub sessions_completed: u32,
    pub issued_at: u64,
    pub revoked: bool,
    pub session_id: Symbol,
    pub rating_at_time: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Backend,
    Counter,
    Cert(u64),
    LearnerCerts(Address),
    SkillCerts(Symbol),
    EscrowContract,
    ReputationContract,
    SessionRegistry,
}

#[contractclient(name = "EscrowClient")]
pub trait EscrowTrait {
    fn get_escrow_by_session(env: Env, session_id: Symbol) -> shared::EscrowRecord;
}

#[contractclient(name = "ReputationClient")]
pub trait ReputationTrait {
    fn get_mentor_rating(env: Env, mentor: Address) -> (u64, u64);
}

#[contractclient(name = "SessionRegistryClient")]
pub trait SessionRegistryTrait {
    fn get_sessions_by_learner(env: Env, learner: Address) -> Vec<Symbol>;
    fn get_session(env: Env, session_id: Symbol) -> shared::escrow::EscrowRecord;
}

#[contract]
pub struct Certificates;

#[contractimpl]
impl Certificates {
    pub fn initialize(
        env: Env,
        admin: Address,
        backend: Address,
        escrow_contract: Address,
        reputation_contract: Address,
        session_registry: Address,
    ) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::Backend, &backend);
        env.storage()
            .persistent()
            .set(&DataKey::EscrowContract, &escrow_contract);
        env.storage()
            .persistent()
            .set(&DataKey::ReputationContract, &reputation_contract);
        env.storage()
            .persistent()
            .set(&DataKey::SessionRegistry, &session_registry);
    }

    /// Issue a gated certificate. Platform backend only.
    /// Verifies: escrow released, mentor rating >= 4.0, learner completed >= N sessions.
    pub fn issue_certificate(
        env: Env,
        learner: Address,
        mentor: Address,
        skill: Symbol,
        session_id: Symbol,
        issued_at: u64,
    ) -> u64 {
        let backend: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Backend)
            .expect("not initialized");
        backend.require_auth();

        // 1. Verify escrow is Released
        let escrow_addr: Address = env
            .storage()
            .persistent()
            .get(&DataKey::EscrowContract)
            .expect("escrow contract not set");
        let escrow_client = EscrowClient::new(&env, &escrow_addr);
        let escrow = escrow_client.get_escrow_by_session(&session_id);
        if escrow.status != shared::EscrowStatus::Released {
            panic!("escrow not released");
        }

        // 2. Verify mentor rating >= MIN_CERT_RATING
        let reputation_addr: Address = env
            .storage()
            .persistent()
            .get(&DataKey::ReputationContract)
            .expect("reputation contract not set");
        let reputation_client = ReputationClient::new(&env, &reputation_addr);
        let (rating, _count) = reputation_client.get_mentor_rating(&mentor);
        if rating < MIN_CERT_RATING {
            panic!("mentor rating too low");
        }

        // 3. Verify learner completed >= MIN_SESSIONS_COMPLETED sessions
        let session_registry_addr: Address = env
            .storage()
            .persistent()
            .get(&DataKey::SessionRegistry)
            .expect("session registry not set");
        let session_client = SessionRegistryClient::new(&env, &session_registry_addr);
        let sessions = session_client.get_sessions_by_learner(&learner);
        if sessions.len() < MIN_SESSIONS_COMPLETED {
            panic!("insufficient sessions completed");
        }

        let id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::Counter)
            .unwrap_or(0)
            + 1;
        env.storage().persistent().set(&DataKey::Counter, &id);

        let cert = CertificateRecord {
            id,
            learner: learner.clone(),
            mentor,
            skill: skill.clone(),
            sessions_completed: sessions.len(),
            issued_at,
            revoked: false,
            session_id: session_id.clone(),
            rating_at_time: rating,
        };

        env.storage().persistent().set(&DataKey::Cert(id), &cert);
        push_id(&env, &DataKey::LearnerCerts(learner.clone()), id);
        push_id(&env, &DataKey::SkillCerts(skill.clone()), id);

        env.events().publish(
            (
                Symbol::new(&env, "CertificateEarned"),
                learner,
            ),
            (mentor, session_id, skill, rating),
        );

        id
    }

    /// Soulbound: transfers are forbidden.
    pub fn transfer(_env: Env, _to: Address, _cert_id: u64) {
        panic!("non-transferable");
    }

    /// Admin only: revoke a certificate.
    pub fn revoke_certificate(env: Env, cert_id: u64) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        let mut cert: CertificateRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Cert(cert_id))
            .expect("cert not found");
        cert.revoked = true;
        env.storage().persistent().set(&DataKey::Cert(cert_id), &cert);

        env.events()
            .publish((symbol_short!("cert_rev"), cert.learner), cert_id);
    }

    /// Returns (is_valid, record). is_valid = exists && !revoked.
    pub fn verify_certificate(env: Env, cert_id: u64) -> (bool, CertificateRecord) {
        let cert: CertificateRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Cert(cert_id))
            .expect("cert not found");
        (!cert.revoked, cert)
    }

    pub fn get_certificates_by_learner(env: Env, learner: Address) -> Vec<CertificateRecord> {
        load_certs(&env, &DataKey::LearnerCerts(learner))
    }

    pub fn get_certificates_by_skill(env: Env, skill: Symbol) -> Vec<CertificateRecord> {
        load_certs(&env, &DataKey::SkillCerts(skill))
    }
}

fn push_id(env: &Env, key: &DataKey, id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(key)
        .unwrap_or_else(|| vec![env]);
    ids.push_back(id);
    env.storage().persistent().set(key, &ids);
}

fn load_certs(env: &Env, key: &DataKey) -> Vec<CertificateRecord> {
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(key)
        .unwrap_or_else(|| vec![env]);
    let mut out: Vec<CertificateRecord> = vec![env];
    for id in ids.iter() {
        if let Some(cert) = env.storage().persistent().get(&DataKey::Cert(id)) {
            out.push_back(cert);
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn deploy(env: &Env) -> (CertificatesClient, Address, Address, Address, Address) {
        let contract_id = env.register_contract(None, Certificates);
        let c = CertificatesClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let backend = Address::generate(env);
        let learner = Address::generate(env);
        let mentor = Address::generate(env);
        let escrow_contract = Address::generate(env);
        let reputation_contract = Address::generate(env);
        let session_registry = Address::generate(env);
        c.initialize(&admin, &backend, &escrow_contract, &reputation_contract, &session_registry);
        (c, admin, backend, learner, mentor)
    }

    #[test]
    fn test_issue_and_verify() {
        let env = Env::default();
        env.mock_all_auths();
        let (c, _, _, learner, mentor) = deploy(&env);

        let skill = symbol_short!("RUST");
        let id = c.issue_certificate(&learner, &mentor, &skill, &5, &1000u64);
        assert_eq!(id, 1);

        let (valid, record) = c.verify_certificate(&id);
        assert!(valid);
        assert_eq!(record.learner, learner);
        assert_eq!(record.skill, skill);
        assert_eq!(record.sessions_completed, 5);
        assert!(!record.revoked);
    }

    #[test]
    fn test_revoke() {
        let env = Env::default();
        env.mock_all_auths();
        let (c, _, _, learner, mentor) = deploy(&env);

        let id = c.issue_certificate(&learner, &mentor, &symbol_short!("RUST"), &3, &500u64);
        c.revoke_certificate(&id);

        let (valid, record) = c.verify_certificate(&id);
        assert!(!valid);
        assert!(record.revoked);
    }

    #[test]
    #[should_panic(expected = "non-transferable")]
    fn test_transfer_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (c, _, _, learner, mentor) = deploy(&env);
        let id = c.issue_certificate(&learner, &mentor, &symbol_short!("RUST"), &1, &0u64);
        let other = Address::generate(&env);
        c.transfer(&other, &id);
    }

    #[test]
    fn test_get_certificates_by_learner() {
        let env = Env::default();
        env.mock_all_auths();
        let (c, _, _, learner, mentor) = deploy(&env);

        let skill = symbol_short!("RUST");
        c.issue_certificate(&learner, &mentor, &skill, &2, &100u64);
        c.issue_certificate(&learner, &mentor, &skill, &4, &200u64);

        let certs = c.get_certificates_by_learner(&learner);
        assert_eq!(certs.len(), 2);
    }

    #[test]
    fn test_get_certificates_by_skill() {
        let env = Env::default();
        env.mock_all_auths();
        let (c, _, _, learner, mentor) = deploy(&env);

        let rust = symbol_short!("RUST");
        let go = symbol_short!("GO");
        let learner2 = Address::generate(&env);

        c.issue_certificate(&learner, &mentor, &rust, &3, &100u64);
        c.issue_certificate(&learner2, &mentor, &rust, &5, &200u64);
        c.issue_certificate(&learner, &mentor, &go, &2, &300u64);

        assert_eq!(c.get_certificates_by_skill(&rust).len(), 2);
        assert_eq!(c.get_certificates_by_skill(&go).len(), 1);
    }
}
