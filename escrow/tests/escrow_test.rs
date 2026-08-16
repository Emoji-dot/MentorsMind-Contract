#![cfg(test)]

use mentorminds_escrow::{EscrowContract, EscrowContractClient, EscrowStatus};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, Vec,
};

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn create_token<'a>(env: &'a Env, admin: &Address) -> (Address, StellarAssetClient<'a>) {
    let token_address = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let sac = StellarAssetClient::new(env, &token_address);
    (token_address, sac)
}

fn advance_time(env: &Env, secs: u64) {
    env.ledger().with_mut(|li| li.timestamp += secs);
}

struct TestFixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    mentor: Address,
    learner: Address,
    treasury: Address,
    token_address: Address,
}

impl TestFixture {
    fn setup() -> Self {
        Self::setup_with_fee(500)
    }
    fn setup_with_fee(fee_bps: u32) -> Self {
        Self::setup_full(fee_bps, 0)
    }

    fn setup_full(fee_bps: u32, auto_release_delay_secs: u64) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| li.timestamp = 14_400);

        let contract_id = env.register_contract(None, EscrowContract);
        let admin = Address::generate(&env);
        let mentor = Address::generate(&env);
        let learner = Address::generate(&env);
        let treasury = Address::generate(&env);

        let (token_address, sac) = create_token(&env, &admin);
        sac.mint(&learner, &100_000);

        let client = EscrowContractClient::new(&env, &contract_id);
        let mut approved = Vec::new(&env);
        approved.push_back(token_address.clone());
        client.initialize(
            &admin,
            &treasury,
            &fee_bps,
            &approved,
            &auto_release_delay_secs,
        );

        TestFixture {
            env,
            contract_id,
            admin,
            mentor,
            learner,
            treasury,
            token_address,
        }
    }

    fn client(&self) -> EscrowContractClient<'_> {
        EscrowContractClient::new(&self.env, &self.contract_id)
    }
    fn token(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token_address)
    }
    #[allow(dead_code)]
    fn sac(&self) -> StellarAssetClient<'_> {
        StellarAssetClient::new(&self.env, &self.token_address)
    }

    fn create_escrow_at(&self, amount: i128, session_end_time: u64, session_id: &str) -> u64 {
        self.client().create_escrow(
            &self.mentor,
            &self.learner,
            &amount,
            &Symbol::new(&self.env, session_id),
            &self.token_address,
            &session_end_time,
            &1u32,
        )
    }

    fn create_package_escrow_at(
        &self,
        amount: i128,
        session_end_time: u64,
        session_id: &str,
        total_sessions: u32,
    ) -> u64 {
        self.client().create_escrow(
            &self.mentor,
            &self.learner,
            &amount,
            &Symbol::new(&self.env, session_id),
            &self.token_address,
            &session_end_time,
            &total_sessions,
        )
    }

    fn open_dispute(&self, escrow_id: u64) {
        self.client()
            .dispute(&self.learner, &escrow_id, &symbol_short!("NO_SHOW"));
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn test_session_id_uniqueness() {
    let f = TestFixture::setup();
    f.create_escrow_at(1_000, 0, "S1");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.create_escrow_at(1_000, 0, "S1");
    }));
    assert!(result.is_err(), "Duplicate session_id must panic");

    f.create_escrow_at(1_000, 0, "S2");
}

#[test]
fn test_release_partial() {
    let f = TestFixture::setup_with_fee(500);
    let id = f.create_package_escrow_at(1_200, 0, "S1", 3);

    let mentor_before = f.token().balance(&f.mentor);
    let treasury_before = f.token().balance(&f.treasury);

    f.client().release_partial(&f.learner, &id);

    assert_eq!(f.token().balance(&f.mentor), mentor_before + 380);
    assert_eq!(f.token().balance(&f.treasury), treasury_before + 20);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.amount, 800);
    assert_eq!(e.sessions_completed, 1);
    assert_eq!(e.status, EscrowStatus::Active);

    f.client().release_partial(&f.learner, &id);
    assert_eq!(f.token().balance(&f.mentor), mentor_before + 760);
    assert_eq!(f.token().balance(&f.treasury), treasury_before + 40);

    let e2 = f.client().get_escrow(&id);
    assert_eq!(e2.amount, 400);
    assert_eq!(e2.sessions_completed, 2);
    assert_eq!(e2.status, EscrowStatus::Active);

    f.client().release_partial(&f.learner, &id);
    assert_eq!(f.token().balance(&f.mentor), mentor_before + 1140);
    assert_eq!(f.token().balance(&f.treasury), treasury_before + 60);

    let e3 = f.client().get_escrow(&id);
    assert_eq!(e3.amount, 0);
    assert_eq!(e3.sessions_completed, 3);
    assert_eq!(e3.status, EscrowStatus::Released);
}

#[test]
fn test_three_session_package_full_lifecycle() {
    let f = TestFixture::setup_with_fee(1000);
    let id = f.create_package_escrow_at(3000, 0, "PKG1", 3);

    f.client().release_partial(&f.learner, &id);
    let e1 = f.client().get_escrow(&id);
    assert_eq!(e1.amount, 2000);
    assert_eq!(e1.sessions_completed, 1);
    assert_eq!(f.token().balance(&f.mentor), 900);

    f.client().release_partial(&f.learner, &id);
    let e2 = f.client().get_escrow(&id);
    assert_eq!(e2.amount, 1000);
    assert_eq!(e2.sessions_completed, 2);
    assert_eq!(f.token().balance(&f.mentor), 1800);

    f.client().release_partial(&f.learner, &id);
    let e3 = f.client().get_escrow(&id);
    assert_eq!(e3.amount, 0);
    assert_eq!(e3.sessions_completed, 3);
    assert_eq!(e3.status, EscrowStatus::Released);
    assert_eq!(f.token().balance(&f.mentor), 2700);
    assert_eq!(f.token().balance(&f.treasury), 300);
}

#[test]
#[should_panic(expected = "Escrow not active")]
fn test_over_release_panics() {
    let f = TestFixture::setup();
    let id = f.create_package_escrow_at(1000, 0, "S1", 1);

    f.client().release_partial(&f.learner, &id);
    f.client().release_partial(&f.learner, &id);
}

#[test]
fn test_resolve_dispute_all_to_mentor() {
    let f = TestFixture::setup_with_fee(0);
    let id = f.create_escrow_at(1_000, 0, "S1");
    f.open_dispute(id);

    let mentor_before = f.token().balance(&f.mentor);
    f.client().resolve_dispute(&id, &100u32);

    assert_eq!(f.token().balance(&f.mentor), mentor_before + 1_000);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Resolved);
    assert_eq!(e.net_amount, 1_000);
    assert_eq!(e.platform_fee, 0);
}

#[test]
fn test_resolve_dispute_all_to_learner() {
    let f = TestFixture::setup_with_fee(0);
    let id = f.create_escrow_at(1_000, 0, "S1");
    f.open_dispute(id);

    let learner_before = f.token().balance(&f.learner);
    f.client().resolve_dispute(&id, &0u32);

    assert_eq!(f.token().balance(&f.learner), learner_before + 1_000);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Resolved);
    assert_eq!(e.net_amount, 0);
    assert_eq!(e.platform_fee, 1_000);
}

#[test]
fn test_resolve_dispute_50_50() {
    let f = TestFixture::setup_with_fee(0);
    let id = f.create_escrow_at(1_000, 0, "S1");
    f.open_dispute(id);

    let mentor_before = f.token().balance(&f.mentor);
    let learner_before = f.token().balance(&f.learner);

    f.client().resolve_dispute(&id, &50u32);

    assert_eq!(f.token().balance(&f.mentor), mentor_before + 500);
    assert_eq!(f.token().balance(&f.learner), learner_before + 500);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Resolved);
    assert_eq!(e.net_amount, 500);
    assert_eq!(e.platform_fee, 500);
}

#[test]
fn test_admin_release() {
    let f = TestFixture::setup_with_fee(500);
    let id = f.create_escrow_at(1_000, 0, "S1");

    f.client().admin_release(&id);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Released);
    assert_eq!(f.token().balance(&f.mentor), 950);
}

#[test]
fn test_try_auto_release() {
    let f = TestFixture::setup_full(500, 3600);
    let now = f.env.ledger().timestamp();
    let id = f.create_escrow_at(1_000, now, "S1");

    advance_time(&f.env, 3600 + 1);
    f.client().try_auto_release(&id);

    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Released);
}

#[test]
fn test_query_by_mentor_pagination() {
    let f = TestFixture::setup();
    let mentor = Address::generate(&f.env);
    let learner = f.learner.clone();

    for i in 0..5u32 {
        let session_id = match i {
            0 => Symbol::new(&f.env, "SM0"),
            1 => Symbol::new(&f.env, "SM1"),
            2 => Symbol::new(&f.env, "SM2"),
            3 => Symbol::new(&f.env, "SM3"),
            _ => Symbol::new(&f.env, "SM4"),
        };
        f.client().create_escrow(
            &mentor,
            &learner,
            &1_000,
            &session_id,
            &f.token_address,
            &0,
            &1u32,
        );
    }

    let page0 = f.client().get_escrows_by_mentor(&mentor, &0, &2);
    assert_eq!(page0.len(), 2);
    assert_eq!(page0.get(0).unwrap().id, 1);
    assert_eq!(page0.get(1).unwrap().id, 2);

    let page1 = f.client().get_escrows_by_mentor(&mentor, &1, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get(0).unwrap().id, 3);
    assert_eq!(page1.get(1).unwrap().id, 4);

    let page2 = f.client().get_escrows_by_mentor(&mentor, &2, &2);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap().id, 5);

    let page3 = f.client().get_escrows_by_mentor(&mentor, &3, &2);
    assert_eq!(page3.len(), 0);
}

#[test]
fn test_query_by_learner_pagination() {
    let f = TestFixture::setup();
    let mentor = f.mentor.clone();
    let learner = Address::generate(&f.env);

    let admin = Address::generate(&f.env);
    let (tok, sac) = create_token(&f.env, &admin);
    sac.mint(&learner, &100_000);
    f.client().set_approved_token(&tok, &true);

    for i in 0..3u32 {
        let session_id = match i {
            0 => Symbol::new(&f.env, "SL0"),
            1 => Symbol::new(&f.env, "SL1"),
            _ => Symbol::new(&f.env, "SL2"),
        };
        f.client().create_escrow(
            &mentor,
            &learner,
            &1_000,
            &session_id,
            &tok,
            &0,
            &1u32,
        );
    }

    let page0 = f.client().get_escrows_by_learner(&learner, &0, &10);
    assert_eq!(page0.len(), 3);
}

#[test]
fn test_query_by_status() {
    let f = TestFixture::setup_with_fee(0);

    let id1 = f.create_escrow_at(1_000, 0, "SS1");
    let _id2 = f.create_escrow_at(1_000, 0, "SS2");

    f.client().release_funds(&f.learner, &id1);

    let active_ids = f.client().get_escrows_by_status(&EscrowStatus::Active);
    let released_ids = f.client().get_escrows_by_status(&EscrowStatus::Released);

    assert!(active_ids.iter().any(|id| id == 2));
    assert!(released_ids.iter().any(|id| id == 1));
}

#[test]
fn test_page_size_cap() {
    let f = TestFixture::setup();
    let mentor = f.mentor.clone();
    let learner = f.learner.clone();

    for i in 0..60u32 {
        let session_id = Symbol::new(&f.env, &alloc::format!("SC{}", i));
        f.client().create_escrow(
            &mentor,
            &learner,
            &100,
            &session_id,
            &f.token_address,
            &0,
            &1u32,
        );
    }

    let results = f.client().get_escrows_by_mentor(&mentor, &0, &100);
    assert_eq!(results.len(), 50);
}

// -----------------------------------------------------------------------
// Token Whitelist Bypass Tests
// -----------------------------------------------------------------------

#[test]
fn test_create_escrow_unapproved_token_panics() {
    let f = TestFixture::setup();
    let bad_token = Address::generate(&f.env);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.client().create_escrow(
            &f.mentor,
            &f.learner,
            &500,
            &symbol_short!("BAD"),
            &bad_token,
            &0u64,
            &1u32,
        );
    }));
    assert!(result.is_err(), "unapproved token must be rejected");
}

#[test]
fn test_create_escrow_revoked_token_panics() {
    let f = TestFixture::setup();
    f.client().set_approved_token(&f.token_address, &false);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.create_escrow_at(500, 0, "REVOKED");
    }));
    assert!(result.is_err(), "revoked token must be rejected");
}

#[test]
fn test_token_whitelist_toggle() {
    let f = TestFixture::setup();
    let new_token = Address::generate(&f.env);

    assert!(!f.client().is_token_approved(&new_token));
    f.client().set_approved_token(&new_token, &true);
    assert!(f.client().is_token_approved(&new_token));
    f.client().set_approved_token(&new_token, &false);
    assert!(!f.client().is_token_approved(&new_token));
}

#[test]
fn test_unknown_tokens_not_approved() {
    let f = TestFixture::setup();
    for _ in 0..5 {
        let random = Address::generate(&f.env);
        assert!(!f.client().is_token_approved(&random));
    }
}

#[test]
fn test_re_approve_token_allows_escrow() {
    let f = TestFixture::setup();
    f.client().set_approved_token(&f.token_address, &false);
    assert!(!f.client().is_token_approved(&f.token_address));

    f.client().set_approved_token(&f.token_address, &true);
    assert!(f.client().is_token_approved(&f.token_address));

    let id = f.create_escrow_at(500, 0, "REAPPR");
    assert_eq!(id, 1);
}

// -----------------------------------------------------------------------
// #761: gas estimation
// -----------------------------------------------------------------------

#[test]
fn test_estimate_release_escrow_cost_is_nonzero_and_view_only() {
    let f = TestFixture::setup_with_fee(500);
    let id = f.create_escrow_at(1_000, 0, "GAS1");

    let estimate = f.client().estimate_release_escrow_cost(&id);
    assert!(estimate.base_instructions > 0);
    assert!(estimate.storage_reads > 0);
    assert!(estimate.storage_writes > 0);
    assert!(estimate.cross_contract_calls > 0);

    f.client().release_funds(&f.learner, &id);
    assert_eq!(f.client().get_escrow(&id).status, EscrowStatus::Released);
}

#[test]
fn test_estimate_release_escrow_cost_accounts_for_fee_transfer() {
    let f_with_fee = TestFixture::setup_with_fee(500);
    let id_with_fee = f_with_fee.create_escrow_at(1_000, 0, "GAS2");
    let with_fee = f_with_fee.client().estimate_release_escrow_cost(&id_with_fee);

    let f_no_fee = TestFixture::setup_with_fee(0);
    let id_no_fee = f_no_fee.create_escrow_at(1_000, 0, "GAS3");
    let no_fee = f_no_fee.client().estimate_release_escrow_cost(&id_no_fee);

    assert!(with_fee.cross_contract_calls > no_fee.cross_contract_calls);
    assert!(with_fee.base_instructions > no_fee.base_instructions);
}

#[test]
fn test_estimate_release_escrow_cost_within_tolerance_of_actual() {
    let f = TestFixture::setup_with_fee(500);
    let id = f.create_escrow_at(1_000, 0, "GAS4");

    let estimate = f.client().estimate_release_escrow_cost(&id);

    f.env.budget().reset_default();
    f.client().release_funds(&f.learner, &id);
    let actual = f.env.budget().cpu_instruction_cost();

    let diff = if actual > estimate.base_instructions {
        actual - estimate.base_instructions
    } else {
        estimate.base_instructions - actual
    };
    let tolerance = actual / 5;
    assert!(
        diff <= tolerance,
        "estimate {} vs actual {} exceeds 20% tolerance",
        estimate.base_instructions,
        actual
    );
}

extern crate alloc;

use soroban_sdk::{contractimpl, BytesN};

// -----------------------------------------------------------------------
// Escrow Auto-Release Failure Recovery Tests
// -----------------------------------------------------------------------

#[test]
fn test_recovery_queries_default_to_zero_state() {
    let f = TestFixture::setup();
    let id = f.create_escrow_at(1_000, 0, "REC0");
    assert_eq!(f.client().get_auto_release_attempts(&id), 0u32);
    assert!(f.client().get_stuck_escrows().is_empty());
    assert_eq!(f.client().get_multisig_admin(), None);
}

#[test]
fn test_set_multisig_admin_authorization() {
    let f = TestFixture::setup();
    let ms = Address::generate(&f.env);
    f.client().set_multisig_admin(&f.admin, &ms);
    assert_eq!(f.client().get_multisig_admin(), Some(ms));

    let rogue = Address::generate(&f.env);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.client().set_multisig_admin(&rogue, &rogue);
    }));
    assert!(result.is_err());
}

// ----- Stuck escrow reporting -----

#[test]
#[should_panic(expected = "Stuck-report grace period not elapsed")]
fn test_report_stuck_escrow_rejected_before_grace_period() {
    let auto_release = 3600u64;
    let f = TestFixture::setup_full(0, auto_release);
    let now = f.env.ledger().timestamp();
    let id = f.create_escrow_at(1_000, now, "STK1");
    advance_time(&f.env, 3600 + 100);
    let reporter = Address::generate(&f.env);
    f.client().report_stuck_escrow(&reporter, &id);
}

#[test]
fn test_report_stuck_escrow_succeeds_after_grace_period() {
    let auto_release = 3600u64;
    let grace = 7u64 * 24 * 60 * 60;
    let f = TestFixture::setup_full(0, auto_release);
    let now = f.env.ledger().timestamp();
    let id = f.create_escrow_at(1_000, now, "STK2");
    advance_time(&f.env, auto_release + grace + 100);
    let reporter = Address::generate(&f.env);
    f.client().report_stuck_escrow(&reporter, &id);

    let watch = f.client().get_stuck_escrows();
    assert_eq!(watch.len(), 1);
    assert_eq!(watch.get(0).unwrap(), id);

    // dedup
    f.client().report_stuck_escrow(&reporter, &id);
    assert_eq!(f.client().get_stuck_escrows().len(), 1);
}

// ----- Attempt-count gate -----

#[test]
#[should_panic(expected = "Emergency release requires 3 failed attempts")]
fn test_emergency_release_rejected_below_max_attempts() {
    let f = TestFixture::setup_with_fee(0);
    let id = f.create_escrow_at(1_000, 0, "EMG1");
    assert_eq!(f.client().get_auto_release_attempts(&id), 0u32);
    let reason = BytesN::<32>::from_array(&f.env, &[0x42u8; 32]);
    f.client()
        .emergency_release(&f.admin, &id, &reason, &0u32);
}

// ----- 3 failing attempts => attempt counter reaches 3 -----

fn simulate_3_failed_attempts(f: &TestFixture, id: u64) {
    let panic_addr = f.env.register_contract(None, PanicMockContract);
    f.client()
        .set_reputation_contract(&f.admin, &panic_addr);

    for i in 0..3u32 {
        f.client().try_auto_release(&id);
        assert_eq!(f.client().get_auto_release_attempts(&id), i + 1);
    }
    assert_eq!(f.client().get_escrow(&id).status, EscrowStatus::Active);
}

#[test]
#[should_panic(expected = "MultisigAdmin not configured")]
fn test_emergency_release_requires_multisig_configured() {
    let f = TestFixture::setup_full(0, 0);
    let now = f.env.ledger().timestamp();
    let id = f.create_escrow_at(1_000, now, "EMG2");
    simulate_3_failed_attempts(&f, id);
    let reason = BytesN::<32>::from_array(&f.env, &[0x11u8; 32]);
    f.client()
        .emergency_release(&f.admin, &id, &reason, &0u32);
}

// ----- Full end-to-end recovery: 3 fails + mock multisig => release -----

#[test]
fn test_three_failures_then_emergency_release_succeeds() {
    let fee_bps = 200u32;
    let f = TestFixture::setup_full(fee_bps, 0);
    let env = &f.env;
    let now = env.ledger().timestamp();
    let id = f.create_escrow_at(10_000, now, "EMG3");
    let expected_net: i128 = 10_000 - (10_000 * fee_bps as i128 / 10_000);
    let expected_fee: i128 = 200;

    let mentor_before = f.token().balance(&f.mentor);
    let treasury_before = f.token().balance(&f.treasury);

    simulate_3_failed_attempts(&f, id);

    // 4th attempt gated
    f.client().try_auto_release(&id);
    assert_eq!(f.client().get_auto_release_attempts(&id), 3u32);
    assert_eq!(f.client().get_escrow(&id).status, EscrowStatus::Active);
    assert_eq!(f.token().balance(&f.mentor), mentor_before);

    // Report stuck (grace period 7 days)
    let grace = 7u64 * 24 * 60 * 60;
    advance_time(env, grace + 1000);
    let reporter = Address::generate(env);
    f.client().report_stuck_escrow(&reporter, &id);
    assert!(f
        .client()
        .get_stuck_escrows()
        .iter()
        .any(|x| x == id));

    // Configure mock multisig with 2-of-3 threshold + passing approvals
    let action_id: u32 = 17;
    let ms = env.register_contract(None, MockMultisigContract);
    MockMultisigContractClient::new(env, &ms).configure(
        &2u32,
        &action_id,
        &env.current_contract_address(),
        &Symbol::new(env, "emergency_release"),
        &id,
        &2u32,
        &(env.ledger().timestamp() + 1_000_000u64),
    );
    f.client().set_multisig_admin(&f.admin, &ms);

    // EMERGENCY RELEASE
    let reason = BytesN::<32>::from_array(env, &[0xAAu8; 32]);
    f.client()
        .emergency_release(&f.admin, &id, &reason, &action_id);

    // Verify balances
    assert_eq!(f.token().balance(&f.mentor), mentor_before + expected_net);
    assert_eq!(f.token().balance(&f.treasury), treasury_before + expected_fee);

    // Verify escrow state
    let e = f.client().get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Released);
    assert_eq!(e.amount, 0);
    assert_eq!(e.net_amount, expected_net);
    assert_eq!(e.platform_fee, expected_fee);

    // Recovery state cleaned up
    assert_eq!(f.client().get_auto_release_attempts(&id), 0u32);
    assert!(!f
        .client()
        .get_stuck_escrows()
        .iter()
        .any(|x| x == id));

    // Audit events emitted
    let evts = env.events().all();
    let emergency_event = evts.iter().find(|e| {
        let topics = e.topics();
        topics.len() >= 3
            && topics.get(1).unwrap() == Symbol::new(env, "Escrow").into_val(env)
            && topics.get(2).unwrap()
                == Symbol::new(env, "EmergencyReleased").into_val(env)
    });
    assert!(emergency_event.is_some());

    let standard_release_event = evts.iter().find(|e| {
        let topics = e.topics();
        topics.len() >= 3
            && topics.get(1).unwrap() == Symbol::new(env, "Escrow").into_val(env)
            && topics.get(2).unwrap() == Symbol::new(env, "Released").into_val(env)
    });
    assert!(standard_release_event.is_some());
}

#[test]
#[should_panic(expected = "Insufficient multi-sig approvals")]
fn test_emergency_release_rejected_below_2_of_3_threshold() {
    let f = TestFixture::setup_full(0, 0);
    let now = f.env.ledger().timestamp();
    let id = f.create_escrow_at(1_000, now, "EMG4");
    simulate_3_failed_attempts(&f, id);
    assert_eq!(f.client().get_auto_release_attempts(&id), 3);

    let action_id: u32 = 5;
    let ms = f.env.register_contract(None, MockMultisigContract);
    MockMultisigContractClient::new(&f.env, &ms).configure(
        &2u32,
        &action_id,
        &f.env.current_contract_address(),
        &Symbol::new(&f.env, "emergency_release"),
        &id,
        &1u32,
        &(f.env.ledger().timestamp() + 1_000_000u64),
    );
    f.client().set_multisig_admin(&f.admin, &ms);

    let reason = BytesN::<32>::from_array(&f.env, &[0xBBu8; 32]);
    f.client()
        .emergency_release(&f.admin, &id, &reason, &action_id);
}

// =======================================================================
// Test Mock Contracts
// =======================================================================

pub struct PanicMockContract;

#[contractimpl]
impl PanicMockContract {
    #[allow(non_snake_case)]
    pub fn on_session_released(
        _env: Env,
        _mentor: Address,
        _learner: Address,
        _escrow_id: u64,
        _amount: i128,
    ) {
        panic!("intentional reputation outage for recovery tests");
    }
}

pub struct MockMultisigContract;

use mentorminds_escrow::ProposalRecordMirror;

#[contractclient(name = "MockMultisigContractClient")]
trait MockMultisigApi {
    #[allow(clippy::too_many_arguments)]
    fn configure(
        env: Env,
        threshold: u32,
        action_id: u32,
        target: Address,
        function: Symbol,
        escrow_id: u64,
        approval_count: u32,
        expiry: u64,
    );
    fn get_proposal(env: Env, action_id: u32) -> ProposalRecordMirror;
    fn get_threshold(env: Env) -> u32;
}

#[contractimpl]
impl MockMultisigApi for MockMultisigContract {
    fn configure(
        env: Env,
        threshold: u32,
        action_id: u32,
        target: Address,
        function: Symbol,
        escrow_id: u64,
        approval_count: u32,
        expiry: u64,
    ) {
        let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
        args.push_back(escrow_id.into_val(&env));

        let record = ProposalRecordMirror {
            id: action_id,
            proposer: Address::generate(&env),
            target,
            function,
            args,
            approval_count,
            expiry,
            executed: false,
            cancelled: false,
        };
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "PROP"), &record);
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "THR"), &threshold);
    }

    fn get_proposal(env: Env, _action_id: u32) -> ProposalRecordMirror {
        env.storage()
            .persistent()
            .get(&Symbol::new(&env, "PROP"))
            .expect("configure not called")
    }

    fn get_threshold(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&Symbol::new(&env, "THR"))
            .expect("configure not called")
    }
}
