param(
    [Parameter(Mandatory = $true)]
    [string]$Stamp,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot,
    [int]$MaxTokens = 600,
    [int]$ConcurrentRequests = 256,
    [int]$TotalRequests = 256,
    [int]$InitialRequests = 0,
    [int]$ArrivalIntervalMs = 0,
    [int]$NativeParallel = 16,
    [string]$LocalHost = "http://127.0.0.1:18089",
    [string]$RemoteGroupHost = "http://127.0.0.1:28083",
    [switch]$Local4080First,
    [switch]$NoProtocolBatch,
    [switch]$BenchmarkIgnoreEog,
    [string]$NodeBatchLimits
)

$ErrorActionPreference = "Stop"
$env:P4_E2E_RUN_ID = $Stamp
$env:P4_PIPELINE_MODEL = "unsloth\Ornith-1.0-35B-GGUF\Ornith-1.0-35B-UD-Q5_K_S.gguf"
$env:P4_PREFILL_PROMPT_FILE = "F:\dev\linkcpp_product\apps\p4\fixtures\prefill-prompts-ko-400t.json"
$env:P4_PIPELINE_BENCHMARK_IGNORE_EOG = if ($BenchmarkIgnoreEog) { "1" } else { "0" }
$effectiveInitialRequests = if ($InitialRequests -eq 0) { $ConcurrentRequests } else { $InitialRequests }
if ($TotalRequests -lt $ConcurrentRequests) { throw 'TotalRequests cannot be lower than ConcurrentRequests' }
if ($effectiveInitialRequests -gt $ConcurrentRequests) { throw 'InitialRequests cannot exceed ConcurrentRequests' }
if ($TotalRequests -gt $effectiveInitialRequests -and $ArrivalIntervalMs -eq 0) { throw 'ArrivalIntervalMs must be positive when TotalRequests exceeds InitialRequests' }
$env:P4_TOTAL_REQUESTS = "$TotalRequests"
$env:P4_INITIAL_REQUESTS = "$effectiveInitialRequests"
$env:P4_ARRIVAL_INTERVAL_MS = "$ArrivalIntervalMs"
$env:P4_EXECUTION_WINDOW = "$TotalRequests"
$env:P4_NATIVE_PARALLEL = "$NativeParallel"
$env:P4_PIPELINE_BATCH = if ($NoProtocolBatch) { "$NativeParallel" } else { "256" }
$env:P4_PIPELINE_UBATCH = if ($NoProtocolBatch) { "$NativeParallel" } else { "128" }
$env:P4_GPU_TELEMETRY = "1"
$env:P4_PIPELINE_EXPECT_SHARED_MEMORY = "1"
$env:P4_NODE_BATCH_LIMITS_JSON = $NodeBatchLimits
$localNodeIds = if ($Local4080First) { "local-4080,local-3090" } else { "local-3090,local-4080" }
$localGpuUuids = if ($Local4080First) {
    "GPU-79caabbe-c843-631f-3cea-9c01e652c78c,GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed"
} else {
    "GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed,GPU-79caabbe-c843-631f-3cea-9c01e652c78c"
}
$localVramGib = if ($Local4080First) { "12,23" } else { "23,12" }
$env:P4_PIPELINE_NODE_IDS = "$localNodeIds,remote-3090-a,remote-3090-b"
$env:P4_PIPELINE_NODE_GPU_UUIDS = "$localGpuUuids,GPU-98d47fe7-caf3-3991-bf94-398029e93c31,GPU-9983bf45-0d89-5b0f-0ec6-3525016f28fe"
$env:P4_PIPELINE_NODE_VRAM_GIB = "$localVramGib,23,23"
$env:P4_PIPELINE_NODE_CORES = "24,24,24,24"
$env:P4_PIPELINE_STAGE_HOSTS = "192.168.0.6,192.168.0.6,192.168.0.29,192.168.0.29"
$env:P4_PIPELINE_REMOTE_AGENT_ENDPOINT = "127.0.0.1:29201"
$env:P4_PIPELINE_REMOTE_GROUP_HOST = $RemoteGroupHost
$env:P4_PIPELINE_REMOTE_STAGE_NODE_IDS = "remote-3090-a,remote-3090-b"
$env:P4_PIPELINE_STAGE_NODE_ORDER = "$localNodeIds,remote-3090-a,remote-3090-b"
$env:P4_PIPELINE_STAGE_PORT_BASE = "52001"

$trace = Join-Path $OutputRoot "trace-$Stamp.jsonl"
$plan = Join-Path $OutputRoot "plan-$Stamp.json"
$summary = Join-Path $OutputRoot "summary-$Stamp.json"
$report = Join-Path $OutputRoot "report-$Stamp.md"

Set-Location "F:\dev\linkcpp_product\apps\p4"
$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false
& "C:\Program Files\nodejs\node.exe" `
    "tools\controller\experiments\pipeline-e2e\run-pipeline-e2e.mjs" `
    "127.0.0.1:19201" `
    "pipeline-2gpu-$Stamp" `
    $LocalHost `
    "preflight" `
    "$MaxTokens" `
    "$TotalRequests" `
    "$ConcurrentRequests" `
    "0" `
    $trace `
    $plan `
    $summary `
    $report `
    "1"
exit $LASTEXITCODE
