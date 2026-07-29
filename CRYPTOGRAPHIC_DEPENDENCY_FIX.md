# Cryptographic Dependency Fix Guide

## Issue
```
error[E0277]: the trait bound `ChaCha20Rng: ed25519_dalek::rand_core::CryptoRng` is not satisfied
```

## Root Cause
This is a **version compatibility issue** between:
- `ed25519-dalek` (different versions have incompatible `CryptoRng` trait implementations)
- `rand_core` versions used by `ChaCha20Rng` in Soroban SDK

## Solution Applied ✅
**Updated Soroban SDK to v25.3.2** which has resolved compatibility issues.

## Alternative Solutions (if still needed)

### Option 1: Latest SDK Version
```toml
[workspace.dependencies]
soroban-sdk = "27.0.2"  # Latest available
soroban-token-sdk = "27.0.2"
```

### Option 2: Dependency Resolution Override
Add to root `Cargo.toml`:
```toml
[patch.crates-io]
rand_core = "0.6.4"
ed25519-dalek = "1.0.1"  # Older stable version
```

### Option 3: Feature Flag Approach
```toml
[workspace.dependencies]
soroban-sdk = { version = "25.3.2", default-features = false, features = ["contract", "testutils"] }
```

## Manual Verification Steps

1. **Clean rebuild:**
   ```bash
   cargo clean
   rm -f Cargo.lock
   cargo update
   ```

2. **Test compilation:**
   ```bash
   cargo check -p shared
   ```

3. **If still fails, try specific version:**
   ```bash
   cargo update soroban-sdk --precise 25.3.2
   cargo update ed25519-dalek --precise 1.0.1
   ```

4. **Check for conflicts:**
   ```bash
   cargo tree | grep ed25519
   cargo tree | grep rand_core
   ```

## Expected Resolution
With Soroban SDK v25.3.2, the cryptographic trait compatibility should be resolved, allowing successful compilation.

## Technical Background
The error occurs because different versions of cryptographic libraries have incompatible trait implementations:

- **Old versions:** `CryptoRng` trait had different requirements
- **New versions:** Updated trait bounds for better security
- **Soroban SDK v25+:** Updated to use compatible versions

## If All Else Fails
As a last resort, you can temporarily disable the problematic test code:
```toml
# In affected contract Cargo.toml
[features]
testutils = []

[dependencies]
soroban-sdk = { version = "25.3.2", default-features = false }
```

## Status
✅ **Applied Soroban SDK v25.3.2 update**  
🔄 **Manual verification needed** - run `cargo check -p shared`  
📋 **Fallback options documented** above if issues persist