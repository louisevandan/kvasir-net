param(
    [ValidateSet("auto", "cuda", "vulkan", "cpu")]
    [string]$Backend = "auto"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Repo

if ($Backend -eq "auto") {
    $hasNvidia = $false
    try {
        & nvidia-smi | Out-Null
        $hasNvidia = $true
    } catch {
        $hasNvidia = $false
    }
    if ($hasNvidia) {
        $Backend = "cuda"
    } else {
        $Backend = "cpu"
    }
}

$buildDir = $env:LINKCPP_NODE_BUILD_DIR
if (-not $buildDir) {
    $buildDir = "build-node-windows-$Backend"
}

$rpcBin = $env:RPC_BIN
if (-not $rpcBin) {
    $candidate = Join-Path $Repo "$buildDir\bin\Release\ggml-rpc-server.exe"
    if (-not (Test-Path $candidate)) {
        $candidate = Join-Path $Repo "$buildDir\bin\ggml-rpc-server.exe"
    }
    $rpcBin = $candidate
}

if (-not (Test-Path $rpcBin)) {
    if ($env:RPC_BIN) {
        throw "RPC_BIN does not exist: $env:RPC_BIN"
    }
    & (Join-Path $PSScriptRoot "build-node-runtime.ps1") -Backend $Backend
}

$venv = $env:LINKCPP_NODE_VENV
if (-not $venv) {
    $venv = ".venv-linkcpp-node"
}

if ($env:LINKCPP_SKIP_VENV -ne "1") {
    & py -3 -m venv $venv
    $python = Join-Path $Repo "$venv\Scripts\python.exe"
    & $python -m pip install --upgrade pip | Out-Null
    & $python -m pip install fastapi "uvicorn[standard]" httpx gguf numpy python-multipart | Out-Null
} else {
    $python = "python"
}

$env:PYTHONPATH = "$Repo;$env:PYTHONPATH"
$env:RPC_BIN = $rpcBin
$env:LINKCPP_LLAMA_CPP_BACKEND = $Backend
if (-not $env:LINKCPP_MODEL_DIR) { $env:LINKCPP_MODEL_DIR = Join-Path $Repo "models" }
if (-not $env:LINKCPP_RPC_CACHE) { $env:LINKCPP_RPC_CACHE = Join-Path $env:USERPROFILE ".cache\linkcpp\rpc" }
if (-not $env:LINKCPP_NODE_STATE) { $env:LINKCPP_NODE_STATE = Join-Path $env:USERPROFILE ".cache\linkcpp\node-state.json" }
if (-not $env:LINKCPP_WORKER_LOG) { $env:LINKCPP_WORKER_LOG = Join-Path $env:USERPROFILE ".cache\linkcpp\worker.log" }
if (-not $env:LINKCPP_RPC_PORT) { $env:LINKCPP_RPC_PORT = "50052" }

New-Item -ItemType Directory -Force -Path $env:LINKCPP_MODEL_DIR | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $env:LINKCPP_NODE_STATE) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path $env:LINKCPP_WORKER_LOG) | Out-Null
New-Item -ItemType Directory -Force -Path $env:LINKCPP_RPC_CACHE | Out-Null

$hostName = $env:LINKCPP_NODE_HOST
if (-not $hostName) { $hostName = "0.0.0.0" }
$port = $env:LINKCPP_NODE_AGENT_PORT
if (-not $port) { $port = "9101" }

Write-Host "node agent backend=$Backend rpc=$env:RPC_BIN api=http://$hostName`:$port rpc_port=$env:LINKCPP_RPC_PORT"
& $python -m uvicorn controller.nodeagent:app --host $hostName --port $port
