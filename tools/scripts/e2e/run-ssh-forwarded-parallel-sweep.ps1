[CmdletBinding()]
param(
    [string]$RunId = (Get-Date -Format 'yyyyMMddHHmmss'),
    [string]$SshTarget = '42mob@192.168.0.29',
    [string]$ArtifactDirectory = '',
    [string]$AgentBinary = '',
    [string]$DriveBinary = '',
    [string]$PromptFile = '',
    [int[]]$ParallelValues = @(1, 2, 4),
    [int]$RequestsPerParallel = 4,
    [int]$Tokens = 20,
    [int]$PromptTokens = 5000,
    [int]$ArriveMilliseconds = 100,
    [int]$ContextSize = 0,
    [int]$BatchSize = 512,
    [int]$UBatchSize = 512,
    [switch]$KeepFinalLoaded
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$parallelValues = @($ParallelValues)
if ($parallelValues.Count -eq 0 -or @($parallelValues | Where-Object { $_ -lt 1 }).Count -gt 0) {
    throw 'ParallelValues must contain positive integers.'
}
if ($RequestsPerParallel -lt 1) { throw 'RequestsPerParallel must be positive.' }

$reservedPorts = @(52000, 52003, 52004, 53001, 53002)
$occupied = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in $reservedPorts } |
    Select-Object -ExpandProperty LocalPort -Unique
if ($occupied) {
    throw "Sweep ports are already listening: $($occupied -join ', '). Stop the existing run before starting a sweep."
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$runner = Join-Path $PSScriptRoot 'run-ssh-forwarded-real-four-node.ps1'
$outputRoot = Join-Path $root "target\ssh-forwarded-four-node-sweep\$RunId"
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

$results = [System.Collections.Generic.List[object]]::new()
for ($index = 0; $index -lt $parallelValues.Count; $index++) {
    $parallel = $parallelValues[$index]
    $childId = "$RunId-p$parallel"
    $childOutput = Join-Path $root "target\ssh-forwarded-four-node-e2e\$childId"
    $arguments = @{
        RunId = $childId
        SshTarget = $SshTarget
        # Every candidate sees the same request count. Native parallel is the
        # loaded sequence capacity, not permission to change the workload.
        Requests = $RequestsPerParallel
        Tokens = $Tokens
        PromptTokens = $PromptTokens
        Parallel = $parallel
        ArriveMilliseconds = $ArriveMilliseconds
        ContextSize = $ContextSize
        BatchSize = $BatchSize
        UBatchSize = $UBatchSize
        Max4080VramMiB = 9000
        PromptFile = $PromptFile
        MinimumPeakNodeQueue = 0
        MinimumPeakInAdapter = 0
    }
    if ($ArtifactDirectory) { $arguments.ArtifactDirectory = $ArtifactDirectory }
    if ($AgentBinary) { $arguments.AgentBinary = $AgentBinary }
    if ($DriveBinary) { $arguments.DriveBinary = $DriveBinary }
    if ($KeepFinalLoaded -and $index -eq $parallelValues.Count - 1) {
        $arguments.KeepLoaded = $true
    }

    $exitCode = 0
    try {
        & $runner @arguments
    } catch {
        $exitCode = 1
    }
    $resultPath = Join-Path $childOutput 'result.json'
    if (Test-Path -LiteralPath $resultPath) {
        $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
        $results.Add([pscustomobject]@{
            parallel = $parallel
            run_id = $childId
            exit_code = $result.exit_code
            passed = $result.passed
            completed = $result.metrics.completed
            failed = $result.metrics.failed
            unanswered = $result.metrics.unanswered
            p95_ms = $result.metrics.latency.p95_ms
            p99_ms = $result.metrics.latency.p99_ms
            prefill_tps_over_run = $result.metrics.prefill_tps_over_run
            generation_tps_over_run = $result.metrics.generation_tps_over_run
            average_session_prefill_tps = $result.metrics.average_session_prefill_tps
            average_session_generation_tps = $result.metrics.average_session_generation_tps
            peak_4080_vram_mib = $result.metrics.peak_4080_vram_mib
            peak_node_queue = $result.overlap.peak_node_queue
            peak_in_adapter = $result.overlap.peak_in_adapter
            evidence = $childOutput
        })
    } else {
        $results.Add([pscustomobject]@{
            parallel = $parallel
            run_id = $childId
            exit_code = $exitCode
            passed = $false
            completed = 0
            failed = $null
            unanswered = $null
            p95_ms = $null
            p99_ms = $null
            prefill_tps_over_run = $null
            generation_tps_over_run = $null
            average_session_prefill_tps = $null
            average_session_generation_tps = $null
            peak_4080_vram_mib = $null
            peak_node_queue = $null
            peak_in_adapter = $null
            evidence = $childOutput
        })
    }
}

$eligible = @($results | Where-Object {
    $_.passed -eq $true -and
    $_.completed -gt 0 -and
    $_.failed -eq 0 -and
    $_.unanswered -eq 0 -and
    $null -ne $_.generation_tps_over_run -and
    $_.generation_tps_over_run -ge 0 -and
    $null -ne $_.prefill_tps_over_run -and
    $_.prefill_tps_over_run -ge 0 -and
    $null -ne $_.average_session_generation_tps -and
    $_.average_session_generation_tps -ge 0 -and
    $null -ne $_.average_session_prefill_tps -and
    $_.average_session_prefill_tps -ge 0 -and
    $null -ne $_.peak_4080_vram_mib -and
    $_.peak_4080_vram_mib -le 9000
})
$winner = $eligible |
    Sort-Object @{ Expression = { [double]$_.generation_tps_over_run }; Descending = $true },
        @{ Expression = { [double]$_.average_session_generation_tps }; Descending = $true },
        @{ Expression = { [double]$_.p95_ms }; Descending = $false },
        @{ Expression = { [int]$_.parallel }; Descending = $false } |
    Select-Object -First 1

$summary = [pscustomobject]@{
    schema = 'p4-staged-parallel-sweep-v1'
    mode = 'non-mtp'
    run_id = $RunId
    topology = 'central RTX 3090 + central RTX 4080 + remote RTX 3090 x2'
    ports = '52000 -> 52003 -> 52004 -> 53001 -> 53002'
    prompt_tokens = $PromptTokens
    generation_tokens = $Tokens
    results = $results
    selection = [pscustomobject]@{
        policy = 'eligible completion/no-failure/no-unanswered; maximize aggregate generation TPS; tie-break average session generation TPS, then p95 latency'
        eligible_parallel = @($eligible | ForEach-Object { $_.parallel })
        selected_parallel = if ($null -ne $winner) { $winner.parallel } else { $null }
        selected_run_id = if ($null -ne $winner) { $winner.run_id } else { $null }
        loaded_parallel = if ($KeepFinalLoaded) { $parallelValues[-1] } else { $null }
        requires_external_vram_guard = $true
        note = 'Candidates are loaded and measured sequentially because native parallel/sequence capacity is fixed at load time; no candidate is claimed optimal without a passing real run.'
    }
}
$summaryPath = Join-Path $outputRoot 'summary.json'
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
Write-Output "PARALLEL_SWEEP_SUMMARY: $summaryPath"
