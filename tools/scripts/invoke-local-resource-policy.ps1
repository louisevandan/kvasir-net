[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('Build', 'Inference')]
    [string]$Mode,

    [Parameter(Mandatory, Position = 1, ValueFromRemainingArguments)]
    [string[]]$Command
)

$ErrorActionPreference = 'Stop'
$desktop3090 = 'GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed'

if ($Command.Count -eq 0 -or [string]::IsNullOrWhiteSpace($Command[0])) {
    throw 'A command is required.'
}

function Invoke-RequestedCommand {
    if ($Command.Count -eq 1) {
        & $Command[0]
    }
    else {
        & $Command[0] $Command[1..($Command.Count - 1)]
    }
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

if ($Mode -eq 'Build') {
    $logicalCpuCount = [Environment]::ProcessorCount
    $buildCpuCount = [Math]::Max(1, [Math]::Floor($logicalCpuCount * 0.70))
    if ($buildCpuCount -gt 63) {
        throw "The local build affinity guard supports at most 63 logical CPUs; detected $logicalCpuCount."
    }
    $affinityMask = if ($buildCpuCount -eq 63) {
        [Int64]::MaxValue
    }
    else {
        [Int64]([Math]::Pow(2, $buildCpuCount) - 1)
    }
    $current = [Diagnostics.Process]::GetCurrentProcess()
    $originalAffinity = $current.ProcessorAffinity
    $env:CARGO_BUILD_JOBS = [string]$buildCpuCount
    $env:CMAKE_BUILD_PARALLEL_LEVEL = [string]$buildCpuCount
    $env:RUST_TEST_THREADS = [string]$buildCpuCount
    try {
        # Child compiler/linker processes inherit this 33-of-48 affinity mask.
        $current.ProcessorAffinity = [IntPtr]$affinityMask
        Write-Host "P4_LOCAL_BUILD_POLICY logical=$logicalCpuCount allowed=$buildCpuCount percent=$([Math]::Round(100 * $buildCpuCount / $logicalCpuCount, 2))"
        Invoke-RequestedCommand
    }
    finally {
        $current.ProcessorAffinity = $originalAffinity
    }
    exit 0
}

$inventory = & nvidia-smi --query-gpu=uuid,name --format=csv,noheader 2>&1
if ($LASTEXITCODE -ne 0 -or -not ($inventory | Select-String -SimpleMatch $desktop3090)) {
    throw "The designated RTX 3090 is unavailable: $desktop3090"
}
$env:CUDA_DEVICE_ORDER = 'PCI_BUS_ID'
$env:CUDA_VISIBLE_DEVICES = $desktop3090
Write-Host "P4_LOCAL_INFERENCE_POLICY cuda_visible_devices=$desktop3090"
Invoke-RequestedCommand
