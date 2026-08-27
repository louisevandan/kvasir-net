Set-StrictMode -Version Latest

function ConvertTo-P4EncodedCommand([string]$Script) {
    [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
}

function ConvertTo-P4PowerShellLiteral([string]$Text) {
    if ($null -eq $Text) { return "''" }
    "'{0}'" -f $Text.Replace("'", "''")
}

function Invoke-P4RemotePowerShell([string]$Target, [string]$Script) {
    $encoded = ConvertTo-P4EncodedCommand $Script
    $priorErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & ssh.exe -T -o BatchMode=yes -o ConnectTimeout=15 $Target `
            "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand $encoded" 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $priorErrorAction
    }
    if ($exitCode -ne 0) {
        throw "Remote command failed with exit code ${exitCode}: $($output -join [Environment]::NewLine)"
    }
    @($output)
}

function Start-P4LocalEventAgent {
    param(
        [Parameter(Mandatory)][string]$Binary,
        [Parameter(Mandatory)][string]$Listen,
        [Parameter(Mandatory)][string]$Address,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][string]$Stdout,
        [Parameter(Mandatory)][string]$Stderr
    )
    if ($Address -notmatch '^tcp://') { throw 'Event agent address must include tcp://.' }
    $script = @"
`$ErrorActionPreference = 'Stop'
`$env:P4_STAGED_LLAMA_INHERIT_STDERR = '1'
& $(ConvertTo-P4PowerShellLiteral $Binary) $(ConvertTo-P4PowerShellLiteral $Listen) $(ConvertTo-P4PowerShellLiteral $Address)
exit `$LASTEXITCODE
"@
    $encoded = ConvertTo-P4EncodedCommand $script
    Start-Process -FilePath 'powershell.exe' -ArgumentList @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded
    ) -WorkingDirectory $WorkingDirectory -RedirectStandardOutput $Stdout `
        -RedirectStandardError $Stderr -WindowStyle Hidden -PassThru
}

function Wait-P4LocalEventAgent {
    param(
        [Parameter(Mandatory)][System.Diagnostics.Process]$Launcher,
        [Parameter(Mandatory)][string]$Binary,
        [Parameter(Mandatory)][string]$Address,
        [Parameter(Mandatory)][int]$Port,
        [Parameter(Mandatory)][string]$Stdout,
        [Parameter(Mandatory)][string]$Stderr,
        [int]$TimeoutSeconds = 30
    )
    $expected = "P4_EVENT_AGENT_READY address=$Address"
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Launcher.Refresh()
        $out = if (Test-Path -LiteralPath $Stdout) { Get-Content -LiteralPath $Stdout -Raw } else { '' }
        $connection = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
        if ($out -match [regex]::Escape($expected) -and $connection.Count -eq 1) {
            $owner = Get-Process -Id $connection[0].OwningProcess -ErrorAction SilentlyContinue
            if ($null -ne $owner -and $owner.Path -eq $Binary) { return $owner.Id }
        }
        if ($Launcher.HasExited) {
            $err = if (Test-Path -LiteralPath $Stderr) { Get-Content -LiteralPath $Stderr -Raw } else { '' }
            throw "Event agent exited before READY. stdout=$out stderr=$err"
        }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    $err = if (Test-Path -LiteralPath $Stderr) { Get-Content -LiteralPath $Stderr -Raw } else { '' }
    throw "Timed out waiting for $expected with an owned listener. stderr=$err"
}

function Start-P4RemoteEventAgent {
    param(
        [Parameter(Mandatory)][string]$Target,
        [Parameter(Mandatory)][string]$TaskName,
        [Parameter(Mandatory)][string]$RunDirectory,
        [Parameter(Mandatory)][string]$UserId,
        [Parameter(Mandatory)][int]$Port,
        [Parameter(Mandatory)][string]$Address
    )
    if ($Address -notmatch '^tcp://') { throw 'Remote Event agent address must include tcp://.' }
    $agent = Join-Path $RunDirectory 'p4-agent.exe'
    $stdout = Join-Path $RunDirectory "agent-$Port.log"
    $stderr = Join-Path $RunDirectory "agent-$Port.err.log"
    $launcher = Join-Path $RunDirectory "run-agent-$Port.cmd"
    $lines = @(
        '@echo off',
        'set "P4_STAGED_LLAMA_INHERIT_STDERR=1"',
        ('"{0}" 127.0.0.1:{1} {2} 1>"{3}" 2>"{4}"' -f $agent, $Port, $Address, $stdout, $stderr)
    )
    $encodedLines = ($lines | ForEach-Object { ConvertTo-P4PowerShellLiteral $_ }) -join ', '
    $script = @"
`$ErrorActionPreference = 'Stop'
`$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force -Path $(ConvertTo-P4PowerShellLiteral $RunDirectory) | Out-Null
Set-Content -LiteralPath $(ConvertTo-P4PowerShellLiteral $launcher) -Value @($encodedLines) -Encoding ascii
`$existing = Get-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName) -ErrorAction SilentlyContinue
if (`$null -ne `$existing) {
    Stop-ScheduledTask -InputObject `$existing -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -InputObject `$existing -Confirm:`$false
}
`$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument $(ConvertTo-P4PowerShellLiteral ("/d /c call `"$launcher`""))
`$principal = New-ScheduledTaskPrincipal -UserId $(ConvertTo-P4PowerShellLiteral $UserId) -LogonType Interactive -RunLevel Limited
`$settings = New-ScheduledTaskSettingsSet -Hidden
Register-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName) -Action `$action -Principal `$principal -Settings `$settings | Out-Null
Start-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName)
"@
    Invoke-P4RemotePowerShell $Target $script | Out-Null
}

function Wait-P4RemoteEventAgent {
    param(
        [Parameter(Mandatory)][string]$Target,
        [Parameter(Mandatory)][string]$RunDirectory,
        [Parameter(Mandatory)][int]$Port,
        [Parameter(Mandatory)][string]$Address,
        [int]$TimeoutSeconds = 30
    )
    $agent = Join-Path $RunDirectory 'p4-agent.exe'
    $stdout = Join-Path $RunDirectory "agent-$Port.log"
    $stderr = Join-Path $RunDirectory "agent-$Port.err.log"
    $expected = "P4_EVENT_AGENT_READY address=$Address"
    $script = @"
`$ErrorActionPreference = 'Stop'
`$ProgressPreference = 'SilentlyContinue'
`$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
do {
    `$out = if (Test-Path -LiteralPath $(ConvertTo-P4PowerShellLiteral $stdout)) { Get-Content -LiteralPath $(ConvertTo-P4PowerShellLiteral $stdout) -Raw } else { '' }
    `$connection = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
    if (`$out -match [regex]::Escape($(ConvertTo-P4PowerShellLiteral $expected)) -and `$connection.Count -eq 1) {
        `$owner = Get-Process -Id `$connection[0].OwningProcess -ErrorAction SilentlyContinue
        if (`$null -ne `$owner -and `$owner.Path -eq $(ConvertTo-P4PowerShellLiteral $agent)) {
            Write-Output "REMOTE_EVENT_AGENT_READY port=$Port pid=`$(`$owner.Id) address=$Address"
            exit 0
        }
    }
    Start-Sleep -Milliseconds 100
} while ((Get-Date) -lt `$deadline)
`$err = if (Test-Path -LiteralPath $(ConvertTo-P4PowerShellLiteral $stderr)) { Get-Content -LiteralPath $(ConvertTo-P4PowerShellLiteral $stderr) -Raw } else { '' }
throw "Remote Event agent did not own port $Port. stdout=`$out stderr=`$err"
"@
    Invoke-P4RemotePowerShell $Target $script
}

function Get-P4RemoteKernelPowerRecord([string]$Target) {
    $script = @"
`$ErrorActionPreference = 'Stop'
`$event = Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-Kernel-Power'; Id=41} -MaxEvents 1
Write-Output `$event.RecordId
"@
    $value = (Invoke-P4RemotePowerShell $Target $script | Select-Object -Last 1).Trim()
    $record = 0L
    if (-not [long]::TryParse($value, [ref]$record)) { throw 'Remote Kernel-Power record is not numeric.' }
    $record
}

function Get-P4AvailableHostBytes([uint64]$FreePhysicalKiB, [uint64]$ReserveMiB,
        [string]$HostName) {
    $freeBytes = $FreePhysicalKiB * [uint64]1KB
    $reserveBytes = $ReserveMiB * [uint64]1MB
    if ($freeBytes -le $reserveBytes) {
        throw "$HostName free physical memory does not exceed its required reserve."
    }
    [uint64]($freeBytes - $reserveBytes)
}

function Invoke-P4StageMemoryPlan {
    param(
        [Parameter(Mandatory)][string]$Binary,
        [Parameter(Mandatory)][string]$Plan,
        [Parameter(Mandatory)][string]$VisibleDevice
    )
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Binary
    $start.ArgumentList.Add('--port')
    $start.ArgumentList.Add('59999')
    $start.ArgumentList.Add('--inspect-memory-plan')
    $start.WorkingDirectory = Split-Path -Parent $Binary
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.Environment['CUDA_VISIBLE_DEVICES'] = $VisibleDevice
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) { throw 'Failed to start the llama.cpp memory inspector.' }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $bytes = [Text.Encoding]::UTF8.GetBytes($Plan)
    $prefix = [BitConverter]::GetBytes([uint32]$bytes.Length)
    $process.StandardInput.BaseStream.Write($prefix, 0, $prefix.Length)
    $process.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    $process.StandardInput.Close()
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) {
        throw "llama.cpp memory inspection failed with exit $($process.ExitCode). stdout=$stdout stderr=$stderr"
    }
    $matches = [regex]::Matches($stderr, '(?m)^MEMORY_PLAN (.+?)\r?$')
    if ($matches.Count -ne 1) { throw "llama.cpp emitted $($matches.Count) memory plans. stderr=$stderr" }
    $result = $matches[0].Groups[1].Value | ConvertFrom-Json
    if ($result.complete -ne $true) { throw 'llama.cpp emitted an incomplete memory plan.' }
    $result
}

function Invoke-P4RemoteStageMemoryPlan {
    param(
        [Parameter(Mandatory)][string]$Target,
        [Parameter(Mandatory)][string]$TaskName,
        [Parameter(Mandatory)][string]$RunDirectory,
        [Parameter(Mandatory)][string]$UserId,
        [Parameter(Mandatory)][string]$Binary,
        [Parameter(Mandatory)][string]$Plan,
        [Parameter(Mandatory)][string]$VisibleDevice,
        [int]$TimeoutSeconds = 1800
    )
    if ($TimeoutSeconds -lt 1) { throw 'Remote memory-plan timeout must be positive.' }
    foreach ($value in @($RunDirectory,$Binary,$VisibleDevice)) {
        if ($value.Contains('"') -or $value.Contains("`r") -or $value.Contains("`n")) {
            throw 'Remote memory-plan command values cannot contain quotes or newlines.'
        }
    }
    $leaf = $TaskName -replace '[^A-Za-z0-9._-]', '_'
    $inputFile = Join-Path $RunDirectory "$leaf.in"
    $stdoutFile = Join-Path $RunDirectory "$leaf.out"
    $stderrFile = Join-Path $RunDirectory "$leaf.err"
    $exitFile = Join-Path $RunDirectory "$leaf.exit"
    $launcher = Join-Path $RunDirectory "$leaf.cmd"
    $planBytes = [Text.Encoding]::UTF8.GetBytes($Plan)
    $inputBytes = [byte[]]::new(4 + $planBytes.Length)
    $length = [uint32]$planBytes.Length
    $inputBytes[0] = [byte]($length -band 0xff)
    $inputBytes[1] = [byte](($length -shr 8) -band 0xff)
    $inputBytes[2] = [byte](($length -shr 16) -band 0xff)
    $inputBytes[3] = [byte](($length -shr 24) -band 0xff)
    [Buffer]::BlockCopy($planBytes, 0, $inputBytes, 4, $planBytes.Length)
    $inputBase64 = [Convert]::ToBase64String($inputBytes)
    $lines = @(
        '@echo off',
        'setlocal',
        ('set "CUDA_VISIBLE_DEVICES={0}"' -f $VisibleDevice),
        ('cd /d "{0}"' -f $RunDirectory),
        ('"{0}" --port 59999 --inspect-memory-plan <"{1}" 1>"{2}" 2>"{3}"' -f $Binary,$inputFile,$stdoutFile,$stderrFile),
        ('>"{0}" echo %errorlevel%' -f $exitFile)
    )
    $encodedLines = ($lines | ForEach-Object { ConvertTo-P4PowerShellLiteral $_ }) -join ', '
    $script = @"
`$ErrorActionPreference='Stop'; `$ProgressPreference='SilentlyContinue'
New-Item -ItemType Directory -Force -Path $(ConvertTo-P4PowerShellLiteral $RunDirectory) | Out-Null
[IO.File]::WriteAllBytes($(ConvertTo-P4PowerShellLiteral $inputFile),[Convert]::FromBase64String($(ConvertTo-P4PowerShellLiteral $inputBase64)))
Set-Content -LiteralPath $(ConvertTo-P4PowerShellLiteral $launcher) -Value @($encodedLines) -Encoding ascii
`$old=Get-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName) -ErrorAction SilentlyContinue
if(`$null -ne `$old){Stop-ScheduledTask -InputObject `$old -ErrorAction SilentlyContinue;Unregister-ScheduledTask -InputObject `$old -Confirm:`$false}
`$action=New-ScheduledTaskAction -Execute 'cmd.exe' -Argument $(ConvertTo-P4PowerShellLiteral ("/d /c call `"$launcher`""))
`$principal=New-ScheduledTaskPrincipal -UserId $(ConvertTo-P4PowerShellLiteral $UserId) -LogonType Interactive -RunLevel Limited
`$settings=New-ScheduledTaskSettingsSet -Hidden -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSeconds)
Register-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName) -Action `$action -Principal `$principal -Settings `$settings | Out-Null
Start-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName)
`$deadline=(Get-Date).AddSeconds($TimeoutSeconds)
`$dispatchDeadline=(Get-Date).AddSeconds(15); `$seenRunning=`$false
while(-not(Test-Path -LiteralPath $(ConvertTo-P4PowerShellLiteral $exitFile))){
    `$task=Get-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName)
    if(`$task.State -eq 'Running'){`$seenRunning=`$true}
    if(`$seenRunning -and `$task.State -eq 'Ready'){throw 'Remote memory inspector ended without an exit record.'}
    if(-not `$seenRunning -and (Get-Date) -ge `$dispatchDeadline){throw 'Remote memory inspector was not dispatched.'}
    if((Get-Date) -ge `$deadline){Stop-ScheduledTask -InputObject `$task -ErrorAction SilentlyContinue;throw 'Remote memory inspector timed out.'}
    Start-Sleep -Milliseconds 200
}
`$exitCode=0
if(-not[int]::TryParse((Get-Content -LiteralPath $(ConvertTo-P4PowerShellLiteral $exitFile) -Raw).Trim(),[ref]`$exitCode)){throw 'Remote memory inspector exit record is invalid.'}
[ordered]@{exit_code=`$exitCode;stdout=[Convert]::ToBase64String([IO.File]::ReadAllBytes($(ConvertTo-P4PowerShellLiteral $stdoutFile)));stderr=[Convert]::ToBase64String([IO.File]::ReadAllBytes($(ConvertTo-P4PowerShellLiteral $stderrFile)))}|ConvertTo-Json -Compress
"@
    try {
        $remoteOutput = @(Invoke-P4RemotePowerShell $Target $script)
        $json = $remoteOutput | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1
        if ([string]::IsNullOrWhiteSpace($json)) { throw 'Remote memory inspector returned no result.' }
        $record = $json | ConvertFrom-Json
        $stdout = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($record.stdout))
        $stderr = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($record.stderr))
        if ($record.exit_code -ne 0) {
            throw "Remote llama.cpp memory inspection failed with exit $($record.exit_code). stdout=$stdout stderr=$stderr"
        }
        $matches = [regex]::Matches($stderr, '(?m)^MEMORY_PLAN (.+?)\r?$')
        if ($matches.Count -ne 1) { throw "Remote llama.cpp emitted $($matches.Count) memory plans. stderr=$stderr" }
        $result = $matches[0].Groups[1].Value | ConvertFrom-Json
        if ($result.complete -ne $true -or $result.fits_current_free -ne $true) {
            throw 'Remote llama.cpp memory plan is incomplete or exceeds current free memory.'
        }
        $result
    } finally {
        $files = @($inputFile,$stdoutFile,$stderrFile,$exitFile,$launcher)
        $cleanup = @"
`$task=Get-ScheduledTask -TaskName $(ConvertTo-P4PowerShellLiteral $TaskName) -ErrorAction SilentlyContinue
if(`$null -ne `$task){Stop-ScheduledTask -InputObject `$task -ErrorAction SilentlyContinue;Unregister-ScheduledTask -InputObject `$task -Confirm:`$false}
Remove-Item -LiteralPath @($(($files | ForEach-Object { ConvertTo-P4PowerShellLiteral $_ }) -join ',')) -Force -ErrorAction SilentlyContinue
"@
        try { Invoke-P4RemotePowerShell $Target $cleanup | Out-Null } catch {}
    }
}

function Stop-P4ProcessTree([System.Diagnostics.Process]$Process) {
    if ($null -ne $Process) {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            & taskkill.exe /PID $Process.Id /T /F *> $null
        }
    }
}

Export-ModuleMember -Function ConvertTo-P4EncodedCommand,ConvertTo-P4PowerShellLiteral,`
    Invoke-P4RemotePowerShell,Start-P4LocalEventAgent,Wait-P4LocalEventAgent,`
    Start-P4RemoteEventAgent,Wait-P4RemoteEventAgent,Get-P4RemoteKernelPowerRecord,`
    Get-P4AvailableHostBytes,Invoke-P4StageMemoryPlan,Invoke-P4RemoteStageMemoryPlan,`
    Stop-P4ProcessTree
