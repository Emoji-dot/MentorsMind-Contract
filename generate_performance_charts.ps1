#!/usr/bin/env pwsh

# Performance Chart Generator for MentorsMind Optimization Report
Write-Host "Generating Performance Comparison Charts" -ForegroundColor Green
Write-Host "========================================"

# Load benchmark data
$before = Get-Content "benchmarks/baselines_before_optimization.json" | ConvertFrom-Json
$after = Get-Content "benchmarks/baselines_after_optimization.json" | ConvertFrom-Json

Write-Host ""
Write-Host "📊 CPU Instructions Performance Chart" -ForegroundColor Yellow
Write-Host "────────────────────────────────────────────────────────────────────────────────────────"

# Create ASCII bar chart for CPU performance
foreach ($beforeFunc in $before) {
    $afterFunc = $after | Where-Object { $_.contract -eq $beforeFunc.contract -and $_.entry_point -eq $beforeFunc.entry_point }
    
    if ($afterFunc) {
        $improvement = [math]::Round((($beforeFunc.cpu_instructions - $afterFunc.cpu_instructions) / $beforeFunc.cpu_instructions) * 100, 1)
        
        # Create visual bars (scale down by 50K for display)
        $beforeBar = "█" * [math]::Floor($beforeFunc.cpu_instructions / 50000)
        $afterBar = "█" * [math]::Floor($afterFunc.cpu_instructions / 50000)
        
        $functionName = "$($beforeFunc.contract)::$($beforeFunc.entry_point)".PadRight(35)
        Write-Host "$functionName $improvement% improvement" -ForegroundColor Cyan
        Write-Host "  Before: $beforeBar ($($beforeFunc.cpu_instructions.ToString('N0')))" -ForegroundColor Red
        Write-Host "  After:  $afterBar ($($afterFunc.cpu_instructions.ToString('N0')))" -ForegroundColor Green
        Write-Host ""
    }
}

Write-Host "📊 Memory Usage Performance Chart" -ForegroundColor Yellow  
Write-Host "────────────────────────────────────────────────────────────────────────────────────────"

foreach ($beforeFunc in $before) {
    $afterFunc = $after | Where-Object { $_.contract -eq $beforeFunc.contract -and $_.entry_point -eq $beforeFunc.entry_point }
    
    if ($afterFunc) {
        $improvement = [math]::Round((($beforeFunc.mem_bytes - $afterFunc.mem_bytes) / $beforeFunc.mem_bytes) * 100, 1)
        
        # Create visual bars (scale down by 1000 for display)  
        $beforeBar = "█" * [math]::Floor($beforeFunc.mem_bytes / 1000)
        $afterBar = "█" * [math]::Floor($afterFunc.mem_bytes / 1000)
        
        $functionName = "$($beforeFunc.contract)::$($beforeFunc.entry_point)".PadRight(35)
        Write-Host "$functionName $improvement% improvement" -ForegroundColor Cyan
        Write-Host "  Before: $beforeBar ($($beforeFunc.mem_bytes.ToString('N0')) bytes)" -ForegroundColor Red  
        Write-Host "  After:  $afterBar ($($afterFunc.mem_bytes.ToString('N0')) bytes)" -ForegroundColor Green
        Write-Host ""
    }
}

# Summary statistics
$totalCpuBefore = ($before | Measure-Object cpu_instructions -Sum).Sum
$totalCpuAfter = ($after | Measure-Object cpu_instructions -Sum).Sum
$totalMemBefore = ($before | Measure-Object mem_bytes -Sum).Sum
$totalMemAfter = ($after | Measure-Object mem_bytes -Sum).Sum

$cpuImprovement = [math]::Round((($totalCpuBefore - $totalCpuAfter) / $totalCpuBefore) * 100, 1)
$memImprovement = [math]::Round((($totalMemBefore - $totalMemAfter) / $totalMemBefore) * 100, 1)

Write-Host "📈 Overall Performance Summary" -ForegroundColor Yellow
Write-Host "────────────────────────────────────────────────────────────────────────────────────────"
Write-Host "CPU Instructions:" -ForegroundColor Cyan
Write-Host "  Total Before: $($totalCpuBefore.ToString('N0'))" -ForegroundColor Red
Write-Host "  Total After:  $($totalCpuAfter.ToString('N0'))" -ForegroundColor Green  
Write-Host "  Improvement:  $cpuImprovement%" -ForegroundColor $(if ($cpuImprovement -ge 15) { 'Green' } else { 'Yellow' })
Write-Host ""
Write-Host "Memory Usage:" -ForegroundColor Cyan
Write-Host "  Total Before: $($totalMemBefore.ToString('N0')) bytes" -ForegroundColor Red
Write-Host "  Total After:  $($totalMemAfter.ToString('N0')) bytes" -ForegroundColor Green
Write-Host "  Improvement:  $memImprovement%" -ForegroundColor $(if ($memImprovement -ge 15) { 'Green' } else { 'Yellow' })
Write-Host ""

# Performance target validation
Write-Host "🎯 Target Achievement Status" -ForegroundColor Yellow
Write-Host "────────────────────────────────────────────────────────────────────────────────────────"
Write-Host "Target: 15% performance improvement" -ForegroundColor White
Write-Host "CPU Achievement: $cpuImprovement% $(if ($cpuImprovement -ge 15) { '✅ PASSED' } else { '❌ FAILED' })" -ForegroundColor $(if ($cpuImprovement -ge 15) { 'Green' } else { 'Red' })
Write-Host "Memory Achievement: $memImprovement% $(if ($memImprovement -ge 15) { '✅ PASSED' } else { '❌ FAILED' })" -ForegroundColor $(if ($memImprovement -ge 15) { 'Green' } else { 'Red' })
Write-Host "Overall Status: $(if ($cpuImprovement -ge 15 -and $memImprovement -ge 15) { '✅ TARGET EXCEEDED' } else { '⚠️ PARTIAL SUCCESS' })" -ForegroundColor $(if ($cpuImprovement -ge 15 -and $memImprovement -ge 15) { 'Green' } else { 'Yellow' })
Write-Host ""

Write-Host "📋 Generated Artifacts:" -ForegroundColor Yellow
Write-Host "  • performance_comparison_report.md - Comprehensive optimization report"
Write-Host "  • benchmarks/baselines_before_optimization.json - Original baselines"
Write-Host "  • benchmarks/baselines_after_optimization.json - Optimized baselines"  
Write-Host "  • gas_optimization_analysis.md - Technical analysis document"
Write-Host ""