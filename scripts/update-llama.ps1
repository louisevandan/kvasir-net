param(
    [switch]$ApplyCompatible,
    [switch]$Yes
)

$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    $args = @("-m", "controller.llama_updater")
    if ($ApplyCompatible) { $args += "--apply-compatible" }
    if ($Yes) { $args += "--yes" }
    $commands = @()
    if ($env:LINKCPP_PYTHON) { $commands += ,@($env:LINKCPP_PYTHON) }
    $commands += ,@("python3")
    $commands += ,@("py", "-3")
    $commands += ,@("python")
    $selected = $null
    foreach ($candidate in $commands) {
        if (-not (Get-Command $candidate[0] -ErrorAction SilentlyContinue)) { continue }
        $prefix = @()
        if ($candidate.Count -gt 1) { $prefix = @($candidate[1..($candidate.Count - 1)]) }
        & $candidate[0] @prefix -c "import sys; print(sys.executable)" *> $null
        if ($LASTEXITCODE -eq 0) { $selected = $candidate; break }
    }
    if (-not $selected) { throw "No working Python 3 interpreter found. Set LINKCPP_PYTHON." }
    $prefix = @()
    if ($selected.Count -gt 1) { $prefix = @($selected[1..($selected.Count - 1)]) }
    & $selected[0] @prefix @args
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
