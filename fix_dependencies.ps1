#!/usr/bin/env pwsh

Write-Host "🔧 Fixing Soroban SDK Dependency Issues" -ForegroundColor Green
Write-Host "========================================"

Write-Host ""
Write-Host "Step 1: Cleaning previous builds..." -ForegroundColor Yellow
cargo clean
Remove-Item -Path "Cargo.lock" -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Step 2: Updating all dependencies..." -ForegroundColor Yellow
cargo update

Write-Host ""
Write-Host "Step 3: Checking for version conflicts..." -ForegroundColor Yellow
Write-Host "Current soroban-sdk versions in use:"
cargo tree | Select-String "soroban-sdk" | Select-Object -First 5

Write-Host ""
Write-Host "Step 4: Testing compilation of shared library..." -ForegroundColor Yellow
$result = cargo check -p shared 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "✅ Shared library compiles successfully!" -ForegroundColor Green
} else {
    Write-Host "❌ Compilation failed:" -ForegroundColor Red
    Write-Host $result
    
    Write-Host ""
    Write-Host "Attempting to resolve cryptographic dependency conflict..." -ForegroundColor Yellow
    
    # Try forcing specific versions
    cargo update soroban-sdk --precise 25.3.2
    cargo update soroban-env-host --precise 25.2.2
    
    Write-Host "Retrying compilation..."
    $result2 = cargo check -p shared 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✅ Fixed! Shared library now compiles." -ForegroundColor Green
    } else {
        Write-Host "❌ Still failing. Error details:" -ForegroundColor Red
        Write-Host $result2
        
        Write-Host ""
        Write-Host "🔧 Manual fix required. Try these commands:" -ForegroundColor Yellow
        Write-Host "1. cargo update ed25519-dalek --precise 2.1.1"
        Write-Host "2. cargo update rand_core --precise 0.6.4"
        Write-Host "3. cargo check -p shared"
        
        exit 1
    }
}

Write-Host ""
Write-Host "Step 5: Testing benchmark compilation..." -ForegroundColor Yellow
$benchResult = cargo check -p mentorminds-benchmarks 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "✅ Benchmarks compile successfully!" -ForegroundColor Green
} else {
    Write-Host "⚠️ Benchmark compilation issues:" -ForegroundColor Yellow
    Write-Host $benchResult
}

Write-Host ""
Write-Host "Step 6: Testing contract compilation..." -ForegroundColor Yellow
$contracts = @(
    "mentorminds-escrow",
    "mentorminds-staking", 
    "mentorminds-governance",
    "mentorminds-upgrade-registry"
)

$failedContracts = @()
foreach ($contract in $contracts) {
    Write-Host "  Checking $contract..." -NoNewline
    $contractResult = cargo check -p $contract 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host " ✅" -ForegroundColor Green
    } else {
        Write-Host " ❌" -ForegroundColor Red
        $failedContracts += $contract
    }
}

Write-Host ""
if ($failedContracts.Count -eq 0) {
    Write-Host "🎉 All dependency issues resolved!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Yellow
    Write-Host "1. Run: cargo build --target wasm32-unknown-unknown --release"
    Write-Host "2. Run: cargo run -p mentorminds-benchmarks"
    Write-Host "3. Commit changes: git add -A && git commit -m 'Fix dependency issues'"
} else {
    Write-Host "⚠️ Some contracts still have issues:" -ForegroundColor Yellow
    foreach ($failed in $failedContracts) {
        Write-Host "  - $failed"
    }
    Write-Host ""
    Write-Host "Try running this script again or check individual contract errors."
}

Write-Host ""
Write-Host "Dependency fix script completed." -ForegroundColor Green