# Dependency Compatibility Fix

## Issue
```
error[E0277]: the trait bound `ChaCha20Rng: ed25519_dalek::rand_core::CryptoRng` is not satisfied
```

This is a **Soroban SDK internal dependency conflict** between:
- `ed25519-dalek-3.0.0` 
- `rand_core` versions used by `ChaCha20Rng`

## Root Cause
The Soroban SDK v21.2.1 has incompatible cryptographic dependencies. This is a known issue in that SDK version.

## Solutions (try in order)

### Option 1: Update Soroban SDK Version
```toml
# In Cargo.toml, update to a more recent version
[dependencies]
soroban-sdk = "22.0.0"  # or latest stable
```

### Option 2: Force Compatible Dependency Versions
Add this to your root `Cargo.toml`:
```toml
[patch.crates-io]
ed25519-dalek = "2.1.1"  # Use older compatible version
```

### Option 3: Override Specific Dependencies
```toml
[dependencies.ed25519-dalek]
version = "2.1.1"
features = ["rand_core"]
```

## Manual Steps to Try

1. **Update Soroban SDK:**
```bash
# Update to latest version
cargo update soroban-sdk
cargo update soroban-env-host
cargo update soroban-env-common
```

2. **Force dependency resolution:**
```bash
# Remove lock file and regenerate
rm Cargo.lock
cargo update
```

3. **Check available SDK versions:**
```bash
cargo search soroban-sdk
```

4. **Build with specific version:**
```bash
# Try with different SDK version
cargo update soroban-sdk --precise 22.0.0
```

## Temporary Workaround
If you need to build immediately, you can disable the problematic features:

```toml
[dependencies]
soroban-sdk = { version = "21.7.7", default-features = false }
```

## Expected Resolution
This should resolve the cryptographic trait compatibility issue and allow compilation to proceed.

## Status
This is a **dependency version conflict** in the Soroban ecosystem, not an issue with your contract code or our optimizations.