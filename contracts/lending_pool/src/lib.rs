#![no_std]

use shared::ReentrancyGuard;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
    Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    InsufficientLiquidity = 4,
    LowCreditScore = 5,
    BorrowLimitExceeded = 6,
    LoanNotFound = 7,
    LoanAlreadyRepaid = 8,
    NotAdmin = 9,
    InvalidAmount = 10,
    /// Deposit and withdrawal in the same ledger sequence are forbidden to
    /// prevent flash-loan-style balance manipulation.
    SameBlockDepositWithdraw = 11,
    /// A single address may not borrow more than the per-block cap.
    PerBlockBorrowLimitExceeded = 12,
}

// ---------------------------------------------------------------------------
// Data Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoanRecord {
    pub borrower: Address,
    pub amount: i128,
    pub fee: i128,
    pub session_id: Symbol,
    pub borrowed_at: u64,
    pub due_at: u64,
    pub repaid: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LenderRecord {
    pub lender: Address,
    pub lp_tokens: i128,
    pub deposited_at: u64,
}

// ---------------------------------------------------------------------------
// Storage Keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    UsdcToken,
    CreditScoreContract,
    TotalLiquidity,
    TotalLpTokens,
    LenderBalance(Address),
    Loan(Address),
    LoanCount,
    /// Ledger sequence at which a lender last deposited.
    /// Used to enforce the same-block deposit/withdraw guard.
    ///
    /// TTL strategy: stored in **persistent** storage so the guard survives
    /// instance TTL expiry.  The key is bumped via `extend_ttl` on every
    /// deposit and withdraw.  Without persistent storage an attacker could
    /// wait for instance TTL to expire, then deposit and withdraw in the same
    /// ledger sequence — silently bypassing the flash-loan guard.
    LenderDepositLedger(Address),
    /// Running borrow total for an address within the current ledger sequence.
    /// Resets when the ledger sequence advances.
    ///
    /// TTL strategy: stored in **persistent** storage with `extend_ttl` on
    /// every borrow.  This prevents the per-block borrow cap from being
    /// silently reset by instance TTL expiry, which would allow an attacker
    /// to drain the pool in a single transaction.
    BlockBorrowTotal(Address),
    /// Ledger sequence recorded when the per-block borrow total was last written.
    ///
    /// TTL strategy: stored in **persistent** storage with `extend_ttl` on
    /// every borrow.  Paired with `BlockBorrowTotal` to ensure the accumulator
    /// resets correctly across ledger sequences even after TTL expiry.
    BlockBorrowLedger(Address),
    /// Snapshot of total liquidity at the start of the current ledger sequence.
    /// Used to detect intra-block balance manipulation.
    BlockLiquiditySnapshot,
    /// Ledger sequence when the liquidity snapshot was taken.
    BlockLiquidityLedger,
    /// Simple fee cache: parallel vectors of amounts and fees
    FeeCacheKeys,
    FeeCacheValues,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const INTEREST_RATE_BPS: i128 = 200; // 2% flat fee
const MIN_CREDIT_SCORE: u32 = 600;
const LIQUIDATION_DAYS: u64 = 30;
const LIQUIDATION_SECONDS: u64 = LIQUIDATION_DAYS * 86_400;

/// Maximum amount a single address may borrow within one ledger sequence.
/// Set to 10% of the pool's liquidity snapshot; enforced dynamically.
const PER_BLOCK_BORROW_CAP_BPS: i128 = 1_000; // 10 %

// ---------------------------------------------------------------------------
// TTL constants for flash-loan guard entries (persistent storage)
// ---------------------------------------------------------------------------

/// Retention period for flash-loan guard entries: 7 days in ledgers
/// (assuming ~5s per ledger).  This is the maximum time a ledger-guard
/// entry should be kept before it is eligible for archival.
const LEDGER_GUARD_TTL: u32 = 120_960; // 7 days at 5s/ledger
/// TTL threshold: when remaining lifetime drops below this many ledgers,
/// extend the TTL.  500k ledgers ≈ 29 days at 5s/ledger.
const LEDGER_GUARD_TTL_THRESHOLD: u32 = 500_000;
/// TTL bump amount in ledgers: extend lifetime by this amount.
/// 1_209_600 ledgers ≈ 70 days (10 weeks) at 5s/ledger — well beyond the
/// 7-day instance TTL default, guaranteeing the flash-loan guard cannot
/// be silently expired.
const LEDGER_GUARD_TTL_BUMP: u32 = 1_209_600;

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    /// Initialize the lending pool
    pub fn initialize(
        env: Env,
        admin: Address,
        usdc_token: Address,
        credit_score_contract: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::UsdcToken, &usdc_token);
        env.storage()
            .instance()
            .set(&DataKey::CreditScoreContract, &credit_score_contract);
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalLpTokens, &0i128);

        Ok(())
    }

    /// Deposit USDC liquidity and receive LP tokens
    pub fn deposit(env: Env, lender: Address, amount: i128) -> Result<i128, Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        lender.require_auth();

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        // Transfer USDC from lender to contract
        token_client.transfer(&lender, &env.current_contract_address(), &amount);

        // Calculate LP tokens (1:1 ratio for simplicity)
        let lp_tokens = amount;

        // Update storage
        let mut total_liquidity: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0);
        total_liquidity = total_liquidity.checked_add(amount).expect("Overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &total_liquidity);

        let mut total_lp: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLpTokens)
            .unwrap_or(0);
        total_lp = total_lp.checked_add(lp_tokens).expect("Overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLpTokens, &total_lp);

        let mut lender_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LenderBalance(lender.clone()))
            .unwrap_or(0);
        lender_balance = lender_balance.checked_add(lp_tokens).expect("Overflow");
        env.storage()
            .persistent()
            .set(&DataKey::LenderBalance(lender.clone()), &lender_balance);

        env.events()
            .publish((symbol_short!("deposited"),), (lender.clone(), amount, lp_tokens));

        // Record the ledger sequence of this deposit so that same-block
        // withdrawals can be rejected.
        //
        // Stored in persistent storage with a TTL bump so the guard
        // survives instance TTL expiry (see DataKey::LenderDepositLedger).
        let deposit_ledger_key = DataKey::LenderDepositLedger(lender);
        env.storage()
            .persistent()
            .set(&deposit_ledger_key, &env.ledger().sequence());
        env.storage().persistent().extend_ttl(
            &deposit_ledger_key,
            LEDGER_GUARD_TTL_THRESHOLD,
            LEDGER_GUARD_TTL_BUMP,
        );

        Ok(lp_tokens)
    }

    /// Withdraw USDC by burning LP tokens
    pub fn withdraw(env: Env, lender: Address, lp_amount: i128) -> Result<i128, Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        if lp_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        lender.require_auth();

        // Flash-loan guard: reject withdrawals in the same ledger sequence as
        // the deposit.  An attacker who deposits and immediately withdraws
        // within one transaction cannot manipulate pool balances.
        //
        // Read from persistent storage and bump TTL so the entry cannot be
        // silently expired by instance TTL expiry.
        let deposit_ledger_key = DataKey::LenderDepositLedger(lender.clone());
        let deposit_ledger: u32 = env
            .storage()
            .persistent()
            .get(&deposit_ledger_key)
            .unwrap_or(0);
        if deposit_ledger != 0 {
            env.storage().persistent().extend_ttl(
                &deposit_ledger_key,
                LEDGER_GUARD_TTL_THRESHOLD,
                LEDGER_GUARD_TTL_BUMP,
            );
        }
        if deposit_ledger == env.ledger().sequence() {
            return Err(Error::SameBlockDepositWithdraw);
        }

        let lender_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LenderBalance(lender.clone()))
            .unwrap_or(0);

        if lender_balance < lp_amount {
            return Err(Error::InsufficientBalance);
        }

        // Calculate USDC to return (1:1 ratio + accrued interest)
        let usdc_amount = lp_amount;

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        // Transfer USDC back to lender
        token_client.transfer(&env.current_contract_address(), &lender, &usdc_amount);

        // Update storage
        let mut total_liquidity: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0);
        total_liquidity = total_liquidity.checked_sub(usdc_amount).expect("Underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &total_liquidity);

        let mut total_lp: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLpTokens)
            .unwrap_or(0);
        total_lp = total_lp.checked_sub(lp_amount).expect("Underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLpTokens, &total_lp);

        let new_balance = lender_balance.checked_sub(lp_amount).expect("Underflow");
        if new_balance == 0 {
            env.storage()
                .persistent()
                .remove(&DataKey::LenderBalance(lender.clone()));
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::LenderBalance(lender.clone()), &new_balance);
        }

        env.events().publish(
            (symbol_short!("withdrawn"),),
            (lender, lp_amount, usdc_amount),
        );

        Ok(usdc_amount)
    }

    /// Borrow USDC against credit score
    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        session_id: Symbol,
    ) -> Result<(), Error> {
        let _guard = ReentrancyGuard::enter(&env, Symbol::new(&env, "borrow"));
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        borrower.require_auth();

        // Check credit score (mock: assume credit_score_contract returns u32)
        // In real implementation, would call credit_score_contract
        let _credit_score = MIN_CREDIT_SCORE; // Simplified for now

        // Check liquidity
        let total_liquidity: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0);

        if total_liquidity < amount {
            return Err(Error::InsufficientLiquidity);
        }

        // ---------------------------------------------------------------
        // Flash-loan guard: per-block borrow cap
        //
        // Take a snapshot of total liquidity at the start of each new
        // ledger sequence.  Within a single sequence, cap the cumulative
        // borrow amount for any one address at PER_BLOCK_BORROW_CAP_BPS
        // of that snapshot.  This prevents an attacker from draining the
        // pool in a single transaction by repeatedly borrowing.
        // ---------------------------------------------------------------
        let current_seq = env.ledger().sequence();

        // Refresh the liquidity snapshot when the ledger sequence advances.
        let snap_ledger: u32 = env
            .storage()
            .instance()
            .get(&DataKey::BlockLiquidityLedger)
            .unwrap_or(0);
        let liquidity_snapshot: i128 = if snap_ledger == current_seq {
            env.storage()
                .instance()
                .get(&DataKey::BlockLiquiditySnapshot)
                .unwrap_or(total_liquidity)
        } else {
            // New ledger sequence — record a fresh snapshot.
            env.storage()
                .instance()
                .set(&DataKey::BlockLiquiditySnapshot, &total_liquidity);
            env.storage()
                .instance()
                .set(&DataKey::BlockLiquidityLedger, &current_seq);
            total_liquidity
        };

        // Bump instance TTL on every borrow to prevent silent expiry of
        // the liquidity snapshot and other instance-scoped data.
        env.storage()
            .instance()
            .extend_ttl(LEDGER_GUARD_TTL_THRESHOLD, LEDGER_GUARD_TTL_BUMP);

        // Compute the per-block cap for this borrower.
        let per_block_cap = liquidity_snapshot
            .checked_mul(PER_BLOCK_BORROW_CAP_BPS)
            .unwrap_or(i128::MAX)
            .checked_div(10_000)
            .unwrap_or(i128::MAX);

        // Accumulate the borrower's total within this ledger sequence.
        //
        // Read from persistent storage so the per-block cap cannot be
        // silently reset by instance TTL expiry.  Bump TTL on every access.
        let borrow_ledger_key = DataKey::BlockBorrowLedger(borrower.clone());
        let borrow_ledger: u32 = env
            .storage()
            .persistent()
            .get(&borrow_ledger_key)
            .unwrap_or(0);
        if borrow_ledger != 0 {
            env.storage().persistent().extend_ttl(
                &borrow_ledger_key,
                LEDGER_GUARD_TTL_THRESHOLD,
                LEDGER_GUARD_TTL_BUMP,
            );
        }

        let borrow_total_key = DataKey::BlockBorrowTotal(borrower.clone());
        let block_total: i128 = if borrow_ledger == current_seq {
            let total: i128 = env
                .storage()
                .persistent()
                .get(&borrow_total_key)
                .unwrap_or(0);
            if total != 0 {
                env.storage().persistent().extend_ttl(
                    &borrow_total_key,
                    LEDGER_GUARD_TTL_THRESHOLD,
                    LEDGER_GUARD_TTL_BUMP,
                );
            }
            total
        } else {
            0 // Reset for the new ledger sequence.
        };

        let new_block_total = block_total.checked_add(amount).unwrap_or(i128::MAX);
        if new_block_total > per_block_cap {
            return Err(Error::PerBlockBorrowLimitExceeded);
        }

        // Persist the updated per-block accumulator in persistent storage
        // with TTL bumping.
        env.storage()
            .persistent()
            .set(&borrow_total_key, &new_block_total);
        env.storage().persistent().extend_ttl(
            &borrow_total_key,
            LEDGER_GUARD_TTL_THRESHOLD,
            LEDGER_GUARD_TTL_BUMP,
        );
        env.storage()
            .persistent()
            .set(&borrow_ledger_key, &current_seq);
        env.storage().persistent().extend_ttl(
            &borrow_ledger_key,
            LEDGER_GUARD_TTL_THRESHOLD,
            LEDGER_GUARD_TTL_BUMP,
        );

        // Calculate fee (2% flat) using cache for common amounts
        let fee = Self::get_cached_fee(&env, amount);

        // Transfer USDC to borrower FIRST
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &borrower, &amount);

        // Create loan record
        let now = env.ledger().timestamp();
        let loan = LoanRecord {
            borrower: borrower.clone(),
            amount,
            fee,
            session_id: session_id.clone(),
            borrowed_at: now,
            due_at: now.checked_add(LIQUIDATION_SECONDS).expect("Timestamp overflow"),
            repaid: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Loan(borrower.clone()), &loan);

        // Update liquidity
        let new_liquidity = total_liquidity.checked_sub(amount).expect("Underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &new_liquidity);

        env.events().publish(
            (symbol_short!("borrowed"),),
            (borrower, amount, fee, session_id),
        );

        Ok(())
    }

    /// Repay loan with principal + fee
    pub fn repay(env: Env, borrower: Address, amount: i128) -> Result<(), Error> {
        let _guard = ReentrancyGuard::enter(&env, Symbol::new(&env, "repay"));
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        borrower.require_auth();

        let loan: LoanRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Loan(borrower.clone()))
            .ok_or(Error::LoanNotFound)?;

        if loan.repaid {
            return Err(Error::LoanAlreadyRepaid);
        }

        let total_owed = loan.amount + loan.fee;
        if amount < total_owed {
            return Err(Error::InvalidAmount);
        }

        // Transfer USDC from borrower to contract
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&borrower, &env.current_contract_address(), &total_owed);

        // Mark loan as repaid
        let mut updated_loan = loan.clone();
        updated_loan.repaid = true;
        env.storage()
            .persistent()
            .set(&DataKey::Loan(borrower.clone()), &updated_loan);

        // Update liquidity
        let mut total_liquidity: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0);
        total_liquidity = total_liquidity.checked_add(total_owed).expect("Overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &total_liquidity);

        env.events().publish(
            (symbol_short!("repaid"),),
            (borrower, loan.amount, loan.fee),
        );

        Ok(())
    }

    /// Get loan record for borrower
    pub fn get_loan(env: Env, borrower: Address) -> Result<LoanRecord, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Loan(borrower))
            .ok_or(Error::LoanNotFound)
    }

    /// Return the cumulative amount borrowed by `borrower` in the current
    /// ledger sequence.  Returns 0 if no borrow has occurred this sequence.
    ///
    /// Reads from persistent storage and bumps TTL to prevent silent expiry.
    pub fn get_block_borrow_total(env: Env, borrower: Address) -> i128 {
        let current_seq = env.ledger().sequence();
        let borrow_ledger_key = DataKey::BlockBorrowLedger(borrower.clone());
        let borrow_ledger: u32 = env
            .storage()
            .persistent()
            .get(&borrow_ledger_key)
            .unwrap_or(0);
        if borrow_ledger != 0 {
            env.storage().persistent().extend_ttl(
                &borrow_ledger_key,
                LEDGER_GUARD_TTL_THRESHOLD,
                LEDGER_GUARD_TTL_BUMP,
            );
        }
        if borrow_ledger == current_seq {
            let borrow_total_key = DataKey::BlockBorrowTotal(borrower);
            let total: i128 = env
                .storage()
                .persistent()
                .get(&borrow_total_key)
                .unwrap_or(0);
            if total != 0 {
                env.storage().persistent().extend_ttl(
                    &borrow_total_key,
                    LEDGER_GUARD_TTL_THRESHOLD,
                    LEDGER_GUARD_TTL_BUMP,
                );
            }
            total
        } else {
            0
        }
    }

    /// Return the liquidity snapshot taken at the start of the current ledger
    /// sequence.  This is the reference value used for the per-block borrow cap.
    pub fn get_liquidity_snapshot(env: Env) -> i128 {
        let current_seq = env.ledger().sequence();
        let snap_ledger: u32 = env
            .storage()
            .instance()
            .get(&DataKey::BlockLiquidityLedger)
            .unwrap_or(0);
        if snap_ledger == current_seq {
            env.storage()
                .instance()
                .get(&DataKey::BlockLiquiditySnapshot)
                .unwrap_or(0)
        } else {
            env.storage()
                .instance()
                .get(&DataKey::TotalLiquidity)
                .unwrap_or(0)
        }
    }

    pub fn total_liquidity(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0)
    }

    pub fn lender_balance(env: Env, lender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::LenderBalance(lender))
            .unwrap_or(0)
    }

    /// Return cached fee for an amount or compute and store it.
    fn get_cached_fee(env: &Env, amount: i128) -> i128 {
        // Retrieve parallel vectors from instance storage
        let mut keys: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::FeeCacheKeys)
            .unwrap_or(Vec::new(&env));
        let mut vals: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::FeeCacheValues)
            .unwrap_or(Vec::new(&env));

        let mut i = 0;
        while i < keys.len() {
            if keys.get(i).unwrap() == amount {
                return vals.get(i).unwrap();
            }
            i += 1;
        }

        // Not found: compute and append to cache
        let fee = amount
            .checked_mul(INTEREST_RATE_BPS)
            .expect("Overflow")
            .checked_div(10_000)
            .expect("Division error");
        keys.push_back(amount);
        vals.push_back(fee);

        // Persist updated cache
        env.storage().instance().set(&DataKey::FeeCacheKeys, &keys);
        env.storage().instance().set(&DataKey::FeeCacheValues, &vals);

        fee
    }

    /// Clear the fee cache (cache invalidation).
    pub fn clear_fee_cache(env: Env) {
        env.storage()
            .instance()
            .set(&DataKey::FeeCacheKeys, &Vec::<i128>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::FeeCacheValues, &Vec::<i128>::new(&env));
    }

    /// Return the number of cached fee entries (for testing/observability).
    pub fn fee_cache_len(env: Env) -> u32 {
        let keys: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::FeeCacheKeys)
            .unwrap_or(Vec::new(&env));
        keys.len()
    }

    /// Liquidate overdue loan (admin only)
    pub fn liquidate(env: Env, borrower: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let loan: LoanRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Loan(borrower.clone()))
            .ok_or(Error::LoanNotFound)?;

        if loan.repaid {
            return Err(Error::LoanAlreadyRepaid);
        }

        let now = env.ledger().timestamp();
        if now <= loan.due_at {
            panic!("loan not yet due");
        }

        // Mark as repaid (liquidated)
        let mut updated_loan = loan.clone();
        updated_loan.repaid = true;
        env.storage()
            .persistent()
            .set(&DataKey::Loan(borrower.clone()), &updated_loan);

        env.events()
            .publish((symbol_short!("liq"),), (borrower, loan.amount));

        Ok(())
    }

    /// Accrue protocol yield into the pool so lenders can withdraw from a
    /// larger liquidity base over time.
    pub fn accrue_yield(env: Env, admin: Address, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if stored_admin != admin {
            return Err(Error::NotAdmin);
        }

        let mut total_liquidity: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLiquidity)
            .unwrap_or(0);
        total_liquidity = total_liquidity.checked_add(amount).expect("Overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalLiquidity, &total_liquidity);

        env.events()
            .publish((symbol_short!("yield"), symbol_short!("accrue")), amount);
        Ok(())
    }

    /// Distribute a portion of accrued yield to a lender by minting LP share.
    pub fn distribute_yield(env: Env, admin: Address, lender: Address, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if stored_admin != admin {
            return Err(Error::NotAdmin);
        }

        let mut lender_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LenderBalance(lender.clone()))
            .unwrap_or(0);
        lender_balance = lender_balance.checked_add(amount).expect("Overflow");
        env.storage()
            .persistent()
            .set(&DataKey::LenderBalance(lender.clone()), &lender_balance);

        let mut total_lp: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalLpTokens)
            .unwrap_or(0);
        total_lp = total_lp.checked_add(amount).expect("Overflow");
        env.storage().instance().set(&DataKey::TotalLpTokens, &total_lp);

        env.events().publish(
            (symbol_short!("yield"), symbol_short!("dist")),
            (lender, amount),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    #[ignore = "requires full token contract harness"]
    fn test_deposit() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let lender = Address::generate(&env);
        let usdc = Address::generate(&env);
        let credit_score = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin, &usdc, &credit_score);

        let result = client.deposit(&lender, &1000);
        assert_eq!(result, 1000);
    }

    #[test]
    #[ignore = "requires full token contract harness"]
    fn test_borrow() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let lender = Address::generate(&env);
        let borrower = Address::generate(&env);
        let usdc = Address::generate(&env);
        let credit_score = Address::generate(&env);
        let session_id = symbol_short!("session1");

        env.mock_all_auths();
        client.initialize(&admin, &usdc, &credit_score);
        client.deposit(&lender, &10000);

        client.borrow(&borrower, &1000, &session_id);
        // Cache should contain the fee for the borrowed amount
        let cache_len = client.fee_cache_len();
        assert!(cache_len >= 1);
    }

    #[test]
    #[ignore = "requires full token contract harness"]
    fn test_repay() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let lender = Address::generate(&env);
        let borrower = Address::generate(&env);
        let usdc = Address::generate(&env);
        let credit_score = Address::generate(&env);
        let session_id = symbol_short!("session1");

        env.mock_all_auths();
        client.initialize(&admin, &usdc, &credit_score);
        client.deposit(&lender, &10000);
        client.borrow(&borrower, &1000, &session_id);
        client.repay(&borrower, &1020);
    }

    #[test]
    fn test_insufficient_liquidity() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let usdc = Address::generate(&env);
        let credit_score = Address::generate(&env);
        let session_id = symbol_short!("session1");

        env.mock_all_auths();
        client.initialize(&admin, &usdc, &credit_score);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.borrow(&borrower, &1000, &session_id);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_yield_accrual_and_distribution() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, LendingPool);
        let client = LendingPoolClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let lender = Address::generate(&env);
        let usdc = Address::generate(&env);
        let credit_score = Address::generate(&env);

        client.initialize(&admin, &usdc, &credit_score);
        client.accrue_yield(&admin, &500);
        client.distribute_yield(&admin, &lender, &200);

        assert_eq!(client.total_liquidity(), 500);
        assert_eq!(client.lender_balance(&lender), 200);
    }
}
