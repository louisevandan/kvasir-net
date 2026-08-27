[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ArtifactDirectory,
    [Parameter(Mandatory = $true)][string]$Model,
    [Parameter(Mandatory = $true)][string]$MessagesFile,
    [Parameter(Mandatory = $true)][string]$OutputFile,
    [string]$SourceDirectory = '',
    [string]$SplitMarker = '',
    [string]$SeedOutputFile = '',
    [string]$SuffixOutputFile = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-CMake() {
    $fromPath = Get-Command cmake -ErrorAction SilentlyContinue
    if ($fromPath) { return $fromPath.Source }
    $candidates = @(
        'C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
    )
    $found = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $found) { throw 'cmake.exe not found' }
    return $found
}

foreach ($path in @($ArtifactDirectory, $Model, $MessagesFile)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "required path not found: $path" }
}
if ([string]::IsNullOrWhiteSpace($SplitMarker) -ne
    ([string]::IsNullOrWhiteSpace($SeedOutputFile) -and
     [string]::IsNullOrWhiteSpace($SuffixOutputFile))) {
    throw 'SplitMarker requires both SeedOutputFile and SuffixOutputFile'
}
$messages = @(Get-Content -LiteralPath $MessagesFile -Raw | ConvertFrom-Json)
$invalidMessages = @($messages | Where-Object {
    [string]::IsNullOrWhiteSpace($_.role) -or [string]::IsNullOrWhiteSpace($_.content)
})
if ($messages.Count -eq 0 -or $invalidMessages.Count -gt 0) {
    throw 'MessagesFile must contain non-empty role/content objects'
}

$artifact = (Resolve-Path $ArtifactDirectory).Path
$artifactBuild = Split-Path -Parent $artifact
$cache = Join-Path $artifactBuild 'CMakeCache.txt'
if (-not (Test-Path -LiteralPath $cache -PathType Leaf)) {
    throw "artifact CMake cache not found: $cache"
}
if ([string]::IsNullOrWhiteSpace($SourceDirectory)) {
    $match = Select-String -LiteralPath $cache -Pattern '^P4_STAGED_LLAMA_SOURCE_DIR:.*=(.*)$' |
        Select-Object -First 1
    if (-not $match) { throw 'artifact cache does not identify its llama.cpp source' }
    $SourceDirectory = $match.Matches[0].Groups[1].Value.Trim()
}
$source = (Resolve-Path $SourceDirectory).Path
$outputDirectory = Split-Path -Parent $OutputFile
if ([string]::IsNullOrWhiteSpace($outputDirectory)) { $outputDirectory = '.' }
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$outputDirectory = (Resolve-Path $outputDirectory).Path
$build = Join-Path $outputDirectory 'template-probe-build'
$project = Join-Path $PSScriptRoot 'tokenizer'
$cmake = Resolve-CMake

& $cmake -S $project -B $build -G 'Visual Studio 17 2022' -A x64 `
    "-DTOKENIZER_SOURCE_DIR=$source" `
    "-DTOKENIZER_ARTIFACT_DIR=$artifact" `
    "-DTOKENIZER_BUILD_DIR=$artifactBuild"
if ($LASTEXITCODE -ne 0) { throw 'chat template probe configure failed' }
& $cmake --build $build --config Release --target linker-chat-template-probe
if ($LASTEXITCODE -ne 0) { throw 'chat template probe build failed' }
$probe = Join-Path $build 'Release\linker-chat-template-probe.exe'
$previousPath = $env:PATH
try {
    $env:PATH = "$artifact;$previousPath"
    & $probe (Resolve-Path $Model).Path (Resolve-Path $MessagesFile).Path $OutputFile
    if ($LASTEXITCODE -ne 0) { throw 'chat template probe execution failed' }
} finally {
    $env:PATH = $previousPath
}
$prompt = Get-Content -LiteralPath $OutputFile -Raw
$markerIndex = -1
if (-not [string]::IsNullOrWhiteSpace($SplitMarker)) {
    $markerIndex = $prompt.IndexOf($SplitMarker, [StringComparison]::Ordinal)
    if ($markerIndex -lt 0 -or
        $prompt.IndexOf($SplitMarker, $markerIndex + $SplitMarker.Length,
            [StringComparison]::Ordinal) -ge 0) {
        throw 'rendered prompt must contain the split marker exactly once'
    }
    Set-Content -LiteralPath $SeedOutputFile -Value $prompt.Substring(0, $markerIndex) `
        -Encoding utf8NoBOM -NoNewline
    Set-Content -LiteralPath $SuffixOutputFile `
        -Value $prompt.Substring($markerIndex + $SplitMarker.Length) `
        -Encoding utf8NoBOM -NoNewline
}
[pscustomobject]@{
    status = 'passed'
    artifact_directory = $artifact
    artifact_llama_sha256 = (Get-FileHash (Join-Path $artifact 'llama.dll') -Algorithm SHA256).Hash
    artifact_common_sha256 = (Get-FileHash (Join-Path $artifact 'llama-common.dll') -Algorithm SHA256).Hash
    model = (Resolve-Path $Model).Path
    model_header_sha256 = (Get-FileHash -LiteralPath $Model -Algorithm SHA256).Hash
    messages = (Resolve-Path $MessagesFile).Path
    prompt = (Resolve-Path $OutputFile).Path
    prompt_chars = $prompt.Length
    split_marker_index = $markerIndex
    template_source = 'stock llama.cpp common_chat_templates_apply with model GGUF metadata and Jinja enabled'
} | ConvertTo-Json -Depth 6 | Set-Content `
    -LiteralPath (Join-Path $outputDirectory 'template-report.json') -Encoding utf8NoBOM
Write-Output "CHAT_TEMPLATE status=passed output=$OutputFile"
