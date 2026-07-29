#!/usr/bin/env pwsh

# CI Integration Validation Script for MentorsMind Benchmarks
Write-Host "MentorsMind CI Integration Validation" -ForegroundColor Green
Write-Host "===================================="

$errors = 0
$warnings = 0

Write-Host ""
Write-Host "Validating CI Configuration..." -ForegroundColor Yellow

# Check GitHub Actions workflow file
if (Test-Path ".github/workflows/benchmarks.yml") {
    Write-Host "Benchmark workflow file exists" -ForegroundColor Green
    
    # Check workflow content
    $workflow = Get-Content ".github/workflows/benchmarks.yml" -Raw
    
    # Validate updated contract paths
    $contracts = @("upgrade_registry", "dispute_evidence")
    foreach ($contract in $contracts) {
        if ($workflow -match "contracts/$contract/") {
            Write-Host "Workflow includes $contract contract" -ForegroundColor Green
        } else {
            Write-Host "Workflow missing $contract contract" -ForegroundColor Red
            $errors++
        }
    }
    
    # Check for optimization validation
    if ($workflow -match "run_optimization_validation") {
        Write-Host "Optimization validation trigger configured" -ForegroundColor Green
    } else {
        Write-Host "Optimization validation trigger not found" -ForegroundColor Yellow
        $warnings++
    }
    
    # Check for enhanced PR comments
    if ($workflow -match "Storage R/W") {
        Write-Host "Enhanced PR comment format configured" -ForegroundColor Green
    } else {
        Write-Host "PR comment format not enhanced" -ForegroundColor Yellow
        $warnings++
    }
} else {
    Write-Host "Benchmark workflow file missing" -ForegroundColor Red
    $errors++
}

Write-Host ""
Write-Host "Validating Performance Targets..." -ForegroundColor Yellow

# Check if optimization targets were met
if ((Test-Path "benchmarks/baselines_before_optimization.json") -and (Test-Path "benchmarks/baselines_after_optimization.json")) {
    $before = Get-Content "benchmarks/baselines_before_optimization.json" | ConvertFrom-Json
    $after = Get-Content "benchmarks/baselines_after_optimization.json" | ConvertFrom-Json
    
    $totalCpuBefore = ($before | Measure-Object cpu_instructions -Sum).Sum
    $totalCpuAfter = ($after | Measure-Object cpu_instructions -Sum).Sum
    
    if ($totalCpuBefore -gt 0) {
        $improvement = [math]::Round((($totalCpuBefore - $totalCpuAfter) / $totalCpuBefore) * 100, 1)
        
        if ($improvement -ge 15) {
            Write-Host "Optimization target met: $improvement% improvement (>= 15% target)" -ForegroundColor Green
        } else {
            Write-Host "Optimization target missed: $improvement% improvement (< 15% target)" -ForegroundColor Red
            $errors++
        }
    }
}

Write-Host ""
Write-Host "Validation Summary" -ForegroundColor Yellow
Write-Host "=================="

if ($errors -eq 0 -and $warnings -eq 0) {
    Write-Host "ALL CHECKS PASSED - CI integration is ready!" -ForegroundColor Green
} elseif ($errors -eq 0) {
    Write-Host "PASSED WITH WARNINGS - CI integration is functional with $warnings warnings" -ForegroundColor Yellow
} else {
    Write-Host "VALIDATION FAILED - $errors errors and $warnings warnings found" -ForegroundColor Red
}

exit $errors