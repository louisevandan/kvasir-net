[CmdletBinding()]
param(
    [string]$Target = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path 'target\remote-3090x2-20260810'),
    [string]$Stamp = 'four-stage-preflight-202608101124',
    [string]$LlamaHost = 'http://127.0.0.1:18082',
    [ValidateRange(1, 1024)][int]$MaxTokens = 16,
    [ValidateRange(1, 256)][int]$Parallel = 1,
    [ValidateRange(1, 256)][int]$ConcurrentRequests = 1,
    [ValidateRange(1, 256)][int]$ExecutionWindow = 1,
    [switch]$Benchmark,
    [switch]$BenchmarkIgnoreEog,
    [switch]$PreserveFailedGroup
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$env:P4_PIPELINE_MODEL = 'unsloth\Ornith-1.0-35B-GGUF\Ornith-1.0-35B-UD-Q5_K_S.gguf'
$env:P4_PIPELINE_NODE_IDS = 'local-3090,local-4080,remote-3090-a,remote-3090-b'
$env:P4_PIPELINE_NODE_GPU_UUIDS = 'GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed,GPU-79caabbe-c843-631f-3cea-9c01e652c78c,GPU-98d47fe7-caf3-3991-bf94-398029e93c31,GPU-9983bf45-0d89-5b0f-0ec6-3525016f28fe'
$env:P4_PIPELINE_NODE_VRAM_GIB = '23,12,23,23'
$env:P4_PIPELINE_NODE_CORES = '24,24,24,24'
$env:P4_PIPELINE_STAGE_HOSTS = '192.168.0.6,192.168.0.6,192.168.0.29,192.168.0.29'
$env:P4_PIPELINE_REMOTE_AGENT_ENDPOINT = '127.0.0.1:29201'
$env:P4_PIPELINE_REMOTE_GROUP_HOST = 'http://127.0.0.1:28083'
$env:P4_PIPELINE_REMOTE_STAGE_NODE_IDS = 'remote-3090-a,remote-3090-b'
$env:P4_PIPELINE_STAGE_PORT_BASE = '52001'
$env:P4_PREFILL_PROMPT_FILE = Join-Path $root 'fixtures\prefill-prompts-ko-400t.json'
$env:P4_EXECUTION_WINDOW = [string]$ExecutionWindow
$env:P4_GPU_TELEMETRY = '1'
$env:P4_PIPELINE_BENCHMARK_IGNORE_EOG = if ($BenchmarkIgnoreEog) { '1' } else { '0' }
$env:P4_E2E_RUN_ID = $Stamp

$trace = Join-Path $Target "trace-$Stamp.jsonl"
$plan = Join-Path $Target "plan-$Stamp.json"
$summary = Join-Path $Target "summary-$Stamp.json"
$report = Join-Path $Target "report-$Stamp.md"
$log = Join-Path $Target "client-$Stamp.log"
$errorLog = Join-Path $Target "client-$Stamp.err.log"
$nodeId = "pipeline-2gpu-$Stamp"
$preserve = if ($PreserveFailedGroup) { 1 } else { 0 }
$benchmarkFlag = if ($Benchmark) { 1 } else { 0 }
$arguments = "tools\controller\experiments\pipeline-e2e\run-pipeline-e2e.mjs 127.0.0.1:19201 $nodeId $LlamaHost preflight $MaxTokens $Parallel $ConcurrentRequests $preserve $trace $plan $summary $report $benchmarkFlag"
$process = Start-Process -FilePath (Get-Command node -ErrorAction Stop).Source -WorkingDirectory $root -ArgumentList $arguments -RedirectStandardOutput $log -RedirectStandardError $errorLog -WindowStyle Hidden -PassThru
[pscustomobject]@{ pid = $process.Id; stamp = $Stamp; log = $log; error_log = $errorLog; summary = $summary; report = $report } | ConvertTo-Json -Compress
