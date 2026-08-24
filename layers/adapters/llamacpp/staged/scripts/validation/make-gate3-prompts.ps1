[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Model,
    [Parameter(Mandatory = $true)][string]$ArtifactDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [int]$TargetTokens = 500,
    [int]$RequestCount = 50
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($TargetTokens -le 0 -or $RequestCount -le 0) {
    throw 'TargetTokens and RequestCount must be positive'
}
$fixtureScript = Join-Path $PSScriptRoot 'make-5000-token-fixture.ps1'
foreach ($path in @($Model, (Join-Path $ArtifactDirectory 'p4_staged_server.exe'), $fixtureScript)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "required file not found: $path" }
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$short = 'Return only the short answer.'
$planetPayload = 'Mercury is closest to the Sun. Venus has a dense atmosphere. Earth has liquid oceans. Mars has iron-rich soil. Jupiter is the largest planet. Saturn has prominent rings. Uranus rotates on its side. Neptune has very fast winds.'
$photoPayload = 'Plants capture sunlight with chlorophyll. They take carbon dioxide from the air and water from the soil. Light energy helps produce glucose. The plant stores chemical energy in glucose. Oxygen is released as a byproduct. This process supports most food chains. It also helps maintain oxygen in the atmosphere. Photosynthesis mainly occurs in chloroplasts.'
$tasks = @(
    @{ question = 'What is the capital of France?'; instruction = $short; answer = 'Paris' },
    @{ question = 'What is twelve multiplied by seven?'; instruction = $short; answer = '84 or Eighty-four' },
    @{ question = 'At sea level, what is the boiling point of water in degrees Celsius?'; instruction = $short; answer = '100 degrees Celsius or 100' },
    @{ question = 'What is the chemical symbol for gold?'; instruction = $short; answer = 'Au' },
    @{ question = 'What is the largest planet in the Solar System?'; instruction = $short; answer = 'Jupiter' },
    @{ question = 'Who wrote the novel 1984?'; instruction = $short; answer = 'George Orwell' },
    @{ question = 'What is the primary language of Brazil?'; instruction = $short; answer = 'Portuguese' },
    @{ question = 'What is the square root of 144?'; instruction = $short; answer = '12' },
    @{
        question = "Copy the text between the copy tags exactly: <copy>$planetPayload</copy>"
        instruction = 'Return the contents verbatim without the copy tags and without any other text.'
        answer = $planetPayload
    },
    @{
        question = "Copy the text between the copy tags exactly: <copy>$photoPayload</copy>"
        instruction = 'Return the contents verbatim without the copy tags and without any other text.'
        answer = $photoPayload
    }
)
$fillerSentence = 'Reference filler: stable batching preserves request identity, causal order, sequence membership, and exact token positions across every physical microbatch. '
$fillerBody = -join (1..120 | ForEach-Object { $fillerSentence })
$filler = "<|im_start|>system`nAnswer the user's final task accurately and concisely.<|im_end|>`n<|im_start|>user`n$fillerBody"
$seedPaths = @()
$suffixPaths = @()
for ($index = 0; $index -lt $tasks.Count; $index++) {
    $number = $index + 1
    $seedPath = Join-Path $OutputDirectory ('seed-{0:D2}.txt' -f $number)
    $suffixPath = Join-Path $OutputDirectory ('suffix-{0:D2}.txt' -f $number)
    $task = $tasks[$index]
    Set-Content -LiteralPath $seedPath -Value $filler -Encoding utf8NoBOM -NoNewline
    $requiredSuffix = "`nThe reference material above is unrelated to the final task.`nFinal task: $($task.question)`n$($task.instruction)<|im_end|>`n<|im_start|>assistant`n"
    Set-Content -LiteralPath $suffixPath -Value $requiredSuffix -Encoding utf8NoBOM -NoNewline
    $seedPaths += $seedPath
    $suffixPaths += $suffixPath
}

$firstDirectory = Join-Path $OutputDirectory 'fixture-01'
& $fixtureScript -Model $Model -ArtifactDirectory $ArtifactDirectory `
    -TargetTokens $TargetTokens -SeedFile $seedPaths[0] -RequiredSuffixFile $suffixPaths[0] `
    -OutputDirectory $firstDirectory
if ($LASTEXITCODE -ne 0) { throw 'first exact-token fixture generation failed' }
$probe = Join-Path $firstDirectory 'tokenizer-build\Release\linker-tokenizer-probe.exe'
if (-not (Test-Path -LiteralPath $probe -PathType Leaf)) { throw "tokenizer probe not found: $probe" }

$fixturePaths = @(Join-Path $firstDirectory "prompt-$TargetTokens-tokens.txt")
for ($index = 1; $index -lt $tasks.Count; $index++) {
    $fixturePath = Join-Path $OutputDirectory ('prompt-{0:D2}-{1}-tokens.txt' -f ($index + 1), $TargetTokens)
    & $probe --artifact-directory $ArtifactDirectory --model $Model `
        --output $fixturePath --target $TargetTokens --seed-file $seedPaths[$index] `
        --required-suffix-file $suffixPaths[$index]
    if ($LASTEXITCODE -ne 0) { throw "fixture $($index + 1) generation failed" }
    $fixturePaths += $fixturePath
}

$prompts = for ($index = 0; $index -lt $RequestCount; $index++) {
    Get-Content -LiteralPath $fixturePaths[$index % $fixturePaths.Count] -Raw
}
$promptFixtures = for ($index = 0; $index -lt $fixturePaths.Count; $index++) {
    [pscustomobject]@{
        path = (Resolve-Path $fixturePaths[$index]).Path
        sha256 = (Get-FileHash $fixturePaths[$index] -Algorithm SHA256).Hash
        token_count = $TargetTokens
        expected_answer = $tasks[$index].answer
    }
}
$promptPath = Join-Path $OutputDirectory 'prompts.json'
$prompts | ConvertTo-Json | Set-Content -LiteralPath $promptPath -Encoding utf8NoBOM
$report = [pscustomobject]@{
    status = 'passed'
    model = (Resolve-Path $Model).Path
    artifact_directory = (Resolve-Path $ArtifactDirectory).Path
    target_tokens = $TargetTokens
    request_count = $RequestCount
    distinct_prompts = $fixturePaths.Count
    prompts_json = (Resolve-Path $promptPath).Path
    expected_answers = @($tasks.answer)
    prompt_layout = 'Qwen chat-template system and user filler followed by a required final task and assistant prefix'
    prompt_fixtures = @($promptFixtures)
    tokenizer_probe = (Resolve-Path $probe).Path
    tokenizer_mode = 'artifact llama.dll; vocab_only=true; add_bos from GGUF; parse_special=true'
} | ConvertTo-Json -Depth 4
$report | Set-Content -LiteralPath (Join-Path $OutputDirectory 'report.json') -Encoding utf8NoBOM
Write-Output "GATE3_PROMPTS status=passed tokens=$TargetTokens requests=$RequestCount output=$OutputDirectory"
