param(
    [ValidateSet("rpc", "proxy")]
    [string]$Mode = "rpc",
    [string]$ArtifactImage = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$previousProxyArtifact = $env:LINKCPP_PROXY_ARTIFACT_IMAGE
$previousRpcArtifact = $env:LINKCPP_RPC_ARTIFACT_IMAGE
Push-Location $repo
try {
    if (-not $ArtifactImage) {
        $ArtifactImage = if ($Mode -eq "proxy") {
            "linkcpp-proxy-artifacts:local"
        } else {
            "linkcpp-rpc-artifacts:local"
        }
    }
    & docker image inspect $ArtifactImage *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "artifact image is missing: $ArtifactImage; run scripts/build-cuda-artifacts.ps1 first"
    }
    if ($Mode -eq "proxy") {
        $env:LINKCPP_PROXY_ARTIFACT_IMAGE = $ArtifactImage
        & docker compose -f docker-compose.yml -f docker-compose.proxy.yml build hub
    } else {
        $env:LINKCPP_RPC_ARTIFACT_IMAGE = $ArtifactImage
        & docker compose -f docker-compose.yml -f docker-compose.cuda.yml build hub
    }
    if ($LASTEXITCODE -ne 0) { throw "runtime image assembly failed" }
} finally {
    if ($null -eq $previousProxyArtifact) {
        Remove-Item Env:LINKCPP_PROXY_ARTIFACT_IMAGE -ErrorAction SilentlyContinue
    } else {
        $env:LINKCPP_PROXY_ARTIFACT_IMAGE = $previousProxyArtifact
    }
    if ($null -eq $previousRpcArtifact) {
        Remove-Item Env:LINKCPP_RPC_ARTIFACT_IMAGE -ErrorAction SilentlyContinue
    } else {
        $env:LINKCPP_RPC_ARTIFACT_IMAGE = $previousRpcArtifact
    }
    Pop-Location
}
