[CmdletBinding()]
param(
    [string]$RunId = (Get-Date -Format 'yyyyMMddHHmmss'),
    [string]$SshTarget = '42mob@192.168.0.29',
    [string]$ArtifactDirectory = '',
    [string]$RemoteArtifactDirectory = '',
    [string]$LocalModel = 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    [string]$RemoteModel = 'D:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    [string]$RemoteAgentRoot = '',
    [string]$AgentBinary = '',
    [string]$DriveBinary = '',
    [int]$Requests = 4,
    [int]$Tokens = 8,
    [int]$PromptTokens = 0,
    [int]$Parallel = 0,
    [int]$ArriveMilliseconds = 1,
    [int]$ContextSize = 0,
    [int]$BatchSize = 0,
    [int]$UBatchSize = 0,
    [int]$FlashAttention = 0,
    [string]$PromptFile = '',
    [int]$QuietMilliseconds = 120000,
    [int]$MinimumPeakNodeQueue = 1,
    [int]$MinimumPeakInAdapter = 1,
    [int]$DriverPort = 52000,
    [int]$LocalAgentPortBase = 52003,
    [int]$ForwardPortBase = 53001,
    [int]$Max4080VramMiB = 9000,
    [string]$StageRanges = '',
    [string]$GpuLayers = '',
    [string]$TensorOverride = '',
    [string]$LocalGpuDevices = '1,0',
    [switch]$KeepRemoteArtifacts,
    [switch]$KeepLoaded
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Requests -lt 1 -or $Tokens -lt 1) { throw 'Requests and Tokens must be positive.' }
$requestedTokens = $Tokens
if ($Parallel -lt 0) { throw 'Parallel must be zero or positive.' }
if ($ArriveMilliseconds -lt 0) { throw 'ArriveMilliseconds must be zero or positive.' }
if ($MinimumPeakNodeQueue -lt 0 -or $MinimumPeakInAdapter -lt 0) {
    throw 'Minimum overlap thresholds must be zero or positive.'
}
if ($Requests -gt 4096 -or $Tokens -gt 100000 -or $Parallel -gt 4096 -or $BatchSize -gt 100000 -or $UBatchSize -gt 100000) {
    throw 'Requests, Tokens, Parallel, BatchSize, or UBatchSize exceeds the safe runner limit.'
}
if ($QuietMilliseconds -lt 1000) { throw 'QuietMilliseconds must be at least 1000.' }
if ($Max4080VramMiB -lt 1) { throw 'Max4080VramMiB must be positive.' }
if ($FlashAttention -notin @(0, 1)) { throw 'FlashAttention must be 0 or 1.' }
if ($DriverPort -ne 52000 -or $LocalAgentPortBase -ne 52003 -or $ForwardPortBase -ne 53001) {
    throw 'This acceptance runner is restricted to the existing firewall ports: driver 52000, central agents 52003/52004, SSH forwards 53001/53002.'
}
if ($RunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$') {
    throw 'RunId must contain only letters, digits, dot, underscore, or hyphen (max 64 characters).'
}
if (-not (Get-Command ssh.exe -ErrorAction SilentlyContinue)) { throw 'ssh.exe is required.' }
if (-not (Get-Command scp.exe -ErrorAction SilentlyContinue)) { throw 'scp.exe is required.' }
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$p4Root = Join-Path $projectRoot 'apps\p4'
function Resolve-LocalInputPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or [System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }
    $fromCaller = Join-Path (Get-Location) $Path
    if (Test-Path -LiteralPath $fromCaller) {
        return (Resolve-Path -LiteralPath $fromCaller).Path
    }
    $fromProject = Join-Path $projectRoot $Path
    if (Test-Path -LiteralPath $fromProject) {
        return (Resolve-Path -LiteralPath $fromProject).Path
    }
    return [System.IO.Path]::GetFullPath($fromCaller)
}
if ([string]::IsNullOrWhiteSpace($ArtifactDirectory)) {
    $ArtifactDirectory = Join-Path $projectRoot '.cache\staged-server-cuda-real-20260818\Release'
}
if ([string]::IsNullOrWhiteSpace($RemoteAgentRoot)) {
    $RemoteAgentRoot = 'C:\Users\42mob\p4-staged-test'
}
if ([string]::IsNullOrWhiteSpace($RemoteArtifactDirectory)) {
    $RemoteArtifactDirectory = Join-Path $RemoteAgentRoot "e2e-$RunId"
}
if ([string]::IsNullOrWhiteSpace($AgentBinary)) {
    $AgentBinary = Join-Path $projectRoot 'target\p4-release-validation-remediation27\release\p4-agent.exe'
    if (-not (Test-Path -LiteralPath $AgentBinary -PathType Leaf)) {
        $AgentBinary = Join-Path $projectRoot 'target\release\p4-agent.exe'
    }
}
if ([string]::IsNullOrWhiteSpace($DriveBinary)) {
    $DriveBinary = Join-Path $projectRoot 'target\p4-release-validation-remediation27\release\p4-drive.exe'
    if (-not (Test-Path -LiteralPath $DriveBinary -PathType Leaf)) {
        $DriveBinary = Join-Path $projectRoot 'target\release\p4-drive.exe'
    }
}
$ArtifactDirectory = Resolve-LocalInputPath $ArtifactDirectory
$AgentBinary = Resolve-LocalInputPath $AgentBinary
$DriveBinary = Resolve-LocalInputPath $DriveBinary
$PromptFile = Resolve-LocalInputPath $PromptFile
$agentBinary = $AgentBinary
$driveBinary = $DriveBinary
$serverBinary = Join-Path $ArtifactDirectory 'p4_staged_server.exe'
$localModelRoot = [System.IO.Path]::GetDirectoryName($LocalModel)
$remoteModelRoot = [System.IO.Path]::GetDirectoryName($RemoteModel)
$outputRoot = Join-Path $projectRoot "target\ssh-forwarded-four-node-e2e\$RunId"
$remoteAgentPorts = @(53001, 53002)
$localForwardPorts = @($ForwardPortBase, ($ForwardPortBase + 1))
$localAgentPorts = @($LocalAgentPortBase, ($LocalAgentPortBase + 1))
$driverPort = $DriverPort
$remoteControlPort = $DriverPort
function Parse-StageRanges([string]$Text) {
    if ([string]::IsNullOrWhiteSpace($Text)) {
        return @(
            [pscustomobject]@{ Begin = 0; End = 4 }
            [pscustomobject]@{ Begin = 4; End = 14 }
            [pscustomobject]@{ Begin = 14; End = 21 }
            [pscustomobject]@{ Begin = 21; End = 28 }
        )
    }
    [void]($parts = $Text.Split(','))
    if ($parts.Count -ne 4) { throw 'StageRanges must contain exactly four begin:end entries.' }
    [void]($parsed = New-Object object[] 4)
    for ($i = 0; $i -lt 4; $i++) {
        [void]($range = $parts[$i].Trim().Split(':'))
        if ($range.Count -ne 2) { throw "Invalid StageRanges entry: $($parts[$i]) (expected begin:end)." }
        [void]($begin = 0); [void]($end = 0)
        if (-not [int]::TryParse($range[0], [ref]$begin) -or -not [int]::TryParse($range[1], [ref]$end)) {
            throw "Invalid StageRanges entry: $($parts[$i]) (expected integers)."
        }
        if ($end -le $begin) { throw "StageRanges entry must have end > begin: $($parts[$i])" }
        [void]($parsed[$i] = [pscustomobject]@{ Begin = $begin; End = $end })
    }
    for ($i = 1; $i -lt 4; $i++) {
        if ($parsed[$i - 1].End -ne $parsed[$i].Begin) { throw 'StageRanges must be contiguous.' }
    }
    return $parsed
}
function Parse-GpuLayers([string]$Text) {
    if ([string]::IsNullOrWhiteSpace($Text)) { return @(99, 99, 99, 99) }
    [void]($parts = $Text.Split(','))
    if ($parts.Count -ne 4) { throw 'GpuLayers must contain exactly four non-negative integers.' }
    [void]($parsed = New-Object int[] 4)
    for ($i = 0; $i -lt 4; $i++) {
        [void]($value = 0)
        if (-not [int]::TryParse($parts[$i].Trim(), [ref]$value) -or $value -lt 0) { throw "Invalid GpuLayers entry: $($parts[$i])" }
        [void]($parsed[$i] = $value)
    }
    return $parsed
}
function Parse-LocalGpuDevices([string]$Text) {
    [void]($parts = $Text.Split(','))
    if ($parts.Count -ne 2) { throw 'LocalGpuDevices must contain exactly two non-negative integers.' }
    [void]($parsed = New-Object int[] 2)
    for ($i = 0; $i -lt 2; $i++) {
        [void]($value = 0)
        if (-not [int]::TryParse($parts[$i].Trim(), [ref]$value) -or $value -lt 0) {
            throw "Invalid LocalGpuDevices entry: $($parts[$i])"
        }
        [void]($parsed[$i] = $value)
    }
    return $parsed
}
$parsedStageRanges = @(Parse-StageRanges $StageRanges)
$parsedGpuLayers = @(Parse-GpuLayers $GpuLayers)
$parsedLocalGpuDevices = @(Parse-LocalGpuDevices $LocalGpuDevices)
$totalModelLayers = $parsedStageRanges[3].End + 1
$parallelSlots = if ($Parallel -gt 0) { $Parallel } else { $Requests }
$effectivePromptTokens = if ($PromptTokens -gt 0) { $PromptTokens } else { 0 }
$requiredContext = if ($effectivePromptTokens -gt 0) {
    [math]::Max(1024, ($effectivePromptTokens + $Tokens) * $parallelSlots)
} else {
    10000
}
$planContextSize = if ($ContextSize -gt 0) { $ContextSize } else { $requiredContext }
if ($planContextSize -lt $requiredContext) {
    throw "ContextSize=$planContextSize is smaller than required context $requiredContext (PromptTokens=$effectivePromptTokens Tokens=$Tokens Parallel=$parallelSlots)."
}
$planBatchSize = if ($BatchSize -gt 0) { $BatchSize } else { [math]::Max(512, $Tokens) }
$planUBatchSize = if ($UBatchSize -gt 0) { $UBatchSize } elseif ($PromptFile -ne '') { $planBatchSize } else { 128 }
$reservedLocalPorts = @($driverPort) + $localAgentPorts + $localForwardPorts
if ($PromptFile -ne '' -and -not (Test-Path -LiteralPath $PromptFile -PathType Leaf)) {
    throw "Prompt file not found: $PromptFile"
}
if (-not (Test-Path -LiteralPath $ArtifactDirectory -PathType Container)) {
    throw "Artifact directory not found: $ArtifactDirectory"
}
function ConvertTo-PowerShellLiteral([string]$Text) {
    if ($null -eq $Text) { return "''" }
    "'{0}'" -f $Text.Replace("'", "''")
}
foreach ($path in @($agentBinary, $driveBinary, $serverBinary, $LocalModel)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required local file not found: $path" }
}
$listeningPorts = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in $reservedLocalPorts } |
    Select-Object -ExpandProperty LocalPort -Unique)
if ($listeningPorts.Count -gt 0) {
    throw "Reserved local E2E port(s) already in use: $($listeningPorts -join ', '). Stop the owning test processes and retry."
}
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$remoteLogDirectory = Join-Path $RemoteAgentRoot "e2e-$RunId-logs"
$localProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new(); $artifactFiles = @()
$tunnel = $null; $result = $null; $vramSampler = $null
$vramLog = Join-Path $outputRoot 'vram-4080.csv'
$peak4080VramMiB = -1
function ConvertTo-EncodedCommand([string]$Text) {
    [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Text))
}
function Invoke-RemotePowerShell([string]$Script) {
    $encoded = ConvertTo-EncodedCommand $Script
    $output = & ssh.exe -T -o BatchMode=yes -o ConnectTimeout=15 $SshTarget `
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded"
    if ($LASTEXITCODE -ne 0) { throw "Remote command failed with exit code ${LASTEXITCODE}: $Script`n$($output -join [Environment]::NewLine)" }
    @($output)
}
function Assert-RemotePreflight {
    $remoteRoot = ConvertTo-PowerShellLiteral $RemoteArtifactDirectory
    $remoteModelLiteral = ConvertTo-PowerShellLiteral $RemoteModel
    $remoteCheck = @"
`$ErrorActionPreference = 'Stop'
if ('$RemoteModel' -notmatch '^[Ss]:\\') {
    if (-not (Test-Path -LiteralPath $remoteModelLiteral -PathType Leaf)) { throw 'Remote model not found: $RemoteModel' }
} else {
    Write-Output 'REMOTE_MODEL_CHECK_DEFERRED_TO_INTERACTIVE_SESSION'
}
if (Test-Path -LiteralPath $remoteRoot) {
    `$existing = @(Get-ChildItem -LiteralPath $remoteRoot -Force -ErrorAction Stop)
    if (`$existing.Count -gt 0) { throw 'Remote artifact directory is not empty: $RemoteArtifactDirectory' }
}
`$ports = @($remoteControlPort, $($remoteAgentPorts -join ', '))
`$busy = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { `$_.LocalPort -in `$ports } |
    Select-Object -ExpandProperty LocalPort -Unique)
if (`$busy.Count -gt 0) { throw "Remote reserved port(s) already in use: `$(`$busy -join ', ')" }
Write-Output 'REMOTE_PREFLIGHT_OK'
"@
    Invoke-RemotePowerShell $remoteCheck | Out-File -LiteralPath (Join-Path $outputRoot 'remote-preflight.log') -Encoding utf8
}
function Start-EncodedLocalProcess([string]$Script, [string]$Stdout, [string]$Stderr) {
    $encoded = ConvertTo-EncodedCommand $Script
    $process = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
        '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded
    ) -WorkingDirectory $p4Root -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr `
        -WindowStyle Hidden -PassThru
    [void]$localProcesses.Add($process)
    return $process
}
function Wait-Listening([int]$Port, [int]$TimeoutSeconds = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $client = [Net.Sockets.TcpClient]::new()
        try {
            $task = $client.ConnectAsync('127.0.0.1', $Port)
            if ($task.Wait(250) -and $client.Connected) { return }
        } catch { }
        finally { $client.Dispose() }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "Timed out waiting for TCP 127.0.0.1:$Port."
}
function Plan([int]$Begin, [int]$End, [int]$GpuLayerCount, [string]$Model, [int]$Slots, [int]$Batch, [int]$UBatch, [int]$Context) {
    $ownedLayers = $End - $Begin
    if ($GpuLayerCount -gt $ownedLayers) { $GpuLayerCount = $ownedLayers }
    $globalGpuLayers = if ($GpuLayerCount -gt 0) { $totalModelLayers - ($End - $GpuLayerCount) } else { 0 }
    $overrideSuffix = if ([string]::IsNullOrWhiteSpace($TensorOverride)) { '' } else { ' --override-tensor "{0}"' -f $TensorOverride }
    '--model "{0}" --layer-begin {1} --layer-end {2} --kv-layer-begin {1} --kv-layer-end {2} --n-seq-max {3} --batch-size {4} --ubatch-size {5} --ctx-size {6} --n-gpu-layers {7} --device CUDA0 --flash-attn {8} --no-mmap --cache-type-k q8_0 --cache-type-v q8_0{9}' -f $Model, $Begin, $End, $Slots, $Batch, $UBatch, $Context, $globalGpuLayers, $(if ($FlashAttention -eq 1) { 'on' } else { 'off' }), $overrideSuffix
}
try {
    $artifactFiles = @((Get-Item -LiteralPath $agentBinary)) + @(Get-ChildItem -LiteralPath $ArtifactDirectory -File | Where-Object {
        $_.Name -eq 'p4_staged_server.exe' -or
        $_.Name -like 'ggml*.dll' -or
        $_.Name -like 'llama*.dll' -or
        $_.Name -like 'cublas*.dll'
    })
    # The local build uses CUDA 13.1 while the remote 3090 host exposes CUDA
    # 12.8. Ship only the two CUDA runtime DLLs required by ggml-cuda; the
    # model remains on the shared S: drive and is never copied.
    $cudaRuntime = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64'
    $artifactFiles += @(
        Get-Item -LiteralPath (Join-Path $cudaRuntime 'cublas64_13.dll')
        Get-Item -LiteralPath (Join-Path $cudaRuntime 'cublasLt64_13.dll')
        Get-Item -LiteralPath (Join-Path $cudaRuntime 'cudart64_13.dll')
    )
    # The CUDA runtime directory can also be the artifact directory. Keep one
    # file per basename so the copy and manifest describe the actual remote
    # directory rather than counting the same DLL twice.
    $artifactFiles = @($artifactFiles | Sort-Object -Property Name -Unique)
    if ($artifactFiles.Count -eq 0) { throw "No .exe/.dll artifact files found in $ArtifactDirectory." }
    if (-not (Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue)) {
        throw 'nvidia-smi.exe is required to prove the 4080 VRAM guard.'
    }
    $vramSampler = Start-Process -FilePath 'nvidia-smi.exe' -ArgumentList @(
        '--query-gpu=index,memory.used', '--format=csv,noheader,nounits', '-lms', '200'
    ) -RedirectStandardOutput $vramLog -RedirectStandardError (Join-Path $outputRoot 'vram-4080.err.log') `
        -WindowStyle Hidden -PassThru
    Assert-RemotePreflight
    $remotePrepare = @"
`$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $(ConvertTo-PowerShellLiteral $RemoteArtifactDirectory) | Out-Null
New-Item -ItemType Directory -Force -Path $(ConvertTo-PowerShellLiteral $remoteLogDirectory) | Out-Null
Write-Output 'REMOTE_PREPARED'
"@
    Invoke-RemotePowerShell $remotePrepare | Out-File -LiteralPath (Join-Path $outputRoot 'remote-prepare.log') -Encoding utf8
    foreach ($file in $artifactFiles) {
        $remoteFile = ($RemoteArtifactDirectory -replace '\\', '/') + '/' + $file.Name
        & scp.exe -q -o BatchMode=yes $file.FullName "$SshTarget`:$remoteFile"
        if ($LASTEXITCODE -ne 0) { throw "scp failed for $($file.Name) with exit code $LASTEXITCODE." }
    }
    $manifest = foreach ($file in $artifactFiles) {
        $hash = ((certutil.exe -hashfile $file.FullName SHA256 | Select-Object -Skip 1 | Select-Object -First 1) -replace '\s','')
        [pscustomobject]@{ name = $file.Name; bytes = $file.Length; sha256 = $hash }
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputRoot 'artifact-manifest.json') -Encoding utf8
    $remoteManifest = @"
`$files = Get-ChildItem -LiteralPath $(ConvertTo-PowerShellLiteral $RemoteArtifactDirectory) -File | Where-Object {
    `$_.Name -eq 'p4-agent.exe' -or `$_.Name -eq 'p4_staged_server.exe' -or `$_.Name -like 'ggml*.dll' -or
    `$_.Name -like 'llama*.dll' -or `$_.Name -like 'cublas*.dll' -or `$_.Name -like 'cudart*.dll'
}
`$files | ForEach-Object { `$h = ((certutil.exe -hashfile `$_.FullName SHA256 | Select-Object -Skip 1 | Select-Object -First 1) -replace '\s',''); [pscustomobject]@{ name = `$_.Name; bytes = `$_.Length; sha256 = `$h } } |
    ConvertTo-Json -Depth 4
"@
    $remoteManifestText = Invoke-RemotePowerShell $remoteManifest
    $remoteManifestText | Set-Content -LiteralPath (Join-Path $outputRoot 'remote-artifact-manifest.json') -Encoding utf8
    $localManifestByName = @{}
    foreach ($entry in $manifest) { $localManifestByName[$entry.name] = $entry.sha256 }
    $remoteEntries = @((($remoteManifestText -join "`n") | ConvertFrom-Json))
    if ($remoteEntries.Count -ne $manifest.Count) {
        throw "Remote artifact manifest count mismatch: local=$($manifest.Count) remote=$($remoteEntries.Count)"
    }
    foreach ($entry in $remoteEntries) {
        if (-not $localManifestByName.ContainsKey($entry.name) -or
            $localManifestByName[$entry.name] -ne $entry.sha256) {
            throw "Remote artifact hash mismatch: $($entry.name)"
        }
    }
    $tunnelArgs = @('-T', '-N', '-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes',
        '-o', 'ServerAliveInterval=10', '-o', 'ServerAliveCountMax=3',
        '-L', "$($localForwardPorts[0]):127.0.0.1:$($remoteAgentPorts[0])",
        '-L', "$($localForwardPorts[1]):127.0.0.1:$($remoteAgentPorts[1])",
        '-R', "$($localAgentPorts[0]):127.0.0.1:$($localAgentPorts[0])",
        '-R', "$($localAgentPorts[1]):127.0.0.1:$($localAgentPorts[1])",
        '-R', "$remoteControlPort`:127.0.0.1:$driverPort", $SshTarget)
    $tunnel = Start-Process -FilePath 'ssh.exe' -ArgumentList $tunnelArgs `
        -RedirectStandardOutput (Join-Path $outputRoot 'ssh-tunnel.log') `
        -RedirectStandardError (Join-Path $outputRoot 'ssh-tunnel.err.log') -WindowStyle Hidden -PassThru
    $localProcesses.Add($tunnel)
    Start-Sleep -Seconds 1
    if ($tunnel.HasExited) { throw "SSH tunnel exited early: $($tunnel.ExitCode)." }
    $remoteAgentScripts = for ($i = 0; $i -lt 2; $i++) {
        $cuda = $i
        $port = $remoteAgentPorts[$i]
        $advertisedPort = $localForwardPorts[$i]
        $pidFile = Join-Path $RemoteArtifactDirectory "agent-$port.pid"
        $traceHopAssignment = if ($env:P4_STAGED_TRACE_HOP -and $env:P4_STAGED_TRACE_HOP -ne '0') {
            "`$env:P4_STAGED_TRACE_HOP = '1'"
        } else { '' }
        $traceSequenceReleaseAssignment = if ($env:P4_STAGED_TRACE_SEQUENCE_RELEASE -and $env:P4_STAGED_TRACE_SEQUENCE_RELEASE -ne '0') {
            "`$env:P4_STAGED_TRACE_SEQUENCE_RELEASE = '1'"
        } else { '' }
        $traceRoutingAssignment = if ($env:P4_AGENT_TRACE_ROUTING -and $env:P4_AGENT_TRACE_ROUTING -ne '0') {
            "`$env:P4_AGENT_TRACE_ROUTING = '1'"
        } else { '' }
        $traceProtocolAssignment = if ($env:P4_STAGED_TRACE_PROTOCOL -and $env:P4_STAGED_TRACE_PROTOCOL -ne '0') {
            "`$env:P4_STAGED_TRACE_PROTOCOL = '1'"
        } else { '' }
        $taskName = "P4-Codex-$RunId-agent-$port"
        $launcherPath = Join-Path $RemoteArtifactDirectory "run-agent-$port.cmd"
        $launcherArgument = "/d /c call `"$launcherPath`""
        $agentPath = Join-Path $RemoteArtifactDirectory 'p4-agent.exe'
        $launcherLines = @(
            '@echo off'
            "set CUDA_VISIBLE_DEVICES=$cuda"
            "set `"P4_STAGED_SERVER_BINARY=$(Join-Path $RemoteArtifactDirectory 'p4_staged_server.exe')`""
            "set `"P4_MODEL_DIR=$remoteModelRoot`""
            'set P4_STAGED_LLAMA_INHERIT_STDERR=1'
            'set P4_STAGED_READY_TIMEOUT_SECS=900'
            'set P4_STAGED_IO_TIMEOUT_SECS=900'
            'set P4_STAGED_TRACE_PROTOCOL=1'
            'set P4_AGENT_STATS=1'
            'set P4_STAGED_SERVER_WINDOWLESS=1'
            "`"$agentPath`" 127.0.0.1:$port 127.0.0.1:$advertisedPort 1>`"$(Join-Path $RemoteArtifactDirectory "agent-$port.log")`" 2>`"$(Join-Path $RemoteArtifactDirectory "agent-$port.err.log")`""
        )
        $launcherAssignment = "Set-Content -LiteralPath $(ConvertTo-PowerShellLiteral $launcherPath) -Value @($(($launcherLines | ForEach-Object { ConvertTo-PowerShellLiteral $_ }) -join ', ')) -Encoding ascii"
        @"
`$taskName = $(ConvertTo-PowerShellLiteral $taskName)
$launcherAssignment
`$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument $(ConvertTo-PowerShellLiteral $launcherArgument)
`$principal = New-ScheduledTaskPrincipal -UserId 'm42-server2\42mob' -LogonType Interactive -RunLevel Limited
`$settings = New-ScheduledTaskSettingsSet -Hidden
schtasks.exe /delete /tn `$taskName /f *> `$null
Register-ScheduledTask -TaskName `$taskName -Action `$action -Principal `$principal -Settings `$settings | Out-Null
Start-ScheduledTask -TaskName `$taskName
`$deadline = (Get-Date).AddSeconds(30)
do {
    `$ready = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { `$_.LocalPort -eq $port })
    if (`$ready.Count -eq 0) { Start-Sleep -Milliseconds 250 }
} while (`$ready.Count -eq 0 -and (Get-Date) -lt `$deadline)
if (`$ready.Count -eq 0) { throw 'Interactive remote Agent did not become ready.' }
Write-Output 'REMOTE_AGENT_LISTENING'
"@
    }
    foreach ($i in 0..1) {
        $remoteAgentLog = Join-Path $outputRoot "remote-agent-$($remoteAgentPorts[$i]).log"
        $remoteAgentErr = Join-Path $outputRoot "remote-agent-$($remoteAgentPorts[$i]).err.log"
        $encoded = ConvertTo-EncodedCommand $remoteAgentScripts[$i]
        $remoteProcess = Start-Process -FilePath 'ssh.exe' -ArgumentList @(
            '-T', '-o', 'BatchMode=yes', $SshTarget,
            "powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded"
        ) -RedirectStandardOutput $remoteAgentLog -RedirectStandardError $remoteAgentErr `
            -WindowStyle Hidden -PassThru
        $localProcesses.Add($remoteProcess)
    }
    $remoteReady = @"
`$deadline = (Get-Date).AddSeconds(30)
foreach (`$port in @($($remoteAgentPorts -join ', '))) {
    `$connected = `$false
    do {
        `$client = [Net.Sockets.TcpClient]::new()
        try {
            `$task = `$client.ConnectAsync('127.0.0.1', `$port)
            if (`$task.Wait(500) -and `$client.Connected) { `$connected = `$true }
        } catch { } finally { `$client.Dispose() }
        if (-not `$connected) { Start-Sleep -Milliseconds 250 }
    } while (-not `$connected -and (Get-Date) -lt `$deadline)
    if (-not `$connected) { throw ('Remote agent port ' + `$port + ' did not become ready.') }
}
Write-Output 'REMOTE_AGENTS_LISTENING'
"@
    Invoke-RemotePowerShell $remoteReady | Out-File -LiteralPath (Join-Path $outputRoot 'remote-agent-readiness.log') -Encoding utf8
    foreach ($port in $localForwardPorts) { Wait-Listening $port }
    foreach ($i in 0..1) {
        $cuda = $parsedLocalGpuDevices[$i]
        $port = $localAgentPorts[$i]
        $log = Join-Path $outputRoot "central-agent-$port.log"
        $err = Join-Path $outputRoot "central-agent-$port.err.log"
        $traceHopAssignment = if ($env:P4_STAGED_TRACE_HOP -and $env:P4_STAGED_TRACE_HOP -ne '0') {
            "`$env:P4_STAGED_TRACE_HOP = '1'"
        } else { '' }
        $traceSequenceReleaseAssignment = if ($env:P4_STAGED_TRACE_SEQUENCE_RELEASE -and $env:P4_STAGED_TRACE_SEQUENCE_RELEASE -ne '0') {
            "`$env:P4_STAGED_TRACE_SEQUENCE_RELEASE = '1'"
        } else { '' }
        $traceRoutingAssignment = if ($env:P4_AGENT_TRACE_ROUTING -and $env:P4_AGENT_TRACE_ROUTING -ne '0') {
            "`$env:P4_AGENT_TRACE_ROUTING = '1'"
        } else { '' }
        $traceProtocolAssignment = if ($env:P4_STAGED_TRACE_PROTOCOL -and $env:P4_STAGED_TRACE_PROTOCOL -ne '0') {
            "`$env:P4_STAGED_TRACE_PROTOCOL = '1'"
        } else { '' }
        $script = @"
`$env:CUDA_VISIBLE_DEVICES = '$cuda'
`$env:P4_STAGED_SERVER_BINARY = $(ConvertTo-PowerShellLiteral $serverBinary)
`$env:P4_MODEL_DIR = $(ConvertTo-PowerShellLiteral $localModelRoot)
`$env:P4_STAGED_LLAMA_INHERIT_STDERR = '1'
`$env:P4_STAGED_READY_TIMEOUT_SECS = '900'
`$env:P4_STAGED_IO_TIMEOUT_SECS = '900'
`$env:P4_AGENT_STATS = '1'
$traceHopAssignment
$traceSequenceReleaseAssignment
$traceRoutingAssignment
$traceProtocolAssignment
`$pidFile = $(ConvertTo-PowerShellLiteral (Join-Path $outputRoot "central-agent-$port.pid"))
`$child = Start-Process -FilePath $(ConvertTo-PowerShellLiteral $agentBinary) -ArgumentList @('127.0.0.1:$port', '127.0.0.1:$port') -WorkingDirectory $(ConvertTo-PowerShellLiteral $p4Root) -RedirectStandardOutput $(ConvertTo-PowerShellLiteral $log) -RedirectStandardError $(ConvertTo-PowerShellLiteral $err) -PassThru
Set-Content -LiteralPath `$pidFile -Value `$child.Id -Encoding ascii
try { `$child.WaitForExit(); exit `$child.ExitCode } finally { Remove-Item -LiteralPath `$pidFile -Force -ErrorAction SilentlyContinue }
"@
        Start-EncodedLocalProcess $script $log $err | Out-Null
    }
    foreach ($port in $localAgentPorts) { Wait-Listening $port }
    $driverPlans = @(
        (Plan $parsedStageRanges[0].Begin $parsedStageRanges[0].End $parsedGpuLayers[0] $LocalModel $parallelSlots $planBatchSize $planUBatchSize $planContextSize),
        (Plan $parsedStageRanges[1].Begin $parsedStageRanges[1].End $parsedGpuLayers[1] $LocalModel $parallelSlots $planBatchSize $planUBatchSize $planContextSize),
        (Plan $parsedStageRanges[2].Begin $parsedStageRanges[2].End $parsedGpuLayers[2] $RemoteModel $parallelSlots $planBatchSize $planUBatchSize $planContextSize),
        (Plan $parsedStageRanges[3].Begin $parsedStageRanges[3].End $parsedGpuLayers[3] $RemoteModel $parallelSlots $planBatchSize $planUBatchSize $planContextSize)
    )
    $chain = '127.0.0.1:{0},127.0.0.1:{1},127.0.0.1:{2},127.0.0.1:{3}' -f `
        $localAgentPorts[0], $localAgentPorts[1], $localForwardPorts[0], $localForwardPorts[1]
    $driverLog = Join-Path $outputRoot 'drive.log'
    $driverErr = Join-Path $outputRoot 'drive.err.log'
    $driverExitCodeFile = Join-Path $outputRoot 'drive.exitcode'
    $planAssignments = for ($i = 0; $i -lt 4; $i++) {
        "`$env:P4_DRIVE_PLAN_$i = $(ConvertTo-PowerShellLiteral $driverPlans[$i])"
    }
    $driverPidFile = Join-Path $outputRoot 'driver.pid'
    $driverTraceAssignment = if ($env:P4_DRIVE_TRACE -and $env:P4_DRIVE_TRACE -ne '0') {
        "`$env:P4_DRIVE_TRACE = '1'"
    } else { '' }
    $driverOptionsAssignment = if ($env:P4_DRIVE_OPTIONS) {
        "`$env:P4_DRIVE_OPTIONS = $(ConvertTo-PowerShellLiteral $env:P4_DRIVE_OPTIONS)"
    } else { '' }
    $driverScript = @"
`$env:P4_DRIVE_DISCOVER = '1'
`$env:P4_DRIVE_ARTIFACT = $(ConvertTo-PowerShellLiteral ([System.IO.Path]::GetFileName($LocalModel)))
`$env:P4_DRIVE_CEILING = '$parallelSlots'
`$env:P4_DRIVE_ARRIVE_MS = '$ArriveMilliseconds'
${driverTraceAssignment}
${driverOptionsAssignment}
`$env:P4_DRIVE_VARY = '$(if ([string]::IsNullOrWhiteSpace($PromptFile)) { '1' } else { '0' })'
`$env:P4_DRIVE_QUIET_MS = '$QuietMilliseconds'
`$env:P4_DRIVE_PROMPT = 'Explain why staged inference uses a hidden-state cut.'
if ('$([string]::IsNullOrWhiteSpace($PromptFile))' -eq 'False') { `$env:P4_DRIVE_PROMPT_FILE = $(ConvertTo-PowerShellLiteral $PromptFile) }
if ('$KeepLoaded' -eq 'True') { `$env:P4_DRIVE_KEEP_LOADED = '1' }
$($planAssignments -join "`n")
Set-Content -LiteralPath $(ConvertTo-PowerShellLiteral $driverPidFile) -Value `$PID -Encoding ascii
    try {
    & $(ConvertTo-PowerShellLiteral $driveBinary) '127.0.0.1:$driverPort' '$chain' '$Requests' '$Tokens' 'llamacpp-staged' '127.0.0.1:$driverPort'
    `$exitCode = [int]`$LASTEXITCODE
    Set-Content -LiteralPath $(ConvertTo-PowerShellLiteral $driverExitCodeFile) -Value `$exitCode -Encoding ascii
    exit `$exitCode
} finally {
    Remove-Item -LiteralPath $(ConvertTo-PowerShellLiteral $driverPidFile) -Force -ErrorAction SilentlyContinue
}
"@
    $driver = Start-EncodedLocalProcess $driverScript $driverLog $driverErr
    $driver.WaitForExit()
    $driver.Refresh()
    if ($null -ne $vramSampler -and -not $vramSampler.HasExited) {
        Stop-Process -Id $vramSampler.Id -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $vramLog) {
        $vramSamples = foreach ($line in @(Get-Content -LiteralPath $vramLog -ErrorAction SilentlyContinue)) {
            $match = [regex]::Match($line, '^\s*1\s*,\s*(\d+)\s*$')
            if ($match.Success) { [int]$match.Groups[1].Value }
        }
        if (@($vramSamples).Count -gt 0) { $peak4080VramMiB = [int](@($vramSamples | Measure-Object -Maximum).Maximum) }
    }
    $driverText = Get-Content -LiteralPath $driverLog -Raw
    $driverExitCode = if (Test-Path -LiteralPath $driverExitCodeFile) {
        [int](Get-Content -LiteralPath $driverExitCodeFile -Raw).Trim()
    } else { -1 }
    function Get-DriverMetric([string]$Name, [string]$Text, [int]$Default = -1) {
        $match = [regex]::Match($Text, "(?m)(?:^|\s)$([regex]::Escape($Name))=([-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?)")
        if (-not $match.Success) { return $Default }
        try {
            [double]::Parse($match.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
        } catch { $Default }
    }
    $peakNodeQueue = [int](Get-DriverMetric 'peak_node_queue' $driverText 0)
    $peakInAdapter = [int](Get-DriverMetric 'peak_in_adapter' $driverText 0)
    $peakMainLane = [int](Get-DriverMetric 'peak_main_lane' $driverText -1)
    $samples = [int](Get-DriverMetric 'samples' $driverText 0)
    $completed = [int](Get-DriverMetric 'completed' $driverText -1)
    $failed = [int](Get-DriverMetric 'failed' $driverText -1)
    $unanswered = [int](Get-DriverMetric 'unanswered' $driverText -1)
    $routes = [int](Get-DriverMetric 'routes' $driverText -1)
    $tokens = [int](Get-DriverMetric 'tokens' $driverText -1)
    $elapsedMs = Get-DriverMetric 'elapsed_ms' $driverText -1
    $framesPerSecond = Get-DriverMetric 'frames_per_second' $driverText -1
    $latencyLine = [regex]::Match($driverText, '(?m)^P4_DRIVE_LATENCY[^\r\n]*$').Value
    $latencyCompleted = Get-DriverMetric 'completed' $latencyLine -1
    $latencyP50Ms = Get-DriverMetric 'p50_ms' $latencyLine -1
    $latencyP95Ms = Get-DriverMetric 'p95_ms' $latencyLine -1
    $latencyP99Ms = Get-DriverMetric 'p99_ms' $latencyLine -1
    $latencyMaxMs = Get-DriverMetric 'max_ms' $latencyLine -1
    $logicalMetricsLine = [regex]::Match($driverText, '(?m)^P4_DRIVE_LOGICAL_METRICS[^\r\n]*$').Value
    $prefillTpsOverRun = Get-DriverMetric 'prefill_tps_over_run' $logicalMetricsLine -1
    $generationTpsOverRun = Get-DriverMetric 'generation_tps_over_run' $logicalMetricsLine -1
    $averageSessionPrefillTps = Get-DriverMetric 'average_session_prefill_tps' $logicalMetricsLine -1
    $averageSessionGenerationTps = Get-DriverMetric 'average_session_generation_tps' $logicalMetricsLine -1
    $overlapPassed = $peakNodeQueue -ge $MinimumPeakNodeQueue -and
        $peakInAdapter -ge $MinimumPeakInAdapter
    $verdictsPassed = $true
    foreach ($verdictName in @(
        'every request answered', 'no request failed', 'every stream in order', 'one terminal per route'
    )) {
        if ($driverText -notmatch "(?m)^\s*\[pass\] $([regex]::Escape($verdictName))$") {
            $verdictsPassed = $false
            break
        }
    }
    $metricsPassed = $driverText -match '(?m)^P4_DRIVE_RESULT requests=' -and
        $driverText -match '(?m)^P4_DRIVE_READY ' -and
        $driverText -notmatch '(?m)^P4_DRIVE_UNREACHABLE ' -and
        $driverText -notmatch '(?m)^\s*NOTE the driver stopped waiting' -and
        $completed -eq $Requests -and $failed -eq 0 -and $unanswered -eq 0 -and
        $routes -ge $Requests -and $tokens -gt 0 -and $elapsedMs -ge 0 -and
        $framesPerSecond -gt 0 -and $latencyCompleted -eq $completed -and
        $latencyP95Ms -ge 0 -and $latencyP99Ms -ge $latencyP95Ms
        $peak4080VramMiB -ge 0 -and $peak4080VramMiB -le $Max4080VramMiB
    $runPassed = [bool](
        ($driverExitCode -eq 0) -and
        [bool]$metricsPassed -and
        $verdictsPassed -and
        [bool]$overlapPassed
    )
    $result = [pscustomobject]@{
        run_id = $RunId
        exit_code = $driverExitCode
        passed = $runPassed
        metrics = [pscustomobject]@{
            completed = $completed
            failed = $failed
            unanswered = $unanswered
            routes = $routes
            tokens = $tokens
            elapsed_ms = $elapsedMs
            frames_per_second = $framesPerSecond
            prefill_tps_over_run = $prefillTpsOverRun
            generation_tps_over_run = $generationTpsOverRun
            average_session_prefill_tps = $averageSessionPrefillTps
            average_session_generation_tps = $averageSessionGenerationTps
            peak_4080_vram_mib = $peak4080VramMiB
            max_4080_vram_mib = $Max4080VramMiB
            latency = [pscustomobject]@{
                completed = $latencyCompleted
                p50_ms = $latencyP50Ms
                p95_ms = $latencyP95Ms
                p99_ms = $latencyP99Ms
                max_ms = $latencyMaxMs
            }
            peak_main_lane = $peakMainLane
            samples = $samples
            passed = $metricsPassed
        }
        overlap = [pscustomobject]@{
            minimum_peak_node_queue = $MinimumPeakNodeQueue
            minimum_peak_in_adapter = $MinimumPeakInAdapter
            peak_node_queue = $peakNodeQueue
            peak_in_adapter = $peakInAdapter
            passed = $overlapPassed
        }
        topology = 'central RTX 3090 + central RTX 4080 + remote RTX 3090 x2'
        requests = $Requests
        tokens_each = $requestedTokens
        parallel_slots = $parallelSlots
        driver_ceiling = $parallelSlots
        arrive_milliseconds = $ArriveMilliseconds
        prompt_tokens = $effectivePromptTokens
        context_size = $planContextSize
        batch_size = $planBatchSize
        ubatch_size = $planUBatchSize
        stage_ranges = ($parsedStageRanges | ForEach-Object { "$($_.Begin):$($_.End)" }) -join ','
        gpu_layers = ($parsedGpuLayers -join ',')
        keep_loaded = [bool]$KeepLoaded
        chain = $chain
        output = $outputRoot
    }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputRoot 'result.json') -Encoding utf8
    if (-not $result.passed) { throw "Four-node driver or overlap gate failed. See $driverLog and $driverErr." }
}
finally {
    if ($null -ne $vramSampler -and -not $vramSampler.HasExited) {
        Stop-Process -Id $vramSampler.Id -Force -ErrorAction SilentlyContinue
    }
    $preserveLoaded = [bool]($KeepLoaded -and $null -ne $result -and $result.passed); if (-not $preserveLoaded) {
        foreach ($process in $localProcesses) {
            if ($null -ne $process -and -not $process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            }
        }
        try {
            foreach ($pidFile in @(Get-ChildItem -LiteralPath $outputRoot -Filter '*.pid' -File -ErrorAction SilentlyContinue)) {
                $pid = [int](Get-Content -LiteralPath $pidFile.FullName -Raw).Trim()
                $child = Get-Process -Id $pid -ErrorAction SilentlyContinue
                if ($null -ne $child -and $child.Path -in @($agentBinary, $driveBinary)) {
                    & taskkill.exe /PID $pid /T /F | Out-Null
                }
            }
        } catch { }
        try {
            $localExecutablePaths = @($agentBinary, $driveBinary)
            foreach ($process in @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
                $_.ExecutablePath -and $_.ExecutablePath -in $localExecutablePaths
            })) {
                & taskkill.exe /PID $process.ProcessId /T /F | Out-Null
            }
        } catch { }
        try {
        $cleanup = @"
`$root = $(ConvertTo-PowerShellLiteral $RemoteArtifactDirectory)
foreach (`$taskName in @('P4-Codex-$RunId-agent-53001', 'P4-Codex-$RunId-agent-53002')) { schtasks.exe /end /tn `$taskName /f *> `$null; schtasks.exe /delete /tn `$taskName /f *> `$null }
foreach (`$pidFile in @(Get-ChildItem -LiteralPath `$root -Filter '*.pid' -File -ErrorAction SilentlyContinue)) {
    try {
        `$pid = [int](Get-Content -LiteralPath `$pidFile.FullName -Raw).Trim()
        `$process = Get-Process -Id `$pid -ErrorAction SilentlyContinue
        if (`$null -ne `$process -and `$process.Path -like "`$root\\*") {
            Stop-Process -Id `$pid -Force -ErrorAction SilentlyContinue
        }
    } catch { }
}
# A disconnected SSH wrapper can remove its pid file before the parent-side
# cleanup runs.  Reconcile by executable path as a second, bounded sweep.
`$rootPrefix = (`$root.TrimEnd('\') + '\')
foreach (`$process in @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    `$_.ExecutablePath -and `$_.ExecutablePath.StartsWith(`$rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})) {
    & taskkill.exe /PID `$process.ProcessId /T /F *> `$null
}
# Some restricted accounts do not expose ExecutablePath through WMI.  The
# command line still contains the unique run directory, so reconcile those
# descendants as well without touching unrelated Agent processes.
foreach (`$process in @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    `$_.CommandLine -and `$_.CommandLine.IndexOf(`$root, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
})) {
    & taskkill.exe /PID `$process.ProcessId /T /F *> `$null
}
"@
            Invoke-RemotePowerShell $cleanup | Out-File -LiteralPath (Join-Path $outputRoot 'remote-cleanup.log') -Encoding utf8
        } catch { }
        if (-not $KeepRemoteArtifacts) {
            try {
            $remoteNames = ($artifactFiles | ForEach-Object { ConvertTo-PowerShellLiteral $_.Name }) -join ','
            $removeCopied = "`$root = $(ConvertTo-PowerShellLiteral $RemoteArtifactDirectory); foreach (`$name in @($remoteNames)) { Remove-Item -LiteralPath (Join-Path `$root `$name) -Force -ErrorAction SilentlyContinue }; Remove-Item -LiteralPath $(ConvertTo-PowerShellLiteral $remoteLogDirectory) -Recurse -Force -ErrorAction SilentlyContinue"
                Invoke-RemotePowerShell $removeCopied | Out-Null
            } catch { }
        }
    } else {
        Write-Output "KEEP_LOADED: agents and SSH tunnel remain active. Evidence: $outputRoot"
    }
}
if ($null -eq $result -or -not $result.passed) { exit 1 }
Write-Output "PASS: SSH-forwarded four-node staged E2E. Evidence: $outputRoot"
