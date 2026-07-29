#!/usr/bin/env pwsh

Write-Host "MentorsMind Gas Optimization - Performance Summary" -ForegroundColor Green
Write-Host "=================================================="

# Load data
$before = Get-Content "benchmarks/baselines_before_optimization.json" | ConvertFrom-Json
$after = Get-Content "benchmarks/baselines_after_optimization.json" | ConvertFrom-Json

Write-Host ""
Write-Host "TOP PERFORMANCE IMPROVEMENTS:" -ForegroundColor Yellow

foreach ($beforeFunc in $before) {
    $afterFunc = $after | Where-Object { $_.contract -eq $beforeFunc.contract -and $_.entry_point -eq $beforeFunc.entry_point }
    
    if ($afterFunc) {
        $cpuImprovement = [math]::Round((($beforeFunc.cpu_instructions - $afterFunc.cpu_instructions) / $beforeFunc.cpu_instructions) * 100, 1)
        $cpuSaved = $beforeFunc.cpu_instructions - $afterFunc.cpu_instructions
        
        Write-Host "$($beforeFunc.contract)::$($beforeFunc.entry_point)" -ForegroundColor Cyan
        Write-Host "  CPU: $($beforeFunc.cpu_instructions.ToString('N0')) -> $($afterFunc.cpu_instructions.ToString('N0')) (-$cpuImprovement%)" -ForegroundColor White
        Write-Host ""
    }
}

# Calculate totals
$totalCpuBefore = ($before | Measure-Object cpu_instructions -Sum).Sum
$totalCpuAfter = ($after | Measure-Object cpu_instructions -Sum).Sum
$totalMemBefore = ($before | Measure-Object mem_bytes -Sum).Sum
$totalMemAfter = ($after | Measure-Object mem_bytes -Sum).Sum

$cpuImprovement = [math]::Round((($totalCpuBefore - $totalCpuAfter) / $totalCpuBefore) * 100, 1)
$memImprovement = [math]::Round((($totalMemBefore - $totalMemAfter) / $totalMemBefore) * 100, 1)

Write-Host "OVERALL RESULTS:" -ForegroundColor Yellow
Write-Host "CPU Instructions: $cpuImprovement% improvement (Target: 15%)" -ForegroundColor $(if ($cpuImprovement -ge 15) { 'Green' } else { 'Red' })
Write-Host "Memory Usage: $memImprovement% improvement" -ForegroundColor $(if ($memImprovement -ge 15) { 'Green' } else { 'Red' })
Write-Host "Status: $(if ($cpuImprovement -ge 15) { 'TARGET EXCEEDED' } else { 'TARGET MISSED' })" -ForegroundColor $(if ($cpuImprovement -ge 15) { 'Green' } else { 'Red' })

Write-Host ""
Write-Host "GENERATED REPORTS:" -ForegroundColor Yellow
Write-Host "- performance_comparison_report.md"
Write-Host "- gas_optimization_analysis.md"  
Write-Host "- optimization_summary.ps1"