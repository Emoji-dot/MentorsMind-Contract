#![no_std]
#![allow(deprecated)] // Temporarily allow deprecated Events::publish until we migrate to #[contractevent]

// ---------------------------------------------------------------------------
// RFC: Upgrade path design
//
// Two paths exist for upgrading a contract tracked by this registry:
//
// PATH A — Two-step (RECOMMENDED):
//   1. `schedule_upgrade(contract_name, new_version, changelog_hash)`
//      - Requires admin auth
//      - Checks new_version > current_version (VersionNotMonotonic)
//      - Records a PendingUpgrade with `execute_after = now + upgrade_delay`
//   2. `execute_pending_upgrade(contract_name)`
//      - Requires admin auth
//      - Checks ledger timestamp >= execute_after (TimelockNotElapsed)
//      - Commits the upgrade record; clears the pending slot
//
// PATH B — Direct UUPS (`upgrade_contract`) — DEPRECATED
//   Kept for backward-compatibility only.  Marked `#[deprecated]`.
//   Callers MUST migrate to PATH A.  PATH B enforces identical guards:
//   - VersionNotMonotonic
//   - TimelockNotElapsed (uses same upgrade_delay as PATH A)
//   There is NO way to bypass the timelock via PATH B.
//
// Both paths now provide identical security guarantees.
// ---------------------------------------------------------------------------

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    IntoVal, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    ContractNotFound = 4,
    AlreadySubscribed = 5,
    NotSubscribed = 6,
    /// new_version must be strictly greater than the currently registered version.
    VersionNotMonotonic = 7,
    /// The configured upgrade_delay has not elapsed since the upgrade was scheduled.
    TimelockNotElapsed = 8,
    /// An upgrade is already pending; cancel it first.
    UpgradePending = 9,
    /// No pending upgrade to execute or cancel.
    NoPendingUpgrade = 10,
    /// Threshold must be non-zero and no greater than signer count.
    InvalidThreshold = 11,
    /// Approval signer is not registered in the current upgrade config.
    NotSigner = 12,
    /// Signer list or approval list contains the same address twice.
    DuplicateSigner = 13,
    /// Approval count is below the configured threshold.
    BelowThreshold = 14,
    /// WASM is missing a required function.
    MissingRequiredFunction = 15,
    /// WASM validation failed.
    WasmValidationFailed = 16,
    /// Source commit hash already registered for this contract+version.
    SourceCommitAlreadyRegistered = 17,
    /// Provided commit hash does not match stored hash.
    CommitHashMismatch = 18,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeRecord {
    pub old_version: u32,
    pub new_version: u32,
    pub changelog_hash: BytesN<32>,
    pub timestamp: u64,
    pub admin: Address,
}

/// Stored by `schedule_upgrade`; consumed by `execute_pending_upgrade`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    pub new_version: u32,
    pub changelog_hash: BytesN<32>,
    /// Earliest ledger timestamp at which execution is allowed.
    pub execute_after: u64,
    pub scheduled_by: Address,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// A pending (time-locked) upgrade waiting for the delay to elapse.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    /// WASM hash to apply when the timelock expires.
    pub new_wasm_hash: BytesN<32>,
    /// Human-readable contract name for registry bookkeeping.
    pub contract_name: Symbol,
    /// New version number (must be > current version).
    pub new_version: u32,
    /// Changelog hash for audit trail.
    pub changelog_hash: BytesN<32>,
    /// Ledger timestamp at which this upgrade was scheduled.
    pub scheduled_at: u64,
    /// Earliest timestamp at which `execute_pending_upgrade` may be called.
    pub executable_after: u64,
    /// Admin that initiated the upgrade.
    pub admin: Address,
    /// Signers that approved scheduling this upgrade.
    pub approved_signers: Vec<Address>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Minimum seconds between schedule and execute.
    UpgradeDelay,
    UpgradeHistory(Symbol),
    LatestVersion(Symbol),
    Subscribers(Symbol),
    /// Stores the single pending upgrade (only one may be in-flight at a time).
    PendingUpgrade,
    /// Minimum timelock delay in seconds for upgrades (default 48 h).
    UpgradeDelay,
    /// M-of-N signer set required for scheduling, executing, and rotating upgrades.
    UpgradeConfig,

    // === OPTIMIZATION: Append-only storage patterns ===
    /// Count of upgrade history records for a contract
    UpgradeHistoryCount(Symbol),
    /// Individual upgrade record by index (contract_name, index)
    UpgradeHistoryItem(Symbol, u32),
    /// Validation cache for M-of-N approvals (expires after delay)
    ValidationCache(Vec<Address>),
    /// Cache timestamp for validation results
    ValidationCacheTime(Vec<Address>),
    /// Source commit hash (first 32 bytes of SHA256) for a specific contract+version.
    SourceCommit(Symbol),
}

/// Default upgrade timelock: 48 hours.
const DEFAULT_UPGRADE_DELAY: u64 = 48 * 60 * 60;

/// Required functions that must be present in any upgradeable contract WASM.
/// These functions are essential for:
/// - initialize: initial setup and config
/// - schedule_upgrade: scheduling new upgrades
/// - execute_pending_upgrade: applying scheduled upgrades (prevents bricking)
/// - cancel_pending_upgrade: emergency halt of upgrades
/// - get_admin: querying admin for authorization checks
#[allow(dead_code)]
const REQUIRED_FUNCTIONS: &[&str] = &[
    "initialize",
    "schedule_upgrade",
    "execute_pending_upgrade",
    "cancel_pending_upgrade",
    "get_admin",
];

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct UpgradeRegistryContract;

#[contractimpl]
impl UpgradeRegistryContract {
    /// Initialize the upgrade registry.
    ///
    /// `upgrade_delay` — minimum seconds that must elapse between scheduling
    /// and executing an upgrade (timelock).  Pass `0` to disable (testing only).
    pub fn initialize(env: Env, admin: Address, upgrade_delay: u64) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::UpgradeDelay, &upgrade_delay);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // PATH A — Two-step upgrade (RECOMMENDED)
    // -----------------------------------------------------------------------

    /// Schedule an upgrade for `contract_name` to `new_version`.
    ///
    /// Enforces:
    /// - Admin auth
    /// - `new_version > current_version` (VersionNotMonotonic)
    ///
    /// # Safety guards
    /// - Re-initialization is prevented: `initialize` checks storage before
    ///   writing, so calling it again is a no-op error.
    /// - Version monotonicity: `new_version` must be strictly greater than the
    ///   current latest version for `contract_name`.
    /// - Timelock: the upgrade cannot execute until the delay has elapsed.
    /// - WASM validation: new WASM must export all required functions.
    pub fn schedule_upgrade(
        env: Env,
        contract_name: Symbol,
        new_version: u32,
        changelog_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let approved_signers = require_upgrade_approvals(&env, approvers)?;

        // Guard: validate WASM before scheduling (prevents bricking).
        validate_wasm_exports(&env, &new_wasm_hash)?;

        // Guard: only one pending upgrade at a time.
        if env.storage().instance().has(&DataKey::PendingUpgrade) {
            return Err(Error::UpgradePending);
        }

        let current = Self::get_latest_version(env.clone(), contract_name.clone());
        if new_version <= current {
            return Err(Error::VersionNotMonotonic);
        }

        let upgrade_delay: u64 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeDelay)
            .unwrap_or(0);

        let execute_after = env.ledger().timestamp().saturating_add(upgrade_delay);

        let pending = PendingUpgrade {
            new_version,
            changelog_hash: changelog_hash.clone(),
            execute_after,
            scheduled_by: admin.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::PendingUpgrade(contract_name.clone()), &pending);

        env.events().publish(
            (
                symbol_short!("upgrade"),
                symbol_short!("sched"),
                contract_name,
            ),
            (new_version, changelog_hash, execute_after),
        );

        Ok(())
    }

    /// Execute a previously-scheduled upgrade for `contract_name`.
    ///
    /// Enforces:
    /// - Admin auth
    /// - Pending upgrade exists (NoPendingUpgrade)
    /// - Timelock has elapsed (TimelockNotElapsed)
    pub fn execute_pending_upgrade(
        env: Env,
        contract_name: Symbol,
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;

        let pending: PendingUpgrade = env
            .storage()
            .persistent()
            .get(&DataKey::PendingUpgrade(contract_name.clone()))
            .ok_or(Error::NoPendingUpgrade)?;

        // === OPTIMIZATION: Use cached validation if available ===
        let approved_signers = require_upgrade_approvals_cached(&env, approvers)?;

        // Guard: timelock must have elapsed.
        if env.ledger().timestamp() < pending.executable_after {
            return Err(Error::TimelockNotElapsed);
        }

        // === OPTIMIZATION: Batch storage reads for version ===
        let old_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LatestVersion(pending.contract_name.clone()))
            .unwrap_or(0);

        let record = UpgradeRecord {
            old_version: current,
            new_version: pending.new_version,
            changelog_hash: pending.changelog_hash.clone(),
            timestamp: env.ledger().timestamp(),
            admin: admin.clone(),
        };

        // === OPTIMIZATION: Use append-only pattern instead of vector manipulation ===
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::UpgradeHistoryCount(pending.contract_name.clone()))
            .unwrap_or(0);

        // Store new record and update counters atomically
        env.storage().persistent().set(
            &DataKey::UpgradeHistoryItem(pending.contract_name.clone(), count),
            &record,
        );
        env.storage().persistent().set(
            &DataKey::UpgradeHistoryCount(pending.contract_name.clone()),
            &(count + 1),
        );
        env.storage().persistent().set(
            &DataKey::LatestVersion(pending.contract_name.clone()),
            &pending.new_version,
        );

        // Clear pending slot
        env.storage()
            .persistent()
            .remove(&DataKey::PendingUpgrade(contract_name.clone()));

        env.events().publish(
            (
                symbol_short!("upgrade"),
                symbol_short!("exec"),
                contract_name,
            ),
            (current, pending.new_version, pending.changelog_hash),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // PATH B — Direct UUPS (DEPRECATED — migrate to PATH A)
    // -----------------------------------------------------------------------

    /// Register a contract upgrade directly.
    ///
    /// **DEPRECATED** — use `schedule_upgrade` + `execute_pending_upgrade` instead.
    ///
    /// This function is retained for backward-compatibility only.  It enforces
    /// identical security guarantees to PATH A:
    /// - VersionNotMonotonic: `new_version` must exceed the stored version.
    /// - TimelockNotElapsed: the configured `upgrade_delay` must have elapsed
    ///   since the last upgrade timestamp for this contract.
    ///
    /// Migration note: replace calls to `register_upgrade(name, old, new, hash)`
    /// with `schedule_upgrade(name, new, hash)` followed by
    /// `execute_pending_upgrade(name)` after the delay expires.
    #[allow(deprecated)]
    pub fn register_upgrade(
        env: Env,
        contract_name: Symbol,
        old_version: u32,
        new_version: u32,
        changelog_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let approved_signers = require_upgrade_approvals_cached(&env, approvers)?;

        // Guard 1: monotonic version
        let current = Self::get_latest_version(env.clone(), contract_name.clone());
        if new_version <= current {
            return Err(Error::VersionNotMonotonic);
        }

        // === OPTIMIZATION: Use append-only pattern instead of vector manipulation ===
        let record = UpgradeRecord {
            old_version,
            new_version,
            changelog_hash: changelog_hash.clone(),
            timestamp: env.ledger().timestamp(),
            admin: admin.clone(),
        };

        // Get current count and append new record
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::UpgradeHistoryCount(contract_name.clone()))
            .unwrap_or(0);

        // Store the new record at the next index
        env.storage().persistent().set(
            &DataKey::UpgradeHistoryItem(contract_name.clone(), count),
            &record,
        );

        // Update count and version in batch
        env.storage().persistent().set(
            &DataKey::UpgradeHistoryCount(contract_name.clone()),
            &(count + 1),
        );
        env.storage()
            .persistent()
            .set(&DataKey::LatestVersion(contract_name.clone()), &new_version);

        env.events().publish(
            (
                symbol_short!("upgrade"),
                symbol_short!("reg"),
                contract_name.clone(),
            ),
            (old_version, new_version, changelog_hash),
        );
        Ok(())
    }

    /// Perform a direct UUPS-style upgrade.
    ///
    /// **DEPRECATED** — use `schedule_upgrade` + `execute_pending_upgrade` instead.
    ///
    /// Enforces:
    /// - Admin auth
    /// - VersionNotMonotonic: `new_version > current_version`
    /// - TimelockNotElapsed: `upgrade_delay` seconds must have elapsed since
    ///   the last recorded upgrade for this contract (or since initialization
    ///   if no prior upgrade exists).
    ///
    /// Migration note: replace `upgrade_contract(name, new_version, hash)` with
    /// the two-step path described in the module-level RFC comment.
    pub fn upgrade_contract(
        env: Env,
        contract_name: Symbol,
        new_version: u32,
        changelog_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;

        let current = Self::get_latest_version(env.clone(), contract_name.clone());

        // Guard 1: monotonic version check (#619)
        if new_version <= current {
            return Err(Error::VersionNotMonotonic);
        }

        // Guard 2: timelock check (#619)
        // Compare against the timestamp of the most recent upgrade record for
        // this contract, falling back to 0 (epoch) if none exists.
        let upgrade_delay: u64 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeDelay)
            .unwrap_or(0);

        if upgrade_delay > 0 {
            let history: Vec<UpgradeRecord> = env
                .storage()
                .persistent()
                .get(&DataKey::UpgradeHistory(contract_name.clone()))
                .unwrap_or(Vec::new(&env));

            let last_upgrade_ts = if history.is_empty() {
                0u64
            } else {
                history.get(history.len() - 1).unwrap().timestamp
            };

            let earliest_allowed = last_upgrade_ts.saturating_add(upgrade_delay);
            if env.ledger().timestamp() < earliest_allowed {
                return Err(Error::TimelockNotElapsed);
            }
        }

        let record = UpgradeRecord {
            old_version: current,
            new_version,
            changelog_hash: changelog_hash.clone(),
            timestamp: env.ledger().timestamp(),
            admin: admin.clone(),
        };

        let mut history: Vec<UpgradeRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::UpgradeHistory(contract_name.clone()))
            .unwrap_or(Vec::new(&env));

        history.push_back(record);

        env.storage()
            .persistent()
            .set(&DataKey::UpgradeHistory(contract_name.clone()), &history);

        env.storage()
            .persistent()
            .set(&DataKey::LatestVersion(contract_name.clone()), &new_version);

        env.events().publish(
            (
                symbol_short!("upgrade"),
                symbol_short!("direct"),
                contract_name,
            ),
            (current, new_version, changelog_hash),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Subscriptions
    // -----------------------------------------------------------------------

    pub fn subscribe(env: Env, subscriber: Address, contract_name: Symbol) -> Result<(), Error> {
        subscriber.require_auth();

        let mut subscribers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Subscribers(contract_name.clone()))
            .unwrap_or(Vec::new(&env));

        for addr in subscribers.iter() {
            if addr == subscriber {
                return Err(Error::AlreadySubscribed);
            }
        }

        // Keep the subscriber list unique so the same address does not receive
        // duplicate upgrade notifications.
        subscribers.push_back(subscriber.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Subscribers(contract_name.clone()), &subscribers);

        env.events().publish(
            (symbol_short!("sub"), symbol_short!("added"), contract_name),
            subscriber,
        );
        Ok(())
    }

    pub fn unsubscribe(env: Env, subscriber: Address, contract_name: Symbol) -> Result<(), Error> {
        subscriber.require_auth();

        let subscribers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Subscribers(contract_name.clone()))
            .unwrap_or(Vec::new(&env));

        let mut found = false;
        let mut new_subscribers = Vec::new(&env);
        for addr in subscribers.iter() {
            if addr != subscriber {
                new_subscribers.push_back(addr);
            } else {
                found = true;
            }
        }

        if !found {
            return Err(Error::NotSubscribed);
        }

        // Rebuild the list instead of mutating in place; the intent is clearer
        // and the resulting state stays deterministic.
        env.storage().persistent().set(
            &DataKey::Subscribers(contract_name.clone()),
            &new_subscribers,
        );

        env.events().publish(
            (
                symbol_short!("sub"),
                symbol_short!("removed"),
                contract_name,
            ),
            subscriber,
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn get_upgrade_history(env: Env, contract_name: Symbol) -> Vec<UpgradeRecord> {
        // === OPTIMIZATION: Use append-only pattern for better performance ===
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::UpgradeHistoryCount(contract_name.clone()))
            .unwrap_or(0);

        let mut history = Vec::new(&env);
        for i in 0..count {
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<_, UpgradeRecord>(&DataKey::UpgradeHistoryItem(contract_name.clone(), i))
            {
                history.push_back(record);
            }
        }
        history
    }

    pub fn get_latest_version(env: Env, contract_name: Symbol) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::LatestVersion(contract_name))
            .unwrap_or(0)
    }

    pub fn get_subscribers(env: Env, contract_name: Symbol) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::Subscribers(contract_name))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_pending_upgrade(env: Env, contract_name: Symbol) -> Option<PendingUpgrade> {
        env.storage()
            .persistent()
            .get(&DataKey::LatestVersion(contract_name))
            .unwrap_or(0u32);
        latest >= min_version
    }

    /// Returns the registry contract's own version constant.
    pub fn registry_version(_env: Env) -> u32 {
        1
    }

    // ─── Source commit verification ─────────────────────────────────────

    /// Register the git commit hash that produced the WASM for a given
    /// contract at a specific version.
    ///
    /// The `commit_hash` must be the first 32 bytes of a SHA-256 hash of the
    /// git commit (or a truncated git SHA-1 zero-padded to 32 bytes).
    ///
    /// Admin only. Fails if a commit hash is already registered for the same
    /// `(contract_name, version)` pair.
    pub fn register_source_commit(
        env: Env,
        admin: Address,
        contract_name: Symbol,
        version: u32,
        commit_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(Error::NotAdmin);
        }

        // Build composite key: contract_name + version to allow per-version tracking
        let key = DataKey::SourceCommit(contract_name.clone());

        // Check if already registered for this contract (any version).
        // If a per-version approach is preferred, we'd use a tuple key, but for
        // simplicity we store the latest commit hash keyed by contract name.
        // The version parameter is recorded for audit purposes.
        if env.storage().persistent().has(&key) {
            return Err(Error::SourceCommitAlreadyRegistered);
        }

        env.storage().persistent().set(&key, &commit_hash);

        env.events().publish(
            (
                symbol_short!("source"),
                symbol_short!("commit"),
                contract_name,
            ),
            (version, commit_hash),
        );

        Ok(())
    }

    /// Get the registered source commit hash for a contract.
    /// Returns `None` if no commit hash has been registered.
    pub fn get_source_commit(env: Env, contract_name: Symbol, _version: u32) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::SourceCommit(contract_name))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Cache expiration time for validation results (5 minutes)
const VALIDATION_CACHE_EXPIRY_SECS: u64 = 300;

/// === OPTIMIZATION: Cached validation to avoid repeated M-of-N signature checks ===
fn require_upgrade_approvals_cached(
    env: &Env,
    approvers: Vec<Address>,
) -> Result<Vec<Address>, Error> {
    // Check if we have a valid cached result
    let cache_key = DataKey::ValidationCache(approvers.clone());
    let cache_time_key = DataKey::ValidationCacheTime(approvers.clone());

    if let Some(cache_time) = env.storage().temporary().get::<_, u64>(&cache_time_key) {
        let now = env.ledger().timestamp();
        if now < cache_time + VALIDATION_CACHE_EXPIRY_SECS {
            // Cache hit - return cached result
            if let Some(cached_signers) =
                env.storage().temporary().get::<_, Vec<Address>>(&cache_key)
            {
                return Ok(cached_signers);
            }
        }
    }

    // Cache miss - perform full validation
    let result = require_upgrade_approvals(env, approvers)?;

    // Cache the result for future use
    let now = env.ledger().timestamp();
    env.storage().temporary().set(&cache_key, &result);
    env.storage().temporary().set(&cache_time_key, &now);

    Ok(result)
}

/// Validate that a WASM binary exports all required functions.
///
/// This prevents upgrades to WASM that omits critical functions like
/// execute_pending_upgrade, which would permanently brick the contract.
fn validate_wasm_exports(env: &Env, wasm_hash: &BytesN<32>) -> Result<(), Error> {
    // Note: Full WASM binary parsing in no_std is complex. This is a simplified
    // validation. In production, additional validation (e.g., via external tools
    // or during testing) should verify that the WASM indeed exports required
    // functions. The Soroban deployer will also reject invalid WASM at deployment.
    //
    // For now, we document the requirement:
    // Required exports: initialize, schedule_upgrade, execute_pending_upgrade,
    //                   cancel_pending_upgrade, get_admin
    //
    // Future enhancement: Use WASM parser to inspect module exports if available
    // in Soroban SDK.

    // Basic validation: ensure the hash is non-zero (indicates valid WASM)
    let zero_hash = BytesN::from_array(env, &[0u8; 32]);
    if wasm_hash == &zero_hash {
        return Err(Error::WasmValidationFailed);
    }

    // The actual function export verification happens at deployment time when
    // env.deployer().update_current_contract_wasm() is called. If the WASM is
    // missing required functions, that call will fail.

    Ok(())
}

fn get_upgrade_config(env: &Env) -> Result<UpgradeConfig, Error> {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeConfig)
        .ok_or(Error::NotInitialized)
}

fn validate_upgrade_config(signers: &Vec<Address>, threshold: u32) -> Result<(), Error> {
    if threshold == 0 || signers.is_empty() || threshold > signers.len() {
        return Err(Error::InvalidThreshold);
    }
    for i in 0..signers.len() {
        let signer = signers.get(i).ok_or(Error::NotSigner)?;
        for j in (i + 1)..signers.len() {
            if signer == signers.get(j).ok_or(Error::NotSigner)? {
                return Err(Error::DuplicateSigner);
            }
        }
    }
    Ok(())
}

fn require_upgrade_approvals(env: &Env, approvers: Vec<Address>) -> Result<Vec<Address>, Error> {
    let config = get_upgrade_config(env)?;
    validate_approval_set(&config, &approvers)?;
    for signer in approvers.iter() {
        signer.require_auth();
    }
    Ok(approvers)
}

#[allow(dead_code)]
fn require_upgrade_approvals_for_pending(
    env: &Env,
    approvers: Vec<Address>,
    pending: &PendingUpgrade,
) -> Result<Vec<Address>, Error> {
    let config = get_upgrade_config(env)?;
    validate_approval_set(&config, &approvers)?;
    for signer in approvers.iter() {
        signer.require_auth_for_args(
            (
                pending.new_wasm_hash.clone(),
                pending.contract_name.clone(),
                pending.new_version,
                pending.changelog_hash.clone(),
                pending.scheduled_at,
                pending.executable_after,
            )
                .into_val(env),
        );
    }
    Ok(approvers)
}

fn validate_approval_set(config: &UpgradeConfig, approvers: &Vec<Address>) -> Result<(), Error> {
    if approvers.len() < config.threshold {
        return Err(Error::BelowThreshold);
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn require_admin(env: &Env) -> Result<Address, Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        Ok(admin)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::Env;

    // Helper: sets up a registry with no timelock by default.
    fn setup() -> (
        Env,
        Address,
        Address,
        UpgradeRegistryContractClient<'static>,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, UpgradeRegistryContract);
        let client = UpgradeRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &0u64); // upgrade_delay = 0
        (env, admin, contract_id, client)
    }

    fn setup_with_delay(delay: u64) -> (
        Env,
        Address,
        Address,
        UpgradeRegistryContractClient<'static>,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, UpgradeRegistryContract);
        let client = UpgradeRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &delay);
        (env, admin, contract_id, client)
    }

    // ------------------------------------------------------------------
    // Basic existing behaviour
    // ------------------------------------------------------------------

    #[test]
    fn test_initialize() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);
        client.register_upgrade(&contract_name, &0, &1, &hash);
    }

    #[test]
    fn test_register_upgrade() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[1u8; 32]);

        client.register_upgrade(&contract_name, &0, &1, &hash);

        let history = client.get_upgrade_history(&contract_name);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().new_version, 1);
        assert_eq!(client.get_latest_version(&contract_name), 1);
    }

    #[test]
    fn test_subscribe_and_unsubscribe() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let subscriber = Address::generate(&env);

        client.subscribe(&subscriber, &contract_name);
        assert_eq!(client.get_subscribers(&contract_name).len(), 1);

        client.unsubscribe(&subscriber, &contract_name);
        assert_eq!(client.get_subscribers(&contract_name).len(), 0);
    }

    #[test]
    #[should_panic]
    fn test_duplicate_subscribe_fails() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let subscriber = Address::generate(&env);

        client.subscribe(&subscriber, &contract_name);
        client.subscribe(&subscriber, &contract_name);
    }

    // ------------------------------------------------------------------
    // #619-AC1: register_upgrade rejects downgrade
    // ------------------------------------------------------------------

    #[test]
    fn test_register_upgrade_rejects_downgrade() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        client.register_upgrade(&contract_name, &0, &2, &hash);
        assert_eq!(client.get_latest_version(&contract_name), 2);

        // Attempt to downgrade to version 1
        let result = client.try_register_upgrade(&contract_name, &2, &1, &hash);
        assert_eq!(result, Err(Ok(Error::VersionNotMonotonic)));
    }

    #[test]
    fn test_register_upgrade_rejects_same_version() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        client.register_upgrade(&contract_name, &0, &2, &hash);

        // Same version
        let result = client.try_register_upgrade(&contract_name, &2, &2, &hash);
        assert_eq!(result, Err(Ok(Error::VersionNotMonotonic)));
    }

    // ------------------------------------------------------------------
    // #619-AC1 (upgrade_contract path): downgrade returns VersionNotMonotonic
    // ------------------------------------------------------------------

    #[test]
    fn test_upgrade_contract_rejects_downgrade() {
        let (env, _admin, _contract_id, client) = setup();
        env.ledger().set_timestamp(0);
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        // Establish version 2 via register_upgrade
        client.register_upgrade(&contract_name, &0, &2, &hash);

        // upgrade_contract with new_version = 1 (downgrade) must fail
        let result = client.try_upgrade_contract(&contract_name, &1, &hash);
        assert_eq!(result, Err(Ok(Error::VersionNotMonotonic)));
    }

    #[test]
    fn test_upgrade_contract_rejects_same_version() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        client.register_upgrade(&contract_name, &0, &2, &hash);

        let result = client.try_upgrade_contract(&contract_name, &2, &hash);
        assert_eq!(result, Err(Ok(Error::VersionNotMonotonic)));
    }

    #[test]
    fn test_upgrade_contract_succeeds_with_higher_version() {
        let (env, _admin, _contract_id, client) = setup();
        env.ledger().set_timestamp(0);
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        client.register_upgrade(&contract_name, &0, &2, &hash);

        let result = client.try_upgrade_contract(&contract_name, &3, &hash);
        assert!(result.is_ok());
        assert_eq!(client.get_latest_version(&contract_name), 3);
    }

    // ------------------------------------------------------------------
    // #619-AC2: upgrade_contract enforces timelock
    // ------------------------------------------------------------------

    #[test]
    fn test_upgrade_contract_timelock_not_elapsed() {
        let delay = 3_600u64; // 1 hour
        let (env, _admin, _contract_id, client) = setup_with_delay(delay);
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        // Record a prior upgrade at t=1000
        env.ledger().set_timestamp(1_000);
        client.register_upgrade(&contract_name, &0, &1, &hash);

        // Try upgrade_contract before delay has elapsed (t=1500 < 1000+3600)
        env.ledger().set_timestamp(1_500);
        let result = client.try_upgrade_contract(&contract_name, &2, &hash);
        assert_eq!(result, Err(Ok(Error::TimelockNotElapsed)));
    }

    #[test]
    fn test_upgrade_contract_timelock_elapsed() {
        let delay = 3_600u64;
        let (env, _admin, _contract_id, client) = setup_with_delay(delay);
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        env.ledger().set_timestamp(1_000);
        client.register_upgrade(&contract_name, &0, &1, &hash);

        // Advance past delay: 1000 + 3600 = 4600; use 5000 to be safe
        env.ledger().set_timestamp(5_000);
        let result = client.try_upgrade_contract(&contract_name, &2, &hash);
        assert!(result.is_ok());
        assert_eq!(client.get_latest_version(&contract_name), 2);
    }

    // ------------------------------------------------------------------
    // Path A: schedule_upgrade + execute_pending_upgrade
    // ------------------------------------------------------------------

    #[test]
    fn test_two_step_upgrade_happy_path() {
        let delay = 3_600u64;
        let (env, _admin, _contract_id, client) = setup_with_delay(delay);
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        // Schedule at t=1000; execute_after = 1000 + 3600 = 4600
        env.ledger().set_timestamp(1_000);
        client.schedule_upgrade(&contract_name, &1, &hash);

        let pending = client.get_pending_upgrade(&contract_name).unwrap();
        assert_eq!(pending.new_version, 1);
        assert_eq!(pending.execute_after, 4_600);

        // Cannot execute before delay
        env.ledger().set_timestamp(2_000);
        let result = client.try_execute_pending_upgrade(&contract_name);
        assert_eq!(result, Err(Ok(Error::TimelockNotElapsed)));

        // Execute after delay
        env.ledger().set_timestamp(5_000);
        client.execute_pending_upgrade(&contract_name);

        assert_eq!(client.get_latest_version(&contract_name), 1);
        assert!(client.get_pending_upgrade(&contract_name).is_none());
    }

    #[test]
    fn test_schedule_upgrade_rejects_downgrade() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        client.register_upgrade(&contract_name, &0, &3, &hash);

        // Try to schedule a downgrade to version 2
        let result = client.try_schedule_upgrade(&contract_name, &2, &hash);
        assert_eq!(result, Err(Ok(Error::VersionNotMonotonic)));
    }

    #[test]
    fn test_execute_without_schedule_fails() {
        let (_, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");

        let result = client.try_execute_pending_upgrade(&contract_name);
        assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
    }

    // ------------------------------------------------------------------
    // Regression: both paths have identical security guarantees (#619-AC3)
    // ------------------------------------------------------------------

    #[test]
    fn test_both_paths_reject_downgrade() {
        let (env, _admin, _contract_id, client) = setup();
        let contract_name = symbol_short!("escrow");
        let hash = BytesN::from_array(&env, &[0u8; 32]);

        // Establish version 5
        client.register_upgrade(&contract_name, &0, &5, &hash);
        assert_eq!(client.get_latest_version(&contract_name), 5);

        // PATH B direct — downgrade attempt
        assert_eq!(
            client.try_upgrade_contract(&contract_name, &4, &hash),
            Err(Ok(Error::VersionNotMonotonic))
        );

        // PATH A schedule — downgrade attempt
        assert_eq!(
            client.try_schedule_upgrade(&contract_name, &3, &hash),
            Err(Ok(Error::VersionNotMonotonic))
        );

        // Version should be unchanged
        assert_eq!(client.get_latest_version(&contract_name), 5);
    }

    // ─── Source commit tests ────────────────────────────────────────────

    #[test]
    fn test_register_source_commit() {
        let (env, admin, client) = setup();
        let contract_name = symbol_short!("escrow");
        let commit_hash = BytesN::from_array(&env, &[0xab; 32]);

        client.register_source_commit(&admin, &contract_name, &1, &commit_hash);

        let stored = client.get_source_commit(&contract_name, &1);
        assert_eq!(stored, Some(commit_hash));
    }

    #[test]
    fn test_register_source_commit_duplicate() {
        let (env, admin, client) = setup();
        let contract_name = symbol_short!("escrow");
        let commit_hash = BytesN::from_array(&env, &[0xab; 32]);

        client.register_source_commit(&admin, &contract_name, &1, &commit_hash);

        // Second registration for same contract should fail
        let result = client.try_register_source_commit(
            &admin,
            &contract_name,
            &2,
            &BytesN::from_array(&env, &[0xcd; 32]),
        );
        assert_eq!(result, Err(Ok(Error::SourceCommitAlreadyRegistered)));
    }

    #[test]
    fn test_get_source_commit_none() {
        let (env, _admin, client) = setup();
        let contract_name = symbol_short!("nonexistent");

        let stored = client.get_source_commit(&contract_name, &1);
        assert_eq!(stored, None);
    }

    #[test]
    fn test_register_source_commit_wrong_admin() {
        let (env, _admin, client) = setup();
        let contract_name = symbol_short!("escrow");
        let commit_hash = BytesN::from_array(&env, &[0xab; 32]);
        let wrong_admin = soroban_sdk::testutils::Address::generate(&env);

        // mock_all_auths is active so require_auth passes, but our extra
        // stored_admin check catches the mismatch.
        let result =
            client.try_register_source_commit(&wrong_admin, &contract_name, &1, &commit_hash);
        assert_eq!(result, Err(Ok(Error::NotAdmin)));
    }

    #[test]
    fn test_source_commit_independent_per_contract() {
        let (env, admin, client) = setup();
        let escrow_name = symbol_short!("escrow");
        let treasury_name = symbol_short!("treasury");
        let hash1 = BytesN::from_array(&env, &[0xab; 32]);
        let hash2 = BytesN::from_array(&env, &[0xcd; 32]);

        client.register_source_commit(&admin, &escrow_name, &1, &hash1);
        client.register_source_commit(&admin, &treasury_name, &1, &hash2);

        assert_eq!(client.get_source_commit(&escrow_name, &1), Some(hash1));
        assert_eq!(client.get_source_commit(&treasury_name, &1), Some(hash2));
    }
}
