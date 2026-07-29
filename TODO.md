# Issue #771: Protocol-wide Solvency Invariant Checks — TODO

## Implementation Steps
- [x] Step 1: Extend `health_dashboard` Config with treasury/insurance/lending_pool/usdc addresses
- [x] Step 2: Add `SolvencyReport` struct and `PendingAllocationView` for cross-contract decoding
- [x] Step 3: Add `get_protocol_solvency()` with cross-contract calls to treasury/insurance/staking/lending_pool
- [x] Step 4: Add helper getters (`get_staker_at` to staking, `pending_allocation_count` to treasury)
- [x] Step 5: Add 5 solvency tests (basic values, insolvent, alert event, non-negative fields, exact values)
- [x] Step 6: Create Node.js monitoring script (`scripts/monitor_solvency.js`)

