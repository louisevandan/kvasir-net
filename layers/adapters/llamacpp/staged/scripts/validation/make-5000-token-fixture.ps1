[CmdletBinding()]
param(
    [string]$Model = 'S:\models\unsloth\Qwen3.8-27B-GGUF\Qwen3.8-27B-Q6_K.gguf',
    [string]$ArtifactDirectory = '',
    [string]$SourceDirectory = '',
    [int]$TargetTokens = 5000,
    [string]$SeedFile = '',
    [string]$RequiredSuffixFile = '',
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Require-File([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Label not found: $Path" }
}

function Require-Directory([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { throw "$Label not found: $Path" }
}

function Find-RepositoryRoot() {
    $current = (Resolve-Path $PSScriptRoot).Path
    for ($index = 0; $index -lt 10; $index++) {
        if (Test-Path -LiteralPath (Join-Path $current 'package.json')) { return $current }
        $parent = Split-Path -Parent $current
        if ($parent -eq $current) { break }
        $current = $parent
    }
    throw 'repository root could not be found'
}

function Read-CacheValue([string]$CachePath, [string]$Name) {
    $match = Select-String -LiteralPath $CachePath -Pattern "^$([regex]::Escape($Name)):.*=(.*)$" | Select-Object -First 1
    if (-not $match) { throw "CMake cache is missing $Name`: $CachePath" }
    return $match.Matches[0].Groups[1].Value.Trim()
}

function Resolve-CMake() {
    $fromPath = Get-Command cmake -ErrorAction SilentlyContinue
    if ($fromPath) { return $fromPath.Source }
    $candidates = @(
        'C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
    )
    $found = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $found) { throw 'cmake.exe not found on PATH or in Visual Studio Build Tools' }
    return $found
}

$root = Find-RepositoryRoot
if ([string]::IsNullOrWhiteSpace($ArtifactDirectory)) {
    $ArtifactDirectory = Join-Path $root '.cache\staged-server-cuda-real-20260818\Release'
}
$ArtifactDirectory = (Resolve-Path $ArtifactDirectory).Path
$artifactBuildDirectory = Split-Path -Parent $ArtifactDirectory
$cachePath = Join-Path $artifactBuildDirectory 'CMakeCache.txt'
Require-File $cachePath 'CUDA artifact CMake cache'
Require-File $Model 'GGUF model'
foreach ($required in @('p4_staged_server.exe', 'llama.dll', 'ggml-cuda.dll')) {
    Require-File (Join-Path $ArtifactDirectory $required) "CUDA artifact $required"
}
if ((Read-CacheValue $cachePath 'GGML_CUDA') -ne 'ON' -or
    (Read-CacheValue $cachePath 'P4_STAGED_CUDA') -ne 'ON') {
    throw "artifact cache is not a CUDA build: $cachePath"
}
if ([string]::IsNullOrWhiteSpace($SourceDirectory)) {
    $SourceDirectory = Read-CacheValue $cachePath 'P4_STAGED_LLAMA_SOURCE_DIR'
}
$SourceDirectory = (Resolve-Path $SourceDirectory).Path
Require-File (Join-Path $SourceDirectory 'include\llama.h') 'prepared llama.cpp public header'
Require-File (Join-Path $SourceDirectory 'CMakeLists.txt') 'prepared llama.cpp source'

$sourceCommit = (& git -C $SourceDirectory rev-parse HEAD 2>$null | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $sourceCommit.Length -eq 0) { throw "prepared source is not a Git worktree: $SourceDirectory" }
$sourceStatus = @(& git -C $SourceDirectory status --short)
$runId = Get-Date -Format 'yyyyMMddHHmmss'
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $root "target\validation-5000-token\$runId"
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$fixture = Join-Path $OutputDirectory "prompt-$TargetTokens-tokens.txt"
$tokenizerBuild = Join-Path $OutputDirectory 'tokenizer-build'
$tokenizerProject = Join-Path $PSScriptRoot 'tokenizer'
$cmake = Resolve-CMake

& $cmake -S $tokenizerProject -B $tokenizerBuild -G 'Visual Studio 17 2022' -A x64 `
    "-DTOKENIZER_SOURCE_DIR=$SourceDirectory"
if ($LASTEXITCODE -ne 0) { throw 'tokenizer probe CMake configure failed' }
& $cmake --build $tokenizerBuild --config Release --target linker-tokenizer-probe
if ($LASTEXITCODE -ne 0) { throw 'tokenizer probe build failed' }
$probe = Join-Path $tokenizerBuild 'Release\linker-tokenizer-probe.exe'
Require-File $probe 'tokenizer probe executable'

$probeStdoutPath = Join-Path $OutputDirectory 'tokenizer-probe.stdout.txt'
$probeStderrPath = Join-Path $OutputDirectory 'tokenizer-probe.stderr.txt'
$probeArguments = @(
    '--artifact-directory', "`"$ArtifactDirectory`"",
    '--model', "`"$Model`"",
    '--output', "`"$fixture`"",
    '--target', "$TargetTokens"
)
if (-not [string]::IsNullOrWhiteSpace($SeedFile)) {
    Require-File $SeedFile 'prompt seed file'
    $probeArguments += @('--seed-file', "`"$((Resolve-Path $SeedFile).Path)`"")
}
if (-not [string]::IsNullOrWhiteSpace($RequiredSuffixFile)) {
    Require-File $RequiredSuffixFile 'required prompt suffix file'
    $probeArguments += @('--required-suffix-file', "`"$((Resolve-Path $RequiredSuffixFile).Path)`"")
}
$probeProcess = Start-Process -FilePath $probe -ArgumentList $probeArguments `
    -RedirectStandardOutput $probeStdoutPath -RedirectStandardError $probeStderrPath `
    -Wait -PassThru -NoNewWindow
$probeExitCode = $probeProcess.ExitCode
$probeStdout = if (Test-Path -LiteralPath $probeStdoutPath) {
    Get-Content -LiteralPath $probeStdoutPath -Raw
} else { '' }
$probeStderr = if (Test-Path -LiteralPath $probeStderrPath) {
    Get-Content -LiteralPath $probeStderrPath -Raw
} else { '' }
$probeOutput = $probeStdout + $probeStderr
if ($probeExitCode -ne 0) { throw "tokenizer probe failed:`n$probeOutput" }
$probeOutput | Set-Content -LiteralPath (Join-Path $OutputDirectory 'tokenizer-probe.txt') -Encoding utf8
$modelIdentity = (Resolve-Path $Model).Path
$hashes = foreach ($file in @('p4_staged_server.exe', 'llama.dll', 'ggml-cuda.dll', 'cublas64_13.dll', 'cublasLt64_13.dll')) {
    $path = Join-Path $ArtifactDirectory $file
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        $item = Get-Item -LiteralPath $path
        [pscustomobject]@{ name = $file; bytes = $item.Length; sha256 = (Get-FileHash $path -Algorithm SHA256).Hash }
    }
}
$sourceHashes = foreach ($file in @('include\llama.h', 'src\llama-model.cpp', 'src\llama.cpp')) {
    $path = Join-Path $SourceDirectory $file
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        [pscustomobject]@{ name = $file; sha256 = (Get-FileHash $path -Algorithm SHA256).Hash }
    }
}
$tokenCount = [int](([regex]::Match($probeOutput, 'TOKEN_COUNT=(\d+)')).Groups[1].Value)
if ($tokenCount -ne $TargetTokens) { throw "probe reported $tokenCount tokens, expected $TargetTokens" }
$report = [pscustomobject]@{
    status = 'passed'
    model = $modelIdentity
    model_sha256 = (Get-FileHash $modelIdentity -Algorithm SHA256).Hash
    target_tokens = $TargetTokens
    fixture = (Resolve-Path $fixture).Path
    fixture_bytes = (Get-Item -LiteralPath $fixture).Length
    artifact_directory = $ArtifactDirectory
    artifact_cmake_cache = $cachePath
    artifact_hashes = @($hashes)
    prepared_source = $SourceDirectory
    prepared_source_commit = $sourceCommit
    prepared_source_dirty_paths = @($sourceStatus)
    prepared_source_hashes = @($sourceHashes)
    tokenizer_probe = $probe
    tokenizer_mode = 'artifact llama.dll; vocab_only=true; add_bos from GGUF; parse_special=true'
    seed_file = if ([string]::IsNullOrWhiteSpace($SeedFile)) { $null } else { (Resolve-Path $SeedFile).Path }
    required_suffix_file = if ([string]::IsNullOrWhiteSpace($RequiredSuffixFile)) { $null } else { (Resolve-Path $RequiredSuffixFile).Path }
    inference = 'not-run'
    explicit_unload = 'not-called'
} | ConvertTo-Json -Depth 8
$report | Set-Content -LiteralPath (Join-Path $OutputDirectory 'report.json') -Encoding utf8
Write-Output "FIXTURE output=$OutputDirectory"
Write-Output "FIXTURE path=$fixture"
Write-Output "FIXTURE tokens=$tokenCount bytes=$((Get-Item -LiteralPath $fixture).Length)"
Write-Output 'FIXTURE inference=not-run explicit_unload=not-called'
