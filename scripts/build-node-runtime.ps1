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

$flags = @(
    "-DCMAKE_BUILD_TYPE=Release",
    "-DGGML_RPC=ON",
    "-DLLAMA_CURL=ON",
    "-DLINKCPP_BUILD=OFF"
)

switch ($Backend) {
    "cuda" { $flags += "-DGGML_CUDA=ON" }
    "vulkan" { $flags += "-DGGML_VULKAN=ON" }
    "cpu" { }
}

$buildDir = $env:LINKCPP_NODE_BUILD_DIR
if (-not $buildDir) {
    $buildDir = "build-node-windows-$Backend"
}

& cmake -S . -B $buildDir @flags
& cmake --build $buildDir --config Release --target ggml-rpc-server

Write-Host "built $Backend node runtime: $buildDir\bin\Release\ggml-rpc-server.exe"
