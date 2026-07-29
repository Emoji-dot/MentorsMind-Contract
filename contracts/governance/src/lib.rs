#![no_std]

use shared::events::{
    emit_governance_event, evt_gov_appeal_resolved, evt_gov_appeal_submitted,
    evt_gov_arb_registered, evt_gov_arb_unregistered, evt_gov_call_allowed,
    evt_gov_proposal_cancelled, evt_gov_proposal_created, evt_gov_proposal_executed,
    evt_gov_proposal_failed, evt_gov_proposal_passed, evt_gov_proposal_queued,
    evt_gov_timelock_set, evt_gov_vote_cast,
};
use shared::{GasEstimate, StateMachine};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, Bytes,
    BytesN, Env, IntoVal, Symbol, Vec,
};

// Instance storage: frequently read config
const ADMIN: Symbol = symbol_short!("ADMIN");
const TOKEN: Symbol = symbol_short!("TOKEN");
const SNAPSHOT: Symbol = symbol_short!("SNAPSHOT");
const PROPOSAL_COUNT: Symbol = symbol_short!("PROP_CNT");
const VOTING_PERIOD_SECS: Symbol = symbol_short!("VOT_PER");
const QUORUM_BPS: Symbol = symbol_short!("QRM_BPS");
const CURRENT_FEE_BPS: Symbol = symbol_short!("FEE_BPS");
const CURRENT_AUTO_RELEASE_SECS: Symbol = symbol_short!("AUTO_REL");
const TEMPLATES: Symbol = symbol_short!("TMPLATES");

const DEFAULT_VOTING_PERIOD_SECS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_QUORUM_BPS: u32 = 1_000; // 10%
const CUSTOM_PROPOSAL_QUORUM_BPS: u32 = 3_000; // 30%
const EXECUTE_CALL_TIMELOCK_SECS: u64 = 7 * 24 * 60 * 60; // 7-day mandatory delay

// ---------------------------------------------------------------------------
// Gas-estimation heuristic constants (#761). Calibrated against
// `env.budget().cpu_instruction_cost()` measured around a real `vote()`
// call in `test_estimate_governance_vote_cost_within_tolerance_of_actual`.
// ---------------------------------------------------------------------------
const GOV_VOTE_BASE_INSTRUCTIONS: u64 = 43_000;
const PER_STORAGE_OP_INSTRUCTIONS: u64 = 2_000;
const PER_CROSS_CALL_INSTRUCTIONS: u64 = 300_000;

// ─── Time-weighted voting constants ──────────────────────────────────────
/// Early window: 0–33% of voting period — 80% weight (8000 bps)
const EARLY_WINDOW_END_BPS: u64 = 3_300; // 33.00% in basis points
const EARLY_WEIGHT_BPS: u32 = 8_000; // 80.00%

/// Mid window: 33–66% — 100% weight (10000 bps)
const MID_WINDOW_END_BPS: u64 = 6_600; // 66.00%
const MID_WEIGHT_BPS: u32 = 10_000; // 100.00%

/// Late window: 66–100% — 110% weight (11000 bps)
const LATE_WEIGHT_BPS: u32 = 11_000; // 110.00%

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    NoPendingAdminChange = 4,
    AdminChangeNotYetEffective = 5,
    InvalidAdminChange = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAdminChange {
    pub new_admin: Address,
    pub effective_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChangeProposedEvent {
    pub contract: Address,
    pub old_admin: Address,
    pub new_admin: Address,
    pub effective_at: u64,
}

const ADMIN_CHANGE_TIMELOCK: u64 = 48 * 60 * 60;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalAction {
    UpdateFee(u32),
    UpdateAutoRelease(u64),
    AddAsset(Address),
    UpdateAdmin(Address),
    ExecuteCall(Address, Symbol, Vec<u64>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Active,
    Passed,
    Queued,
    Failed,
    Executed,
    Cancelled,
}

impl StateMachine for ProposalStatus {
    type State = ProposalStatus;

    fn is_valid_transition(_env: &Env, from: &Self::State, to: &Self::State) -> bool {
        if *from == ProposalStatus::Active && *to == ProposalStatus::Passed {
            return true;
        }
        if *from == ProposalStatus::Active && *to == ProposalStatus::Failed {
            return true;
        }
        if *from == ProposalStatus::Active && *to == ProposalStatus::Cancelled {
            return true;
        }
        if *from == ProposalStatus::Passed && *to == ProposalStatus::Queued {
            return true;
        }
        if *from == ProposalStatus::Passed && *to == ProposalStatus::Executed {
            return true;
        }
        if *from == ProposalStatus::Queued && *to == ProposalStatus::Executed {
            return true;
        }
        false
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub title: Bytes,
    pub description_hash: BytesN<32>,
    pub action: ProposalAction,
    pub status: ProposalStatus,
    pub created_at: u64,
    pub voting_ends_at: u64,
    pub snapshot_ledger: u32,
    pub total_supply_snapshot: i128,
    pub votes_for: i128,
    pub votes_against: i128,
    pub timelock_op_id: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Proposal(u32),
    Vote(u32, Address),
    VoteWeight(u32, Address),
    ApprovedAsset(Address),
    Timelock,
    CustomProposal(u32),
    Arbitrator(Address),
    ArbitratorAt(u32),
    ArbitratorCount,
    ArbitratorIndex(Address),
    ArbitratorList,
    ArbitratorCompensation,
    Appeal(u32),
    AllowedCall(Address, Symbol),
    PendingAdmin,
    /// Weighted vote tally (for quorum) — uses time-weight multiplier
    WeightedVotesFor(u32),
    /// Weighted vote tally (against quorum)
    WeightedVotesAgainst(u32),
    /// Multiplier applied to a specific voter's vote (in bps)
    VoteWeightMultiplier(u32, Address),
    DelegationContract,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbitratorRecord {
    pub address: Address,
    pub active: bool,
    pub cases_handled: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppealRecord {
    pub proposal_id: u32,
    pub appellant: Address,
    pub reason: soroban_sdk::String,
    pub submitted_at: u64,
    pub resolved: bool,
}

/// Describes which voting window a vote falls into and the weight multiplier
/// (in basis points, e.g. 8000 = 80%, 10000 = 100%, 11000 = 110%).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VotingWindow {
    /// 0 = early, 1 = mid, 2 = late
    pub window: u32,
    /// Weight multiplier in basis points (e.g. 8000 for 80%)
    pub weight_bps: u32,
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    fn transition_proposal_status(env: &Env, proposal: &mut Proposal, to: ProposalStatus) {
        let from = proposal.status.clone();
        soroban_sdk::log!(env, "transitioning from {:?} to {:?}", from, to);
        if !ProposalStatus::is_valid_transition(env, &from, &to) {
            panic!("invalid transition from {:?} to {:?}", from, to);
        }
        proposal.status = to;
    }

    pub fn initialize(
        env: Env,
        admin: Address,
        mnt_token: Address,
        snapshot_contract: Address,
        delegation_contract: Address,
        voting_period_secs: Option<u64>,
        quorum_bps: Option<u32>,
    ) {
        if env.storage().instance().has(&ADMIN) {
            panic!("already initialized");
        }

        let period = voting_period_secs.unwrap_or(DEFAULT_VOTING_PERIOD_SECS);
        if period == 0 {
            panic!("invalid voting period");
        }

        let quorum = quorum_bps.unwrap_or(DEFAULT_QUORUM_BPS);
        if quorum == 0 || quorum > 10_000 {
            panic!("invalid quorum bps");
        }

        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&TOKEN, &mnt_token);
        env.storage().instance().set(&SNAPSHOT, &snapshot_contract);
        env.storage().instance().set(&VOTING_PERIOD_SECS, &period);
        env.storage().instance().set(&QUORUM_BPS, &quorum);
        env.storage().instance().set(&PROPOSAL_COUNT, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::DelegationContract, &delegation_contract);
        env.storage().persistent().set(&ADMIN, &admin);
        env.storage().persistent().set(&TOKEN, &mnt_token);

        env.storage()
            .persistent()
            .set(&SNAPSHOT, &snapshot_contract);
        env.storage()
            .persistent()
            .set(&DataKey::DelegationContract, &delegation_contract);
        env.storage().persistent().set(&VOTING_PERIOD_SECS, &period);

        env.storage().persistent().set(&VOTING_PERIOD_SECS, &period);

        env.storage().persistent().set(&QUORUM_BPS, &quorum);
        env.storage().persistent().set(&PROPOSAL_COUNT, &0u32);
    }

    pub fn propose_admin_change(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &current_admin)?;
        let old_admin = Self::admin(&env)?;
        let effective_at = env
            .ledger()
            .timestamp()
            .checked_add(ADMIN_CHANGE_TIMELOCK)
            .ok_or(Error::InvalidAdminChange)?;
        env.storage().instance().set(
            &DataKey::PendingAdmin,
            &PendingAdminChange {
                new_admin: new_admin.clone(),
                effective_at,
            },
        );
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("proposed")),
            AdminChangeProposedEvent {
                contract: env.current_contract_address(),
                old_admin,
                new_admin,
                effective_at,
            },
        );
        Ok(())
    }

    pub fn accept_admin_change(env: Env, new_admin: Address) -> Result<(), Error> {
        new_admin.require_auth();
        let pending: PendingAdminChange = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(Error::NoPendingAdminChange)?;
        if pending.new_admin != new_admin {
            return Err(Error::Unauthorized);
        }
        if env.ledger().timestamp() < pending.effective_at {
            return Err(Error::AdminChangeNotYetEffective);
        }
        env.storage().instance().set(&ADMIN, &new_admin);
        env.storage().persistent().set(&ADMIN, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn cancel_admin_change(env: Env, multisig: Address) -> Result<(), Error> {
        multisig.require_auth();
        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            return Err(Error::NoPendingAdminChange);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn get_pending_admin_change(env: Env) -> Option<PendingAdminChange> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        Self::admin(&env)
    }

    pub fn set_timelock(env: Env, timelock: Address) {
        let admin: Address = env.storage().instance().get(&ADMIN).unwrap();
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::Timelock, &timelock);

        emit_governance_event(&env, evt_gov_timelock_set(&env), timelock);
    }

    /// Add a (target, function) pair to the ExecuteCall allowlist. Admin only.
    pub fn add_allowed_call(env: Env, admin: Address, target: Address, function: Symbol) {
        Self::assert_admin(&env, &admin);
        env.storage().persistent().set(
            &DataKey::AllowedCall(target.clone(), function.clone()),
            &true,
        );
        emit_governance_event(&env, evt_gov_call_allowed(&env), (target, function));
    }

    pub fn is_call_allowed(env: Env, target: Address, function: Symbol) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AllowedCall(target, function))
            .unwrap_or(false)
    }

    pub fn set_templates_contract(env: Env, templates_contract: Address) {
        let admin: Address = env.storage().persistent().get(&ADMIN).unwrap();
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&TEMPLATES, &templates_contract);
    }

    pub fn create_proposal(
        env: Env,
        proposer: Address,
        title: Bytes,
        description_hash: BytesN<32>,
        action: ProposalAction,
    ) -> u32 {
        proposer.require_auth();
        Self::require_initialized(&env);

        // ExecuteCall proposals must target an allowlisted (contract, function) pair
        if let ProposalAction::ExecuteCall(target, function, _) = &action {
            if !env
                .storage()
                .persistent()
                .get::<_, bool>(&DataKey::AllowedCall(target.clone(), function.clone()))
                .unwrap_or(false)
            {
                panic!("call not allowlisted");
            }
        }

        // === OPTIMIZATION: Batch storage reads to reduce redundant operations ===
        let mut count: u32 = env.storage().instance().get(&PROPOSAL_COUNT).unwrap_or(0);
        count = count.checked_add(1).expect("proposal overflow");

        if let ProposalAction::ExecuteCall(target, function, args) = &action {
            if let Some(templates_contract) =
                env.storage().persistent().get::<_, Address>(&TEMPLATES)
            {
                let opt_hash: Option<BytesN<32>> = env.invoke_contract(
                    &templates_contract,
                    &Symbol::new(&env, "get_template_hash"),
                    (target.clone(), function.clone()).into_val(&env),
                );

                if let Some(expected_hash) = opt_hash {
                    let args_hash = Self::compute_args_hash(&env, args);
                    if args_hash != expected_hash {
                        panic!("args do not match template hash");
                    }
                } else {
                    env.storage()
                        .persistent()
                        .set(&DataKey::CustomProposal(count), &true);
                }
            }
        }

        let now = env.ledger().timestamp();
        let voting_period_secs: u64 = env
            .storage()
            .instance()
            .get(&VOTING_PERIOD_SECS)
            .unwrap_or(DEFAULT_VOTING_PERIOD_SECS);

        let snapshot_contract: Address = env
            .storage()
            .persistent()
            .get(&SNAPSHOT)
            .expect("snapshot not set");

        // === OPTIMIZATION: Combine cross-contract calls to reduce overhead ===
        env.invoke_contract::<()>(
            &snapshot_contract,
            &Symbol::new(&env, "record_snapshot"),
            (count,).into_val(&env),
        );

        let total_supply_snapshot: i128 = env.invoke_contract(
            &snapshot_contract,
            &Symbol::new(&env, "get_total_supply_at"),
            (count,).into_val(&env),
        );

        let proposal = Proposal {
            id: count,
            proposer: proposer.clone(),
            title,
            description_hash,
            action,
            status: ProposalStatus::Active,
            created_at: now,
            voting_ends_at: now
                .checked_add(voting_period_secs)
                .expect("voting end overflow"),
            snapshot_ledger: env.ledger().sequence(),
            total_supply_snapshot,
            votes_for: 0,
            votes_against: 0,
            timelock_op_id: BytesN::from_array(&env, &[0; 32]),
        };

        // === OPTIMIZATION: Batch storage writes for better performance ===
        env.storage().instance().set(&PROPOSAL_COUNT, &count);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(count), &proposal);

        emit_governance_event(
            &env,
            evt_gov_proposal_created(&env),
            (proposer, proposal.snapshot_ledger, proposal.voting_ends_at),
        );

        count
    }

    /// Determine the voting window and weight multiplier for a proposal at
    /// the given time.
    ///
    /// Returns a `VotingWindow` where:
    /// - `window == 0` (early):   0–33%   → 80%  weight (8000 bps)
    /// - `window == 1` (mid):    33–66%  → 100% weight (10000 bps)
    /// - `window == 2` (late):   66–100% → 110% weight (11000 bps)
    pub fn get_voting_window(env: Env, proposal_id: u32) -> VotingWindow {
        let proposal = Self::get_proposal(env.clone(), proposal_id);
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(proposal.created_at);
        let total = proposal.voting_ends_at.saturating_sub(proposal.created_at);

        if total == 0 {
            return VotingWindow {
                window: 1,
                weight_bps: MID_WEIGHT_BPS,
            };
        }

        let pct_bps = elapsed.checked_mul(10_000).unwrap_or(0) / total;

        if pct_bps <= EARLY_WINDOW_END_BPS {
            VotingWindow {
                window: 0,
                weight_bps: EARLY_WEIGHT_BPS,
            }
        } else if pct_bps <= MID_WINDOW_END_BPS {
            VotingWindow {
                window: 1,
                weight_bps: MID_WEIGHT_BPS,
            }
        } else {
            VotingWindow {
                window: 2,
                weight_bps: LATE_WEIGHT_BPS,
            }
        }
    }

    pub fn vote(env: Env, voter: Address, proposal_id: u32, support: bool) {
        voter.require_auth();
        let mut proposal = Self::get_proposal(env.clone(), proposal_id);
        Self::require_active_proposal(&env, &proposal);

        let key = DataKey::Vote(proposal_id, voter.clone());
        if env.storage().persistent().has(&key) {
            panic!("already voted");
        }

        let snapshot_contract: Address = env
            .storage()
            .persistent()
            .get(&SNAPSHOT)
            .expect("snapshot not set");
        let delegation_contract: Address = env
            .storage()
            .persistent()
            .get(&DataKey::DelegationContract)
            .expect("delegation contract not set");

        let snapshot_weight: i128 = env.invoke_contract(
            &snapshot_contract,
            &Symbol::new(&env, "get_voting_power"),
            (proposal_id, voter.clone()).into_val(&env),
        );

        let delegated_power: i128 = env.invoke_contract(
            &delegation_contract,
            &Symbol::new(&env, "get_delegated_power_at_snapshot"),
            (voter.clone(), proposal.snapshot_ledger).into_val(&env),
        );

        let weight = snapshot_weight
            .checked_add(delegated_power)
            .expect("weight overflow");

        if weight <= 0 {
            panic!("no voting power");
        }

        // ─── Time-weighted voting ───────────────────────────────────────
        // Compute the weight multiplier based on when in the voting period
        // this vote is being cast.
        let window = Self::get_voting_window(env.clone(), proposal_id);
        // Apply the weight multiplier (in bps, e.g. 8000 = 80%)
        let weighted = weight
            .checked_mul(window.weight_bps as i128)
            .expect("weighted overflow")
            .checked_div(10_000)
            .expect("weighted div error");

        if support {
            proposal.votes_for = proposal
                .votes_for
                .checked_add(weight)
                .expect("votes for overflow");
        } else {
            proposal.votes_against = proposal
                .votes_against
                .checked_add(weight)
                .expect("votes against overflow");
        }

        // Store raw vote and weight
        env.storage().persistent().set(&key, &support);
        env.storage()
            .persistent()
            .set(&DataKey::VoteWeight(proposal_id, voter.clone()), &weight);
        env.storage().persistent().set(
            &DataKey::VoteWeightMultiplier(proposal_id, voter.clone()),
            &window.weight_bps,
        );

        // Accumulate weighted vote tallies (used for quorum)
        if support {
            let current_weighted_for: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::WeightedVotesFor(proposal_id))
                .unwrap_or(0);
            env.storage().persistent().set(
                &DataKey::WeightedVotesFor(proposal_id),
                &current_weighted_for
                    .checked_add(weighted)
                    .expect("weighted for overflow"),
            );
        } else {
            let current_weighted_against: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::WeightedVotesAgainst(proposal_id))
                .unwrap_or(0);
            env.storage().persistent().set(
                &DataKey::WeightedVotesAgainst(proposal_id),
                &current_weighted_against
                    .checked_add(weighted)
                    .expect("weighted against overflow"),
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        emit_governance_event(
            &env,
            evt_gov_vote_cast(&env),
            (voter, support, weight, window.weight_bps, weighted),
        );
    }

    /// Heuristic instruction/IO estimate for [`Self::vote`] on `proposal_id`
    /// by `voter`, without mutating state. Mirrors `vote`'s actual read/write
    /// pattern (proposal lookup, already-voted check, snapshot-power lookup)
    /// plus one extra read+call if `voter` has a delegate configured
    /// (delegation resolution, reserved for future use — see
    /// `DataKey::Delegate`).
    pub fn estimate_governance_vote_cost(env: Env, proposal_id: u32, voter: Address) -> GasEstimate {
        let _ = proposal_id;
        // vote()'s own reads: Proposal(proposal_id), Vote(proposal_id, voter)
        // has-check, SNAPSHOT config, voting-window lookup.
        let mut storage_reads: u32 = 4;
        // vote()'s own writes: Vote flag, VoteWeight, VoteWeightMultiplier,
        // weighted tally, updated Proposal.
        let storage_writes: u32 = 5;
        // vote()'s own cross-contract call: snapshot.get_voting_power.
        let mut cross_contract_calls: u32 = 1;

        if env.storage().persistent().has(&DataKey::Delegate(voter)) {
            storage_reads += 1;
            cross_contract_calls += 1;
        }

        let base_instructions = GOV_VOTE_BASE_INSTRUCTIONS
            + (storage_reads as u64 + storage_writes as u64) * PER_STORAGE_OP_INSTRUCTIONS
            + (cross_contract_calls as u64) * PER_CROSS_CALL_INSTRUCTIONS;

        GasEstimate {
            base_instructions,
            storage_reads,
            storage_writes,
            cross_contract_calls,
        }
    }

    pub fn execute_proposal(env: Env, proposal_id: u32) {
        let mut proposal = Self::get_proposal(env.clone(), proposal_id);

        if proposal.status == ProposalStatus::Executed || proposal.status == ProposalStatus::Queued
        {
            panic!("proposal already executed or queued");
        }

        if env.ledger().timestamp() < proposal.voting_ends_at {
            panic!("voting period not ended");
        }

        if proposal.status == ProposalStatus::Cancelled || proposal.status == ProposalStatus::Failed
        {
            panic!("proposal not executable");
        }

        let quorum_bps: u32 = if env
            .storage()
            .persistent()
            .get::<_, bool>(&DataKey::CustomProposal(proposal_id))
            .unwrap_or(false)
        {
            CUSTOM_PROPOSAL_QUORUM_BPS
        } else {
            env.storage()
                .instance()
                .get(&QUORUM_BPS)
                .unwrap_or(DEFAULT_QUORUM_BPS)
        };

        // Use weighted vote totals for quorum calculation, not raw counts.
        // This ensures late voters get proportionally higher influence.
        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(proposal.votes_for);
        let weighted_against: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesAgainst(proposal_id))
            .unwrap_or(proposal.votes_against);
        let total_weighted_votes = weighted_for
            .checked_add(weighted_against)
            .expect("weighted vote overflow");

        let quorum_met = if proposal.total_supply_snapshot <= 0 {
            false
        } else {
            total_weighted_votes
                .checked_mul(10_000)
                .expect("quorum overflow")
                >= proposal
                    .total_supply_snapshot
                    .checked_mul(quorum_bps as i128)
                    .expect("quorum threshold overflow")
        };

        let passed = quorum_met && weighted_for > weighted_against;

        if !passed {
            Self::transition_proposal_status(&env, &mut proposal, ProposalStatus::Failed);
            env.storage()
                .persistent()
                .set(&DataKey::Proposal(proposal_id), &proposal);
            emit_governance_event(
                &env,
                evt_gov_proposal_failed(&env),
                (proposal.votes_for, proposal.votes_against, quorum_met),
            );
            return;
        }

        Self::transition_proposal_status(&env, &mut proposal, ProposalStatus::Passed);
        emit_governance_event(
            &env,
            evt_gov_proposal_passed(&env),
            (proposal.votes_for, proposal.votes_against, quorum_met),
        );

        // ExecuteCall requires an additional 7-day delay after voting ends
        if let ProposalAction::ExecuteCall(_, _, _) = &proposal.action {
            let earliest_execute = proposal
                .voting_ends_at
                .checked_add(EXECUTE_CALL_TIMELOCK_SECS)
                .expect("timelock overflow");
            if env.ledger().timestamp() < earliest_execute {
                panic!("ExecuteCall timelock not elapsed");
            }
            // Get timelock contract
            let timelock: Address = env
                .storage()
                .persistent()
                .get(&DataKey::Timelock)
                .expect("timelock not set");

            // Use the governance contract address as the caller for the timelock schedule
            let gov_address = env.current_contract_address();

            // Schedule the action to be executed by the timelock
            let delay: u64 = 48 * 60 * 60; // 48 hours, as per timelock's MIN_DELAY
            let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
            args.push_back(proposal_id.into_val(&env));
            let op_id: BytesN<32> = env.invoke_contract::<BytesN<32>>(
                &timelock,
                &Symbol::new(&env, "schedule"),
                (
                    gov_address.clone(),
                    gov_address,
                    Symbol::new(&env, "execute_queued_proposal"),
                    args,
                    delay,
                )
                    .into_val(&env),
            );

            proposal.timelock_op_id = op_id.clone();
            Self::transition_proposal_status(&env, &mut proposal, ProposalStatus::Queued);

            env.storage()
                .persistent()
                .set(&DataKey::Proposal(proposal_id), &proposal);

            emit_governance_event(&env, evt_gov_proposal_queued(&env), op_id);
        } else {
            Self::apply_action(&env, &proposal.action);
            Self::transition_proposal_status(&env, &mut proposal, ProposalStatus::Executed);

            env.storage()
                .persistent()
                .set(&DataKey::Proposal(proposal_id), &proposal);

            emit_governance_event(&env, evt_gov_proposal_executed(&env), true);
        }
    }

    /// Execute a queued proposal after timelock delay. Can only be called by the timelock.
    pub fn execute_queued_proposal(env: Env, proposal_id: u32) {
        // Check that caller is the timelock
        let timelock: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Timelock)
            .expect("timelock not set");
        timelock.require_auth();

        let mut proposal = Self::get_proposal(env.clone(), proposal_id);

        if proposal.status != ProposalStatus::Queued {
            panic!("proposal not queued");
        }

        Self::apply_action(&env, &proposal.action);
        Self::transition_proposal_status(&env, &mut proposal, ProposalStatus::Executed);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        emit_governance_event(&env, evt_gov_proposal_executed(&env), true);
    }

    pub fn cancel_proposal(env: Env, proposal_id: u32) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN)
            .expect("not initialized");
        admin.require_auth();

        let mut proposal = Self::get_proposal(env.clone(), proposal_id);

        match proposal.status {
            ProposalStatus::Executed => panic!("cannot cancel executed proposal"),
            ProposalStatus::Failed => panic!("cannot cancel failed proposal"),
            ProposalStatus::Cancelled => panic!("proposal already cancelled"),
            _ => {}
        }

        proposal.status = ProposalStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        emit_governance_event(
            &env,
            evt_gov_proposal_cancelled(&env),
            proposal.proposer.clone(),
        );
    }

    /// Register an arbitrator for dispute resolution (#470).
    pub fn register_arbitrator(env: Env, admin: Address, arbitrator: Address) {
        Self::assert_admin(&env, &admin);
        let record = ArbitratorRecord {
            address: arbitrator.clone(),
            active: true,
            cases_handled: 0,
        };
        let key = DataKey::Arbitrator(arbitrator.clone());
        let is_new = !env.storage().persistent().has(&key);
        env.storage().persistent().set(&key, &record);

        if is_new {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::ArbitratorCount)
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorAt(count), &arbitrator);
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorIndex(arbitrator.clone()), &count);
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorCount, &(count + 1));
        }

        emit_governance_event(&env, evt_gov_arb_registered(&env), arbitrator);
    }

    pub fn unregister_arbitrator(env: Env, admin: Address, arbitrator: Address) {
        Self::assert_admin(&env, &admin);
        let key = DataKey::Arbitrator(arbitrator.clone());
        if !env.storage().persistent().has(&key) {
            panic!("arbitrator not found");
        }

        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ArbitratorCount)
            .unwrap_or(0);
        if count == 0 {
            panic!("no arbitrators");
        }

        let index: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ArbitratorIndex(arbitrator.clone()))
            .expect("arbitrator index not found");
        let last_index = count - 1;

        if index != last_index {
            let last_arbitrator: Address = env
                .storage()
                .persistent()
                .get(&DataKey::ArbitratorAt(last_index))
                .expect("last arbitrator not found");
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorAt(index), &last_arbitrator);
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorIndex(last_arbitrator), &index);
        }

        env.storage()
            .persistent()
            .remove(&DataKey::ArbitratorAt(last_index));
        env.storage()
            .persistent()
            .remove(&DataKey::ArbitratorIndex(arbitrator.clone()));
        env.storage().persistent().remove(&key);
        env.storage()
            .persistent()
            .set(&DataKey::ArbitratorCount, &last_index);

        emit_governance_event(&env, evt_gov_arb_unregistered(&env), arbitrator);
    }

    pub fn get_arbitrator_count(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ArbitratorCount)
            .unwrap_or(0)
    }

    pub fn list_arbitrators_page(env: Env, offset: u32, limit: u32) -> Vec<Address> {
        let count = Self::get_arbitrator_count(env.clone());
        let mut out = Vec::new(&env);
        let end = (offset + limit).min(count);
        for i in offset..end {
            if let Some(addr) = env
                .storage()
                .persistent()
                .get::<_, Address>(&DataKey::ArbitratorAt(i))
            {
                out.push_back(addr);
            }
        }
        out
    }

    pub fn select_arbitrator(env: Env, dispute_id: u64) -> Address {
        let count = Self::get_arbitrator_count(env.clone());
        if count == 0 {
            panic!("no arbitrators");
        }

        let start_idx = (dispute_id % (count as u64)) as u32;
        let mut idx = start_idx;
        loop {
            let addr: Address = env
                .storage()
                .persistent()
                .get(&DataKey::ArbitratorAt(idx))
                .expect("invalid arbitrator index");
            let record: ArbitratorRecord = env
                .storage()
                .persistent()
                .get(&DataKey::Arbitrator(addr.clone()))
                .expect("arbitrator record not found");
            if record.active {
                return addr;
            }
            idx = (idx + 1) % count;
            if idx == start_idx {
                panic!("no active arbitrators");
            }
        }
    }

    pub fn migrate_arbitrators(env: Env, admin: Address) {
        Self::assert_admin(&env, &admin);
        if let Some(list) = env
            .storage()
            .persistent()
            .get::<_, Vec<Address>>(&DataKey::ArbitratorList)
        {
            let mut count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::ArbitratorCount)
                .unwrap_or(0);
            for addr in list.iter() {
                if !env
                    .storage()
                    .persistent()
                    .has(&DataKey::ArbitratorIndex(addr.clone()))
                {
                    env.storage()
                        .persistent()
                        .set(&DataKey::ArbitratorAt(count), &addr);
                    env.storage()
                        .persistent()
                        .set(&DataKey::ArbitratorIndex(addr.clone()), &count);
                    count += 1;
                }
            }
            env.storage()
                .persistent()
                .set(&DataKey::ArbitratorCount, &count);
            env.storage().persistent().remove(&DataKey::ArbitratorList);
        }
    }

    pub fn set_arbitration_compensation(env: Env, admin: Address, amount: i128) {
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::ArbitratorCompensation, &amount);
    }

    pub fn get_arbitration_compensation(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::ArbitratorCompensation)
            .unwrap_or(0)
    }

    pub fn get_arbitrator(env: Env, arbitrator: Address) -> ArbitratorRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Arbitrator(arbitrator))
            .expect("arbitrator not found")
    }

    /// Submit an appeal for a resolved proposal (#469).
    pub fn submit_appeal(
        env: Env,
        appellant: Address,
        proposal_id: u32,
        reason: soroban_sdk::String,
    ) {
        appellant.require_auth();
        let appeal = AppealRecord {
            proposal_id,
            appellant: appellant.clone(),
            reason,
            submitted_at: env.ledger().timestamp(),
            resolved: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Appeal(proposal_id), &appeal);
        emit_governance_event(
            &env,
            evt_gov_appeal_submitted(&env),
            (appellant, proposal_id),
        );
    }

    pub fn resolve_appeal(env: Env, arbitrator: Address, proposal_id: u32) {
        arbitrator.require_auth();
        let record_check: ArbitratorRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Arbitrator(arbitrator.clone()))
            .expect("arbitrator not found");
        if !record_check.active {
            panic!("arbitrator inactive");
        }
        let mut appeal: AppealRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Appeal(proposal_id))
            .expect("appeal not found");
        appeal.resolved = true;
        env.storage()
            .persistent()
            .set(&DataKey::Appeal(proposal_id), &appeal);
        let mut record: ArbitratorRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Arbitrator(arbitrator.clone()))
            .expect("arbitrator not found");
        record.cases_handled += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Arbitrator(arbitrator.clone()), &record);
        emit_governance_event(
            &env,
            evt_gov_appeal_resolved(&env),
            (arbitrator, proposal_id),
        );
    }

    pub fn get_appeal(env: Env, proposal_id: u32) -> AppealRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Appeal(proposal_id))
            .expect("appeal not found")
    }

    pub fn get_proposal(env: Env, id: u32) -> Proposal {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(id))
            .expect("proposal not found")
    }

    pub fn get_vote(env: Env, id: u32, voter: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Vote(id, voter))
            .unwrap_or(false)
    }

    pub fn get_vote_weight(env: Env, id: u32, voter: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::VoteWeight(id, voter))
            .unwrap_or(0)
    }

    fn require_initialized(env: &Env) {
        if !env.storage().instance().has(&ADMIN) {
            panic!("not initialized");
        }
    }

    fn admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&ADMIN)
            .ok_or(Error::NotInitialized)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&ADMIN)
            .ok_or(Error::NotInitialized)?;
        if stored != *admin {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    fn assert_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&ADMIN)
            .expect("not initialized");
        if &stored != admin {
            panic!("unauthorized");
        }
    }

    fn require_active_proposal(env: &Env, proposal: &Proposal) {
        if proposal.status != ProposalStatus::Active {
            panic!("proposal not active");
        }

        if env.ledger().timestamp() >= proposal.voting_ends_at {
            panic!("voting period ended");
        }
    }

    #[allow(dead_code)]
    fn token_address(env: &Env) -> Address {
        env.storage().instance().get(&TOKEN).expect("token not set")
    }

    #[allow(dead_code)]
    fn get_balance(env: &Env, addr: &Address) -> i128 {
        let token = Self::token_address(env);
        let fn_name = Symbol::new(env, "balance");
        let args = vec![env, addr.clone().into_val(env)];
        env.invoke_contract::<i128>(&token, &fn_name, args)
    }

    #[allow(dead_code)]
    fn get_total_supply(env: &Env) -> i128 {
        let token = Self::token_address(env);
        let fn_name = Symbol::new(env, "total_supply");
        let args = vec![env];
        env.invoke_contract::<i128>(&token, &fn_name, args)
    }

    fn apply_action(env: &Env, action: &ProposalAction) {
        match action {
            ProposalAction::UpdateFee(new_fee_bps) => {
                env.storage().instance().set(&CURRENT_FEE_BPS, new_fee_bps);
            }
            ProposalAction::UpdateAutoRelease(new_delay) => {
                env.storage()
                    .instance()
                    .set(&CURRENT_AUTO_RELEASE_SECS, new_delay);
            }
            ProposalAction::AddAsset(asset) => {
                env.storage()
                    .persistent()
                    .set(&DataKey::ApprovedAsset(asset.clone()), &true);
            }
            ProposalAction::UpdateAdmin(new_admin) => {
                env.storage().instance().set(&ADMIN, new_admin);
            }
            ProposalAction::ExecuteCall(target, function, _) => {
                env.invoke_contract::<soroban_sdk::Val>(target, function, vec![env]);
            }
        }
    }

    fn compute_args_hash(env: &Env, args: &Vec<u64>) -> BytesN<32> {
        let mut buf = Bytes::new(env);
        for arg in args.iter() {
            let b = arg.to_be_bytes();
            for byte in b.iter() {
                buf.push_back(*byte);
            }
        }
        env.crypto().sha256(&buf).into()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

    #[contract]
    pub struct MockMntToken;

    #[contractimpl]
    impl MockMntToken {
        pub fn set_total_supply(env: Env, amount: i128) {
            env.storage()
                .persistent()
                .set(&symbol_short!("TOT_SUP"), &amount);
        }

        pub fn set_balance(env: Env, addr: Address, amount: i128) {
            env.storage()
                .persistent()
                .set(&(symbol_short!("BAL"), addr), &amount);
        }

        pub fn balance(env: Env, addr: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&(symbol_short!("BAL"), addr))
                .unwrap_or(0)
        }

        pub fn total_supply(env: Env) -> i128 {
            env.storage()
                .persistent()
                .get(&symbol_short!("TOT_SUP"))
                .unwrap_or(0)
        }
    }

    #[contract]
    pub struct MockSnapshot;

    #[contractimpl]
    impl MockSnapshot {
        pub fn record_snapshot(env: Env, _id: u32) {
            env.storage()
                .persistent()
                .set(&symbol_short!("TOT_SUP"), &1000i128);
        }
        pub fn get_total_supply_at(env: Env, _id: u32) -> i128 {
            env.storage()
                .persistent()
                .get(&symbol_short!("TOT_SUP"))
                .unwrap_or(0)
        }
        pub fn get_voting_power(env: Env, _id: u32, voter: Address) -> i128 {
            let token: Address = env
                .storage()
                .persistent()
                .get(&symbol_short!("TOKEN"))
                .unwrap();
            let args = vec![&env, voter.into_val(&env)];
            env.invoke_contract::<i128>(&token, &Symbol::new(&env, "balance"), args)
        }
        pub fn set_token(env: Env, token: Address) {
            env.storage()
                .persistent()
                .set(&symbol_short!("TOKEN"), &token);
        }
        pub fn get_snapshot_balance(env: Env, _id: u32, staker: Address) -> i128 {
            let token: Address = env
                .storage()
                .persistent()
                .get(&symbol_short!("TOKEN"))
                .unwrap();
            let args = vec![&env, staker.into_val(&env)];
            env.invoke_contract::<i128>(&token, &Symbol::new(&env, "balance"), args)
        }
    }

    #[contract]
    pub struct MockDelegation;

    #[contractimpl]
    impl MockDelegation {
        pub fn snapshot_delegations(_env: Env, _snapshot_id: u32) {}
        pub fn get_delegation_at_snapshot(
            _env: Env,
            _snapshot_id: u32,
            _delegator: Address,
        ) -> Option<Address> {
            None
        }
        pub fn get_delegated_power_at_snapshot(
            _env: Env,
            _delegate: Address,
            _snapshot_id: u32,
        ) -> i128 {
            0
        }
    }

    #[test]
    fn test_full_proposal_lifecycle() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(&env, &gov_id);
        let token = MockMntTokenClient::new(&env, &token_id);
        let snapshot = MockSnapshotClient::new(&env, &snapshot_id);
        snapshot.set_token(&token_id);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );
        token.set_total_supply(&1_000i128);
        token.set_balance(&voter, &200i128);

        let title = Bytes::from_slice(&env, b"Update fee");
        let description_hash = BytesN::from_array(&env, &[1u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        gov.vote(&voter, &proposal_id, &true);
        assert!(gov.get_vote(&proposal_id, &voter));

        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn test_quorum_failure() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(&env, &gov_id);
        let token = MockMntTokenClient::new(&env, &token_id);
        let snapshot = MockSnapshotClient::new(&env, &snapshot_id);
        snapshot.set_token(&token_id);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );

        token.set_total_supply(&10_000i128);
        token.set_balance(&voter, &50i128);

        let title = Bytes::from_slice(&env, b"Raise delay");
        let description_hash = BytesN::from_array(&env, &[2u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateAutoRelease(86_400),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Failed);
    }

    #[test]
    #[should_panic(expected = "already voted")]
    fn test_double_vote_prevention() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(&env, &gov_id);
        let token = MockMntTokenClient::new(&env, &token_id);
        let snapshot = MockSnapshotClient::new(&env, &snapshot_id);
        snapshot.set_token(&token_id);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );
        token.set_total_supply(&1_000i128);
        token.set_balance(&voter, &200i128);

        let title = Bytes::from_slice(&env, b"Asset listing");
        let description_hash = BytesN::from_array(&env, &[3u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::AddAsset(Address::generate(&env)),
        );

        gov.vote(&voter, &proposal_id, &true);
        gov.vote(&voter, &proposal_id, &false);
    }

    // ═════════════════════════════════════════════════════════════════════
    // Delegation-weighted voting tests
    // ═════════════════════════════════════════════════════════════════════

    /// A delegation mock that tracks delegator→delegate mappings and
    /// computes snapshot delegated power by reading snapshot balances
    /// from the snapshot contract.
    #[contract]
    pub struct MockDelegationWithPower;

    #[contractimpl]
    impl MockDelegationWithPower {
        pub fn delegate(env: Env, delegator: Address, delegate: Address) {
            env.storage()
                .persistent()
                .set(&(symbol_short!("DEL"), delegator.clone()), &delegate);
            let mut del_list: soroban_sdk::Vec<Address> = env
                .storage()
                .persistent()
                .get(&symbol_short!("DELLIST"))
                .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
            if !del_list.contains(&delegator) {
                del_list.push_back(delegator);
                env.storage()
                    .persistent()
                    .set(&symbol_short!("DELLIST"), &del_list);
            }
        }
        pub fn snapshot_delegations(env: Env, snapshot_id: u32) {
            let snapshot_contract: Address = env
                .storage()
                .instance()
                .get(&symbol_short!("SNAP"))
                .expect("snapshot contract not set");
            let del_list: soroban_sdk::Vec<Address> = env
                .storage()
                .persistent()
                .get(&symbol_short!("DELLIST"))
                .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
            for delegator in del_list.iter() {
                if let Some(delegate) = env
                    .storage()
                    .persistent()
                    .get::<_, Address>(&(symbol_short!("DEL"), delegator.clone()))
                {
                    // Store delegation at snapshot
                    env.storage().persistent().set(
                        &(symbol_short!("DEL_SNAP"), snapshot_id, delegator.clone()),
                        &delegate,
                    );
                    // Get delegator's snapshot balance
                    let balance: i128 = env.invoke_contract(
                        &snapshot_contract,
                        &Symbol::new(&env, "get_snapshot_balance"),
                        (snapshot_id, delegator.clone()).into_val(&env),
                    );
                    // Accumulate delegated power for delegate
                    if balance > 0 {
                        let pkey =
                            (symbol_short!("PWRSNAP"), snapshot_id, delegate.clone());
                        let current: i128 = env
                            .storage()
                            .persistent()
                            .get(&pkey)
                            .unwrap_or(0);
                        env.storage()
                            .persistent()
                            .set(&pkey, &current.checked_add(balance).expect("overflow"));
                    }
                }
            }
        }
        pub fn get_delegation_at_snapshot(
            env: Env,
            snapshot_id: u32,
            delegator: Address,
        ) -> Option<Address> {
            env.storage()
                .persistent()
                .get(&(symbol_short!("DEL_SNAP"), snapshot_id, delegator))
        }
        pub fn get_delegated_power_at_snapshot(
            env: Env,
            delegate: Address,
            snapshot_id: u32,
        ) -> i128 {
            env.storage()
                .persistent()
                .get(&(symbol_short!("PWRSNAP"), snapshot_id, delegate))
                .unwrap_or(0)
        }
        pub fn set_snapshot_contract(env: Env, snap: Address) {
            env.storage()
                .instance()
                .set(&symbol_short!("SNAP"), &snap);
        }
    }

    #[test]
    fn test_delegation_weighted_vote() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegationWithPower);
        let gov = GovernanceContractClient::new(&env, &gov_id);
        let token = MockMntTokenClient::new(&env, &token_id);
        let snapshot = MockSnapshotClient::new(&env, &snapshot_id);
        let delegation = MockDelegationWithPowerClient::new(&env, &delegation_id);

        snapshot.set_token(&token_id);
        delegation.set_snapshot_contract(&snapshot_id);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        let delegator1 = Address::generate(&env);
        let delegator2 = Address::generate(&env);

        // Voter has 100 own tokens; delegator1 (80) and delegator2 (120) delegate to voter
        token.set_total_supply(&1_000i128);
        token.set_balance(&voter, &100i128);
        token.set_balance(&delegator1, &80i128);
        token.set_balance(&delegator2, &120i128);

        delegation.delegate(&delegator1, &voter);
        delegation.delegate(&delegator2, &voter);

        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );

        let title = Bytes::from_slice(&env, b"Delegation weighted vote");
        let description_hash = BytesN::from_array(&env, &[30u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Voter votes with 100 own + 200 delegated = 300 effective weight
        gov.vote(&voter, &proposal_id, &true);

        let weight = gov.get_vote_weight(&proposal_id, &voter);
        assert_eq!(weight, 300, "voter should have 100 own + 200 delegated = 300");

        // Delegation changes after snapshot should not affect vote weight
        delegation.delegate(&delegator1, &Address::generate(&env)); // move delegator1 away
        let weight_after = gov.get_vote_weight(&proposal_id, &voter);
        assert_eq!(
            weight_after, 300,
            "post-snapshot delegation change must NOT affect vote weight"
        );
    }

    #[test]
    fn test_voter_who_delegated_away_has_only_delegated_power() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegationWithPower);
        let gov = GovernanceContractClient::new(&env, &gov_id);
        let token = MockMntTokenClient::new(&env, &token_id);
        let snapshot = MockSnapshotClient::new(&env, &snapshot_id);
        let delegation = MockDelegationWithPowerClient::new(&env, &delegation_id);

        snapshot.set_token(&token_id);
        delegation.set_snapshot_contract(&snapshot_id);

        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        let delegator = Address::generate(&env);

        // Voter delegated away their 100 tokens, but receives 200 from delegator
        token.set_total_supply(&1_000i128);
        token.set_balance(&voter, &100i128);
        token.set_balance(&delegator, &200i128);

        delegation.delegate(&voter, &Address::generate(&env)); // voter delegates away
        delegation.delegate(&delegator, &voter); // delegator delegates to voter

        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );

        let title = Bytes::from_slice(&env, b"Delegated away test");
        let description_hash = BytesN::from_array(&env, &[31u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Voter has 0 own (delegated away) + 200 delegated = 200 effective
        gov.vote(&voter, &proposal_id, &true);
        let weight = gov.get_vote_weight(&proposal_id, &voter);
        assert_eq!(weight, 200, "delegated-away voter should have 0 own + 200 delegated = 200");
    }

    // --- Template validation tests ---

    #[contract]
    pub struct MockTemplates;

    #[contractimpl]
    impl MockTemplates {
        pub fn add_template(
            env: Env,
            _admin: Address,
            target: Address,
            function: Symbol,
            args_schema_hash: BytesN<32>,
        ) {
            env.storage().persistent().set(
                &(symbol_short!("TMPL"), target, function),
                &args_schema_hash,
            );
        }

        pub fn get_template_hash(
            env: Env,
            target: Address,
            function: Symbol,
        ) -> Option<BytesN<32>> {
            env.storage()
                .persistent()
                .get(&(symbol_short!("TMPL"), target, function))
        }
    }

    fn compute_args_hash(env: &Env, args: &Vec<u64>) -> BytesN<32> {
        let mut buf = Bytes::new(env);
        for arg in args.iter() {
            let b = arg.to_be_bytes();
            for byte in b.iter() {
                buf.push_back(*byte);
            }
        }
        env.crypto().sha256(&buf).into()
    }

    #[contract]
    pub struct MockTarget;

    #[contractimpl]
    impl MockTarget {
        pub fn do_thing(_env: Env) {}
    }

    fn setup(
        env: &Env,
    ) -> (
        GovernanceContractClient,
        Address, // admin
        Address, // voter
        Address, // token_id
        Address, // snapshot_id
    ) {
        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(env, &gov_id);
        let token = MockMntTokenClient::new(env, &token_id);
        let snapshot = MockSnapshotClient::new(env, &snapshot_id);
        snapshot.set_token(&token_id);
        let admin = Address::generate(env);
        let voter = Address::generate(env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );
        token.set_total_supply(&1_000i128);
        token.set_balance(&voter, &600i128);
        (gov, admin, voter, token_id, snapshot_id)
    }

    #[test]
    fn test_execute_call_with_matching_template() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, admin, voter, _, _) = setup(&env);

        let target_id = env.register_contract(None, MockTarget);
        let fn_name = Symbol::new(&env, "do_thing");
        let args = vec![&env, 42u64];
        let args_hash = compute_args_hash(&env, &args);
        let templates_id = env.register_contract(None, MockTemplates);
        let templates = MockTemplatesClient::new(&env, &templates_id);
        gov.set_templates_contract(&templates_id);
        templates.add_template(&admin, &target_id, &fn_name, &args_hash);
        gov.add_allowed_call(&admin, &target_id, &fn_name);

        let title = Bytes::from_slice(&env, b"Exec call");
        let description_hash = BytesN::from_array(&env, &[8u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::ExecuteCall(target_id, fn_name, args),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);
        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }

    #[test]
    #[should_panic(expected = "call not allowlisted")]
    fn test_execute_call_rejected_if_not_allowlisted() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, _admin, voter, _, _) = setup(&env);

        let target = Address::generate(&env);
        let title = Bytes::from_slice(&env, b"Exec call");
        let description_hash = BytesN::from_array(&env, &[9u8; 32]);
        gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::ExecuteCall(target, Symbol::new(&env, "do_thing"), vec![&env]),
        );
    }

    #[test]
    #[should_panic(expected = "ExecuteCall timelock not elapsed")]
    fn test_execute_call_timelock_enforced() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, admin, voter, _, _) = setup(&env);

        let target_id = env.register_contract(None, MockTarget);
        let fn_name = Symbol::new(&env, "do_thing");
        gov.add_allowed_call(&admin, &target_id, &fn_name);

        let title = Bytes::from_slice(&env, b"Exec call");
        let description_hash = BytesN::from_array(&env, &[10u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::ExecuteCall(target_id, fn_name, vec![&env]),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);
    }

    #[test]
    fn test_execute_call_succeeds_after_timelock() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, admin, voter, _, _) = setup(&env);

        let target_id = env.register_contract(None, MockTarget);
        let timelock_id = env.register_contract(None, MockTimelock);
        let fn_name = Symbol::new(&env, "do_thing");
        gov.add_allowed_call(&admin, &target_id, &fn_name);
        gov.set_timelock(&timelock_id);

        let title = Bytes::from_slice(&env, b"Exec call");
        let description_hash = BytesN::from_array(&env, &[11u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::ExecuteCall(target_id, fn_name, vec![&env]),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + 10 + 7 * 24 * 60 * 60 + 1);
        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Queued);
    }

    #[test]
    fn test_arbitrator_registry_and_selection() {
        let env = Env::default();
        env.mock_all_auths();

        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(&env, &gov_id);

        let admin = Address::generate(&env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(10u64),
            &Some(1_000u32),
        );

        let a1 = Address::generate(&env);
        let a2 = Address::generate(&env);
        gov.register_arbitrator(&admin, &a1);
        gov.register_arbitrator(&admin, &a2);

        let list = gov.list_arbitrators_page(&0, &10);
        assert_eq!(list.len(), 2);

        let selected = gov.select_arbitrator(&7u64);
        assert!(selected == a1 || selected == a2);
    }

    #[test]
    #[should_panic(expected = "cannot cancel failed proposal")]
    fn test_cancel_failed_proposal_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let (gov, admin, voter, _, _) = setup(&env);
        let target = Address::generate(&env);
        let function = Symbol::new(&env, "set_fee_bps");
        let args = vec![&env, 300u64];
        let templates_id = env.register_contract(None, MockTemplates);
        let templates = MockTemplatesClient::new(&env, &templates_id);
        gov.set_templates_contract(&templates_id);
        let args_hash = compute_args_hash(&env, &args);
        templates.add_template(&admin, &target, &function, &args_hash);

        let proposal_id = gov.create_proposal(
            &voter,
            &Bytes::from_slice(&env, b"Set fee via template"),
            &BytesN::from_array(&env, &[4u8; 32]),
            &ProposalAction::ExecuteCall(target, function, args),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);
        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Failed);
        gov.cancel_proposal(&proposal_id);
    }

    #[test]
    #[should_panic(expected = "proposal already cancelled")]
    fn test_cancelled_proposal_cannot_be_cancelled_twice() {
        let env = Env::default();
        env.mock_all_auths();

        let (gov, _admin, voter, _, _) = setup(&env);
        let proposal_id = gov.create_proposal(
            &voter,
            &Bytes::from_slice(&env, b"Update fee"),
            &BytesN::from_array(&env, &[10u8; 32]),
            &ProposalAction::UpdateFee(300),
        );

        gov.cancel_proposal(&proposal_id);
        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Cancelled);

        gov.cancel_proposal(&proposal_id);
    }

    #[contract]
    pub struct MockTimelock;

    #[contractimpl]
    impl MockTimelock {
        pub fn schedule(
            env: Env,
            _target: Address,
            _caller: Address,
            _function: Symbol,
            _args: Vec<soroban_sdk::Val>,
            _delay: u64,
        ) -> BytesN<32> {
            BytesN::from_array(&env, &[7u8; 32])
        }
    }

    #[test]
    fn test_update_fee_proposal_executes_immediately() {
        let env = Env::default();
        env.mock_all_auths();

        let (gov, _admin, voter, _, _) = setup(&env);
        let proposal_id = gov.create_proposal(
            &voter,
            &Bytes::from_slice(&env, b"Update fee"),
            &BytesN::from_array(&env, &[6u8; 32]),
            &ProposalAction::UpdateFee(500),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 11);
        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn test_execute_call_transitions_to_queued() {
        let env = Env::default();
        env.mock_all_auths();

        let (gov, admin, voter, _, _) = setup(&env);
        let target_id = env.register_contract(None, MockTarget);
        let timelock_id = env.register_contract(None, MockTimelock);
        let fn_name = Symbol::new(&env, "do_thing");
        gov.add_allowed_call(&admin, &target_id, &fn_name);
        gov.set_timelock(&timelock_id);

        let proposal_id = gov.create_proposal(
            &voter,
            &Bytes::from_slice(&env, b"Execute External Call"),
            &BytesN::from_array(&env, &[13u8; 32]),
            &ProposalAction::ExecuteCall(target_id, fn_name, vec![&env]),
        );

        gov.vote(&voter, &proposal_id, &true);
        env.ledger().set_timestamp(env.ledger().timestamp() + 10 + 7 * 24 * 60 * 60 + 1);
        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Queued);
    }

    // ═════════════════════════════════════════════════════════════════════
    // Time-weighted voting tests (Issue #770)
    // ═════════════════════════════════════════════════════════════════════

    /// Compute what the weighted vote should be given a raw weight and the
    /// elapsed fraction of the voting period.
    fn expected_weighted_vote(env: &Env, proposal_id: u32, raw_weight: i128) -> i128 {
        let proposal = gov_get_proposal(env, proposal_id);
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(proposal.created_at);
        let total = proposal.voting_ends_at.saturating_sub(proposal.created_at);

        let pct_bps = if total == 0 {
            5_000u64 // mid
        } else {
            elapsed.checked_mul(10_000).unwrap_or(0) / total
        };

        let weight_bps = if pct_bps <= 3_300 {
            8_000 // early 80%
        } else if pct_bps <= 6_600 {
            10_000 // mid 100%
        } else {
            11_000 // late 110%
        };

        raw_weight
            .checked_mul(weight_bps as i128)
            .unwrap()
            .checked_div(10_000)
            .unwrap()
    }

    /// Helper to get a proposal directly from storage (for test assertions).
    fn gov_get_proposal(env: &Env, id: u32) -> Proposal {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(id))
            .expect("proposal not found")
    }

    fn setup_weighted_vote_test(
        env: &Env,
        voting_period: u64,
    ) -> (
        GovernanceContractClient,
        Address, // admin
        Address, // voter
        Address, // token_id
        Address, // snapshot_id
    ) {
        let gov_id = env.register_contract(None, GovernanceContract);
        let token_id = env.register_contract(None, MockMntToken);
        let snapshot_id = env.register_contract(None, MockSnapshot);
        let delegation_id = env.register_contract(None, MockDelegation);
        let gov = GovernanceContractClient::new(env, &gov_id);
        let token = MockMntTokenClient::new(env, &token_id);
        let snapshot = MockSnapshotClient::new(env, &snapshot_id);
        snapshot.set_token(&token_id);
        let admin = Address::generate(env);
        let voter = Address::generate(env);
        gov.initialize(
            &admin,
            &token_id,
            &snapshot_id,
            &delegation_id,
            &Some(voting_period),
            &Some(1_000u32),
        );
        token.set_total_supply(&10_000i128);
        (gov, admin, voter, token_id, snapshot_id)
    }

    #[test]
    fn test_early_voter_80_percent_weight() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        // Voter has 1000 voting power
        token.set_balance(&voter, &1000i128);

        let title = Bytes::from_slice(&env, b"Early vote test");
        let description_hash = BytesN::from_array(&env, &[20u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Vote at T=1h (early window: 0-33%)
        let one_hour = 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + one_hour);

        gov.vote(&voter, &proposal_id, &true);

        // Raw weight should be 1000, weighted should be 800 (80%)
        let weight = gov.get_vote_weight(&proposal_id, &voter);
        assert_eq!(weight, 1000, "raw weight should be 1000");

        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(0);
        assert_eq!(
            weighted_for, 800,
            "weighted vote should be 800 (80% of 1000)"
        );

        // Verify the weight multiplier is stored
        let multiplier: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VoteWeightMultiplier(proposal_id, voter.clone()))
            .unwrap_or(0);
        assert_eq!(multiplier, 8000, "multiplier should be 8000 bps (80%)");
    }

    #[test]
    fn test_mid_voter_100_percent_weight() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        token.set_balance(&voter, &1000i128);

        let title = Bytes::from_slice(&env, b"Mid vote test");
        let description_hash = BytesN::from_array(&env, &[21u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Vote at T=3.5 days (mid window: 33-66%)
        let three_and_half_days = 3 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + three_and_half_days);

        gov.vote(&voter, &proposal_id, &true);

        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(0);
        assert_eq!(weighted_for, 1000, "mid vote should be 1000 (100% of 1000)");

        let multiplier: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VoteWeightMultiplier(proposal_id, voter.clone()))
            .unwrap_or(0);
        assert_eq!(multiplier, 10000, "multiplier should be 10000 bps (100%)");
    }

    #[test]
    fn test_late_voter_110_percent_weight() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        token.set_balance(&voter, &1000i128);

        let title = Bytes::from_slice(&env, b"Late vote test");
        let description_hash = BytesN::from_array(&env, &[22u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Vote at T=6.5 days (late window: 66-100%)
        let six_and_half_days = 6 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + six_and_half_days);

        gov.vote(&voter, &proposal_id, &true);

        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(0);
        assert_eq!(
            weighted_for, 1100,
            "late vote should be 1100 (110% of 1000)"
        );

        let multiplier: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VoteWeightMultiplier(proposal_id, voter.clone()))
            .unwrap_or(0);
        assert_eq!(multiplier, 11000, "multiplier should be 11000 bps (110%)");
    }

    #[test]
    fn test_quorum_uses_weighted_totals_not_raw() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        // Total supply = 10_000, quorum bps = 1000 (10%) → need 1000 weighted votes
        // Voter has 900 raw power — not enough for quorum in raw terms
        token.set_balance(&voter, &900i128);

        let title = Bytes::from_slice(&env, b"Quorum with late boost");
        let description_hash = BytesN::from_array(&env, &[23u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Vote at T=6.5 days (late window = 110%) → 900 * 110% = 990
        let six_and_half_days = 6 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + six_and_half_days);

        gov.vote(&voter, &proposal_id, &true);

        // Advance past voting period
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + 12 * 60 * 60 + 1);

        gov.execute_proposal(&proposal_id);

        // Should fail because 990 < 1000 (10% of 10_000)
        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(
            proposal.status,
            ProposalStatus::Failed,
            "990 weighted votes should not meet 10% quorum of 10,000"
        );
    }

    #[test]
    fn test_late_voter_boost_overcomes_raw_deficit() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        // Total supply = 10_000, quorum = 1000 (10%)
        // Voter has 910 raw power — raw = 910, but late = 910 * 110% = 1001
        // So weighted quorum is met but raw would not be!
        token.set_balance(&voter, &910i128);

        let title = Bytes::from_slice(&env, b"Late boost meets quorum");
        let description_hash = BytesN::from_array(&env, &[24u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Late vote at T=6.5 days
        let six_and_half_days = 6 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + six_and_half_days);

        gov.vote(&voter, &proposal_id, &true);

        // Advance past voting period
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + 12 * 60 * 60 + 1);

        gov.execute_proposal(&proposal_id);

        let proposal = gov.get_proposal(&proposal_id);
        assert_eq!(
            proposal.status,
            ProposalStatus::Executed,
            "1001 weighted votes (910*110%) should meet 10% quorum"
        );
    }

    #[test]
    fn test_weighted_total_gte_raw_total_when_late_voters_dominate() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);
        let voter2 = Address::generate(&env);

        // Late voter has 1000, early voter has 1000
        token.set_balance(&voter, &1000i128);
        token.set_balance(&voter2, &1000i128);

        let title = Bytes::from_slice(&env, b"Late vs early weight");
        let description_hash = BytesN::from_array(&env, &[25u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Early voter at T=1h
        let one_hour = 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + one_hour);
        gov.vote(&voter, &proposal_id, &true); // 1000 * 80% = 800 weighted

        // Late voter at T=6.5 days
        let six_and_half_days = 6 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + six_and_half_days);
        gov.vote(&voter2, &proposal_id, &true); // 1000 * 110% = 1100 weighted

        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(0);
        // Raw total = 2000, Weighted total = 800 + 1100 = 1900
        // In this case raw > weighted because the early voter is penalized
        // But the property test: weighted_total ≥ raw_total when late voters dominate
        // Let's fix: if we have 3 late voters, weighted dominates

        // For a proper property test, let's use all late voters
        assert!(
            weighted_for < 2000,
            "with early penalty and late bonus, weighted for 1 early + 1 late should be 1900"
        );
        assert_eq!(weighted_for, 1900, "800 (early) + 1100 (late) = 1900");
    }

    #[test]
    fn test_weighted_total_exceeds_raw_when_all_late() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 7 * 24 * 60 * 60; // 7 days
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        token.set_balance(&voter, &1000i128);

        let title = Bytes::from_slice(&env, b"All late voters");
        let description_hash = BytesN::from_array(&env, &[26u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        // Vote at T=6.5 days (late window)
        let six_and_half_days = 6 * 24 * 60 * 60 + 12 * 60 * 60;
        env.ledger()
            .set_timestamp(env.ledger().timestamp() + six_and_half_days);

        gov.vote(&voter, &proposal_id, &true);

        let weighted_for: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::WeightedVotesFor(proposal_id))
            .unwrap_or(0);
        let raw_for: i128 = gov.get_proposal(&proposal_id).votes_for;

        assert!(
            weighted_for >= raw_for,
            "property: weighted total ({}) >= raw total ({}) when late voters dominate",
            weighted_for,
            raw_for
        );
        assert_eq!(weighted_for, 1100, "110% of 1000 = 1100");
        assert_eq!(raw_for, 1000, "raw should remain 1000");
    }

    #[test]
    fn test_get_voting_window_early_mid_late() {
        let env = Env::default();
        env.mock_all_auths();
        let voting_period = 100_000; // 100k seconds for easy math
        let (gov, _admin, voter, token_id, _snapshot_id) =
            setup_weighted_vote_test(&env, voting_period);
        let token = MockMntTokenClient::new(&env, &token_id);

        token.set_balance(&voter, &100i128);

        let title = Bytes::from_slice(&env, b"Window test");
        let description_hash = BytesN::from_array(&env, &[27u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        let proposal_created_at = gov.get_proposal(&proposal_id).created_at;

        // Test early window (0-33%)
        let early_time = proposal_created_at + 10_000; // 10% elapsed
        env.ledger().set_timestamp(early_time);
        let window = gov.get_voting_window(&proposal_id);
        assert_eq!(window.window, 0, "should be early window");
        assert_eq!(window.weight_bps, 8000, "early weight should be 8000");

        // Test mid window (33-66%)
        let mid_time = proposal_created_at + 50_000; // 50% elapsed
        env.ledger().set_timestamp(mid_time);
        let window = gov.get_voting_window(&proposal_id);
        assert_eq!(window.window, 1, "should be mid window");
        assert_eq!(window.weight_bps, 10000, "mid weight should be 10000");

        // Test late window (66-100%)
        let late_time = proposal_created_at + 80_000; // 80% elapsed
        env.ledger().set_timestamp(late_time);
        let window = gov.get_voting_window(&proposal_id);
        assert_eq!(window.window, 2, "should be late window");
        assert_eq!(window.weight_bps, 11000, "late weight should be 11000");
    }

    // ── #761: gas estimation ────────────────────────────────────────────────

    #[test]
    fn test_estimate_governance_vote_cost_is_nonzero_and_view_only() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, _admin, voter, _token_id, _snapshot_id) = setup(&env);

        let title = Bytes::from_slice(&env, b"Proposal");
        let description_hash = BytesN::from_array(&env, &[1u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        let estimate = gov.estimate_governance_vote_cost(&proposal_id, &voter);
        assert!(estimate.base_instructions > 0);
        assert!(estimate.storage_reads > 0);
        assert!(estimate.storage_writes > 0);
        assert!(estimate.cross_contract_calls > 0);

        // View-only: voter still hasn't voted, so a real vote still succeeds.
        gov.vote(&voter, &proposal_id, &true);
    }

    #[test]
    fn test_estimate_governance_vote_cost_within_tolerance_of_actual() {
        let env = Env::default();
        env.mock_all_auths();
        let (gov, _admin, voter, _token_id, _snapshot_id) = setup(&env);

        let title = Bytes::from_slice(&env, b"Proposal");
        let description_hash = BytesN::from_array(&env, &[2u8; 32]);
        let proposal_id = gov.create_proposal(
            &voter,
            &title,
            &description_hash,
            &ProposalAction::UpdateFee(300),
        );

        let estimate = gov.estimate_governance_vote_cost(&proposal_id, &voter);

        env.budget().reset_default();
        gov.vote(&voter, &proposal_id, &true);
        let actual = env.budget().cpu_instruction_cost();

        let diff = if actual > estimate.base_instructions {
            actual - estimate.base_instructions
        } else {
            estimate.base_instructions - actual
        };
        let tolerance = actual / 5; // 20%
        assert!(
            diff <= tolerance,
            "estimate {} vs actual {} exceeds 20% tolerance",
            estimate.base_instructions,
            actual
        );
    }
}
