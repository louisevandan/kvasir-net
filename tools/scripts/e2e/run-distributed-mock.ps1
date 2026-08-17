[CmdletBinding()]
param(
    [string]$RunId = (Get-Date -Format 'yyyyMMddHHmmss'),
    [int]$Workers = 4,
    [int]$Requests = 128,
    [int]$Tokens = 16
)

$ErrorActionPreference = 'Stop'
if ($Workers -lt 1 -or $Workers -gt 8) { throw 'Workers must be between 1 and 8' }
if ($Requests -lt 1 -or $Tokens -lt 1) { throw 'Requests and Tokens must be positive' }

$p4Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$agentBinary = Join-Path $p4Root 'target\release\p4-agent.exe'
$driveBinary = Join-Path $p4Root 'target\release\p4-drive.exe'
$outputRoot = Join-Path $p4Root "target\parallel-mock-e2e\$RunId"

foreach ($binary in @($agentBinary, $driveBinary)) {
    if (-not (Test-Path -LiteralPath $binary)) {
        throw "release binary not found: $binary"
    }
}
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

$startedAgents = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$startedDrivers = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$workerAgents = @{}
$workingSetPeaks = @{}
$workingSetMins = @{}
$workingSetSamples = @{}

try {
    $jobs = @()
    for ($worker = 0; $worker -lt $Workers; $worker++) {
        $basePort = 52100 + ($worker * 10)
        $stage0 = "127.0.0.1:$basePort"
        $stage1 = "127.0.0.1:$($basePort + 1)"
        $driver = "127.0.0.1:$($basePort + 2)"
        $workerDir = Join-Path $outputRoot "worker-$worker"
        New-Item -ItemType Directory -Force -Path $workerDir | Out-Null
        $workerAgents[$worker] = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
        $workingSetPeaks[$worker] = [uint64]0
        $workingSetMins[$worker] = [uint64]::MaxValue
        $workingSetSamples[$worker] = 0

        foreach ($stage in @($stage0, $stage1)) {
            $port = $stage.Split(':')[1]
            $agent = Start-Process -FilePath $agentBinary `
                -ArgumentList $stage, $stage `
                -WorkingDirectory $p4Root `
                -RedirectStandardOutput (Join-Path $workerDir "agent-$port.log") `
                -RedirectStandardError (Join-Path $workerDir "agent-$port.err.log") `
                -WindowStyle Hidden -PassThru
            $startedAgents.Add($agent)
            $workerAgents[$worker].Add($agent)
        }

        $driverCommand = @"
`$env:P4_DRIVE_CEILING='16'
`$env:P4_DRIVE_ARRIVE_MS='2'
`$env:P4_DRIVE_VARY='1'
& '$driveBinary' '$driver' '$stage0,$stage1' $Requests $Tokens mock '$driver'
exit `$LASTEXITCODE
"@
        $driverProcess = Start-Process -FilePath 'powershell.exe' `
            -ArgumentList '-NoProfile', '-Command', $driverCommand `
            -WorkingDirectory $p4Root `
            -RedirectStandardOutput (Join-Path $workerDir 'drive.log') `
            -RedirectStandardError (Join-Path $workerDir 'drive.err.log') `
            -WindowStyle Hidden -PassThru
        $startedDrivers.Add($driverProcess)
        $jobs += [pscustomobject]@{ Worker = $worker; Driver = $driver; Stages = @($stage0, $stage1) }
    }

    while (@($startedDrivers | Where-Object { -not $_.HasExited }).Count -gt 0) {
        foreach ($worker in 0..($Workers - 1)) {
            $workingSet = [uint64]0
            foreach ($agent in $workerAgents[$worker]) {
                if ($agent.HasExited) { continue }
                try {
                    $agent.Refresh()
                    $workingSet += [uint64]$agent.WorkingSet64
                } catch [System.InvalidOperationException] {
                    # The process can exit between HasExited and Refresh.
                }
            }
            if ($workingSet -gt $workingSetPeaks[$worker]) { $workingSetPeaks[$worker] = $workingSet }
            if ($workingSet -gt 0 -and $workingSet -lt $workingSetMins[$worker]) { $workingSetMins[$worker] = $workingSet }
            $workingSetSamples[$worker]++
        }
        Start-Sleep -Milliseconds 1000
    }
    $results = foreach ($job in $jobs) {
        $log = Join-Path $outputRoot "worker-$($job.Worker)\drive.log"
        $text = if (Test-Path -LiteralPath $log) { Get-Content -Raw -LiteralPath $log } else { '' }
        [pscustomobject]@{
            worker = $job.Worker
            driver = $job.Driver
            stages = $job.Stages
            passed = $text -match '\[pass\] every request answered' -and
                $text -match '\[pass\] every stream in order'
            log = $log
            peak_working_set_bytes = $workingSetPeaks[$job.Worker]
            min_working_set_bytes = $workingSetMins[$job.Worker]
            working_set_delta_bytes = $workingSetPeaks[$job.Worker] - $workingSetMins[$job.Worker]
            working_set_samples = $workingSetSamples[$job.Worker]
        }
    }
    $manifest = [pscustomobject]@{
        run_id = $RunId
        workers = $Workers
        requests = $Requests
        tokens = $Tokens
        binary = $agentBinary
        results = @($results)
    }
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputRoot 'manifest.json')
    if (@($results | Where-Object { -not $_.passed }).Count -gt 0) { exit 1 }
}
finally {
    foreach ($process in $startedDrivers + $startedAgents) {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
}
