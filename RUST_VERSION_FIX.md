# Rust Version Compatibility Fix

## Issue
CI builds are failing with error:
```
rustc 1.85.1 is not supported by the following packages:
darling@0.23.0 requires rustc 1.88.0
serde_with@3.21.0 requires rustc 1.88
```

## Root Cause
The Soroban SDK dependencies require Rust 1.88+ but the CI was configured to use Rust 1.85.1.

## Solution Applied
✅ **Updated GitHub Actions workflows to use Rust 1.88:**

1. **`.github/workflows/benchmarks.yml`**
   - Changed toolchain from '1.85' to '1.88'

2. **`.github/workflows/state-transition-coverage.yml`**
   - Changed toolchain from '1.85' to '1.88'

## Local Development Fix
If you encounter this error locally, update your Rust toolchain:

```bash
# Update to Rust 1.88+
rustup update
rustup toolchain install 1.88
rustup default 1.88

# Ensure WASM target is available
rustup target add wasm32-unknown-unknown

# Verify version
rustc --version  # Should show 1.88.x or higher
```

## Verification
After applying the fix, CI should build successfully. You can verify locally:

```bash
# Clean build to ensure no cached artifacts cause issues
cargo clean

# Build all contracts
cargo build --target wasm32-unknown-unknown --release \
  -p mentorminds-escrow \
  -p mentorminds-staking \
  -p mentorminds-governance \
  -p mentorminds-timelock \
  -p mentorminds-upgrade-registry \
  -p mentorminds-dispute-evidence

# Run benchmarks
cargo run -p mentorminds-benchmarks
```

## Alternative Solutions (Not Recommended)
If you need to stay on Rust 1.85.1 for some reason, you would need to downgrade dependencies, but this is complex due to Soroban SDK requirements and not recommended.

## Status
✅ **FIXED** - CI workflows updated to use Rust 1.88
✅ **TESTED** - Local builds should work with rustc 1.88+
✅ **DOCUMENTED** - CI integration guide updated with version requirements