[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ModelRoot,
    [Parameter(Mandatory = $true)][string]$RuntimeDir,
    [Parameter(Mandatory = $true)][string]$StateRoot,
    [ValidateRange(10000, 59999)][int]$Port = 18083
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..\..')).Path
$node = (Get-Command node -ErrorAction Stop).Source
$server = Join-Path $root 'apps\llama\dist\server\index.js'
$runtime = (Resolve-Path -LiteralPath $RuntimeDir).Path
$models = (Resolve-Path -LiteralPath $ModelRoot).Path
New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null
$state = (Resolve-Path -LiteralPath $StateRoot).Path
$log = Join-Path $state 'supervisor.log'

$previous = @{
    PORT = $env:PORT
    NODE_ENV = $env:NODE_ENV
    LLAMA_NATIVE_BIN_DIR = $env:LLAMA_NATIVE_BIN_DIR
    LLAMA_MODEL_DIR = $env:LLAMA_MODEL_DIR
    LLAMA_STATE_DIR = $env:LLAMA_STATE_DIR
    LINKER_LLAMA_CPP_BACKEND = $env:LINKER_LLAMA_CPP_BACKEND
}
try {
    $env:PORT = [string]$Port
    $env:NODE_ENV = 'production'
    $env:LLAMA_NATIVE_BIN_DIR = $runtime
    $env:LLAMA_MODEL_DIR = $models
    $env:LLAMA_STATE_DIR = $state
    $env:LINKER_LLAMA_CPP_BACKEND = 'cuda'
    $process = Start-Process -FilePath $node -ArgumentList $server `
        -RedirectStandardOutput $log -RedirectStandardError "$log.err" `
        -WindowStyle Hidden -PassThru
    [pscustomobject]@{ process_id = $process.Id; endpoint = "http://127.0.0.1:$Port"; runtime = $runtime } | ConvertTo-Json -Compress
} finally {
    foreach ($entry in $previous.GetEnumerator()) {
        if ($null -eq $entry.Value) {
            Remove-Item "Env:$($entry.Key)" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:$($entry.Key)" $entry.Value
        }
    }
}
