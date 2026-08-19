[CmdletBinding()]
param(
    [string]$RunId = (Get-Date -Format 'yyyyMMddHHmmss'),
    [string]$AgentBinary = '',
    [string]$DriveBinary = '',
    [string]$ServerBinary = '',
    [string]$Model = 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    [string]$PromptFile = 'F:\dev\linkcpp_product\target\validation-5000-token\20260818-qwen25\prompt-5000-tokens.txt',
    [int]$Requests = 1,
    [int]$Tokens = 5000,
    [int]$PromptTokens = 5000,
    [int]$Parallel = 1,
    [int]$NativeSlots = 0,
    # The secondary GPU is the display adapter. Keep its stage deliberately
    # small; the 5k-token KV/compute footprint is not represented by weight
    # bytes alone.
    [int]$LayerBoundary = 20,
    [int]$LayerCount = 28,
    [int]$MaxSecondaryVramMiB = 9000,
    [int]$BatchSize = 6000,
    [int]$UBatchSize = 5000,
    [int]$ContextSize = 0,
    [int]$ArriveMilliseconds = 0,
    [int]$QuietMilliseconds = 600000,
    [int]$BasePort = 52700,
    [string]$AdvertisedHost = '192.168.0.6',
    [switch]$ReuseLoaded,
    [switch]$KeepLoaded,
    [switch]$VaryPrompts
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$p4 = Join-Path $root 'apps\p4'
if ([string]::IsNullOrWhiteSpace($AgentBinary)) { $AgentBinary = Join-Path $root 'target\p4-rebuild-20260818\release\p4-agent.exe' }
if ([string]::IsNullOrWhiteSpace($DriveBinary)) { $DriveBinary = Join-Path $root 'target\p4-rebuild-20260818\release\p4-drive.exe' }
if ([string]::IsNullOrWhiteSpace($ServerBinary)) { $ServerBinary = Join-Path $root '.cache\staged-server-cuda-final\Release\p4_staged_server.exe' }
foreach ($path in @($AgentBinary, $DriveBinary, $ServerBinary, $Model, $PromptFile)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required file not found: $path" }
}
if ($LayerBoundary -le 0 -or $LayerBoundary -ge $LayerCount) { throw "LayerBoundary must be between 1 and LayerCount-1." }
if ($MaxSecondaryVramMiB -le 0) { throw 'MaxSecondaryVramMiB must be positive.' }
$out = Join-Path $root "target\real-two-stage-5000\$RunId"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$ports = @($BasePort, ($BasePort + 1)); $driverPort = $BasePort + 10
$busy = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in ($ports + $driverPort) })
if (-not $ReuseLoaded -and $busy.Count -gt 0) { throw "Reserved port is busy: $($busy.LocalPort -join ', ')" }
function Literal([string]$value) { "'{0}'" -f $value.Replace("'", "''") }
function Encoded([string]$value) { [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($value)) }
function Start-Agent([int]$port, [int]$cuda) {
    $stdout = Join-Path $out "agent-$port.log"; $stderr = Join-Path $out "agent-$port.err.log"
    $env:CUDA_VISIBLE_DEVICES = [string]$cuda
    $env:P4_STAGED_SERVER_BINARY = $ServerBinary
    $env:P4_MODEL_DIR = [IO.Path]::GetDirectoryName($Model)
    $env:P4_STAGED_LLAMA_INHERIT_STDERR = '1'
    $env:P4_AGENT_STATS = '1'
    # Bind all local interfaces and advertise a reachable non-loopback address.
    # 127.0.0.1 makes every agent classify itself as local-only, so a stage
    # HOP cannot be delivered to the next agent even though both processes are
    # alive and their models are loaded.
    Start-Process -FilePath $AgentBinary -ArgumentList @("0.0.0.0:$port","$AdvertisedHost`:$port") -WorkingDirectory $p4 -RedirectStandardOutput $stdout -RedirectStandardError $stderr -WindowStyle Hidden -PassThru
}
function Wait-Port([int]$port) {
    $deadline = (Get-Date).AddSeconds(60)
    do {
        if (@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue).Count -gt 0) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "Agent port $port did not become ready."
}
function Assert-StageExecution([int[]]$stagePorts) {
    $driveLog = Join-Path $out 'drive.log'
    $telemetryLine = @(Select-String -LiteralPath $driveLog -Pattern '^P4_DRIVE_TELEMETRY_JSON ' | Select-Object -Last 1)
    if ($telemetryLine.Count -eq 0) { throw "Stage execution evidence gate failed: no drive telemetry; logs retained at $out" }
    $telemetry = $telemetryLine[0].Line.Substring('P4_DRIVE_TELEMETRY_JSON '.Length) | ConvertFrom-Json
    $missing = @()
    foreach ($stage in @('stage-0', 'tail-1')) {
        $stageSamples = @($telemetry.samples | Where-Object { $_.node -eq $stage })
        if (@($stageSamples | Where-Object { $_.phase -eq 'prefill' }).Count -eq 0) { $missing += "$stage:no-prefill-telemetry" }
        # Very short smoke runs may finish before the asynchronous telemetry
        # snapshot captures decode. Long-model gates still require it.
        if ($Tokens -ge 100 -and @($stageSamples | Where-Object { $_.phase -eq 'generation' }).Count -eq 0) { $missing += "$stage:no-generation-telemetry" }
    }
    foreach ($port in $stagePorts) {
        $log = Join-Path $out "agent-$port.log"
        if (-not (Test-Path -LiteralPath $log)) {
            $missing += "$port:no-log"
            continue
        }
        $samples = @(Select-String -LiteralPath $log -Pattern 'P4_RUNTIME_SAMPLE_V1' -SimpleMatch)
        if ($samples.Count -eq 0) { $missing += "$port:no-runtime-sample" }
        $encodeErrors = @(Select-String -LiteralPath $log -Pattern 'P4_AGENT_ENCODE_FAILED' -SimpleMatch)
        if ($encodeErrors.Count -gt 0) { $missing += "$port:encode-failed=$($encodeErrors.Count)" }
    }
    if ($missing.Count -gt 0) {
        throw "Stage execution evidence gate failed: $($missing -join ', '); logs retained at $out"
    }
}
$agents = @(); $drive = $null; $vramGuardFailed = $false
try {
    if ($ReuseLoaded) {
        foreach ($port in $ports) {
            if (@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue).Count -eq 0) {
                throw "ReuseLoaded requested but stage port $port is not listening."
            }
        }
    } else {
        $agents += Start-Agent $ports[0] 0; $agents += Start-Agent $ports[1] 1
        Wait-Port $ports[0]; Wait-Port $ports[1]
    }
    # Parallel is the admitted request width, not the number of native KV
    # slots needed while a two-stage pipeline is full. Keep one extra slot so
    # stage 0 may carry the next request while the previous request is still
    # draining through the tail.
    $sequenceCapacity = [Math]::Max(1, $Parallel + 1)
    if ($NativeSlots -gt 0) { $sequenceCapacity = [Math]::Max($sequenceCapacity, $NativeSlots) }
    # llama.cpp's --ctx-size is the total context across n_seq_max.  A
    # 5k-token prompt plus 5k generated tokens therefore needs 20k total
    # context when the two-stage pipeline reserves two sequence slots.  The
    # former fixed 10k value produced n_ctx_seq=5120 and stalled at roughly
    # 121 generated tokens after the 5k-token prefill.
    $requiredContext = [Math]::Max(1024, ($PromptTokens + $Tokens) * $sequenceCapacity)
    if ($ContextSize -le 0) { $ContextSize = $requiredContext }
    if ($ContextSize -lt $requiredContext) {
        throw "ContextSize=$ContextSize is smaller than required per-request capacity $requiredContext (PromptTokens=$PromptTokens Tokens=$Tokens Parallel=$Parallel)."
    }
    $plans = @(
        ('--model "{0}" --layer-begin 0 --layer-end {5} --kv-layer-begin 0 --kv-layer-end {5} --n-seq-max {1} --batch-size {2} --ubatch-size {3} --ctx-size {4} --device CUDA0 --flash-attn 0' -f $Model,$sequenceCapacity,$BatchSize,$UBatchSize,$ContextSize,$LayerBoundary),
        ('--model "{0}" --layer-begin {5} --layer-end {6} --kv-layer-begin {5} --kv-layer-end {6} --n-seq-max {1} --batch-size {2} --ubatch-size {3} --ctx-size {4} --device CUDA0 --flash-attn 0' -f $Model,$sequenceCapacity,$BatchSize,$UBatchSize,$ContextSize,$LayerBoundary,$LayerCount)
    )
    $env:P4_DRIVE_DISCOVER = '1'
    $env:P4_DRIVE_ARTIFACT = 'Qwen2.5-1.5B-Instruct-Q8_0.gguf'
    $env:P4_DRIVE_CEILING = [string]$Parallel
    $env:P4_DRIVE_ARRIVE_MS = [string]$ArriveMilliseconds
    $env:P4_DRIVE_VARY = if ($VaryPrompts) { '1' } else { '0' }
    $env:P4_DRIVE_PROMPT_FILE = $PromptFile
    $env:P4_DRIVE_KEEP_LOADED = if ($KeepLoaded) { '1' } else { '0' }
    $env:P4_DRIVE_QUIET_MS = [string]$QuietMilliseconds
    $env:P4_STAGED_SERVER_BINARY = $ServerBinary
    $env:P4_DRIVE_PLAN_0 = $plans[0]
    $env:P4_DRIVE_PLAN_1 = $plans[1]
    $chain = "$AdvertisedHost`:$($ports[0]),$AdvertisedHost`:$($ports[1])"
    $drive = Start-Process -FilePath $DriveBinary -ArgumentList @("127.0.0.1:$driverPort",$chain,$Requests,$Tokens,'llamacpp-staged',"127.0.0.1:$driverPort") -WorkingDirectory $p4 -RedirectStandardOutput (Join-Path $out 'drive.log') -RedirectStandardError (Join-Path $out 'drive.err.log') -WindowStyle Hidden -PassThru
    $loadDeadline = (Get-Date).AddSeconds(180)
    do {
        $driveText = if (Test-Path -LiteralPath (Join-Path $out 'drive.log')) {
            Get-Content -LiteralPath (Join-Path $out 'drive.log') -Raw -ErrorAction SilentlyContinue
        } else { '' }
        if ($driveText -match 'P4_DRIVE_LOADED nodes=') { break }
        if ($drive.HasExited) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $loadDeadline)
    if ($driveText -notmatch 'P4_DRIVE_LOADED nodes=') {
        throw "drive did not reach LOADED before VRAM guard; see $out"
    }
    $vramLines = @(nvidia-smi --query-gpu=index,memory.used --format=csv,noheader,nounits 2>&1)
    $secondaryLine = $vramLines | Where-Object { $_ -match '^\s*1\s*,' } | Select-Object -First 1
    if ($null -eq $secondaryLine) {
        $vramGuardFailed = $true
        throw 'Could not read GPU1 VRAM usage after model load.'
    }
    $secondaryUsed = [int](($secondaryLine -split ',')[1].Trim())
    "P4_VRAM_GUARD gpu=1 used_mib=$secondaryUsed limit_mib=$MaxSecondaryVramMiB layer_range=$LayerBoundary-$LayerCount" |
        Set-Content -LiteralPath (Join-Path $out 'vram-guard.log') -Encoding utf8
    if ($secondaryUsed -gt $MaxSecondaryVramMiB) {
        $vramGuardFailed = $true
        throw "GPU1 VRAM guard rejected inference: used=${secondaryUsed}MiB limit=${MaxSecondaryVramMiB}MiB; reduce LayerBoundary or batch/context."
    }
    # The load-time check is only a floor. KV and compute buffers grow during
    # a long prefill/decode, so keep sampling the display GPU until the drive
    # exits and fail closed if the runtime crosses the limit.
    $peakSecondaryUsed = $secondaryUsed
    $peakSamples = @("P4_VRAM_SAMPLE elapsed_ms=0 used_mib=$secondaryUsed limit_mib=$MaxSecondaryVramMiB")
    $startedMonitoring = Get-Date
    while (-not $drive.HasExited) {
        $sampleLines = @(nvidia-smi --query-gpu=index,memory.used --format=csv,noheader,nounits 2>&1)
        $sampleLine = $sampleLines | Where-Object { $_ -match '^\s*1\s*,' } | Select-Object -First 1
        if ($null -ne $sampleLine) {
            $sampleUsed = [int](($sampleLine -split ',')[1].Trim())
            if ($sampleUsed -gt $peakSecondaryUsed) { $peakSecondaryUsed = $sampleUsed }
            $elapsedMs = [int]((Get-Date) - $startedMonitoring).TotalMilliseconds
            $peakSamples += "P4_VRAM_SAMPLE elapsed_ms=$elapsedMs used_mib=$sampleUsed limit_mib=$MaxSecondaryVramMiB"
            if ($sampleUsed -gt $MaxSecondaryVramMiB) {
                $vramGuardFailed = $true
                $peakSamples += "P4_VRAM_GUARD action=abort reason=peak_limit_exceeded"
                $peakSamples | Set-Content -LiteralPath (Join-Path $out 'vram-guard.log') -Encoding utf8
                Stop-Process -Id $drive.Id -Force -ErrorAction SilentlyContinue
                throw "GPU1 VRAM guard rejected inference during execution: used=${sampleUsed}MiB limit=${MaxSecondaryVramMiB}MiB; reduce LayerBoundary, Parallel, or context."
            }
        }
        Start-Sleep -Milliseconds 500
        $drive.Refresh()
    }
    $peakSamples | Set-Content -LiteralPath (Join-Path $out 'vram-guard.log') -Encoding utf8
    $drive.Refresh()
    Get-Content (Join-Path $out 'drive.log') -ErrorAction SilentlyContinue
    $driveExitCode = [int]$drive.ExitCode
    if ($driveExitCode -ne 0) { throw "drive exited with $driveExitCode; see $out" }
    Assert-StageExecution $ports
    [pscustomobject]@{ run_id=$RunId; passed=$true; output=$out; keep_loaded=[bool]$KeepLoaded; agents=$ports; batch_size=$BatchSize; ubatch_size=$UBatchSize; native_slots=$sequenceCapacity; context_size=$ContextSize; prompt_tokens=$PromptTokens; generation_tokens=$Tokens } |
        ConvertTo-Json -Depth 5 | Set-Content (Join-Path $out 'result.json') -Encoding utf8
} finally {
    # Explicit keep-loaded runs are diagnostic/throughput sessions. Preserve
    # the loaded agents even when the driver reports a failed request so the
    # caller can inspect state and continue a controlled follow-up. Ordinary
    # runs retain the fail-safe cleanup path.
    if (-not $KeepLoaded -or $vramGuardFailed) {
        if ($vramGuardFailed -and $null -ne $drive -and -not $drive.HasExited) {
            Stop-Process -Id $drive.Id -Force -ErrorAction SilentlyContinue
        }
        foreach ($agent in $agents) { if ($null -ne $agent -and -not $agent.HasExited) { Stop-Process -Id $agent.Id -Force -ErrorAction SilentlyContinue } }
    }
}
