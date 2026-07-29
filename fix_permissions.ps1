#!/usr/bin/env pwsh

Write-Host "🔧 Fixing Windows Directory Permissions" -ForegroundColor Green
Write-Host "======================================="

$projectPath = "C:\Users\DELL\MentorsMind-Contract"
$targetPath = "$projectPath\target"

Write-Host ""
Write-Host "Step 1: Checking current permissions..." -ForegroundColor Yellow

# Check if target directory exists and try to remove it
if (Test-Path $targetPath) {
    Write-Host "Target directory exists. Attempting to remove..." -ForegroundColor Yellow
    try {
        Remove-Item -Path $targetPath -Recurse -Force -ErrorAction Stop
        Write-Host "✅ Removed existing target directory" -ForegroundColor Green
    } catch {
        Write-Host "❌ Could not remove target directory: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host "Trying alternative approach..." -ForegroundColor Yellow
        
        # Try to take ownership and remove
        takeown /f $targetPath /r /d y 2>$null
        icacls $targetPath /grant "$env:USERNAME:(OI)(CI)F" /t 2>$null
        Remove-Item -Path $targetPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "Step 2: Setting directory permissions..." -ForegroundColor Yellow

# Ensure the project directory has full permissions
try {
    icacls $projectPath /grant "$env:USERNAME:(OI)(CI)F" /t
    Write-Host "✅ Set full permissions for user" -ForegroundColor Green
} catch {
    Write-Host "⚠️ Permission setting failed, but may still work" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Step 3: Creating target directory manually..." -ForegroundColor Yellow

try {
    New-Item -Path $targetPath -ItemType Directory -Force
    Write-Host "✅ Created target directory" -ForegroundColor Green
} catch {
    Write-Host "❌ Could not create target directory: $($_.Exception.Message)" -ForegroundColor Red
}

Write-Host ""
Write-Host "Step 4: Testing Cargo access..." -ForegroundColor Yellow

# Test if cargo can access the directory
Push-Location $projectPath
try {
    cargo --version | Out-Null
    Write-Host "✅ Cargo is accessible" -ForegroundColor Green
    
    # Try a simple cargo operation
    $result = cargo metadata --no-deps --format-version 1 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✅ Cargo can read project metadata" -ForegroundColor Green
    } else {
        Write-Host "⚠️ Cargo metadata issue: $result" -ForegroundColor Yellow
    }
} catch {
    Write-Host "❌ Cargo access failed: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "🔧 Alternative Solutions:" -ForegroundColor Yellow
Write-Host "1. Run PowerShell as Administrator and re-run this script"
Write-Host "2. Use WSL2 for building (recommended for complex Rust projects)"
Write-Host "3. Move project to a different directory (like C:\dev\MentorsMind-Contract)"
Write-Host "4. Use 'cargo build --target-dir C:\temp\cargo-target' to use different target directory"

Write-Host ""
Write-Host "Permission fix script completed." -ForegroundColor Green