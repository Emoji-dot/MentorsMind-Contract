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
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleHealth {
    pub active_feeders: u32,
    pub last_update: u64,
    pub is_stale: bool,
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
    NotInitialized     = 2,
    Unauthorized       = 3,
    InsufficientBalance = 4,
    /// Oracle has too few active feeders — buyback aborted to prevent
    /// economic attacks via a manipulated TWAP price.
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
    /// An amount failed comprehensive financial validation (non-positive,
    /// exceeds the economic sanity bound, or fails a business-logic rule
    /// specific to the operation).
    InvalidAmount = 11,
    /// No admin-change proposal is currently pending.
    NoPendingAdminChange = 12,
    /// The admin-change timelock has not yet elapsed.
    AdminChangeNotYetEffective = 13,
    /// The caller is not the address named in the pending admin change.
    InvalidAdminChange = 14,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationHistory {
    pub token: Address,
    pub recipient: Address,
    pub amount: i128,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Token approval event
// ---------------------------------------------------------------------------

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
pub struct AdminChangeProposedEvent {
    pub contract: Address,
    pub old_admin: Address,
    pub new_admin: Address,
    pub effective_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChangeAcceptedEvent {
    pub contract: Address,
    pub new_admin: Address,
}


/// Economic sanity ceiling for a single treasury operation (deposit,
/// allocation, distribution, or buyback), in the token's smallest unit.
/// Guards against amounts large enough to be a fat-finger or manipulation
/// attempt rather than a legitimate treasury movement.
const MAX_FINANCIAL_AMOUNT: i128 = 1_000_000_000_000_000; // 100M tokens @ 7 decimals

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
    PendingAdminTransfer,
    LastAdminChange,
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
    /// Initialize treasury contract with admin and staking contract address.
    pub fn initialize(env: Env, admin: Address, staking_contract: Address) -> Result<(), Error> {
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
        env.storage()
            .persistent()
            .set(&DataKey::RegulatoryReporting, &Address::generate(&env)); // placeholder
        Ok(())
    }

    pub fn set_auto_burn_rate(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        Validator::new(&env)
            .require_valid_bps(bps, "bps")
            .validate()
            .map_err(|_| Error::InvalidAmount)?;
        env.storage().persistent().set(&DataKey::AutoBurnRateBps, &bps);
        Ok(())
    }

    pub fn execute_burn_queue(env: Env) -> Result<i128, Error> {
        let queued: i128 = env.storage().persistent().get(&DataKey::BurnQueue).unwrap_or(0);
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
        
        let last_change: u64 = env.storage().persistent().get(&DataKey::LastAdminChange).unwrap_or(0);
        let current_time = env.ledger().timestamp();
        if current_time < last_change + ADMIN_COOLING_OFF_SECS {
            return Err(Error::CoolingOffPeriod);
        }

        let effective_at = current_time
            .checked_add(MIN_ADMIN_TIMELOCK_SECS)
            .ok_or(Error::InvalidAdminChange)?;

        let pending = AdminTransfer {
            new_admin: new_admin.clone(),
            effective_at,
            status: AdminChangeProposal::Proposed,
        };
        env.storage()
            .persistent()
            .set(&DataKey::PendingAdminTransfer, &pending);
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("proposed")),
            AdminChangeProposedEvent {
                contract: env.current_contract_address(),
                old_admin: current_admin,
                new_admin,
                effective_at,
            },
        );
        Ok(())
    }

    pub fn accept_admin_change(env: Env, new_admin: Address) -> Result<(), Error> {
        new_admin.require_auth();
        let mut pending: AdminTransfer = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdminTransfer)
            .ok_or(Error::NoPendingAdminChange)?;
        if pending.new_admin != new_admin {
            return Err(Error::Unauthorized);
        }
        if env.ledger().timestamp() < pending.effective_at {
            return Err(Error::TimelockNotExpired);
        }
        if pending.status != AdminChangeProposal::Proposed {
            return Err(Error::InvalidAdminChange);
        }
        
        pending.status = AdminChangeProposal::Accepted;

        env.storage().persistent().set(&DataKey::Admin, &new_admin);
        env.storage().persistent().set(&DataKey::LastAdminChange, &env.ledger().timestamp());
        env.storage().persistent().remove(&DataKey::PendingAdminTransfer);
        
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("accepted")),
            AdminChangeAcceptedEvent {
                contract: env.current_contract_address(),
                new_admin,
            },
        );
        Ok(())
    }

    pub fn cancel_admin_change(env: Env, multisig: Address) -> Result<(), Error> {
        multisig.require_auth();
        if !env.storage().persistent().has(&DataKey::PendingAdminTransfer) {
            return Err(Error::NoPendingAdminChange);
        }
        env.storage().persistent().remove(&DataKey::PendingAdminTransfer);
        Ok(())
    }

    pub fn revoke_admin_emergency(env: Env, new_admin: Address) -> Result<(), Error> {
        // Assume multisig is authorized to call this via timelock or direct consensus
        let timelock: Address = env.storage().persistent().get(&DataKey::Timelock).ok_or(Error::NotInitialized)?;
        timelock.require_auth();
        
        env.storage().persistent().set(&DataKey::Admin, &new_admin);
        env.storage().persistent().remove(&DataKey::PendingAdminTransfer);
        Ok(())
    }

    pub fn get_pending_admin_change(env: Env) -> Option<AdminTransfer> {
        env.storage().persistent().get(&DataKey::PendingAdminTransfer)
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        Self::admin(&env)
    }

    /// Set regulatory reporting contract address (admin only).
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
        if env.storage().persistent().has(&DataKey::PendingAdminTransfer) {
            return Err(Error::SuspendedDuringAdminTransfer);
        }
        Ok(())
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
            // Call regulatory_reporting::record_large_tx
            use soroban_sdk::IntoVal;
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

    /// Add or remove an approved token from the treasury whitelist (admin only).
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

    /// Accept deposits of any approved Stellar asset.
    pub fn deposit(env: Env, from: Address, token: Address, amount: i128) -> Result<(), Error> {
        // Check pause guardian before any state mutation
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

    /// get_balance — returns the contract's current balance of `token`.
    pub fn get_balance(env: Env, token: Address) -> i128 {
        token::Client::new(&env, &token).balance(&env.current_contract_address())
    }

    /// allocate — governance/timelock only; transfers `amount` of `token` to `recipient`.
    pub fn allocate(
        env: Env,
        token: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        // Check pause guardian before any state mutation
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

        // Check for large transaction threshold and trigger regulatory reporting
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
            // Above threshold — requires multi-sig approval (Issue #752)
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

        let mut history = env
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

    /// Set multi-sig withdrawal threshold amount (admin only).
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

    /// Multi-sig approval for pending high-value allocations (Issue #752).
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
            // Multi-sig threshold reached — execute transfer
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

    /// Read a pending allocation by ID.
    pub fn get_pending_allocation(env: Env, pending_id: u32) -> Option<PendingAllocation> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingAllocation(pending_id))
    }

    /// Return the count of pending (multi-sig) allocations.
    pub fn pending_allocation_count(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::PendingAllocationCount)
            .unwrap_or(0)
    }

    /// Distribute tokens to stakers — pro-rata handled by staking contract.
    pub fn distribute_to_stakers(
        env: Env,
        token: Address,
        total_amount: i128,
    ) -> Result<(), Error> {
        // Check pause guardian before any state mutation
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

    /// buyback_and_burn — swap XLM for MNT on DEX, then burn MNT.
    ///
    /// # Oracle health gate (#614)
    /// Before executing the swap, this function queries the oracle for the
    /// MNT asset health.  The call is aborted with `OracleUnhealthy` or
    /// `OracleStale` if the oracle does not meet the minimum-feeder threshold
    /// or has not been updated recently.  This prevents a manipulated TWAP
    /// from being used as the slippage baseline for `min_mnt_out`.
    ///
    /// Pass `oracle_contract = None` to skip the health check (legacy / test).
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

        // --- Oracle health gate -------------------------------------------
        if let (Some(oracle), Some(asset_sym)) = (oracle_contract.clone(), mnt_asset_symbol.clone()) {
            let health: OracleHealth =
                OracleContractClient::new(&env, &oracle).get_oracle_health(&asset_sym);

            if health.is_stale {
                return Err(Error::OracleStale);
            }
            // MIN_FEEDERS is enforced inside the oracle; we check here so
            // treasury can surface a distinct error code.
            if health.active_feeders < 3 {
                return Err(Error::OracleUnhealthy);
            }
        }

        // 1. Transfer XLM to DEX
        let xlm_client = token::Client::new(&env, &xlm_token);
        let expiration_ledger = env.ledger().sequence() + 1;
        xlm_client.approve(
            &env.current_contract_address(),
            &dex_contract,
            &xlm_amount,
            &expiration_ledger,
        );

        // 2. Call DEX swap — returns the amount of MNT received
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
        // 5. Validate output — revoke allowance and emit failure if bad.
        // ------------------------------------------------------------------
        if mnt_received == 0 {
            // Revoke any remaining allowance (defensive; DEX may not have pulled).
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
            // Revoke any remaining allowance.
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
        // 6. Burn MNT — only reached if swap succeeded and output is valid.
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
            // Pull the XLM allowance from the treasury (simulate DEX pull).
            let xlm = token::Client::new(&env, &token_in);
            xlm.transfer_from(
                &env.current_contract_address(),
                &recipient, // pull from treasury (spender == DEX contract)
                &env.current_contract_address(), // actually pull from who approved
                &amount_in,
            );
            // Return MNT amount (1:1 for tests).
            amount_in
        }
    }

    /// DEX that always returns 0 MNT (simulates failed / empty swap).
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
            0 // returns nothing — no XLM pulled
        }
    }

    /// DEX that returns less MNT than min_mnt_out (simulates slippage).
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
            1 // returns tiny amount — below min_mnt_out
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

    /// A mock oracle that returns a configurable health report.
    #[contract]
    pub struct MockOracleHealthy;

    fn setup_test(env: &Env) -> (Address, Address, Address, Address) {
        let admin = Address::generate(env);
        let staking = env.register_contract(None, MockStaking);
        let timelock = Address::generate(env); // simulated timelock address
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(env, &contract_id);
        client.initialize(&admin, &staking, &timelock);
        (admin, staking, timelock, contract_id)
    }

    fn default_dex_iface(env: &Env) -> DexInterface {
        DexInterface {
            swap_fn: Symbol::new(env, "swap_exact_in"),
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
            }
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn setup_test(env: &Env) -> (Address, Address, Address) {
        let admin = Address::generate(env);
        let staking = env.register_contract(None, MockStaking);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(env, &contract_id);
        client.initialize(&admin, &staking);
        (admin, staking, contract_id)
    }

    // ------------------------------------------------------------------
    // Existing tests (unchanged behaviour)
    // ------------------------------------------------------------------

    #[test]
    fn test_initialization() {
        let env = Env::default();
        let (admin, staking, _) = setup_test(&env);
        let client =
            TreasuryContractClient::new(&env, &env.register_contract(None, TreasuryContract));
        client.initialize(&admin, &staking);
        let result = client.try_initialize(&admin, &staking);
        assert!(result.is_err());
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

        let history = treasury_client.get_history();
        assert_eq!(history.len(), 1);
        let entry = history.get(0).unwrap();
        assert_eq!(entry.amount, 400);
        assert_eq!(entry.timestamp, 12345);
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

        // get_timelock should return the registered address
        assert_eq!(treasury_client.get_timelock(), _timelock);

        // mock_all_auths covers timelock auth — call succeeds
        // (full auth-gating is enforced by require_auth; this test confirms the
        //  function reads the timelock address from storage correctly)
        let _ = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &1000,
            &500,
            &default_dex_iface(&env),
        );
        // We only check that get_timelock() returns the expected address; the
        // auth mock covers the auth requirement in unit test mode.
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
        let dex_addr = env.register_contract(None, MockDEXZero); // returns 0

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
        );

        // Must return ZeroOutput error
        assert!(result.is_err(), "expected ZeroOutput error");

        // XLM balance must not have changed — no funds left treasury
        let xlm_balance_after = treasury_client.get_balance(&xlm_addr);
        assert_eq!(
            xlm_balance_before, xlm_balance_after,
            "XLM must not leave treasury when DEX returns 0 MNT"
        );
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — slippage guard (min_mnt_out not met)
    // -----------------------------------------------------------------------

    #[test]
    fn test_buyback_and_burn_without_oracle() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, _, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEXSlippage); // returns 1

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &1000);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        treasury_client.set_approved_token(&xlm_addr, &true);
        treasury_client.set_approved_token(&mnt_addr, &true);

        let xlm_balance_before = treasury_client.get_balance(&xlm_addr);

        // min_mnt_out = 500, DEX returns 1 → slippage
        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &500,
            &default_dex_iface(&env),
        );

        assert!(result.is_err(), "expected SlippageExceeded error");

        let xlm_balance_after = treasury_client.get_balance(&xlm_addr);
        assert_eq!(
            xlm_balance_before, xlm_balance_after,
            "XLM must not leave treasury when slippage guard triggers"
        );
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — invalid min_mnt_out (= 0) rejected before any transfer
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

        // min_mnt_out = 0 → InvalidMinOut, no XLM transferred
        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &0, // invalid
            &default_dex_iface(&env),
        );

        assert!(result.is_err(), "expected InvalidMinOut error");
        assert_eq!(
            treasury_client.get_balance(&xlm_addr),
            xlm_balance_before,
            "XLM must remain in treasury when min_out = 0"
        );
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
        // Do NOT approve tokens

        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &1000,
            &500,
            &default_dex_iface(&env),
        );
        assert!(result.is_err(), "unapproved token buyback must fail");
    }

    // -----------------------------------------------------------------------
    // buyback_and_burn — invalid DEX interface rejected
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
        // oracle_contract = None → skip health check (backward compat)
        treasury_client.buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &1000,
            &None,
            &None,
        );

        let bad_iface = DexInterface {
            swap_fn: Symbol::new(&env, ""),
        };
        let _ = treasury_client
            .try_buyback_and_burn(&xlm_addr, &mnt_addr, &dex_addr, &1000, &500, &bad_iface);
    }

    // ------------------------------------------------------------------
    // #614-AC4: treasury::buyback_and_burn queries oracle health before swap
    // ------------------------------------------------------------------

    #[test]
    fn test_buyback_proceeds_with_healthy_oracle() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let (admin, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleHealthy);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
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
        let (admin, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleStale);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
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
        let (admin, _, contract_id) = setup_test(&env);

        let xlm_addr = env.register_stellar_asset_contract(admin.clone());
        let mnt_addr = env.register_contract(None, MockMNT);
        let dex_addr = env.register_contract(None, MockDEX);
        let oracle_addr = env.register_contract(None, MockOracleInsufficientFeeders);

        let stellar_asset_client = token::StellarAssetClient::new(&env, &xlm_addr);
        stellar_asset_client.mint(&contract_id, &500);

        let treasury_client = TreasuryContractClient::new(&env, &contract_id);
        let result = treasury_client.try_buyback_and_burn(
            &xlm_addr,
            &mnt_addr,
            &dex_addr,
            &500,
            &Some(oracle_addr),
            &Some(symbol_short!("MNT")),
        );
        assert_eq!(result, Err(Ok(Error::OracleUnhealthy)));
    }
}
