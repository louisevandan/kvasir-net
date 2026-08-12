[CmdletBinding()]
param(
    [string]$Model = 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    [string]$LlamaServer = 'C:\Users\hika0\AppData\Local\Linker\llama\runtime\active\llama-server.exe',
    [ValidateRange(0, 1024)][int]$AgentWorkers = 0,
    [ValidateRange(1024, 65535)][int]$P4ListenPort = 19101,
    [ValidateRange(1, 256)][int]$Parallel = 8,
    [ValidateRange(256, 32768)][int]$ContextPerSlot = 1024,
    [ValidateRange(32, 8192)][int]$BatchSize = 2048,
    [ValidateRange(32, 8192)][int]$UBatchSize = 512,
    [ValidateRange(1, 4096)][int]$AdapterMaxInflight = 256,
    [ValidateRange(1, 4096)][int]$AdapterMaxQueued = 1024,
    [ValidateRange(1, 4096)][int]$AdapterBatchMax = 256,
    [ValidateRange(0, 60000)][int]$AdapterBatchLingerMs = 0,
    [ValidateRange(1, 256)][int]$ConcurrentRequests = 8
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$target = Join-Path $root 'target\real-e2e'
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
New-Item -ItemType Directory -Force -Path $target | Out-Null

if (!(Test-Path -LiteralPath $Model) -or !(Test-Path -LiteralPath $LlamaServer)) { throw 'model or llama-server.exe is missing' }

$owned = @()
function Start-Owned([string]$FilePath, [string[]]$Arguments, [string]$Name) {
    $out = Join-Path $target "$Name-$stamp.log"
    $err = Join-Path $target "$Name-$stamp.err.log"
    $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -RedirectStandardOutput $out -RedirectStandardError $err -WindowStyle Hidden -PassThru
    $script:owned += $process
    return $process
}
function Wait-Tcp([int]$Port, [int]$Seconds = 90) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        if ((Test-NetConnection -ComputerName '127.0.0.1' -Port $Port -InformationLevel Quiet -WarningAction SilentlyContinue)) { return }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "port $Port did not open within $Seconds seconds"
}
function Wait-Http([string]$Url, [int]$Seconds = 180) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri $Url -TimeoutSec 3
            if ($response.StatusCode -eq 200) { return }
        } catch {}
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "$Url did not become healthy within $Seconds seconds"
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
    cargo build --workspace | Out-Host
    $bin = Join-Path $root 'target\debug'
    $totalContext = $ContextPerSlot * $Parallel
    Start-Owned $LlamaServer @('--model', $Model, '--host', '127.0.0.1', '--port', '19104', '--ctx-size', [string]$totalContext, '--parallel', [string]$Parallel, '--batch-size', [string]$BatchSize, '--ubatch-size', [string]$UBatchSize, '--gpu-layers', '0') 'llama-server' | Out-Null
    Wait-Tcp 19104
    Wait-Http 'http://127.0.0.1:19104/health'
    $agentArguments = @("127.0.0.1:$P4ListenPort")
    if ($AgentWorkers -gt 0) { $agentArguments += @('--workers', [string]$AgentWorkers) }
    Start-Owned (Join-Path $bin 'p4-agent.exe') $agentArguments 'p4-agent' | Out-Null
    Wait-Log (Join-Path $target "p4-agent-$stamp.log") 'P4_AGENT_READY'
    $env:P4_LLAMACPP_MAX_INFLIGHT = [string]$AdapterMaxInflight
    $env:P4_LLAMACPP_MAX_QUEUED = [string]$AdapterMaxQueued
    $env:P4_LLAMACPP_BATCH_MAX = [string]$AdapterBatchMax
    $env:P4_LLAMACPP_BATCH_LINGER_MS = [string]$AdapterBatchLingerMs
    Start-Owned (Join-Path $bin 'p4-llamacpp.exe') @('127.0.0.1:19103', "127.0.0.1:$P4ListenPort", 'llamacpp-stock', 'http://127.0.0.1:19104', 'Qwen2.5-1.5B-Instruct-Q8_0.gguf') 'p4-llamacpp' | Out-Null
    Wait-Log (Join-Path $target "p4-llamacpp-$stamp.log") 'P4_LLAMACPP_READY'
    $output = & node (Join-Path $root 'tools\controller\inference\run-inference.mjs') "127.0.0.1:$P4ListenPort" 'gpu-0' 'Reply with exactly one Korean greeting.' 'Qwen2.5-1.5B-Instruct-Q8_0.gguf' ([string]$ConcurrentRequests) 2>&1
    $output | Out-Host
    $transcript = $output -join "`n"
    $doneCount = ([regex]::Matches($transcript, 'P4_DONE index=\d+ session=.*tokens=[1-9]')).Count
    if ($LASTEXITCODE -ne 0 -or $doneCount -ne $ConcurrentRequests) { throw "Node.js ControllerInstance completed $doneCount/$ConcurrentRequests real streams" }
    Write-Output "P4_REAL_E2E_PASS logs=$target"
}
finally {
    foreach ($process in $owned) { if (!$process.HasExited) { Stop-Process -Id $process.Id -Force } }
    Pop-Location
}
