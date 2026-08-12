[CmdletBinding()]
param(
    [string]$LlamaHost = 'http://127.0.0.1:18082',
    [ValidateRange(1, 1024)][int]$MaxTokens = 500,
    [int[]]$ConcurrentSessions = @(100, 50, 10, 2, 1),
    [ValidateRange(0, 1024)][int]$AgentWorkers = 0,
    [ValidateRange(1024, 65535)][int]$P4ListenPort = 29221
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$target = Join-Path $root 'target\pipeline-e2e'
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
$summaries = @()
foreach ($sessions in $ConcurrentSessions) {
    if ($sessions -lt 1 -or $sessions -gt 256) { throw 'ConcurrentSessions must be between 1 and 256' }
    & (Join-Path $PSScriptRoot '..\..\e2e\pipeline\run-pipeline-e2e.ps1') -LlamaHost $LlamaHost -MaxTokens $MaxTokens -Parallel $sessions -ConcurrentRequests $sessions -AgentWorkers $AgentWorkers -P4ListenPort $P4ListenPort -Benchmark
    if ($LASTEXITCODE -ne 0) { throw "P4 concurrency run failed at $sessions sessions" }
    $summary = Get-ChildItem -LiteralPath $target -Filter 'summary-*.json' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (!$summary) { throw "P4 run at $sessions sessions did not produce a summary" }
    $summaries += $summary.FullName
}
$suiteSummary = Join-Path $target "sweep-$stamp.json"
$suiteReport = Join-Path $target "sweep-$stamp.md"
& node (Join-Path $root 'tools\controller\evidence\summarize-pipeline-sweep.mjs') $suiteSummary $suiteReport $summaries
if ($LASTEXITCODE -ne 0) { throw 'P4 sweep summary generation failed' }
Write-Output "P4_SWEEP_PASS summary=$suiteSummary report=$suiteReport"
