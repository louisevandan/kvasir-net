[CmdletBinding()]
param([string]$Matrix, [string]$Csv, [int]$Tokens = 200)
$ErrorActionPreference = 'Continue'
# Derived from this script's own location, so a clone anywhere runs it.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$runner = Join-Path $root 'apps\p4\tools\scripts\e2e\run-local-real-two-stage.ps1'
$prompt = Join-Path $root '.cache\rust-korean-qwen.txt'
$agent  = Join-Path $root 'apps\p4\target\release\p4-agent.exe'
$drive  = Join-Path $root 'apps\p4\target\release\p4-drive.exe'
$server = Join-Path $root '.cache\staged-server-cuda-final\Release\p4_staged_server.exe'

# Column meanings, because the wrong one was read once already:
#   delivered_tokens        tokens the caller actually received. A run can pass
#                           every verdict with zero of these.
#   logical_gen_tps_wall    service throughput: one count per sequence/phase/hop
#                           over wall time. THIS is the benchmark number.
#   stage_compute_gen_tps   every stage's tokens pooled over every stage's
#                           elapsed: one ratio over the pooled totals, not a sum
#                           of per-stage rates. A two-stage chain counts each
#                           logical token in both stages, so this is compute
#                           efficiency and never throughput. Flat here while the
#                           wall figure climbs is the signature of a refused
#                           batch falling back to per-sequence.
#   stage_sum_gen_tps_wall  the same stage sum over wall time; kept only so an
#                           older number can be reconciled, never quoted alone.
if (-not (Test-Path $Csv)) {
  'model,blocks,boundary,parallel,requests,tokens,verdicts,completed,failed,unanswered,delivered_tokens,logical_gen_tps_wall,logical_prefill_tps_wall,stage_compute_gen_tps,stage_sum_gen_tps_wall,avg_session_gen_tps,elapsed_us,run_dir' |
    Set-Content -LiteralPath $Csv -Encoding utf8
}

foreach ($line in (Get-Content -LiteralPath $Matrix)) {
  if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith('#')) { continue }
  $f = $line -split '\|'
  $name = $f[0]; $model = $f[1]; $blocks = [int]$f[2]; $boundary = [int]$f[3]
  $par = [int]$f[4]; $req = [int]$f[5]
  $runId = "tps-$name-p$par-$(Get-Date -Format 'HHmmss')"
  Write-Host "RUN $name blocks=$blocks boundary=$boundary parallel=$par requests=$req"
  # Killing by image name reaches every P4 process on the machine, including
  # a deployment somebody else is holding open. Only the ports this sweep is
  # about to bind are ours to clear.
  foreach ($busyPort in @(52700, 52701, 52710)) {
    foreach ($owner in @(Get-NetTCPConnection -State Listen -LocalPort $busyPort -ErrorAction SilentlyContinue)) {
      $proc = Get-Process -Id $owner.OwningProcess -ErrorAction SilentlyContinue
      if ($null -ne $proc -and $proc.ProcessName -in @('p4-agent', 'p4-drive', 'p4_staged_server')) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
  Start-Sleep -Seconds 2
  $out = Join-Path $root "target\real-two-stage-5000\$runId"
  # The runner owns the evidence path and restores the caller's environment,
  # so it is passed as an argument rather than exported from here.
  $log = Join-Path $root "target\tps-$runId.log"
  try {
    & $runner -RunId $runId -Model $model -PromptFile $prompt -PromptTokens 64 -Tokens $Tokens `
      -Parallel $par -Requests $req -UBatchSize 512 -QuietMilliseconds 180000 `
      -LayerCount $blocks -LayerBoundary $boundary -MaxSecondaryVramMiB 11000 `
      -LoadTimeoutSeconds 900 -StageReadyTimeoutSeconds 1800 `
      -AgentBinary $agent -DriveBinary $drive -ServerBinary $server `
      -EvidenceFile (Join-Path $out 'evidence.md') *>&1 |
      Tee-Object -FilePath $log | Out-Null
  } catch { "SCRIPT_THREW: $_" | Add-Content -LiteralPath $log }

  $pass = 0; $fail = 0
  $c = ''; $fa = ''; $u = ''; $delivered = ''
  $lgen = ''; $lpre = ''; $comp = ''; $sum = ''; $avg = ''; $el = ''
  $drivelog = Join-Path $out 'drive.log'
  if (Test-Path $drivelog) {
    $txt = Get-Content -LiteralPath $drivelog -Raw
    $pass = ([regex]::Matches($txt, '\[pass\]')).Count
    $fail = ([regex]::Matches($txt, '\[FAIL\]')).Count
    $m = [regex]::Match($txt, 'completed=(\d+) failed=(\d+) unanswered=(\d+)')
    if ($m.Success) { $c = $m.Groups[1].Value; $fa = $m.Groups[2].Value; $u = $m.Groups[3].Value }
    $m = [regex]::Match($txt, 'tokens=(\d+) elapsed_ms=')
    if ($m.Success) { $delivered = $m.Groups[1].Value }
    $tl = [regex]::Matches($txt, '(?m)^P4_DRIVE_TELEMETRY_JSON (.*)$')
    if ($tl.Count -gt 0) {
      try {
        $t = $tl[$tl.Count - 1].Groups[1].Value | ConvertFrom-Json
        $a = $t.aggregate
        $lgen = $a.logical_generation_tps_over_run
        $lpre = $a.logical_prefill_tps_over_run
        $comp = $a.generation.tps
        $sum  = $a.generation_tps_over_run
        $avg  = $a.average_session_generation_tps
        $el   = $t.run_elapsed_us
      } catch {}
    }
  }
  "$name,$blocks,$boundary,$par,$req,$Tokens,$pass/$($pass+$fail),$c,$fa,$u,$delivered,$lgen,$lpre,$comp,$sum,$avg,$el,$runId" |
    Add-Content -LiteralPath $Csv -Encoding utf8
  Write-Host "  -> verdicts=$pass/$($pass+$fail) completed=$c failed=$fa unanswered=$u delivered=$delivered logical_wall_tps=$lgen"
}
