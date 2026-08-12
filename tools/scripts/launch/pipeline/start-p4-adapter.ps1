[CmdletBinding()]
param(
    [string]$AgentEndpoint = '127.0.0.1:19201',
    [string]$ListenEndpoint = '127.0.0.1:19203',
    [string]$AdapterId = 'adapter-local',
    [string]$LlamaHost = 'http://127.0.0.1:18082',
    [string]$Target = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path 'target\remote-3090x2-20260810')
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$binary = Join-Path $root 'target\release\p4-adapter.exe'
New-Item -ItemType Directory -Force -Path $Target | Out-Null
$process = Start-Process -FilePath $binary -ArgumentList $ListenEndpoint, $AgentEndpoint, $AdapterId, $LlamaHost -RedirectStandardOutput (Join-Path $Target 'p4-adapter-local.out.log') -RedirectStandardError (Join-Path $Target 'p4-adapter-local.err.log') -WindowStyle Hidden -PassThru
[pscustomobject]@{ pid = $process.Id; binary = $binary; agent = $AgentEndpoint; adapter = $AdapterId } | ConvertTo-Json -Compress
