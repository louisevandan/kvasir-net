[CmdletBinding()]
param(
    [string]$LlamaHost = 'http://127.0.0.1:18082',
    [string]$Prompt = 'Reply with one short Korean greeting.',
    [ValidateRange(1, 1024)][int]$MaxTokens = 8,
    [string]$PromptFile,
    [ValidateRange(0, 65535)][int]$PromptOffset = 0,
    [ValidateRange(0, 1024)][int]$ExecutionWindow = 0,
    [ValidateRange(1, 256)][int]$Parallel = 1,
    [ValidateRange(0, 256)][int]$ConcurrentRequests = 0,
    [ValidateRange(0, 1024)][int]$TotalRequests = 0,
    [ValidateRange(0, 256)][int]$InitialRequests = 0,
    [ValidateRange(0, 60000)][int]$ArrivalIntervalMs = 0,
    [ValidateRange(0, 1024)][int]$AgentWorkers = 0,
    [switch]$PreserveFailedGroup,
    [switch]$Benchmark,
    [switch]$BenchmarkIgnoreEog,
    [switch]$ExpectSharedMemory,
    [ValidateRange(1024, 65535)][int]$P4ListenPort = 19201,
    [ValidateRange(1024, 65535)][int]$P4RingListenPort = 19203
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$bin = Join-Path $root 'target\debug'
$target = Join-Path $root 'target\pipeline-e2e'
$stamp = if ([string]::IsNullOrWhiteSpace($env:P4_E2E_RUN_ID)) {
    Get-Date -Format 'yyyyMMddHHmmss'
} else {
    $env:P4_E2E_RUN_ID
}
$effectiveConcurrent = if ($ConcurrentRequests -eq 0) { $Parallel } else { $ConcurrentRequests }
if ($effectiveConcurrent -gt $Parallel) { throw 'ConcurrentRequests cannot exceed Parallel' }
$effectiveTotal = if ($TotalRequests -eq 0) { $effectiveConcurrent } else { $TotalRequests }
$effectiveInitial = if ($InitialRequests -eq 0) { $effectiveConcurrent } else { $InitialRequests }
if ($effectiveTotal -lt $effectiveConcurrent) { throw 'TotalRequests cannot be lower than ConcurrentRequests' }
if ($effectiveInitial -gt $effectiveConcurrent) { throw 'InitialRequests cannot exceed ConcurrentRequests' }
if ($effectiveTotal -gt $effectiveInitial -and $ArrivalIntervalMs -eq 0) { throw 'ArrivalIntervalMs must be positive when TotalRequests exceeds InitialRequests' }
if ($BenchmarkIgnoreEog -and -not $Benchmark) { throw 'BenchmarkIgnoreEog requires Benchmark' }
$previousBenchmarkIgnoreEog = $env:P4_PIPELINE_BENCHMARK_IGNORE_EOG
$previousPromptFile = $env:P4_PREFILL_PROMPT_FILE
$previousPromptOffset = $env:P4_PREFILL_PROMPT_OFFSET
$previousExecutionWindow = $env:P4_EXECUTION_WINDOW
$previousTotalRequests = $env:P4_TOTAL_REQUESTS
$previousInitialRequests = $env:P4_INITIAL_REQUESTS
$previousArrivalIntervalMs = $env:P4_ARRIVAL_INTERVAL_MS
$previousExpectSharedMemory = $env:P4_PIPELINE_EXPECT_SHARED_MEMORY
New-Item -ItemType Directory -Force -Path $target | Out-Null
$owned = @()
function Start-Owned([string]$FilePath, [string[]]$Arguments, [string]$Name) {
    $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -RedirectStandardOutput (Join-Path $target "$Name-$stamp.log") -RedirectStandardError (Join-Path $target "$Name-$stamp.err.log") -WindowStyle Hidden -PassThru
    $script:owned += $process
}
function Wait-Tcp([int]$Port) {
    $deadline = (Get-Date).AddSeconds(20)
    do { if (Test-NetConnection -ComputerName 127.0.0.1 -Port $Port -InformationLevel Quiet -WarningAction SilentlyContinue) { return }; Start-Sleep -Milliseconds 250 } while ((Get-Date) -lt $deadline)
    throw "port $Port did not open"
}
function Wait-Log([string]$Path, [string]$Marker, [int]$Seconds = 20) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        if ((Test-Path -LiteralPath $Path) -and ((Get-Content -Raw -LiteralPath $Path -ErrorAction SilentlyContinue) -match [regex]::Escape($Marker))) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "$Marker did not appear in $Path"
}
try {
    Push-Location $root
    if ($env:P4_SKIP_BUILD -ne '1') {
        cargo build --workspace | Out-Host
    }
    $env:P4_PIPELINE_BENCHMARK_IGNORE_EOG = if ($BenchmarkIgnoreEog) { '1' } else { '0' }
    $env:P4_PIPELINE_EXPECT_SHARED_MEMORY = if ($ExpectSharedMemory) { '1' } else { '0' }
    if ([string]::IsNullOrWhiteSpace($PromptFile)) {
        Remove-Item Env:P4_PREFILL_PROMPT_FILE -ErrorAction SilentlyContinue
    } else {
        $env:P4_PREFILL_PROMPT_FILE = (Resolve-Path -LiteralPath $PromptFile).Path
    }
    $env:P4_PREFILL_PROMPT_OFFSET = $PromptOffset
    $effectiveExecutionWindow = if ($ExecutionWindow -eq 0) { $effectiveTotal } else { $ExecutionWindow }
    if ($effectiveExecutionWindow -gt $effectiveTotal) { throw 'ExecutionWindow cannot exceed TotalRequests' }
    if ($effectiveTotal -gt $effectiveInitial -and $effectiveExecutionWindow -lt $effectiveTotal) { throw 'ExecutionWindow must cover TotalRequests for steady ingress' }
    $env:P4_TOTAL_REQUESTS = $effectiveTotal
    $env:P4_INITIAL_REQUESTS = $effectiveInitial
    $env:P4_ARRIVAL_INTERVAL_MS = $ArrivalIntervalMs
    $env:P4_EXECUTION_WINDOW = $effectiveExecutionWindow
    $agentArguments = @("127.0.0.1:$P4ListenPort")
    if ($AgentWorkers -gt 0) { $agentArguments += @('--workers', [string]$AgentWorkers) }
    Start-Owned (Join-Path $bin 'p4-agent.exe') $agentArguments 'p4-agent'
    Wait-Log (Join-Path $target "p4-agent-$stamp.log") 'P4_AGENT_READY'
    Start-Owned (Join-Path $bin 'p4-adapter.exe') @("127.0.0.1:$P4RingListenPort", "127.0.0.1:$P4ListenPort", 'adapter-local', $LlamaHost) 'p4-adapter'
    Wait-Log (Join-Path $target "p4-adapter-$stamp.log") 'P4_ADAPTER_READY'
    $clientLog = Join-Path $target "client-$stamp.log"
    $traceFile = Join-Path $target "trace-$stamp.jsonl"
    $planFile = Join-Path $target "plan-$stamp.json"
    $summaryFile = Join-Path $target "summary-$stamp.json"
    $reportFile = Join-Path $target "report-$stamp.md"
    & node (Join-Path $root 'tools\controller\experiments\pipeline-e2e\run-pipeline-e2e.mjs') "127.0.0.1:$P4ListenPort" 'pipeline-2gpu' $LlamaHost $Prompt $MaxTokens $Parallel $effectiveConcurrent ([int]$PreserveFailedGroup.IsPresent) $traceFile $planFile $summaryFile $reportFile ([int]$Benchmark.IsPresent) 2>&1 | Tee-Object -FilePath $clientLog
    if ($LASTEXITCODE -ne 0) { throw 'P4 pipeline Node.js client failed' }
    Write-Output "P4_PIPELINE_E2E_PASS logs=$target"
} finally {
    foreach ($process in $owned) { if (!$process.HasExited) { Stop-Process -Id $process.Id -Force } }
    $env:P4_PIPELINE_BENCHMARK_IGNORE_EOG = $previousBenchmarkIgnoreEog
    $env:P4_PREFILL_PROMPT_FILE = $previousPromptFile
    $env:P4_PREFILL_PROMPT_OFFSET = $previousPromptOffset
    $env:P4_EXECUTION_WINDOW = $previousExecutionWindow
    $env:P4_TOTAL_REQUESTS = $previousTotalRequests
    $env:P4_INITIAL_REQUESTS = $previousInitialRequests
    $env:P4_ARRIVAL_INTERVAL_MS = $previousArrivalIntervalMs
    if ($null -eq $previousExpectSharedMemory) {
        Remove-Item Env:P4_PIPELINE_EXPECT_SHARED_MEMORY -ErrorAction SilentlyContinue
    } else {
        $env:P4_PIPELINE_EXPECT_SHARED_MEMORY = $previousExpectSharedMemory
    }
    Pop-Location
}
