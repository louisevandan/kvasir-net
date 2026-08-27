[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Model,
    [Parameter(Mandatory = $true)][string]$ArtifactDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [int]$TargetTokens = 500,
    [int]$RequestCount = 40,
    [int]$MinimumGeneratedTokens = 180
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($TargetTokens -le 0 -or $RequestCount -le 0 -or $MinimumGeneratedTokens -le 0) {
    throw 'token and request counts must be positive'
}
$fixtureScript = Join-Path $PSScriptRoot 'make-5000-token-fixture.ps1'
$templateScript = Join-Path $PSScriptRoot 'render-chat-template.ps1'
foreach ($path in @($Model, (Join-Path $ArtifactDirectory 'p4_staged_server.exe'),
    (Join-Path $ArtifactDirectory 'llama-common.dll'), $fixtureScript, $templateScript)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "required file not found: $path"
    }
}
$Model = (Resolve-Path $Model).Path
$ArtifactDirectory = (Resolve-Path $ArtifactDirectory).Path
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$reference = @'
Rust manages memory through ownership rather than a tracing garbage collector. Every owned value has one owner, and assigning an owned String to another binding moves ownership unless the value is explicitly cloned. Shared references permit concurrent reading, while a mutable reference permits mutation only while competing aliases are excluded. Borrowing lets code use a value without taking ownership, and the borrow checker verifies that references never outlive the data they refer to. Lifetime annotations describe relationships between reference validity ranges; they do not extend the lifetime of an object.

Rust represents recoverable failure with Result<T, E>. The question-mark operator unwraps Ok values and returns an error early after applying any required conversion. Optional values use Option<T>, whose Some variant contains a value and whose None variant represents absence. Pattern matching is exhaustive, so code must handle every relevant enum variant or provide a catch-all arm. Traits describe shared behavior, and a bound such as T: Display requires the selected type to implement Display.

Concurrency traits express distinct guarantees. Send permits ownership of a value to move between threads, while Sync permits shared references to be used from multiple threads. An async function returns a Future. An executor repeatedly polls that Future, and await suspends the current async computation until progress is possible. Cargo features are named additive configuration flags declared in Cargo.toml; enabling more than one feature normally combines them rather than selecting exactly one. Conditional compilation can inspect a feature with cfg(feature = "name"). These rules let generic libraries expose optional behavior without moving client-specific policy into their core.

An implementation should distinguish mechanism from consequence. A move changes which binding owns a value; a borrow temporarily grants access under aliasing rules; a clone creates another owned allocation. A lifetime annotation constrains relationships checked by the compiler but cannot keep a dropped value alive. Result and Option make failure and absence explicit in the type system. Send and Sync concern thread transfer and shared access, not asynchronous scheduling. Future and await concern cooperative progress, not automatic parallel execution. Cargo features select code at compilation and should not be confused with runtime flags.
'@

$tasks = @(
    @{ name = 'ownership'; fact = 'For an owned String, `let b = a;` moves the value to b. The old binding a is unusable unless the String was cloned before the move.'; focus = 'ownership transfers to b' },
    @{ name = 'mutable-borrow'; fact = 'A mutable reference `&mut T` permits mutation and requires exclusive access for the duration of that borrow.'; focus = 'exclusive mutable access' },
    @{ name = 'lifetimes'; fact = 'Lifetime annotations describe validity relationships between references. They do not extend how long the referenced object lives.'; focus = 'do not extend object lifetime' },
    @{ name = 'result-question-mark'; fact = 'On Result, the question-mark operator extracts Ok and returns Err early, applying From conversion when required.'; focus = 'returns an error early' },
    @{ name = 'option'; fact = 'Option uses Some to carry a present value and None to represent absence without a null reference.'; focus = 'Some and None' },
    @{ name = 'send-sync'; fact = 'Send allows ownership transfer between threads. Sync allows shared references to be accessed from multiple threads.'; focus = 'Send transfers ownership; Sync permits shared references' },
    @{ name = 'async-future'; fact = 'An async function returns a Future. The executor polls it, and await suspends the current computation until progress is possible.'; focus = 'executor polls the Future' },
    @{ name = 'trait-bound'; fact = 'A bound written `T: Display` requires T to implement the Display trait before the generic operation may format it.'; focus = 'T implements Display' },
    @{ name = 'exhaustive-match'; fact = 'A match over an enum must cover every possible variant explicitly or through an applicable catch-all pattern.'; focus = 'match must be exhaustive' },
    @{ name = 'cargo-features'; fact = 'Cargo features are named additive flags declared in Cargo.toml and tested with `cfg(feature = "name")`.'; focus = 'features are additive' }
)
$marker = 'P4_SEMANTIC_PREFIX_BOUNDARY_7A29E4'
$fixturePaths = @()
$expectations = @()
$manifest = @()
$baseFixtures = @()
$probe = $null
$baseCount = [Math]::Min($tasks.Count, $RequestCount)
$allMarkers = @(1..$RequestCount | ForEach-Object { 'P4-RUST-{0:D3}' -f $_ })

for ($index = 0; $index -lt $baseCount; $index++) {
    $number = $index + 1
    $task = $tasks[$index]
    $requestMarker = $allMarkers[$index]
    $messagesPath = Join-Path $OutputDirectory ('messages-base-{0:D2}.json' -f $number)
    $renderedPath = Join-Path $OutputDirectory ('rendered-base-{0:D2}.txt' -f $number)
    $seedPath = Join-Path $OutputDirectory ('seed-base-{0:D2}.txt' -f $number)
    $suffixPath = Join-Path $OutputDirectory ('suffix-base-{0:D2}.txt' -f $number)
    $baseFixture = Join-Path $OutputDirectory `
        ('prompt-base-{0:D2}-{1}-tokens.txt' -f $number, $TargetTokens)
    $user = @"
$reference

$marker
Authoritative fact for this request: $($task.fact)

Validation request marker: $requestMarker
Explain the Rust feature named '$($task.name)' using only the supplied reference. Write 180 to 300 English tokens. Begin with the marker '$requestMarker'. Use all four headings Claim:, Mechanism:, Example:, and Boundary:. Include the exact sentence fragment '$($task.focus)'. The Example section must contain a small Rust code example. Do not answer with only a word or one sentence.
"@
    @(
        [pscustomobject]@{ role = 'system'; content = 'Use only the supplied Rust reference. Produce a complete technical explanation and preserve the requested validation marker.' },
        [pscustomobject]@{ role = 'user'; content = $user }
    ) | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $messagesPath -Encoding utf8NoBOM
    & $templateScript -ArtifactDirectory $ArtifactDirectory -Model $Model `
        -MessagesFile $messagesPath -OutputFile $renderedPath -SplitMarker $marker `
        -SeedOutputFile $seedPath -SuffixOutputFile $suffixPath
    if ($LASTEXITCODE -ne 0) { throw "chat template rendering failed for request $number" }

    if ($index -eq 0) {
        $first = Join-Path $OutputDirectory 'fixture-001'
        & $fixtureScript -Model $Model -ArtifactDirectory $ArtifactDirectory `
            -TargetTokens $TargetTokens -SeedFile $seedPath -RequiredSuffixFile $suffixPath `
            -SemanticBoundary -OutputDirectory $first
        if ($LASTEXITCODE -ne 0) { throw 'first exact-token fixture generation failed' }
        $probe = Join-Path $first 'tokenizer-build\Release\linker-tokenizer-probe.exe'
        Copy-Item -LiteralPath (Join-Path $first "prompt-$TargetTokens-tokens.txt") `
            -Destination $baseFixture
    } else {
        & $probe --artifact-directory $ArtifactDirectory --model $Model --output $baseFixture `
            --target $TargetTokens --seed-file $seedPath --required-suffix-file $suffixPath `
            --semantic-boundary
        if ($LASTEXITCODE -ne 0) { throw "base fixture generation failed for task $number" }
    }
    $prompt = Get-Content -LiteralPath $baseFixture -Raw
    if ($prompt -match '(?m)[A-Za-z][\r\n]+Authoritative fact') {
        throw "base fixture $number contains a word fragment before the authoritative fact"
    }
    $baseFixtures += $baseFixture
}

for ($index = 0; $index -lt $RequestCount; $index++) {
    $number = $index + 1
    $taskIndex = $index % $tasks.Count
    $task = $tasks[$taskIndex]
    $requestMarker = $allMarkers[$index]
    $baseMarker = $allMarkers[$taskIndex]
    $fixturePath = Join-Path $OutputDirectory `
        ('prompt-{0:D3}-{1}-tokens.txt' -f $number, $TargetTokens)
    $prompt = (Get-Content -LiteralPath $baseFixtures[$taskIndex] -Raw).Replace(
        $baseMarker, $requestMarker)
    Set-Content -LiteralPath $fixturePath -Value $prompt -Encoding utf8NoBOM -NoNewline
    & $probe --artifact-directory $ArtifactDirectory --model $Model --input-file $fixturePath `
        --target $TargetTokens
    if ($LASTEXITCODE -ne 0) { throw "token verification failed for request $number" }
    $fixturePaths += $fixturePath
    $expectations += [pscustomobject]@{
        minimum_generated_tokens = $MinimumGeneratedTokens
        minimum_response_chars = 700
        required_substrings = @($requestMarker, 'Claim:', 'Mechanism:', 'Example:', 'Boundary:', $task.focus)
        forbidden_substrings = @($allMarkers | Where-Object { $_ -ne $requestMarker })
    }
    $manifest += [pscustomobject]@{
        request = $number
        task = $task.name
        marker = $requestMarker
        path = (Resolve-Path $fixturePath).Path
        sha256 = (Get-FileHash $fixturePath -Algorithm SHA256).Hash
        token_count = $TargetTokens
    }
}

$promptPath = Join-Path $OutputDirectory 'prompts.json'
$responsesPath = Join-Path $OutputDirectory 'response-expectations.json'
$fixturePaths | ForEach-Object { Get-Content -LiteralPath $_ -Raw } |
    ConvertTo-Json | Set-Content -LiteralPath $promptPath -Encoding utf8NoBOM
$expectations | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath $responsesPath -Encoding utf8NoBOM
$manifest | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath (Join-Path $OutputDirectory 'manifest.json') -Encoding utf8NoBOM
[pscustomobject]@{
    status = 'passed'
    model = (Resolve-Path $Model).Path
    artifact_directory = (Resolve-Path $ArtifactDirectory).Path
    target_tokens = $TargetTokens
    request_count = $RequestCount
    distinct_prompts = $fixturePaths.Count
    prompts_json = (Resolve-Path $promptPath).Path
    response_expectations_json = (Resolve-Path $responsesPath).Path
    acceptance = [pscustomobject]@{
        minimum_generated_tokens = $MinimumGeneratedTokens
        expected_prefill_rows = $TargetTokens
        allowed_stop_reasons = @('eos', 'length')
        responses_file = (Resolve-Path $responsesPath).Path
    }
    prompt_layout = 'model GGUF Jinja template; coherent Rust reference; semantic-boundary exact-token padding; long structured answer'
    tokenizer_mode = 'artifact llama.dll; vocab_only=true; add_bos from GGUF; parse_special=true'
    template_mode = 'artifact llama-common.dll; model metadata Jinja; add_generation_prompt=true'
} | ConvertTo-Json -Depth 8 |
    Set-Content -LiteralPath (Join-Path $OutputDirectory 'report.json') -Encoding utf8NoBOM
Write-Output "GATE3_PROMPTS status=passed tokens=$TargetTokens requests=$RequestCount distinct=$($fixturePaths.Count) output=$OutputDirectory"
