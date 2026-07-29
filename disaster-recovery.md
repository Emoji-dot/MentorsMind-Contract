# MentorsMind Protocol — Disaster Recovery Runbook

> **Classification:** Internal Operations — Security Sensitive  
> **Last Updated:** 2026-07-29  
> **Owner:** Protocol Security Team

---

## Table of Contents

1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Emergency Signer Registry](#3-emergency-signer-registry)
4. [Pre-Upgrade Procedure (Mandatory)](#4-pre-upgrade-procedure-mandatory)
5. [Post-Upgrade Verification](#5-post-upgrade-verification)
6. [Rollback Decision Tree](#6-rollback-decision-tree)
7. [Rollback Execution Procedure](#7-rollback-execution-procedure)
8. [Snapshot Retention Policy](#8-snapshot-retention-policy)
9. [Contract-Specific Reference](#9-contract-specific-reference)
10. [Security Considerations](#10-security-considerations)
11. [Drill Schedule](#11-drill-schedule)
12. [Glossary](#12-glossary)

---

## 1. Overview

The MentorsMind protocol implements on-chain disaster recovery through three
complementary mechanisms available in the **Escrow**, **Multisig Admin**, and
**Staking** contracts:

| Function | Purpose |
|----------|---------|
| `snapshot_state(env, admin, snapshot_id)` | Capture all critical state before an upgrade |
| `verify_post_upgrade_state(env, snapshot_id)` | Field-level integrity check after an upgrade |
| `rollback_to_snapshot(env, proposal_id)` | Restore pre-upgrade WASM + state after 4-of-7 multi-sig |

### When to Use This Runbook

- **Before any production contract upgrade** — mandatory snapshot step.
- **Immediately after any production upgrade** — run post-upgrade verification.
- **When verification detects corruption** — trigger the rollback procedure.
- **During quarterly DR drills** — test the full flow end-to-end on testnet.

---

## 2. Architecture

```
                 ┌─────────────────────────────────────────┐
                 │           Protocol Upgrade Flow          │
                 └─────────────────────────────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │  Step 1: snapshot_state()        │
                    │  (admin, pre-upgrade)            │
                    └────────────────┬────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │  Step 2: upgrade_contract()      │
                    │  (deploy new WASM)               │
                    └────────────────┬────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │  Step 3: verify_post_upgrade_    │
                    │  state()                         │
                    └────┬───────────────────────┬────┘
                         │                       │
                   ✅ Clean                ❌ Mismatches
                         │                       │
                   Continue              ┌───────▼──────────┐
                                         │  Rollback Flow   │
                                         │  (4-of-7 multisig)│
                                         └──────────────────┘
```

### Storage Layout

Each protected contract uses isolated storage key namespaces:

**Escrow Contract:**
```
DataKey::Snapshot(u32)              → Vec<EscrowRecord>
DataKey::SnapshotMetadata(u32)      → SnapshotMeta
DataKey::SnapshotIndex              → Vec<u32>  (rolling, max 3)
DataKey::EmergencySigners           → Vec<Address>  (7 signers)
DataKey::RollbackProposal(u32)      → RollbackProposal
DataKey::RollbackApproval(u32, Address) → bool
DataKey::RollbackProposalCount      → u32
```

**Multisig Admin Contract:**
```
DataKey::GovSnapshot(u32)           → GovConfigSnapshot
DataKey::GovSnapshotMeta(u32)       → SnapshotMeta
DataKey::GovSnapshotIndex           → Vec<u32>
DataKey::GovEmergencySigners        → Vec<Address>
DataKey::GovRollbackProposal(u32)   → RollbackProposal
DataKey::GovRollbackApproval(u32, Address) → bool
DataKey::GovRollbackProposalCount   → u32
```

**Staking Contract:**
```
DataKey::StakeSnapshot(u32)         → Vec<StakeSnapshot>
DataKey::StakeSnapshotMeta(u32)     → SnapshotMeta
DataKey::StakeSnapshotIndex         → Vec<u32>
DataKey::StakeEmergencySigners      → Vec<Address>
DataKey::StakeRollbackProposal(u32) → RollbackProposal
DataKey::StakeRollbackApproval(u32, Address) → bool
DataKey::StakeRollbackProposalCount → u32
```

---

## 3. Emergency Signer Registry

> ⚠️ **IMPORTANT**: Emergency signers are **separate** from regular multisig
> signers. They form a dedicated break-glass authority. The registry must be
> configured once during initial deployment and updated whenever personnel
> change.

### Configuration (one-time per contract)

```bash
# Escrow contract
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $ADMIN_SECRET \
  --network mainnet \
  -- set_emergency_signers \
  --admin $ADMIN_ADDR \
  --signers "[$SIGNER_1,$SIGNER_2,$SIGNER_3,$SIGNER_4,$SIGNER_5,$SIGNER_6,$SIGNER_7]"

# Multisig Admin contract
stellar contract invoke \
  --id $MULTISIG_CONTRACT_ID \
  --source $SIGNER_1_SECRET \
  --network mainnet \
  -- set_emergency_signers \
  --caller $SIGNER_1_ADDR \
  --signers "[$SIGNER_1,$SIGNER_2,$SIGNER_3,$SIGNER_4,$SIGNER_5,$SIGNER_6,$SIGNER_7]"

# Staking contract
stellar contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source $ADMIN_SECRET \
  --network mainnet \
  -- set_emergency_signers \
  --admin $ADMIN_ADDR \
  --signers "[$SIGNER_1,$SIGNER_2,$SIGNER_3,$SIGNER_4,$SIGNER_5,$SIGNER_6,$SIGNER_7]"
```

### Required Quorum

**4-of-7** emergency signers must approve a rollback proposal before it can be
executed. This threshold is enforced on-chain by the `EMERGENCY_THRESHOLD`
constant (`= 4`) in `shared/src/disaster_recovery.rs`.

---

## 4. Pre-Upgrade Procedure (Mandatory)

> ⛔ **DO NOT upgrade any protected contract without first completing all steps
> in this section.** Skipping the snapshot means rollback is impossible.

### Step 4.1 — Pause the Contract (Recommended)

```bash
# Pause escrow (if pause_guardian is configured)
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $ADMIN_SECRET \
  -- pause
```

### Step 4.2 — Choose a Snapshot ID

Snapshot IDs are arbitrary `u32` values. Use a timestamp-based ID for
traceability:

```bash
export SNAPSHOT_ID=$(date +%s | cut -c1-8)  # e.g. 17538012
echo "Using snapshot ID: $SNAPSHOT_ID"
```

> **Note:** Only 3 snapshots are retained. If you create a 4th, the oldest is
> deleted automatically. Use IDs in ascending order.

### Step 4.3 — Record the Current WASM Hash

```bash
# Record the current WASM hash BEFORE upgrading — you will need it for rollback.
export OLD_WASM_HASH=$(stellar contract inspect \
  --id $ESCROW_CONTRACT_ID \
  --network mainnet \
  | jq -r '.wasm_hash')
echo "Pre-upgrade WASM hash: $OLD_WASM_HASH"
# SAVE THIS VALUE IN YOUR INCIDENT RUNBOOK LOG.
```

### Step 4.4 — Take Snapshot on All 3 Contracts

```bash
# Escrow
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $ADMIN_SECRET \
  --network mainnet \
  -- snapshot_state \
  --admin $ADMIN_ADDR \
  --snapshot_id $SNAPSHOT_ID

# Multisig Admin (any registered signer can call)
stellar contract invoke \
  --id $MULTISIG_CONTRACT_ID \
  --source $SIGNER_1_SECRET \
  --network mainnet \
  -- snapshot_state \
  --caller $SIGNER_1_ADDR \
  --snapshot_id $SNAPSHOT_ID

# Staking
stellar contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source $ADMIN_SECRET \
  --network mainnet \
  -- snapshot_state \
  --admin $ADMIN_ADDR \
  --snapshot_id $SNAPSHOT_ID
```

### Step 4.5 — Confirm Snapshot Metadata

```bash
# Verify the snapshot was persisted
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  -- get_snapshot_metadata \
  --snapshot_id $SNAPSHOT_ID

# Check the rolling index
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  -- get_snapshot_index
```

Expected output should include `created_at`, `block_height`, `contract_version`,
`record_count`, and a non-zero `checksum`.

### Step 4.6 — Upload the New WASM to the Network

```bash
export NEW_WASM_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/mentorminds_escrow.wasm \
  --source $DEPLOYER_SECRET \
  --network mainnet)
echo "New WASM hash: $NEW_WASM_HASH"
```

### Step 4.7 — Upgrade the Contract

```bash
stellar contract upgrade \
  --id $ESCROW_CONTRACT_ID \
  --source $ADMIN_SECRET \
  --network mainnet \
  --wasm-hash $NEW_WASM_HASH
```

---

## 5. Post-Upgrade Verification

Run **immediately** after every upgrade. If any mismatches are detected, halt
all user operations and initiate the rollback procedure.

```bash
# Escrow contract
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --network mainnet \
  -- verify_post_upgrade_state \
  --snapshot_id $SNAPSHOT_ID

# Multisig Admin contract
stellar contract invoke \
  --id $MULTISIG_CONTRACT_ID \
  --network mainnet \
  -- verify_post_upgrade_state \
  --snapshot_id $SNAPSHOT_ID

# Staking contract
stellar contract invoke \
  --id $STAKING_CONTRACT_ID \
  --network mainnet \
  -- verify_post_upgrade_state \
  --snapshot_id $SNAPSHOT_ID
```

### Interpreting Results

| `fields_checked` | `mismatches` | Action |
|---|---|---|
| > 0 | empty (`[]`) | ✅ State intact — proceed normally |
| > 0 | non-empty | ❌ Corruption detected — initiate rollback |
| 0 | any | ⚠️ Snapshot missing or empty — investigate |

### Sample Healthy Output

```json
{
  "fields_checked": 11023,
  "mismatches": []
}
```

### Sample Corrupted Output

```json
{
  "fields_checked": 11023,
  "mismatches": [
    "EscrowRecord.amount mismatch",
    "EscrowRecord.status mismatch"
  ]
}
```

---

## 6. Rollback Decision Tree

```
Verification reports mismatches?
│
├─ YES ─► How many escrows are affected?
│         ├─ < 10 (isolated)  ─► Manual fix via admin functions may suffice.
│         │                       Consult Security Team before proceeding.
│         └─ ≥ 10 (widespread) ─► INITIATE FULL ROLLBACK (Section 7)
│
└─ NO ──► Is the new version behaving correctly?
          ├─ YES ─► No action required.
          └─ NO ──► Is there active fund risk?
                    ├─ YES ─► PAUSE contract immediately, then INITIATE ROLLBACK
                    └─ NO ──► File bug, plan hotfix upgrade
```

---

## 7. Rollback Execution Procedure

> ⚠️ **PRE-CONDITION**: The pre-upgrade WASM binary must be re-uploaded to the
> network (`soroban contract install`) if it has expired from the ledger.
> The hash from Step 4.3 is required.

### Step 7.1 — Re-upload the Old WASM (if needed)

```bash
# Check if old WASM is still on the network
stellar ledger get-entry --type ContractCode --key $OLD_WASM_HASH || \
  stellar contract install \
    --wasm target/wasm32-unknown-unknown/release/mentorminds_escrow_old.wasm \
    --source $DEPLOYER_SECRET \
    --network mainnet
```

### Step 7.2 — Propose a Rollback (Emergency Signer #1)

```bash
# For escrow contract
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $EMERGENCY_SIGNER_1_SECRET \
  --network mainnet \
  -- propose_rollback \
  --proposer $EMERGENCY_SIGNER_1_ADDR \
  --snapshot_id $SNAPSHOT_ID \
  --old_wasm_hash $OLD_WASM_HASH

# Save the returned proposal ID
export ROLLBACK_PROPOSAL_ID=<returned_id>
```

> Signer 1's approval is **automatically counted** as the first vote.

### Step 7.3 — Gather 3 Additional Approvals (Signers 2–4)

Each signer must independently run this command using their own key:

```bash
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $EMERGENCY_SIGNER_N_SECRET \
  --network mainnet \
  -- approve_rollback \
  --signer $EMERGENCY_SIGNER_N_ADDR \
  --proposal_id $ROLLBACK_PROPOSAL_ID
```

Verify approval count is accumulating:

```bash
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  -- get_rollback_proposal \
  --proposal_id $ROLLBACK_PROPOSAL_ID
# Check: approval_count should equal 4 before proceeding
```

### Step 7.4 — Execute the Rollback

```bash
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $DEPLOYER_SECRET \
  --network mainnet \
  -- rollback_to_snapshot \
  --proposal_id $ROLLBACK_PROPOSAL_ID
```

This single transaction:
1. Validates 4-of-7 approvals on-chain
2. Restores all `EscrowRecord` entries from the snapshot
3. Re-applies the pre-upgrade WASM binary

### Step 7.5 — Repeat for Other Contracts

Repeat Steps 7.2–7.4 for `MultisigAdminContract` (using `propose_emergency_rollback` /
`approve_emergency_rollback` / `execute_emergency_rollback`) and `StakingContract`
(using `propose_rollback` / `approve_rollback` / `rollback_to_snapshot`) as needed.

### Step 7.6 — Post-Rollback Verification

```bash
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  -- verify_post_upgrade_state \
  --snapshot_id $SNAPSHOT_ID
# Expected: mismatches = []
```

### Step 7.7 — Unpause (if paused)

```bash
stellar contract invoke \
  --id $ESCROW_CONTRACT_ID \
  --source $ADMIN_SECRET \
  -- unpause
```

---

## 8. Snapshot Retention Policy

### Rolling Window

A maximum of **3 snapshots** are retained per contract at any time. When a
4th snapshot is created, the **oldest** snapshot (and its metadata) is
automatically deleted from persistent storage.

```
Window state after 4 snapshot calls:
  Before: [snap_1, snap_2, snap_3]   ← window full
  After:  [snap_2, snap_3, snap_4]   ← snap_1 evicted
```

### Consequences

- If you create more than 3 snapshots, the oldest rollback target is lost.
- Always verify the snapshot index before upgrading:
  ```bash
  stellar contract invoke --id $ESCROW_CONTRACT_ID -- get_snapshot_index
  ```
- In an active rollback situation, do **not** create new snapshots — it may
  evict the snapshot you need to roll back to.

### TTL

Snapshot data uses `ESCROW_TTL_THRESHOLD = 500,000` / `ESCROW_TTL_BUMP = 1,000,000`
ledgers (~57 days). If a contract enters ledger archival, snapshot data must be
restored via TTL extension before a rollback can be executed.

---

## 9. Contract-Specific Reference

### 9.1 Escrow Contract (`escrow/src/lib.rs`)

| Function | Auth | Description |
|----------|------|-------------|
| `set_emergency_signers(admin, signers)` | Admin | Register 7 emergency signers |
| `snapshot_state(admin, snapshot_id)` | Admin | Capture all `EscrowRecord`s |
| `verify_post_upgrade_state(snapshot_id)` | Public | Field-level integrity check |
| `propose_rollback(proposer, snapshot_id, old_wasm_hash)` | Emergency signer | Open rollback proposal |
| `approve_rollback(signer, proposal_id)` | Emergency signer | Vote to approve |
| `rollback_to_snapshot(proposal_id)` | Public (enforces 4-of-7 on-chain) | Execute rollback |
| `get_snapshot_metadata(snapshot_id)` | Public | View snapshot metadata |
| `get_snapshot_index()` | Public | View retained snapshot IDs |
| `get_rollback_proposal(proposal_id)` | Public | View proposal state |

**Fields verified** by `verify_post_upgrade_state`:
- `EscrowCount` (from metadata)
- Per record: `id`, `mentor`, `learner`, `amount`, `status`, `token_address`,
  `platform_fee`, `net_amount`, `session_end_time`, `total_sessions`,
  `sessions_completed`

### 9.2 Multisig Admin Contract (`contracts/multisig_admin/src/lib.rs`)

| Function | Auth | Description |
|----------|------|-------------|
| `set_emergency_signers(caller, signers)` | Regular signer | Register 7 emergency signers |
| `snapshot_state(caller, snapshot_id)` | Regular signer | Capture governance config |
| `verify_post_upgrade_state(snapshot_id)` | Public | Check Threshold/SignerCount/ProposalCount |
| `propose_emergency_rollback(proposer, snapshot_id, old_wasm_hash)` | Emergency signer | Open rollback |
| `approve_emergency_rollback(signer, proposal_id)` | Emergency signer | Vote |
| `execute_emergency_rollback(proposal_id)` | Public (enforces 4-of-7) | Execute rollback |
| `get_gov_snapshot_meta(snapshot_id)` | Public | View metadata |
| `get_gov_snapshot_index()` | Public | View retained IDs |
| `get_gov_rollback_proposal(proposal_id)` | Public | View proposal |

**Fields verified**: `Threshold`, `SignerCount`, `ProposalCount`

### 9.3 Staking Contract (`contracts/staking/src/lib.rs`)

| Function | Auth | Description |
|----------|------|-------------|
| `set_emergency_signers(admin, signers)` | Admin | Register 7 emergency signers |
| `snapshot_state(admin, snapshot_id)` | Admin | Capture all `StakeRecord`s |
| `verify_post_upgrade_state(snapshot_id)` | Public | Check staker count + per-staker fields |
| `propose_rollback(proposer, snapshot_id, old_wasm_hash)` | Emergency signer | Open rollback |
| `approve_rollback(signer, proposal_id)` | Emergency signer | Vote |
| `rollback_to_snapshot(proposal_id)` | Public (enforces 4-of-7) | Execute rollback |
| `get_stake_snapshot_meta(snapshot_id)` | Public | View metadata |
| `get_stake_snapshot_index()` | Public | View retained IDs |
| `get_stake_rollback_proposal(proposal_id)` | Public | View proposal |

**Fields verified**: `StakerCount`, per-staker `amount`, `tier`, `staked_at`,
`unlock_at`

---

## 10. Security Considerations

### Threat Model

| Threat | Mitigation |
|--------|-----------|
| Malicious upgrade corrupts state | Pre-upgrade snapshot + post-upgrade verification |
| Single signer compromised | 4-of-7 threshold prevents unilateral rollback |
| Attacker proposes fake rollback | Must be in emergency signer registry + require_auth |
| Snapshot data tampered on-chain | SHA-256 checksum in `SnapshotMeta.checksum` |
| Old WASM unavailable for rollback | Operator must retain WASM binary and pre-upload |
| Snapshot evicted from window | Max 3 retained; operator must not create excess snapshots during incident |

### Emergency Signer Key Management

- Emergency signer private keys must be stored in **hardware security modules
  (HSMs)** or **multi-party computation (MPC) wallets**.
- No single person should control more than 1 of the 7 keys.
- Key rotation requires a new `set_emergency_signers` call with the updated list.

### Rollback Limitations

1. **WASM rollback is atomic with state restore** — there is no way to roll back
   state without also rolling back the WASM binary.
2. **Rollback does not reverse token transfers** — funds already sent out during
   the corrupted upgrade cannot be reclaimed by rollback alone.
3. **New escrows created after the snapshot are lost** — any escrow created
   between snapshot and rollback will have its state overwritten.

---

## 11. Drill Schedule

| Frequency | Scope | Environment |
|-----------|-------|-------------|
| Monthly | Snapshot + verify (no rollback) | Testnet |
| Quarterly | Full flow including rollback | Testnet |
| Semi-annually | Emergency signer key rotation | Testnet |
| Per major upgrade | Mandatory snapshot before production upgrade | Mainnet |

### Drill Checklist

- [ ] All 7 emergency signers can authenticate successfully
- [ ] `snapshot_state` completes without error
- [ ] `verify_post_upgrade_state` returns `mismatches: []` on clean state
- [ ] `verify_post_upgrade_state` detects injected corruption (test: modify one field manually)
- [ ] 4-of-7 rollback approval accumulates correctly
- [ ] `rollback_to_snapshot` restores all records
- [ ] Post-rollback verification passes
- [ ] Contracts resume normal operation after rollback

---

## 12. Glossary

| Term | Definition |
|------|-----------|
| **Snapshot** | A complete copy of critical on-chain state captured before an upgrade |
| **Snapshot ID** | A `u32` identifier assigned by the operator to a snapshot |
| **SnapshotMeta** | Metadata struct: `created_at`, `block_height`, `contract_version`, `admin`, `checksum`, `record_count`, `snapshot_index` |
| **StateVerificationReport** | Returned by `verify_post_upgrade_state`: lists all field mismatches |
| **RollbackProposal** | On-chain proposal tracking 4-of-7 approvals for a rollback |
| **Emergency Signer** | One of 7 designated addresses authorised to propose/approve rollbacks |
| **Rolling Window** | The constraint that at most 3 snapshots are retained; oldest is evicted on 4th creation |
| **WASM Hash** | SHA-256 digest of a compiled Soroban contract binary |
| **4-of-7** | The emergency quorum: 4 out of 7 designated emergency signers must approve |
| **EMERGENCY_THRESHOLD** | Constant `4` in `shared/src/disaster_recovery.rs` |
| **MAX_SNAPSHOTS** | Constant `3` in `shared/src/disaster_recovery.rs` |
