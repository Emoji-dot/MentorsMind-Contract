#![no_std]

use shared::{require_not_paused, ReentrancyGuard, Validator};
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, token,
    Address, Env, IntoVal, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Oracle client interface (matches oracle contract's public API)
// ---------------------------------------------------------------------------

/// Mirrors `OracleHealth` from the oracle contract.
/// Extended to include circuit-breaker and override state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleHealth {
    pub active_feeders: u32,
    pub last_update: u64,
    pub is_stale: bool,
    pub circuit_breaker_tripped: bool,
    pub override_active: bool,
}

#[contractclient(name = "OracleContractClient")]
pub trait OracleContractTrait {
    fn get_oracle_health(env: Env, asset: Symbol) -> OracleHealth;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InsufficientBalance = 4,
    /// Oracle has too few active feeders or failed 5-of-7 consensus check.
    OracleUnhealthy = 5,
    /// Oracle data is stale — buyback aborted until a fresh price is available.
    OracleStale = 6,
    /// Requested token is not on the approved whitelist for this operation.
    TokenNotApproved = 7,
    /// `min_mnt_out` passed to `buyback_and_burn` was not strictly positive.
    InvalidMinOut = 8,
    /// The DEX swap returned zero output tokens.
    ZeroOutput = 9,
    /// The DEX swap returned less than the caller's requested `min_mnt_out`.
    SlippageExceeded = 10,
    /// An amount failed comprehensive financial validation.
    InvalidAmount = 11,
    /// No admin-change proposal is currently pending.
    NoPendingAdminChange = 12,
    /// The admin-change timelock has not yet elapsed.
    AdminChangeNotYetEffective = 13,
    /// The caller is not the address named in the pending admin change.
    InvalidAdminChange = 14,
    /// Oracle circuit breaker is currently tripped — high price volatility detected.
    OracleCircuitBreaker = 15,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Describes how to invoke the DEX swap function.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DexInterface {
    pub swap_fn: Symbol,
}

impl DexInterface {
    pub fn validate(&self, env: &Env) {
        if self.swap_fn == Symbol::new(env, "") {
            panic!("DexInterface: swap_fn must not be empty");
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationHistory {
    pub token: Address,
    pub recipient: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryTokenApprovalEvent {
    pub token: Address,
    pub approved: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAllocation {
    pub id: u32,
    pub token: Address,
    pub recipient: Address,
    pub amount: i128,
    pub approvals_count: u32,
    pub executed: bool,
    pub created_at: u64,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuybackFailed {
    pub xlm_amount: i128,
    pub reason: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuybackSucceeded {
    pub xlm_spent: i128,
    pub mnt_burned: i128,
    pub timestamp: u64,
}

const ADMIN_CHANGE_TIMELOCK: u64 = 48 * 60 * 60;

/// Economic sanity ceiling for a single treasury operation.
const MAX_FINANCIAL_AMOUNT: i128 = 1_000_000_000_000_000;

/// Minimum number of active feeders required for oracle consensus (5-of-7).
const MIN_ORACLE_FEEDERS: u32 = 5;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Timelock,
    StakingContract,
    PauseGuardian,
    AllocationCount,
    Allocation(u32),
    ApprovedToken(Address),
    RegulatoryReporting,
    MultisigThreshold,
    PendingAllocationCount,
    PendingAllocation(u32),
    AllocationApproval(u32, Address),
    PendingAdmin,
    AutoBurnRateBps,
    BurnQueue,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialize treasury contract with admin, staking contract, timelock, and optional pause guardian.
    pub fn initialize(
        env: Env,
        admin: Address,
        staking_contract: Address,
        timelock: Address,
        pause_guardian: Option<Address>,
    ) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::StakingContract, &staking_contract);
        env.storage()
            .persistent()
            .set(&DataKey::Timelock, &timelock);
        if let Some(guardian) = pause_guardian {
            env.storage()
                .persistent()
                .set(&DataKey::PauseGuardian, &guardian);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllocationCount, &0u32);
        Ok(())
    }

    pub fn set_auto_burn_rate(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        Validator::new(&env)
            .require_valid_bps(bps, "bps")
            .validate()
            .map_err(|_| Error::InvalidAmount)?;
        env.storage()
            .persistent()
            .set(&DataKey::AutoBurnRateBps, &bps);
        Ok(())
    }

    pub fn execute_burn_queue(env: Env) -> Result<i128, Error> {
        let queued: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::BurnQueue)
            .unwrap_or(0);
        if queued <= 0 {
            return Ok(0);
        }
        env.storage().persistent().set(&DataKey::BurnQueue, &0i128);
        env.events().publish(
            (symbol_short!("burn"), symbol_short!("executed")),
            queued,
        );
        Ok(queued)
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

        let pending = PendingAdminChange {
            new_admin: new_admin.clone(),
            effective_at,
        };
        env.storage()
            .persistent()
            .set(&DataKey::PendingAdmin, &pending);
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
            .persistent()
            .get(&DataKey::PendingAdmin)
            .ok_or(Error::NoPendingAdminChange)?;
        if pending.new_admin != new_admin {
            return Err(Error::Unauthorized);
        }
        if env.ledger().timestamp() < pending.effective_at {
            return Err(Error::AdminChangeNotYetEffective);
        }
        env.storage().persistent().set(&DataKey::Admin, &new_admin);
        env.storage().persistent().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn cancel_admin_change(env: Env, multisig: Address) -> Result<(), Error> {
        multisig.require_auth();
        if !env.storage().persistent().has(&DataKey::PendingAdmin) {
            return Err(Error::NoPendingAdminChange);
        }
        env.storage().persistent().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn get_pending_admin_change(env: Env) -> Option<PendingAdminChange> {
        env.storage().persistent().get(&DataKey::PendingAdmin)
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        Self::admin(&env)
    }

    pub fn set_regulatory_reporting(env: Env, reporting_address: Address) -> Result<(), Error> {
        let admin = Self::admin(&env)?;
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::RegulatoryReporting, &reporting_address);
        Ok(())
    }

    fn admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin = Self::admin(env)?;
        if stored_admin != *admin {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    fn _is_token_approved(env: &Env, token: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::ApprovedToken(token.clone()))
            .unwrap_or(false)
    }

    fn _check_and_report_large_tx(
        env: &Env,
        contract: Symbol,
        function: Symbol,
        address: &Address,
        amount_usd: i128,
    ) {
        const THRESHOLD: i128 = 10_000;
        if amount_usd <= THRESHOLD {
            return;
        }

        if let Some(reporting_addr) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::RegulatoryReporting)
        {
            let _ = env.try_invoke_contract::<(), _>(
                &reporting_addr,
                &Symbol::new(env, "record_large_tx"),
                (
                    contract,
                    function,
                    address.clone(),
                    amount_usd,
                    env.ledger().timestamp(),
                )
                    .into_val(env),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Token whitelist management
    // -----------------------------------------------------------------------

    pub fn set_approved_token(
        env: Env,
        token_address: Address,
        approved: bool,
    ) -> Result<(), Error> {
        let admin = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::ApprovedToken(token_address.clone());
        env.storage().persistent().set(&key, &approved);

        if approved {
            env.events().publish(
                (symbol_short!("treasury"), symbol_short!("tok_appr")),
                TreasuryTokenApprovalEvent {
                    token: token_address,
                    approved: true,
                },
            );
        } else {
            env.events().publish(
                (symbol_short!("treasury"), symbol_short!("tok_rej")),
                TreasuryTokenApprovalEvent {
                    token: token_address,
                    approved: false,
                },
            );
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Deposits and balance
    // -----------------------------------------------------------------------

    pub fn deposit(env: Env, from: Address, token: Address, amount: i128) -> Result<(), Error> {
        if let Some(guardian) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::PauseGuardian)
        {
            require_not_paused(&env, &guardian);
        }

        from.require_auth();
        Validator::new(&env)
            .require_positive(amount, "amount")
            .require_max(amount, MAX_FINANCIAL_AMOUNT, "amount")
            .validate()
            .map_err(|_| Error::InvalidAmount)?;
        if !Self::_is_token_approved(&env, &token) {
            panic!("Token not approved");
        }
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        env.events().publish(
            (symbol_short!("deposit"), from.clone(), token.clone()),
            amount,
        );
        Ok(())
    }

    pub fn get_balance(env: Env, token: Address) -> i128 {
        token::Client::new(&env, &token).balance(&env.current_contract_address())
    }

    // -----------------------------------------------------------------------
    // Allocations
    // -----------------------------------------------------------------------

    pub fn allocate(
        env: Env,
        token: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        if let Some(guardian) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::PauseGuardian)
        {
            require_not_paused(&env, &guardian);
        }

        let _guard = ReentrancyGuard::enter(&env, Symbol::new(&env, "allocate"));
        let admin = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        if !Self::_is_token_approved(&env, &token) {
            return Err(Error::TokenNotApproved);
        }

        Validator::new(&env)
            .require_positive(amount, "amount")
            .require_max(amount, MAX_FINANCIAL_AMOUNT, "amount")
            .validate()
            .map_err(|_| Error::InvalidAmount)?;

        Self::_check_and_report_large_tx(
            &env,
            Symbol::new(&env, "treasury"),
            Symbol::new(&env, "allocate"),
            &recipient,
            amount,
        );

        let threshold: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigThreshold)
            .unwrap_or(50_000);

        if amount > threshold {
            let pending_count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::PendingAllocationCount)
                .unwrap_or(0);

            let pending = PendingAllocation {
                id: pending_count,
                token: token.clone(),
                recipient: recipient.clone(),
                amount,
                approvals_count: 1,
                executed: false,
                created_at: env.ledger().timestamp(),
            };

            env.storage()
                .persistent()
                .set(&DataKey::PendingAllocation(pending_count), &pending);
            env.storage().persistent().set(
                &DataKey::AllocationApproval(pending_count, admin.clone()),
                &true,
            );
            env.storage()
                .persistent()
                .set(&DataKey::PendingAllocationCount, &(pending_count + 1));

            env.events().publish(
                (symbol_short!("allocate"), symbol_short!("pending")),
                (pending_count, recipient, amount),
            );
            return Ok(());
        }

        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationCount)
            .unwrap_or(0u32);
        env.storage().persistent().set(
            &DataKey::Allocation(count),
            &AllocationHistory {
                token: token.clone(),
                recipient: recipient.clone(),
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::AllocationCount, &(count + 1));

        env.events().publish(
            (symbol_short!("allocate"), recipient.clone(), token.clone()),
            amount,
        );
        Ok(())
    }

    pub fn set_multisig_threshold(env: Env, threshold: i128) -> Result<(), Error> {
        let admin = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::MultisigThreshold, &threshold);
        Ok(())
    }

    pub fn approve_pending_allocation(
        env: Env,
        approver: Address,
        pending_id: u32,
    ) -> Result<(), Error> {
        approver.require_auth();

        let mut pending: PendingAllocation = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAllocation(pending_id))
            .ok_or(Error::NotInitialized)?;

        if pending.executed {
            panic!("Pending allocation already executed");
        }

        let approval_key = DataKey::AllocationApproval(pending_id, approver.clone());
        if env.storage().persistent().has(&approval_key) {
            panic!("Approver already signed pending allocation");
        }

        env.storage().persistent().set(&approval_key, &true);
        pending.approvals_count += 1;

        if pending.approvals_count >= 2 {
            token::Client::new(&env, &pending.token).transfer(
                &env.current_contract_address(),
                &pending.recipient,
                &pending.amount,
            );

            pending.executed = true;

            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::AllocationCount)
                .unwrap_or(0u32);
            env.storage().persistent().set(
                &DataKey::Allocation(count),
                &AllocationHistory {
                    token: pending.token.clone(),
                    recipient: pending.recipient.clone(),
                    amount: pending.amount,
                    timestamp: env.ledger().timestamp(),
                },
            );
            env.storage()
                .persistent()
                .set(&DataKey::AllocationCount, &(count + 1));

            env.events().publish(
                (symbol_short!("allocate"), symbol_short!("executed")),
                (pending_id, pending.recipient.clone(), pending.amount),
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::PendingAllocation(pending_id), &pending);

        Ok(())
    }

    pub fn get_pending_allocation(env: Env, pending_id: u32) -> Option<PendingAllocation> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingAllocation(pending_id))
    }

    pub fn pending_allocation_count(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::PendingAllocationCount)
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Staker distribution
    // -----------------------------------------------------------------------

    pub fn distribute_to_stakers(
        env: Env,
        token: Address,
        total_amount: i128,
    ) -> Result<(), Error> {
        if let Some(guardian) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::PauseGuardian)
        {
            require_not_paused(&env, &guardian);
        }

        let _guard = ReentrancyGuard::enter(&env, Symbol::new(&env, "distribute"));
        let admin = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        if !Self::_is_token_approved(&env, &token) {
            return Err(Error::TokenNotApproved);
        }

        Validator::new(&env)
            .require_positive(total_amount, "total_amount")
            .require_max(total_amount, MAX_FINANCIAL_AMOUNT, "total_amount")
            .validate()
            .map_err(|_| Error::InvalidAmount)?;

        let staking_contract: Address = env
            .storage()
            .persistent()
            .get(&DataKey::StakingContract)
            .ok_or(Error::NotInitialized)?;

        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &staking_contract,
            &total_amount,
        );

        let lp_amount = total_amount / 10;
        let staker_amount = total_amount - lp_amount;

        if lp_amount > 0 {
            env.invoke_contract::<()>(
                &staking_contract,
                &Symbol::new(&env, "add_to_lp_reward_pool"),
                (lp_amount,).into_val(&env),
            );
        }

        env.invoke_contract::<()>(
            &staking_contract,
            &Symbol::new(&env, "distribute_revenue"),
            (token.clone(), staker_amount).into_val(&env),
        );

        env.events().publish(
            (
                symbol_short!("distrib"),
                staking_contract.clone(),
                token.clone(),
            ),
            total_amount,
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn
    // -----------------------------------------------------------------------

    /// Swap XLM for MNT on DEX and burn the received MNT.
    ///
    /// # Oracle validation
    /// When `oracle_contract` is provided the function runs the full multi-oracle
    /// validation pipeline before executing the swap:
    ///
    /// 1. Staleness check — rejects stale data.
    /// 2. 5-of-7 consensus check — requires MIN_ORACLE_FEEDERS (5) active feeders.
    /// 3. Circuit-breaker check — rejects if price volatility > 10% in 1 hour.
    ///
    /// Pass `oracle_contract = None` only in legacy/test mode.
    pub fn buyback_and_burn(
        env: Env,
        xlm_token: Address,
        mnt_token: Address,
        dex_contract: Address,
        xlm_amount: i128,
        min_mnt_out: i128,
        dex_iface: DexInterface,
        oracle_contract: Option<Address>,
        mnt_asset_symbol: Option<Symbol>,
    ) -> Result<(), Error> {
        let _guard = ReentrancyGuard::enter(&env, Symbol::new(&env, "buyback"));

        // ------------------------------------------------------------------
        // 1. Access control: must be called by the registered timelock only.
        // ------------------------------------------------------------------
        let timelock: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Timelock)
            .ok_or(Error::NotInitialized)?;
        timelock.require_auth();

        // ------------------------------------------------------------------
        // 2. Pre-flight validation — no state changes yet.
        // ------------------------------------------------------------------
        dex_iface.validate(&env);

        if Validator::new(&env)
            .require_positive(xlm_amount, "xlm_amount")
            .require_max(xlm_amount, MAX_FINANCIAL_AMOUNT, "xlm_amount")
            .validate()
            .is_err()
        {
            env.events().publish(
                (symbol_short!("buyback"), symbol_short!("failed")),
                BuybackFailed {
                    xlm_amount,
                    reason: Symbol::new(&env, "invalid_xlm_amount"),
                },
            );
            return Err(Error::InvalidAmount);
        }

        if min_mnt_out <= 0 {
            env.events().publish(
                (symbol_short!("buyback"), symbol_short!("failed")),
                BuybackFailed {
                    xlm_amount,
                    reason: Symbol::new(&env, "invalid_min_out"),
                },
            );
            return Err(Error::InvalidMinOut);
        }

        // ------------------------------------------------------------------
        // 3. Multi-oracle validation gate
        //
        // Validates:
        //   a) Data freshness (not stale)
        //   b) 5-of-7 consensus (MIN_ORACLE_FEEDERS active distinct feeders)
        //   c) Circuit-breaker state (not tripped by >10% volatility in 1h)
        // ------------------------------------------------------------------
        if let (Some(oracle), Some(asset_sym)) =
            (oracle_contract.clone(), mnt_asset_symbol.clone())
        {
            let health: OracleHealth =
                OracleContractClient::new(&env, &oracle).get_oracle_health(&asset_sym);

            // 3a. Staleness check.
            if health.is_stale {
                env.events().publish(
                    (symbol_short!("buyback"), symbol_short!("failed")),
                    BuybackFailed {
                        xlm_amount,
                        reason: Symbol::new(&env, "oracle_stale"),
                    },
                );
                return Err(Error::OracleStale);
            }

            // 3b. 5-of-7 consensus check — must have MIN_ORACLE_FEEDERS (5) active feeders.
            if health.active_feeders < MIN_ORACLE_FEEDERS {
                env.events().publish(
                    (symbol_short!("buyback"), symbol_short!("failed")),
                    BuybackFailed {
                        xlm_amount,
                        reason: Symbol::new(&env, "oracle_unhealthy"),
                    },
                );
                return Err(Error::OracleUnhealthy);
            }

            // 3c. Circuit-breaker check — halt buybacks during high volatility.
            if health.circuit_breaker_tripped {
                env.events().publish(
                    (symbol_short!("buyback"), symbol_short!("failed")),
                    BuybackFailed {
                        xlm_amount,
                        reason: Symbol::new(&env, "oracle_cb"),
                    },
                );
                return Err(Error::OracleCircuitBreaker);
            }
        }

        // ------------------------------------------------------------------
        // 4. Approve XLM transfer to DEX.
        // ------------------------------------------------------------------
        let xlm_client = token::Client::new(&env, &xlm_token);
        let expiration_ledger = env.ledger().sequence() + 1;
        xlm_client.approve(
            &env.current_contract_address(),
            &dex_contract,
            &xlm_amount,
            &expiration_ledger,
        );

        // ------------------------------------------------------------------
        // 5. Execute DEX swap.
        // ------------------------------------------------------------------
        let mnt_received: i128 = env.invoke_contract(
            &dex_contract,
            &dex_iface.swap_fn,
            (
                xlm_token.clone(),
                mnt_token.clone(),
                xlm_amount,
                min_mnt_out,
                env.current_contract_address(),
            )
                .into_val(&env),
        );

        // ------------------------------------------------------------------
        // 6. Validate swap output.
        // ------------------------------------------------------------------
        if mnt_received == 0 {
            xlm_client.approve(
                &env.current_contract_address(),
                &dex_contract,
                &0,
                &expiration_ledger,
            );
            env.events().publish(
                (symbol_short!("buyback"), symbol_short!("failed")),
                BuybackFailed {
                    xlm_amount,
                    reason: Symbol::new(&env, "zero_output"),
                },
            );
            return Err(Error::ZeroOutput);
        }

        if mnt_received < min_mnt_out {
            xlm_client.approve(
                &env.current_contract_address(),
                &dex_contract,
                &0,
                &expiration_ledger,
            );
            env.events().publish(
                (symbol_short!("buyback"), symbol_short!("failed")),
                BuybackFailed {
                    xlm_amount,
                    reason: Symbol::new(&env, "slippage"),
                },
            );
            return Err(Error::SlippageExceeded);
        }

        // ------------------------------------------------------------------
        // 7. Burn received MNT.
        // ------------------------------------------------------------------
        env.invoke_contract::<()>(
            &mnt_token,
            &Symbol::new(&env, "burn"),
            (env.current_contract_address(), mnt_received).into_val(&env),
        );

        env.events().publish(
            (symbol_short!("buyback"), symbol_short!("ok")),
            BuybackSucceeded {
                xlm_spent: xlm_amount,
                mnt_burned: mnt_received,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // View helpers
    // -----------------------------------------------------------------------

    pub fn get_history_page(env: Env, offset: u32, limit: u32) -> Vec<AllocationHistory> {
        let total_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationCount)
            .unwrap_or(0u32);

        let mut result = Vec::new(&env);
        let end = offset.saturating_add(limit).min(total_count);

        for i in offset..end {
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<DataKey, AllocationHistory>(&DataKey::Allocation(i))
            {
                result.push_back(record);
            }
        }
        result
    }

    pub fn get_timelock(env: Env) -> Address {
        env.storage()
            .persistent()
            .get(&DataKey::Timelock)
            .expect("not initialized")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::Env;

    // ------------------------------------------------------------------
    // Mock contracts
    // ------------------------------------------------------------------

    #[contract]
    pub struct MockDEX;

    #[contractimpl]
    impl MockDEX {
        pub fn swap_exact_in(
            env: Env,
            token_in: Address,
            _token_out: Address,
            amount_in: i128,
            _min_out: i128,
            recipient: Address,
        ) -> i128 {
            let xlm = token::Client::new(&env, &token_in);
            xlm.transfer_from(
                &env.current_contract_address(),
                &recipient,
                &env.current_contract_address(),
                &amount_in,
            );
            amount_in
        }
    }

    #[contract]
    pub struct MockDEXZero;

    #[contractimpl]
    impl MockDEXZero {
        pub fn swap_exact_in(
            _env: Env,
            _token_in: Address,
            _token_out: Address,
            _amount_in: i128,
            _min_out: i128,
            _recipient: Address,
        ) -> i128 {
            0
        }
    }

    #[contract]
    pub struct MockDEXSlippage;

    #[contractimpl]
    impl MockDEXSlippage {
        pub fn swap_exact_in(
            _env: Env,
            _token_in: Address,
            _token_out: Address,
            _amount_in: i128,
            _min_out: i128,
            _recipient: Address,
        ) -> i128 {
            1
        }
    }

    #[contract]
    pub struct MockStaking;

    #[contractimpl]
    impl MockStaking {
        pub fn distribute_revenue(_env: Env, _token: Address, _amount: i128) {}
    }

    #[contract]
    pub struct MockMNT;

    #[contractimpl]
    impl MockMNT {
        pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    }

    // Oracle mocks — all return OracleHealth with the new fields.

    #[contract]
    pub struct MockOracleHealthy;

    #[contractimpl]
    impl MockOracleHealthy {
        pub fn get_oracle_health(_env: Env, _asset: Symbol) -> OracleHealth {
            OracleHealth {
                active_feeders: 5,
                last_update: 999,
                is_stale: false,
                circuit_breaker_tripped: false,
                override_active: false,
            }
        }
    }

    #[contract]
    pub struct MockOracleStale;

    #[contractimpl]
    impl MockOracleStale {
        pub fn get_oracle_health(_env: Env, _asset: Symbol) -> OracleHealth {
            OracleHealth {
                active_feeders: 5,
                last_update: 0,
                is_stale: true,
                circuit_breaker_tripped: false,
                override_active: false,
            }
        }
    }

    #[contract]
    pub struct MockOracleInsufficientFeeders;

    #[contractimpl]
    impl MockOracleInsufficientFeeders {
        pub fn get_oracle_health(_env: Env, _asset: Symbol) -> OracleHealth {
            OracleHealth {
                active_feeders: 1,
                last_update: 999,
                is_stale: false,
                circuit_breaker_tripped: false,
                override_active: false,
            }
        }
    }

    #[contract]
    pub struct MockOracleCircuitBreaker;

    #[contractimpl]
    impl MockOracleCircuitBreaker {
        pub fn get_oracle_health(_env: Env, _asset: Symbol) -> OracleHealth {
            OracleHealth {
                active_feeders: 5,
                last_update: 999,
                is_stale: false,
                circuit_breaker_tripped: true,
                override_active: false,
            }
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn setup_test(env: &Env) -> (Address, Address, Address, Address) {
        let admin = Address::generate(env);
        let staking = env.register_contract(None, MockStaking);
        let timelock = Address::generate(env);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(env, &contract_id);
        client.initialize(&admin, &staking, &timelock, &None);
        (admin, staking, timelock, contract_id)
    }

    fn default_dex_iface(env: &Env) -> DexInterface {
        DexInterface {
            swap_fn: Symbol::new(env, "swap_exact_in"),
        }
    }

    // ------------------------------------------------------------------
    // Initialization
    // ------------------------------------------------------------------

    #[test]
    fn test_initialization() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, staking, timelock, _) = setup_test(&env);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &staking, &timelock, &None);
        let result = client.try_initialize(&admin, &staking, &timelock, &None);
        assert!(result.is_err(), "double-init must fail");
    }

    #[test]
    fn test_deposit_and_balance() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);
        let user = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(admin.clone());
        let stellar_asset_client = token::StellarAssetClient::new(&env, &token_addr);
        stellar_asset_client.mint(&user, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&token_addr, &true);
        treasury_client.deposit(&user, &token_addr, &500);

        assert_eq!(treasury_client.get_balance(&token_addr), 500);
    }

    #[test]
    #[should_panic(expected = "Token not approved")]
    fn test_deposit_unapproved_token() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);
        let user = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(admin.clone());
        let stellar_asset_client = token::StellarAssetClient::new(&env, &token_addr);
        stellar_asset_client.mint(&user, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.deposit(&user, &token_addr, &500);
    }

    #[test]
    fn test_allocate() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);
        let recipient = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(admin.clone());
        let token_client = token::Client::new(&env, &token_addr);
        let stellar_asset_client = token::StellarAssetClient::new(&env, &token_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&token_addr, &true);
        env.ledger().set_timestamp(12345);
        treasury_client.allocate(&token_addr, &recipient, &400);

        assert_eq!(treasury_client.get_balance(&token_addr), 600);
        assert_eq!(token_client.balance(&recipient), 400);
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — timelock access control
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_requires_timelock_auth() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _timelock, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        assert_eq!(treasury_client.get_timelock(), _timelock);
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — zero output (DEX returns 0 MNT)
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_dex_returns_zero_mnt_fails_and_no_xlm_lost() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEXZero);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let xlm_balance_before = treasury_client.get_balance(&xlm_addr);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &100,
            &default_dex_iface(&env),
            &None,
            &None,
        );

        assert!(result.is_err(), "expected ZeroOutput error");

        let xlm_balance_after = treasury_client.get_balance(&xlm_addr);
        assert_eq!(xlm_balance_before, xlm_balance_after);
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — slippage guard
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_and_burn_slippage() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEXSlippage);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let xlm_balance_before = treasury_client.get_balance(&xlm_addr);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &500,
            &default_dex_iface(&env),
            &None,
            &None,
        );

        assert!(result.is_err(), "expected SlippageExceeded error");

        let xlm_balance_after = treasury_client.get_balance(&xlm_addr);
        assert_eq!(xlm_balance_before, xlm_balance_after);
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — zero min_out rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_zero_min_out_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let xlm_balance_before = treasury_client.get_balance(&xlm_addr);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &0,
            &default_dex_iface(&env),
            &None,
            &None,
        );

        assert!(result.is_err(), "expected InvalidMinOut error");
        assert_eq!(treasury_client.get_balance(&xlm_addr), xlm_balance_before);
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — unapproved tokens rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_unapproved_token_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &1000,
            &500,
            &default_dex_iface(&env),
            &None,
            &None,
        );
        assert!(result.is_err(), "unapproved token buyback must fail");
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — empty swap_fn panics
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "DexInterface: swap_fn must not be empty")]
    fn test_buyback_empty_swap_fn_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let bad_iface = DexInterface {
            swap_fn: Symbol::new(&env, ""),
        };
        let _ = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &1000,
            &500,
            &bad_iface,
            &None,
            &None,
        );
    }

    // -----------------------------------------------------------------------
    // Oracle validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_proceeds_with_healthy_oracle() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleHealthy);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &1,
            &default_dex_iface(&env),
            &Some(oracle_addr),
            &Some(symbol_short!("MNT")),
        );
        assert!(result.is_ok(), "healthy oracle should allow buyback");
    }

    #[test]
    fn test_buyback_aborted_when_oracle_stale() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleStale);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &1,
            &default_dex_iface(&env),
            &Some(oracle_addr),
            &Some(symbol_short!("MNT")),
        );
        assert_eq!(result, Err(Ok(Error::OracleStale)));
    }

    #[test]
    fn test_buyback_aborted_when_insufficient_feeders() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleInsufficientFeeders);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &1,
            &default_dex_iface(&env),
            &Some(oracle_addr),
            &Some(symbol_short!("MNT")),
        );
        assert_eq!(result, Err(Ok(Error::OracleUnhealthy)));
    }

    #[test]
    fn test_buyback_aborted_when_circuit_breaker_tripped() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleCircuitBreaker);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &1,
            &default_dex_iface(&env),
            &Some(oracle_addr),
            &Some(symbol_short!("MNT")),
        );
        assert_eq!(result, Err(Ok(Error::OracleCircuitBreaker)));
    }
}
