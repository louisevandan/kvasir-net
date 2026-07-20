param(
    [ValidateSet("all", "rpc", "proxy")]
    [string]$Mode = "all",
    [string]$CudaVersion = "13.0.0",
    [string]$CudaArchs = "75;80;86;89;90;120;121",
    [int]$Jobs = 0,
    [switch]$RebuildToolchain,
    [string]$PrebuiltDir = "",
    [switch]$NoPrebuiltCache,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot

if ($Jobs -le 0) {
    # Keep CUDA/CMake builds fast on a well-provisioned Docker Desktop host
    # without allowing an unbounded compiler fan-out to exhaust memory.
    $logicalProcessors = [Environment]::ProcessorCount
    $Jobs = [Math]::Min(32, [Math]::Max(4, [Math]::Floor($logicalProcessors * 0.75)))
}

function Invoke-Checked([string[]]$Command) {
    Write-Host ("+ " + ($Command -join " "))
    if ($DryRun) { return }
    $executable = $Command[0]
    $arguments = @($Command[1..($Command.Length - 1)])
    & $executable @arguments
    if ($LASTEXITCODE -ne 0) { throw "command failed with exit code $LASTEXITCODE" }
}

function Image-Exists([string]$Image) {
    if ($DryRun) { return $false }
    & docker image inspect $Image *> $null
    return $LASTEXITCODE -eq 0
}

function Get-StringSha256([string]$Value) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value)
    $hash = [Security.Cryptography.SHA256]::Create().ComputeHash($bytes)
    return -join ($hash | ForEach-Object { $_.ToString("x2") })
}

function Restore-Prebuilt([string]$Image, [string]$Archive) {
    if (Image-Exists $Image) { return $true }
    if ($NoPrebuiltCache -or -not (Test-Path $Archive)) { return $false }
    Write-Host "= restore $Image from $Archive"
    Invoke-Checked @("docker", "load", "--input", $Archive)
    return Image-Exists $Image
}

function Save-Prebuilt([string]$Image, [string]$Archive, [hashtable]$Manifest) {
    if ($DryRun -or $NoPrebuiltCache) { return }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Archive) | Out-Null
    Write-Host "= cache $Image at $Archive"
    Invoke-Checked @("docker", "save", "--output", $Archive, $Image)
    $Manifest | ConvertTo-Json | Set-Content -Encoding utf8 ($Archive + ".json")
}

Push-Location $repo
try {
    # Docker contexts copy only tracked runtime inputs; unrelated user scratch
    # files and the ignored prebuilt cache must not block artifact reuse.
    $dirty = (& git status --porcelain --untracked-files=no)
    if ($dirty) {
        throw "artifact builds require a clean worktree; commit the exact sources before building"
    }
    $llamaRevision = (& git -C external/llama.cpp rev-parse HEAD).Trim()
    # Hash only inputs that affect the proxy binaries.  Controller/docs/script
    # commits must not invalidate an otherwise compatible CUDA artifact.
    $adapterObjects = @(& git rev-parse HEAD:CMakeLists.txt HEAD:apps HEAD:src HEAD:cmake HEAD:scripts/package-ring-runtime.py)
    $adapterRevision = Get-StringSha256 (($adapterObjects -join "`n") + "`n")
    $llamaShort = $llamaRevision.Substring(0, 12)
    $adapterShort = $adapterRevision.Substring(0, 12)
    $cudaTag = $CudaVersion.Replace(".", "-")
    $archTag = $CudaArchs.Replace(";", "-")
    $toolchain = "linkcpp-cuda-toolchain:$cudaTag"
    $llamaKey = "$llamaShort-cuda$cudaTag-sm$archTag"
    $proxyKey = "$llamaShort-$adapterShort-cuda$cudaTag-sm$archTag"
    $llamaBuild = "linkcpp-llama-build:$llamaKey"
    $rpcArtifact = "linkcpp-rpc-artifacts:$llamaKey"
    $proxyArtifact = "linkcpp-proxy-artifacts:$proxyKey"
    if (-not $PrebuiltDir) { $PrebuiltDir = Join-Path $repo "artifacts/prebuilt" }
    $containerPlatform = (& docker version --format "{{.Server.Os}}-{{.Server.Arch}}").Trim()
    $cacheDir = Join-Path $PrebuiltDir "$containerPlatform-cuda$cudaTag-sm$archTag/$llamaShort/$adapterShort"
    $rpcArchive = Join-Path $cacheDir "rpc-artifacts.tar"
    $proxyArchive = Join-Path $cacheDir "proxy-artifacts.tar"
    $manifest = @{ platform = $containerPlatform; cuda_version = $CudaVersion; cuda_archs = $CudaArchs;
                   llama_revision = $llamaRevision; adapter_revision = $adapterRevision; created_at = (Get-Date).ToUniversalTime().ToString("o") }

    if ($RebuildToolchain -or -not (Image-Exists $toolchain)) {
        Invoke-Checked @(
            "docker", "build", "-f", "docker/cuda/Dockerfile.toolchain",
            "--build-arg", "CUDA_DEVEL_IMAGE=nvidia/cuda:$CudaVersion-devel-ubuntu22.04",
            "-t", $toolchain, "-t", "linkcpp-cuda-toolchain:local", "docker/cuda"
        )
    } else {
        Write-Host "= reuse $toolchain"
        Invoke-Checked @("docker", "tag", $toolchain, "linkcpp-cuda-toolchain:local")
    }

    if ($Mode -in @("all", "rpc", "proxy")) {
        if (-not (Restore-Prebuilt $rpcArtifact $rpcArchive)) {
            Invoke-Checked @(
                "docker", "build", "-f", "docker/cuda/Dockerfile.llama", "--target", "llama-build",
                "--build-arg", "CUDA_TOOLCHAIN_IMAGE=$toolchain",
                "--build-arg", "CUDA_ARCHS=$CudaArchs", "--build-arg", "LINKCPP_BUILD_JOBS=$Jobs",
                "-t", $llamaBuild, "-t", "linkcpp-llama-build:local", "."
            )
            Invoke-Checked @(
                "docker", "build", "-f", "docker/cuda/Dockerfile.llama", "--target", "llama-artifacts",
                "--build-arg", "CUDA_TOOLCHAIN_IMAGE=$toolchain",
                "--build-arg", "CUDA_ARCHS=$CudaArchs", "--build-arg", "LINKCPP_BUILD_JOBS=$Jobs",
                "-t", $rpcArtifact, "-t", "linkcpp-rpc-artifacts:local", "."
            )
            $rpcManifest = $manifest.Clone()
            $rpcManifest.image = $rpcArtifact
            $rpcManifest.kind = "rpc"
            Save-Prebuilt $rpcArtifact $rpcArchive $rpcManifest
        }
    }

    if ($Mode -in @("all", "proxy")) {
        $buildId = "$llamaRevision.$adapterRevision"
        if (-not (Restore-Prebuilt $proxyArtifact $proxyArchive)) {
            if (-not (Image-Exists $llamaBuild)) {
                Invoke-Checked @(
                    "docker", "build", "-f", "docker/cuda/Dockerfile.llama", "--target", "llama-build",
                    "--build-arg", "CUDA_TOOLCHAIN_IMAGE=$toolchain",
                    "--build-arg", "CUDA_ARCHS=$CudaArchs", "--build-arg", "LINKCPP_BUILD_JOBS=$Jobs",
                    "-t", $llamaBuild, "-t", "linkcpp-llama-build:local", "."
                )
            }
            Invoke-Checked @(
                "docker", "build", "-f", "docker/cuda/Dockerfile.proxy", "--target", "proxy-artifacts",
                "--build-arg", "LLAMA_BUILD_IMAGE=$llamaBuild",
                "--build-arg", "LINKCPP_BUILD_JOBS=$Jobs",
                "--build-arg", "LINKCPP_RING_BUILD_ID=$buildId",
                "-t", $proxyArtifact, "-t", "linkcpp-proxy-artifacts:local", "."
            )
            $proxyManifest = $manifest.Clone()
            $proxyManifest.image = $proxyArtifact
            $proxyManifest.kind = "proxy"
            Save-Prebuilt $proxyArtifact $proxyArchive $proxyManifest
        }
        Invoke-Checked @("docker", "tag", $proxyArtifact, "linkcpp-proxy-artifacts:local")
    }

    [ordered]@{
        toolchain = $toolchain
        llama_build = $llamaBuild
        rpc_artifact = $rpcArtifact
        proxy_artifact = if ($Mode -in @("all", "proxy")) { $proxyArtifact } else { $null }
        prebuilt_cache_dir = $cacheDir
    } | ConvertTo-Json
} finally {
    Pop-Location
}
