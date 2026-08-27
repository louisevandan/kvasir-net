Set-StrictMode -Version Latest

function Resolve-P4CanonicalPath([string]$Value) {
    (Resolve-Path -LiteralPath $Value).Path
}

function Test-P4SamePath([string]$Left, [string]$Right) {
    [string]::Equals(
        [IO.Path]::GetFullPath($Left),
        [IO.Path]::GetFullPath($Right),
        [StringComparison]::OrdinalIgnoreCase)
}

function Get-P4TextSha256([string]$Text) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
}

function Assert-P4FixtureProvenance {
    param(
        [Parameter(Mandatory)][string]$PromptsFile,
        [Parameter(Mandatory)][string]$ResponsesFile,
        [Parameter(Mandatory)][string]$ArtifactDirectory,
        [Parameter(Mandatory)][string]$Model,
        [Parameter(Mandatory)][int]$RequestCount,
        [Parameter(Mandatory)][int]$PromptTokens
    )
    if ($RequestCount -lt 1 -or $PromptTokens -lt 1) {
        throw 'Fixture request and token counts must be positive.'
    }
    $promptsPath = Resolve-P4CanonicalPath $PromptsFile
    $responsesPath = Resolve-P4CanonicalPath $ResponsesFile
    $artifactPath = Resolve-P4CanonicalPath $ArtifactDirectory
    $modelPath = Resolve-P4CanonicalPath $Model
    $fixtureRoot = Split-Path -Parent $promptsPath
    $reportPath = Join-Path $fixtureRoot 'report.json'
    $templatePath = Join-Path $fixtureRoot 'template-report.json'
    $manifestPath = Join-Path $fixtureRoot 'manifest.json'
    foreach ($file in @($reportPath,$templatePath,$manifestPath,
        (Join-Path $artifactPath 'llama.dll'),(Join-Path $artifactPath 'llama-common.dll'))) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "Fixture provenance file not found: $file"
        }
    }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $template = Get-Content -LiteralPath $templatePath -Raw | ConvertFrom-Json
    $manifest = @(Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json)
    $prompts = @(Get-Content -LiteralPath $promptsPath -Raw | ConvertFrom-Json)
    $responses = @(Get-Content -LiteralPath $responsesPath -Raw | ConvertFrom-Json)
    foreach ($binding in @(
        @($promptsPath,$report.prompts_json_sha256,'prompts'),
        @($responsesPath,$report.response_expectations_json_sha256,'responses'),
        @($manifestPath,$report.manifest_sha256,'manifest'),
        @($templatePath,$report.template_report_sha256,'template report')
    )) {
        $actual = (Get-FileHash -LiteralPath $binding[0] -Algorithm SHA256).Hash
        if ($actual -ne $binding[1]) { throw "Fixture $($binding[2]) content mismatch." }
    }
    $expectedTokenizerMode = 'artifact llama.dll; vocab_only=true; add_special=true; parse_special=true; BOS/EOS policies from GGUF'
    if ($report.status -ne 'passed' -or $template.status -ne 'passed' -or
        $report.tokenizer_mode -ne $expectedTokenizerMode -or
        $report.target_tokens -ne $PromptTokens -or
        $report.request_count -lt $RequestCount -or
        $report.distinct_prompts -lt $RequestCount -or
        $prompts.Count -lt $RequestCount -or $responses.Count -lt $RequestCount -or
        $manifest.Count -lt $RequestCount) {
        throw 'Fixture report does not cover the requested exact-token run.'
    }
    $p4Root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
    $fixtureSources = @{
        generator_source_sha256 = Join-Path $p4Root 'layers\adapters\llamacpp\staged\scripts\validation\make-gate3-prompts.ps1'
        tokenizer_source_sha256 = Join-Path $p4Root 'layers\adapters\llamacpp\staged\scripts\validation\tokenizer\llama-token-count.cpp'
        template_renderer_source_sha256 = Join-Path $p4Root 'layers\adapters\llamacpp\staged\scripts\validation\render-chat-template.ps1'
    }
    foreach ($field in $fixtureSources.Keys) {
        $actual = (Get-FileHash -LiteralPath $fixtureSources[$field] -Algorithm SHA256).Hash
        if ($actual -ne $report.$field) {
            throw "Fixture generator source mismatch: $field"
        }
    }
    foreach ($pair in @(
        @($report.prompts_json,$promptsPath),
        @($report.response_expectations_json,$responsesPath),
        @($report.artifact_directory,$artifactPath),
        @($report.model,$modelPath),
        @($template.artifact_directory,$artifactPath),
        @($template.model,$modelPath)
    )) {
        if (-not (Test-P4SamePath $pair[0] $pair[1])) {
            throw "Fixture provenance path mismatch: reported=$($pair[0]) actual=$($pair[1])"
        }
    }
    $llamaHash = (Get-FileHash -LiteralPath (Join-Path $artifactPath 'llama.dll') -Algorithm SHA256).Hash
    $commonHash = (Get-FileHash -LiteralPath (Join-Path $artifactPath 'llama-common.dll') -Algorithm SHA256).Hash
    if ($llamaHash -ne $template.artifact_llama_sha256 -or
        $commonHash -ne $template.artifact_common_sha256) {
        throw 'Fixture was tokenized or rendered by different llama.cpp artifacts.'
    }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    for ($index = 0; $index -lt $RequestCount; $index++) {
        $entry = $manifest[$index]
        $fixturePath = Resolve-P4CanonicalPath $entry.path
        if ($entry.request -ne ($index + 1) -or $entry.token_count -ne $PromptTokens -or
            -not (Test-P4SamePath (Split-Path -Parent $fixturePath) $fixtureRoot)) {
            throw "Fixture manifest identity mismatch at request $($index + 1)."
        }
        $fileHash = (Get-FileHash -LiteralPath $fixturePath -Algorithm SHA256).Hash
        $textHash = Get-P4TextSha256 ([string]$prompts[$index])
        if ($fileHash -ne $entry.sha256 -or $textHash -ne $entry.sha256) {
            throw "Fixture content hash mismatch at request $($index + 1)."
        }
        if (-not $seen.Add([string]$prompts[$index])) {
            throw "Fixture prompt is duplicated at request $($index + 1)."
        }
    }
    [pscustomobject]@{
        status = 'passed'
        request_count = $RequestCount
        prompt_tokens = $PromptTokens
        prompts_sha256 = (Get-FileHash -LiteralPath $promptsPath -Algorithm SHA256).Hash.ToLowerInvariant()
        responses_sha256 = (Get-FileHash -LiteralPath $responsesPath -Algorithm SHA256).Hash.ToLowerInvariant()
        llama_sha256 = $llamaHash.ToLowerInvariant()
        llama_common_sha256 = $commonHash.ToLowerInvariant()
        model = $modelPath
        fixture_root = $fixtureRoot
        tokenizer_mode = $report.tokenizer_mode
        template_mode = $report.template_mode
    }
}

Export-ModuleMember -Function Assert-P4FixtureProvenance
