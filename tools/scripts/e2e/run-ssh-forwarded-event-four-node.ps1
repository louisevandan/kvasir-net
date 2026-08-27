[CmdletBinding()]
param(
    [string]$RunId = (Get-Date -Format 'yyyyMMddHHmmss'),
    [string]$SshTarget = '42mob@192.168.0.29',
    [string]$RemoteUserId = 'm42-server2\42mob',
    [string]$ArtifactDirectory = '',
    [string]$AgentBinary = '',
    [string]$EventDriveBinary = '',
    [string]$LocalModel = 'S:\models\unsloth\MiniMax-M3-GGUF\MiniMax-M3-UD-Q5_K_S-00001-of-00008.gguf',
    [string]$RemoteModel = 'S:\models\unsloth\MiniMax-M3-GGUF\MiniMax-M3-UD-Q5_K_S-00001-of-00008.gguf',
    [string]$PlacementPlanFile = '',
    [string]$PromptsFile = 'target\p4-minimax-m3-semantic-fixture-40-current\prompts.json',
    [string]$ResponsesFile = 'target\p4-minimax-m3-semantic-fixture-40-current\response-expectations.json',
    [string]$OptionsFile = 'target\p4-minimax-m3-gate-20260827\options.json',
    [int]$Requests = 1,
    [int]$Parallel = 1,
    [int]$ContextSize = 1200,
    [int]$PromptTokens = 500,
    [int]$Tokens = 500,
    [int]$BatchSize = 512,
    [int]$UBatchSize = 512,
    [int]$InitialBurst = 1,
    [int]$BatchRequests = 0,
    [int]$BatchIntervalMilliseconds = 0,
    [int]$FlashAttention = 1,
    [ValidateSet('none','draft-mtp')][string]$SpeculativeType = 'none',
    [int]$Max4080VramMiB = 12288,
    [int]$Max3090VramMiB = 23552,
    [int]$DeviceRuntimeReserveMiB = 512,
    [int]$HostMemoryReserveMiB = 8192,
    [switch]$SerialCorrectness,
    [switch]$KeepRemoteArtifacts
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
Import-Module (Join-Path $PSScriptRoot 'event-four-node-runtime.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'event-four-node-fixture.psm1') -Force
function Resolve-Input([string]$Value) {
    if ([IO.Path]::IsPathRooted($Value)) { return $Value }
    [IO.Path]::GetFullPath((Join-Path $projectRoot $Value))
}
function Assert-File([string]$Value, [string]$Name) {
    if (-not (Test-Path -LiteralPath $Value -PathType Leaf)) { throw "$Name not found: $Value" }
}
function Get-LatestWrite([string[]]$Roots) {
    $files = @($Roots | ForEach-Object { Get-ChildItem -LiteralPath $_ -Recurse -File } |
        Where-Object { $_.Name -eq 'CMakeLists.txt' -or
            $_.Extension -in @('.rs','.cpp','.h','.hpp','.inc','.patch','.json','.cmake') })
    if ($files.Count -eq 0) { throw 'Source freshness roots contain no files.' }
    ($files | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1).LastWriteTimeUtc
}
function Assert-Fresh([string]$Binary, [string[]]$Roots, [string]$Name) {
    $source = Get-LatestWrite $Roots
    $item = Get-Item -LiteralPath $Binary
    if ($item.LastWriteTimeUtc -lt $source) {
        throw "$Name is older than its source: binary=$($item.LastWriteTimeUtc.ToString('o')) source=$($source.ToString('o'))"
    }
}
function New-Waves {
    if ($InitialBurst -lt 1 -or $InitialBurst -gt $Requests) { throw 'InitialBurst must be between 1 and Requests.' }
    $waves = [Collections.Generic.List[object]]::new()
    $waves.Add([pscustomobject]@{ after_ms = 0; count = $InitialBurst })
    $sent = $InitialBurst; $after = 0
    while ($sent -lt $Requests) {
        if ($BatchRequests -lt 1 -or $BatchIntervalMilliseconds -lt 1) {
            throw 'Additional requests require positive BatchRequests and BatchIntervalMilliseconds.'
        }
        $after += $BatchIntervalMilliseconds
        $count = [Math]::Min($BatchRequests, $Requests - $sent)
        $waves.Add([pscustomobject]@{ after_ms = $after; count = $count })
        $sent += $count
    }
    @($waves)
}
function Get-GpuRows([string[]]$Lines, [string]$HostName) {
    @($Lines | Where-Object { $_ -match ',' } | ForEach-Object {
        $parts = $_.Split(',') | ForEach-Object { $_.Trim() }
        [pscustomobject]@{ host=$HostName; index=[int]$parts[0]; name=$parts[1]; memory_used_mib=[int]$parts[2] }
    })
}
function Get-GpuInventory([string[]]$Lines, [string]$HostName) {
    @($Lines | Where-Object { $_ -match ',' } | ForEach-Object {
        $parts = $_.Split(',') | ForEach-Object { $_.Trim() }
        [pscustomobject]@{
            host=$HostName; index=[int]$parts[0]; uuid=$parts[1]; name=$parts[2]
            pci_bus_id=$parts[3]; memory_total_mib=[int]$parts[4]; memory_used_mib=[int]$parts[5]
        }
    })
}
function Assert-ProcessAlive([Diagnostics.Process]$Process, [string]$Name, [string]$ErrorLog) {
    if ($null -eq $Process) { throw "$Name process was not started." }
    $Process.Refresh()
    if ($Process.HasExited) {
        $detail = if (Test-Path -LiteralPath $ErrorLog) { Get-Content -LiteralPath $ErrorLog -Raw } else { '' }
        throw "$Name exited unexpectedly with code $($Process.ExitCode). stderr=$detail"
    }
}
function Assert-Vram([object[]]$Rows, [string]$Phase) {
    $local4080 = @($Rows | Where-Object { $_.host -eq 'local' -and $_.name -match '4080' })
    $all3090 = @($Rows | Where-Object { $_.name -match '3090' })
    if ($Rows.Count -ne 4 -or $local4080.Count -ne 1 -or $all3090.Count -ne 3) {
        throw "$Phase GPU inventory is not one RTX 4080 plus three RTX 3090 devices."
    }
    if ($local4080[0].memory_used_mib -gt $Max4080VramMiB) {
        throw "$Phase RTX 4080 VRAM exceeded: $($local4080[0].memory_used_mib)/$Max4080VramMiB MiB."
    }
    $over = @($all3090 | Where-Object { $_.memory_used_mib -gt $Max3090VramMiB })
    if ($over.Count -gt 0) { throw "$Phase RTX 3090 VRAM exceeded: $($over | ConvertTo-Json -Compress)." }
}

if ($RunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$') { throw 'RunId is invalid.' }
if ([string]::IsNullOrWhiteSpace($PlacementPlanFile)) {
    throw 'PlacementPlanFile is required and must be planned for this exact context and parallel width.'
}
if ($Requests -lt 1 -or $Parallel -lt 1 -or $ContextSize -lt 1 -or
    $PromptTokens -lt 1 -or $Tokens -lt 1) { throw 'Request capacities must be positive.' }
if ($PromptTokens + $Tokens -gt $ContextSize) { throw 'Prompt plus output budget exceeds per-sequence context.' }
if ($SerialCorrectness -and ($Requests -ne 1 -or $Parallel -ne 1)) { throw 'SerialCorrectness requires Requests=1 and Parallel=1.' }
if ($UBatchSize -lt 1 -or $UBatchSize -gt $BatchSize) { throw 'UBatchSize must be positive and no greater than BatchSize.' }
if ($DeviceRuntimeReserveMiB -lt 256) { throw 'DeviceRuntimeReserveMiB must leave at least 256 MiB.' }
if ($HostMemoryReserveMiB -lt 1024) { throw 'HostMemoryReserveMiB must leave at least 1024 MiB.' }
if ($FlashAttention -notin @(0,1)) { throw 'FlashAttention must be zero or one.' }
if (-not (Get-Command ssh.exe -ErrorAction SilentlyContinue)) { throw 'ssh.exe is required.' }
if (-not (Get-Command scp.exe -ErrorAction SilentlyContinue)) { throw 'scp.exe is required.' }
$waves = @(New-Waves)
$PlacementPlanFile = Resolve-Input $PlacementPlanFile
$PromptsFile = Resolve-Input $PromptsFile
$ResponsesFile = Resolve-Input $ResponsesFile
$OptionsFile = Resolve-Input $OptionsFile
foreach ($item in @(
    @($PlacementPlanFile,'PlacementPlanFile'), @($PromptsFile,'PromptsFile'),
    @($ResponsesFile,'ResponsesFile'), @($OptionsFile,'OptionsFile'), @($LocalModel,'LocalModel')
)) { Assert-File $item[0] $item[1] }
if (-not [string]::Equals($LocalModel, $RemoteModel, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'This acceptance run requires the same absolute shared model path on both hosts.'
}
if ([string]::IsNullOrWhiteSpace($ArtifactDirectory)) { $ArtifactDirectory = Join-Path $projectRoot '.cache\staged-server-m3-86-89\Release' }
if ([string]::IsNullOrWhiteSpace($AgentBinary)) { $AgentBinary = Join-Path $projectRoot 'apps\p4\target\release\p4-agent.exe' }
if ([string]::IsNullOrWhiteSpace($EventDriveBinary)) { $EventDriveBinary = Join-Path $projectRoot 'apps\p4\target\release\p4-event-drive.exe' }
$ArtifactDirectory = Resolve-Input $ArtifactDirectory
$AgentBinary = Resolve-Input $AgentBinary
$EventDriveBinary = Resolve-Input $EventDriveBinary
$stageServer = Join-Path $ArtifactDirectory 'p4_staged_server.exe'
foreach ($item in @(@($AgentBinary,'AgentBinary'),@($EventDriveBinary,'EventDriveBinary'),@($stageServer,'StageServer'))) { Assert-File $item[0] $item[1] }
Assert-Fresh $AgentBinary @((Join-Path $projectRoot 'apps\p4\entrypoints\agent\src'),(Join-Path $projectRoot 'apps\p4\layers\protocol\src'),(Join-Path $projectRoot 'apps\p4\layers\agent\src'),(Join-Path $projectRoot 'apps\p4\layers\adapters\llamacpp\staged\adapter\src')) 'p4-agent'
Assert-Fresh $EventDriveBinary @((Join-Path $projectRoot 'apps\p4\tools\event-drive\src'),(Join-Path $projectRoot 'apps\p4\layers\protocol\src'),(Join-Path $projectRoot 'apps\p4\layers\adapters\llamacpp\staged\adapter\src')) 'p4-event-drive'
Assert-Fresh $stageServer @((Join-Path $projectRoot 'apps\p4\layers\adapters\llamacpp\staged\server\src'),(Join-Path $projectRoot 'apps\p4\layers\adapters\llamacpp\staged\compat')) 'p4_staged_server'

$outputRoot = Join-Path $projectRoot "target\p4-event-four-node\$RunId"
if (Test-Path -LiteralPath $outputRoot) { throw "Run output already exists: $outputRoot" }
$fixtureProvenance = Assert-P4FixtureProvenance -PromptsFile $PromptsFile `
    -ResponsesFile $ResponsesFile -ArtifactDirectory $ArtifactDirectory -Model $LocalModel `
    -RequestCount $Requests -PromptTokens $PromptTokens
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$fixtureProvenance | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath `
    (Join-Path $outputRoot 'fixture-provenance.json') -Encoding utf8
$remoteRoot = "C:\Users\42mob\p4-event-$RunId"
$remoteTasks = @(
    "P4-Event-$RunId-agent-53001",
    "P4-Event-$RunId-memory-stage-2",
    "P4-Event-$RunId-memory-stage-3"
)
$reserved = @(52003,52103,52104,53001)
$busy = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in $reserved })
if ($busy.Count -gt 0) { throw "Reserved local ports are busy: $(@($busy.LocalPort) -join ',')." }
$remotePreflight = @"
`$ErrorActionPreference='Stop'; `$ProgressPreference='SilentlyContinue'
`$ports=@(52003,53001,53103,53104)
`$busy=@(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { `$_.LocalPort -in `$ports })
if (`$busy.Count -gt 0) { throw "Remote ports busy: `$(`$busy.LocalPort -join ',')" }
if (Test-Path -LiteralPath $(ConvertTo-P4PowerShellLiteral $remoteRoot)) { throw 'Remote run directory already exists.' }
New-Item -ItemType Directory -Force -Path $(ConvertTo-P4PowerShellLiteral $remoteRoot) | Out-Null
Write-Output 'REMOTE_EVENT_PREFLIGHT_OK'
"@
Invoke-P4RemotePowerShell $SshTarget $remotePreflight | Set-Content -LiteralPath (Join-Path $outputRoot 'remote-preflight.log')
$kernelPowerBefore = Get-P4RemoteKernelPowerRecord $SshTarget
$remoteSystem = @"
`$os=Get-CimInstance Win32_OperatingSystem
`$page=@(Get-CimInstance Win32_PageFileUsage | Select-Object Name,AllocatedBaseSize,CurrentUsage,PeakUsage)
[ordered]@{captured_at=(Get-Date).ToUniversalTime().ToString('o');total_visible_kib=[uint64]`$os.TotalVisibleMemorySize;free_physical_kib=[uint64]`$os.FreePhysicalMemory;total_virtual_kib=[uint64]`$os.TotalVirtualMemorySize;free_virtual_kib=[uint64]`$os.FreeVirtualMemory;page_files=`$page}|ConvertTo-Json -Depth 5 -Compress
"@
((Invoke-P4RemotePowerShell $SshTarget $remoteSystem) -join "`n") | Set-Content -LiteralPath (Join-Path $outputRoot 'remote-system-before.json') -Encoding utf8
$inventoryQuery="& nvidia-smi.exe '--query-gpu=index,uuid,name,pci.bus_id,memory.total,memory.used' '--format=csv,noheader,nounits'"
$localInventory=Get-GpuInventory @(& nvidia-smi.exe '--query-gpu=index,uuid,name,pci.bus_id,memory.total,memory.used' '--format=csv,noheader,nounits') 'local'
$remoteInventory=Get-GpuInventory @(Invoke-P4RemotePowerShell $SshTarget $inventoryQuery) 'remote'
$inventory=@($localInventory)+@($remoteInventory)
$local4080=@($localInventory | Where-Object name -Match '4080')
$local3090=@($localInventory | Where-Object name -Match '3090')
$remote3090=@($remoteInventory | Where-Object name -Match '3090' | Sort-Object pci_bus_id)
if($local4080.Count -ne 1 -or $local3090.Count -ne 1 -or $remote3090.Count -ne 2){throw 'GPU UUID inventory is not one local RTX 4080, one local RTX 3090, and two remote RTX 3090 devices.'}
$inventory | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputRoot 'gpu-inventory.json') -Encoding utf8

$artifactFiles = @(Get-Item -LiteralPath $AgentBinary) + @(Get-ChildItem -LiteralPath $ArtifactDirectory -File | Where-Object {
    $_.Name -eq 'p4_staged_server.exe' -or $_.Name -like 'ggml*.dll' -or $_.Name -like 'llama*.dll'
})
$cudaRoot = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64'
$artifactFiles += @('cublas64_13.dll','cublasLt64_13.dll','cudart64_13.dll') | ForEach-Object { Get-Item -LiteralPath (Join-Path $cudaRoot $_) }
$artifactFiles = @($artifactFiles | Sort-Object Name -Unique)
$manifest = @($artifactFiles | ForEach-Object { [pscustomobject]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()} })
$manifest | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $outputRoot 'artifact-manifest.json') -Encoding utf8
foreach ($file in $artifactFiles) {
    $remoteFile = ($remoteRoot -replace '\\','/') + '/' + $file.Name
    & scp.exe -q -o BatchMode=yes $file.FullName "$SshTarget`:$remoteFile"
    if ($LASTEXITCODE -ne 0) { throw "scp failed for $($file.Name)." }
}
$remoteVerify = @"
Get-ChildItem -LiteralPath $(ConvertTo-P4PowerShellLiteral $remoteRoot) -File | ForEach-Object {
    [pscustomobject]@{name=`$_.Name;bytes=`$_.Length;sha256=(Get-FileHash -LiteralPath `$_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
} | ConvertTo-Json -Compress
"@
$remoteManifest = ((Invoke-P4RemotePowerShell $SshTarget $remoteVerify) -join "`n") | ConvertFrom-Json
foreach ($local in $manifest) {
    $remote = @($remoteManifest | Where-Object name -eq $local.name)
    if ($remote.Count -ne 1 -or $remote[0].sha256 -ne $local.sha256) { throw "Remote artifact mismatch: $($local.name)" }
}

$remoteServer = Join-Path $remoteRoot 'p4_staged_server.exe'
$spec = [ordered]@{
    ingress_agent='tcp://127.0.0.1:52003';channel="gate-$RunId";session_id="session-$RunId";request_id="request-$RunId"
    request_count=$Requests;parallel=$Parallel;context_size=$ContextSize;n_batch=$BatchSize;n_ubatch=$UBatchSize;max_tokens=$Tokens
    speculative_type=$SpeculativeType
    prompts_file=$PromptsFile;responses_file=$ResponsesFile;placement_file=$PlacementPlanFile;options_file=$OptionsFile;waves=$waves
    flash_attention=($FlashAttention -eq 1);pre_inference_hold_ms=30000;minimum_generated_tokens=180;expected_prefill_rows=$PromptTokens
    allowed_stop_reasons=@('eos','length');timeout_ms=7200000
    stages=@(
        [ordered]@{agent='tcp://127.0.0.1:52003';node='stage-0';binary=$stageServer;endpoint='127.0.0.1:52103';model=$LocalModel;cuda_visible_devices=$local4080[0].uuid},
        [ordered]@{agent='tcp://127.0.0.1:52003';node='stage-1';binary=$stageServer;endpoint='127.0.0.1:52104';model=$LocalModel;cuda_visible_devices=$local3090[0].uuid},
        [ordered]@{agent='tcp://127.0.0.1:53001';node='stage-2';binary=$remoteServer;endpoint='127.0.0.1:53103';model=$RemoteModel;cuda_visible_devices=$remote3090[0].uuid},
        [ordered]@{agent='tcp://127.0.0.1:53001';node='stage-3';binary=$remoteServer;endpoint='127.0.0.1:53104';model=$RemoteModel;cuda_visible_devices=$remote3090[1].uuid}
    )
}
$specFile=Join-Path $outputRoot 'spec.json'; $configFile=Join-Path $outputRoot 'config.json'
$spec | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $specFile -Encoding utf8
& node.exe (Join-Path $PSScriptRoot 'event-four-node-config.mjs') $specFile $configFile
if ($LASTEXITCODE -ne 0) { throw 'Event config generation failed.' }
$config = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
$deviceTargets = @{
    'stage-0'=[pscustomobject]@{location='local';target=$local4080[0];cap=$Max4080VramMiB;task=$null}
    'stage-1'=[pscustomobject]@{location='local';target=$local3090[0];cap=$Max3090VramMiB;task=$null}
    'stage-2'=[pscustomobject]@{location='remote';target=$remote3090[0];cap=$Max3090VramMiB;task=$remoteTasks[1]}
    'stage-3'=[pscustomobject]@{location='remote';target=$remote3090[1];cap=$Max3090VramMiB;task=$remoteTasks[2]}
}
$memoryPlans = @($config.nodes | ForEach-Object {
    $device = $deviceTargets[$_.node]
    if ($null -eq $device) { throw "No physical memory target for node $($_.node)." }
    $plan = if ($device.location -eq 'remote') {
        Invoke-P4RemoteStageMemoryPlan -Target $SshTarget -TaskName $device.task `
            -RunDirectory $remoteRoot -UserId $RemoteUserId -Binary $_.binary `
            -Plan $_.plan -VisibleDevice $device.target.uuid
    } else {
        Invoke-P4StageMemoryPlan -Binary $_.binary -Plan $_.plan `
            -VisibleDevice $device.target.uuid
    }
    if ($plan.schema -ne 2 -or $plan.memory_topology.mode -ne 'discrete' -or
        @($plan.memory_topology.host_shared_devices).Count -ne 0) {
        throw "Node $($_.node) did not apply the explicit discrete memory topology."
    }
    $shape = $plan.execution_shape
    if ($null -eq $shape -or $shape.n_ctx_seq -lt $_.context_size -or
        $shape.n_ctx -lt $_.total_context_size -or $shape.n_batch -ne $_.n_batch -or
        $shape.n_ubatch -ne $_.n_ubatch -or
        $shape.n_seq_max -ne $_.sequence_capacity -or $shape.kv_unified -ne $true) {
        throw "Node $($_.node) no-allocation execution shape differs from its load contract: $($shape | ConvertTo-Json -Compress)."
    }
    $deviceEntries = @($plan.entries | Where-Object scope -eq 'device')
    $hostEntries = @($plan.entries | Where-Object scope -eq 'host')
    if ($deviceEntries.Count -ne 1 -or $hostEntries.Count -ne 1) { throw "Node $($_.node) did not produce one device and one host memory entry." }
    $availableDevice = ([int64]$device.cap - [int64]$device.target.memory_used_mib -
        [int64]$DeviceRuntimeReserveMiB) * 1MB
    if ([uint64]$deviceEntries[0].required -gt [uint64][Math]::Max(0, $availableDevice)) {
        throw "Node $($_.node) exceeds its GPU cap before allocation: required=$($deviceEntries[0].required) available=$availableDevice."
    }
    [pscustomobject]@{node=$_.node;agent=$_.agent;target_gpu=$device.target;plan=$plan}
})
$localOs = Get-CimInstance Win32_OperatingSystem
$remoteBefore = Get-Content -LiteralPath (Join-Path $outputRoot 'remote-system-before.json') -Raw | ConvertFrom-Json
$hostAvailable = @{
    'tcp://127.0.0.1:52003'=Get-P4AvailableHostBytes ([uint64]$localOs.FreePhysicalMemory) ([uint64]$HostMemoryReserveMiB) 'local host'
    'tcp://127.0.0.1:53001'=Get-P4AvailableHostBytes ([uint64]$remoteBefore.free_physical_kib) ([uint64]$HostMemoryReserveMiB) 'remote host'
}
foreach ($agent in $hostAvailable.Keys) {
    $required = [uint64](($memoryPlans | Where-Object agent -eq $agent | ForEach-Object {
        [uint64](@($_.plan.entries | Where-Object scope -eq 'host')[0].required)
    } | Measure-Object -Sum).Sum)
    if ($required -gt $hostAvailable[$agent]) { throw "Agent $agent exceeds host memory before allocation: required=$required available=$($hostAvailable[$agent])." }
}
$memoryPlans | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $outputRoot 'memory-preflight.json') -Encoding utf8

$localAgents = [Collections.Generic.List[Diagnostics.Process]]::new()
$tunnel = $null; $localSampler = $null; $remoteSampler = $null; $drive = $null
$driveArtifact = Join-Path $outputRoot 'artifact.json'
try {
    $tunnel = Start-Process ssh.exe -ArgumentList @('-T','-N','-o','BatchMode=yes','-o','ExitOnForwardFailure=yes','-o','ServerAliveInterval=10','-o','ServerAliveCountMax=3','-L','53001:127.0.0.1:53001','-R','52003:127.0.0.1:52003',$SshTarget) -RedirectStandardOutput (Join-Path $outputRoot 'tunnel.log') -RedirectStandardError (Join-Path $outputRoot 'tunnel.err.log') -WindowStyle Hidden -PassThru
    Start-Sleep -Seconds 1; $tunnel.Refresh(); if ($tunnel.HasExited) { throw 'SSH tunnel exited before agent launch.' }
    $remotePort=53001; $remoteAddress="tcp://127.0.0.1:$remotePort"
    Start-P4RemoteEventAgent -Target $SshTarget -TaskName $remoteTasks[0] -RunDirectory $remoteRoot -UserId $RemoteUserId -Port $remotePort -Address $remoteAddress
    Wait-P4RemoteEventAgent -Target $SshTarget -RunDirectory $remoteRoot -Port $remotePort -Address $remoteAddress | Set-Content -LiteralPath (Join-Path $outputRoot "remote-ready-$remotePort.log")
    $localPort=52003; $localAddress="tcp://127.0.0.1:$localPort"; $out=Join-Path $outputRoot "agent-$localPort.log"; $err=Join-Path $outputRoot "agent-$localPort.err.log"
    $launcher=Start-P4LocalEventAgent -Binary $AgentBinary -Listen "127.0.0.1:$localPort" -Address $localAddress -WorkingDirectory (Join-Path $projectRoot 'apps\p4') -Stdout $out -Stderr $err
    [void]$localAgents.Add($launcher)
    Wait-P4LocalEventAgent -Launcher $launcher -Binary $AgentBinary -Address $localAddress -Port $localPort -Stdout $out -Stderr $err | Set-Content -LiteralPath (Join-Path $outputRoot "local-ready-$localPort.log")
    $localGpu=Join-Path $outputRoot 'gpu-local.csv'; $remoteGpu=Join-Path $outputRoot 'gpu-remote.csv'
    $localSampler=Start-Process nvidia-smi.exe -ArgumentList @('--query-gpu=timestamp,index,uuid,name,utilization.gpu,memory.used,power.draw','--format=csv,noheader,nounits','-lms','250') -RedirectStandardOutput $localGpu -RedirectStandardError (Join-Path $outputRoot 'gpu-local.err.log') -WindowStyle Hidden -PassThru
    $remoteGpuCommand="& nvidia-smi.exe '--query-gpu=timestamp,index,uuid,name,utilization.gpu,memory.used,power.draw' '--format=csv,noheader,nounits' '-lms' '250'"
    $remoteSampler=Start-Process ssh.exe -ArgumentList @('-T','-o','BatchMode=yes',$SshTarget,"powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand $(ConvertTo-P4EncodedCommand $remoteGpuCommand)") -RedirectStandardOutput $remoteGpu -RedirectStandardError (Join-Path $outputRoot 'gpu-remote.err.log') -WindowStyle Hidden -PassThru
    $driveOut=Join-Path $outputRoot 'drive.log'; $driveErr=Join-Path $outputRoot 'drive.err.log'
    $drive=Start-Process -FilePath $EventDriveBinary -ArgumentList @($configFile,$driveArtifact) -WorkingDirectory (Join-Path $projectRoot 'apps\p4') -RedirectStandardOutput $driveOut -RedirectStandardError $driveErr -WindowStyle Hidden -PassThru
    $loadDeadline=(Get-Date).AddHours(2)
    do {
        $drive.Refresh(); $text=if(Test-Path -LiteralPath $driveOut){Get-Content -LiteralPath $driveOut -Raw}else{''}
        Assert-ProcessAlive $tunnel 'SSH tunnel' (Join-Path $outputRoot 'tunnel.err.log')
        Assert-ProcessAlive $localSampler 'local GPU sampler' (Join-Path $outputRoot 'gpu-local.err.log')
        Assert-ProcessAlive $remoteSampler 'remote GPU sampler' (Join-Path $outputRoot 'gpu-remote.err.log')
        if($text -match 'P4_EVENT_GATE_LOADED'){break}
        if($drive.HasExited){$errorText=if(Test-Path -LiteralPath $driveErr){Get-Content -LiteralPath $driveErr -Raw}else{''};throw "Event drive exited before loaded state. stderr=$errorText"}
        Start-Sleep -Seconds 1
    } while((Get-Date) -lt $loadDeadline)
    if($text -notmatch 'P4_EVENT_GATE_LOADED'){throw 'Timed out waiting for all four loaded Event nodes.'}
    $localRows=Get-GpuRows @(& nvidia-smi.exe '--query-gpu=index,name,memory.used' '--format=csv,noheader,nounits') 'local'
    $remoteQuery="& nvidia-smi.exe '--query-gpu=index,name,memory.used' '--format=csv,noheader,nounits'"
    $remoteRows=Get-GpuRows @(Invoke-P4RemotePowerShell $SshTarget $remoteQuery) 'remote'
    $loadedRows=@($localRows)+@($remoteRows); $loadedRows | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $outputRoot 'loaded-vram.json')
    Assert-Vram $loadedRows 'post-load'
    while(-not $drive.HasExited){
        Assert-ProcessAlive $tunnel 'SSH tunnel' (Join-Path $outputRoot 'tunnel.err.log')
        Assert-ProcessAlive $localSampler 'local GPU sampler' (Join-Path $outputRoot 'gpu-local.err.log')
        Assert-ProcessAlive $remoteSampler 'remote GPU sampler' (Join-Path $outputRoot 'gpu-remote.err.log')
        Start-Sleep -Seconds 1; $drive.Refresh()
    }
    if($drive.ExitCode -ne 0){$errorText=if(Test-Path -LiteralPath $driveErr){Get-Content -LiteralPath $driveErr -Raw}else{''};throw "Event drive failed with exit $($drive.ExitCode). stderr=$errorText"}
    if(-not(Test-Path -LiteralPath $driveArtifact)){throw 'Event drive produced no artifact.'}
    $artifact=Get-Content -LiteralPath $driveArtifact -Raw | ConvertFrom-Json
    if($artifact.passed -ne $true){throw 'Event artifact did not pass semantic acceptance.'}
    Stop-P4ProcessTree $localSampler; Stop-P4ProcessTree $remoteSampler; $localSampler=$null; $remoteSampler=$null
    $summaryText=& node.exe (Join-Path $projectRoot 'test\benchmarks\direct-pipeline\summarize-gpu-csv.mjs') $localGpu $remoteGpu
    if($LASTEXITCODE -ne 0){throw 'GPU summary failed.'}
    $summary=($summaryText -join "`n") | ConvertFrom-Json; $summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputRoot 'gpu-summary.json')
    $peakRows=@($summary.devices | ForEach-Object {[pscustomobject]@{host=$(if(@($_.sources | Where-Object {$_ -eq $localGpu}).Count){'local'}else{'remote'});index=$_.index;name=$_.name;memory_used_mib=[int][Math]::Ceiling($_.memory_used_mib_max)}})
    Assert-Vram $peakRows 'full-run peak'
    if(@($summary.devices | Where-Object {$_.samples -lt 2 -or $_.utilization_gpu_percent_max -le 0}).Count -gt 0){throw 'Every GPU must have sampled non-zero activity.'}
    $kernelPowerAfter=Get-P4RemoteKernelPowerRecord $SshTarget
    if($kernelPowerAfter -ne $kernelPowerBefore){throw "Remote Kernel-Power 41 changed: before=$kernelPowerBefore after=$kernelPowerAfter"}
    [ordered]@{
        execution_complete=$true
        automated_acceptance_passed=$true
        manual_semantic_verdict='pending'
        reportable_performance=$false
        run_id=$RunId
        source_head=(& git.exe -C $projectRoot rev-parse HEAD).Trim()
        requests=$Requests;parallel=$Parallel;context_size=$ContextSize;prompt_tokens=$PromptTokens;tokens=$Tokens
        kernel_power_41_before=$kernelPowerBefore;kernel_power_41_after=$kernelPowerAfter
        binaries=[ordered]@{
            agent=(Get-FileHash $AgentBinary -Algorithm SHA256).Hash.ToLowerInvariant()
            event_drive=(Get-FileHash $EventDriveBinary -Algorithm SHA256).Hash.ToLowerInvariant()
            stage_server=(Get-FileHash $stageServer -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        artifact=$driveArtifact
        gpu_summary=(Join-Path $outputRoot 'gpu-summary.json')
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath `
        (Join-Path $outputRoot 'result.json') -Encoding utf8
    Write-Output "EVIDENCE_READY: manual semantic review is pending. Evidence: $outputRoot"
} finally {
    foreach($process in @($drive,$localSampler,$remoteSampler,$tunnel)){if($null -ne $process){Stop-P4ProcessTree $process}}
    foreach($process in $localAgents){Stop-P4ProcessTree $process}
    try {
        $kernelPowerFinal = Get-P4RemoteKernelPowerRecord $SshTarget
        [ordered]@{
            captured_at=(Get-Date).ToUniversalTime().ToString('o')
            before=$kernelPowerBefore
            after=$kernelPowerFinal
            changed=($kernelPowerFinal -ne $kernelPowerBefore)
        } | ConvertTo-Json -Compress | Set-Content -LiteralPath `
            (Join-Path $outputRoot 'remote-kernel-power-final.json') -Encoding utf8
        ((Invoke-P4RemotePowerShell $SshTarget $remoteSystem) -join "`n") |
            Set-Content -LiteralPath (Join-Path $outputRoot 'remote-system-final.json') -Encoding utf8
        Invoke-P4RemotePowerShell $SshTarget $inventoryQuery |
            Set-Content -LiteralPath (Join-Path $outputRoot 'remote-gpu-final.csv') -Encoding utf8
    } catch {
        $_.Exception.Message | Set-Content -LiteralPath `
            (Join-Path $outputRoot 'remote-final-evidence.err.log') -Encoding utf8
    }
    try {
        foreach($port in @(53001)){
            $remoteFile=($remoteRoot -replace '\\','/')+"/agent-$port.log"; & scp.exe -q -o BatchMode=yes "$SshTarget`:$remoteFile" (Join-Path $outputRoot "agent-$port.log")
            $remoteFile=($remoteRoot -replace '\\','/')+"/agent-$port.err.log"; & scp.exe -q -o BatchMode=yes "$SshTarget`:$remoteFile" (Join-Path $outputRoot "agent-$port.err.log")
        }
    } catch {}
    try {
        $cleanup=@"
`$root=$(ConvertTo-P4PowerShellLiteral $remoteRoot)
foreach(`$task in @($(($remoteTasks | ForEach-Object {ConvertTo-P4PowerShellLiteral $_}) -join ','))){`$registered=Get-ScheduledTask -TaskName `$task -ErrorAction SilentlyContinue;if(`$null -ne `$registered){Stop-ScheduledTask -InputObject `$registered -ErrorAction SilentlyContinue;Unregister-ScheduledTask -InputObject `$registered -Confirm:`$false}}
if(Test-Path -LiteralPath `$root){foreach(`$process in @(Get-CimInstance Win32_Process | Where-Object {`$_.ExecutablePath -and `$_.ExecutablePath.StartsWith((`$root.TrimEnd('\')+'\'),[StringComparison]::OrdinalIgnoreCase)})){taskkill.exe /PID `$process.ProcessId /T /F *>`$null}}
"@
        Invoke-P4RemotePowerShell $SshTarget $cleanup | Out-Null
        if(-not $KeepRemoteArtifacts){Invoke-P4RemotePowerShell $SshTarget "Remove-Item -LiteralPath $(ConvertTo-P4PowerShellLiteral $remoteRoot) -Recurse -Force -ErrorAction SilentlyContinue" | Out-Null}
    } catch {}
}
