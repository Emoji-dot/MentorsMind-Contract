#!/usr/bin/env pwsh

# Gas Optimization Results Summary
Write-Host "MentorsMind Contract Gas Optimization Results" -ForegroundColor Green
Write-Host "=============================================="

# Load before and after benchmarks
$before = Get-Content "benchmarks/baselines_before_optimization.json" | ConvertFrom-Json
$after = Get-Content "benchmarks/baselines_after_optimization.json" | ConvertFrom-Json

Write-Host ""
Write-Host "Top 5 Optimized Functions Performance Comparison:" -ForegroundColor Yellow
Write-Host ""

# Calculate improvements for each function
foreach ($beforeFunc in $before) {
    $afterFunc = $after | Where-Object { $_.contract -eq $beforeFunc.contract -and $_.entry_point -eq $beforeFunc.entry_point }
    
    if ($afterFunc) {
        $cpuImprovement = [math]::Round((($beforeFunc.cpu_instructions - $afterFunc.cpu_instructions) / $beforeFunc.cpu_instructions) * 100, 1)
        $memImprovement = [math]::Round((($beforeFunc.mem_bytes - $afterFunc.mem_bytes) / $beforeFunc.mem_bytes) * 100, 1)
        $cpuSaved = $beforeFunc.cpu_instructions - $afterFunc.cpu_instructions
        $memSaved = $beforeFunc.mem_bytes - $afterFunc.mem_bytes
        
        Write-Host "$($beforeFunc.contract)::$($beforeFunc.entry_point)" -ForegroundColor Magenta
        Write-Host "  CPU Instructions: $($beforeFunc.cpu_instructions.ToString('N0')) -> $($afterFunc.cpu_instructions.ToString('N0')) (-$($cpuSaved.ToString('N0')), -$cpuImprovement%)" -ForegroundColor Green
        Write-Host "  Memory Usage: $($beforeFunc.mem_bytes.ToString('N0')) -> $($afterFunc.mem_bytes.ToString('N0')) bytes (-$($memSaved.ToString('N0')), -$memImprovement%)" -ForegroundColor Green
        Write-Host ""
    }
}

Write-Host "Optimization Techniques Applied:" -ForegroundColor Yellow
Write-Host ""
Write-Host "1. Storage Layout Optimization" -ForegroundColor Cyan
Write-Host "   - Replaced vector append pattern with append-only keys"
Write-Host "   - Eliminated full vector deserialization/serialization"
Write-Host "   - Reduced O(n) operations to O(1) for history updates"
Write-Host ""

Write-Host "2. Validation Result Caching" -ForegroundColor Cyan  
Write-Host "   - Added 5-minute cache for M-of-N signature validations"
Write-Host "   - Prevents redundant authorization checks"
Write-Host "   - Uses temporary storage for cache efficiency"
Write-Host ""

Write-Host "3. Batch Storage Operations" -ForegroundColor Cyan
Write-Host "   - Combined multiple storage reads into single operations"
Write-Host "   - Eliminated N+1 query problem in staking distribution"
Write-Host "   - Reduced storage I/O overhead significantly"
Write-Host ""

Write-Host "4. Cross-Contract Call Optimization" -ForegroundColor Cyan
Write-Host "   - Minimized redundant contract invocations"
Write-Host "   - Batched related external calls where possible"
Write-Host "   - Reduced serialization overhead for large payloads"
Write-Host ""

# Calculate overall improvements
$totalCpuBefore = ($before | Measure-Object cpu_instructions -Sum).Sum
$totalCpuAfter = ($after | Measure-Object cpu_instructions -Sum).Sum
$totalMemBefore = ($before | Measure-Object mem_bytes -Sum).Sum  
$totalMemAfter = ($after | Measure-Object mem_bytes -Sum).Sum

$overallCpuImprovement = [math]::Round((($totalCpuBefore - $totalCpuAfter) / $totalCpuBefore) * 100, 1)
$overallMemImprovement = [math]::Round((($totalMemBefore - $totalMemAfter) / $totalMemBefore) * 100, 1)

Write-Host "Overall Performance Improvement:" -ForegroundColor Yellow
Write-Host "  CPU Instructions: $overallCpuImprovement% reduction" -ForegroundColor Green
Write-Host "  Memory Usage: $overallMemImprovement% reduction" -ForegroundColor Green
Write-Host "  Target Met: $(if ($overallCpuImprovement -ge 15) { 'YES' } else { 'NO' }) (target: 15%)" -ForegroundColor $(if ($overallCpuImprovement -ge 15) { 'Green' } else { 'Red' })
Write-Host ""

Write-Host "Total Gas Savings:" -ForegroundColor Yellow
Write-Host "  CPU Instructions Saved: $(($totalCpuBefore - $totalCpuAfter).ToString('N0'))" -ForegroundColor Green
Write-Host "  Memory Bytes Saved: $(($totalMemBefore - $totalMemAfter).ToString('N0'))" -ForegroundColor Green
Write-Host ""

Write-Host "Next Steps:" -ForegroundColor Yellow
Write-Host "1. Deploy optimized contracts to testnet for validation"
Write-Host "2. Run comprehensive integration tests" 
Write-Host "3. Monitor real-world performance improvements"
Write-Host "4. Consider additional optimizations based on usage patterns"
Write-Host ""