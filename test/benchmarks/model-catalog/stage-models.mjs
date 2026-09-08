// Copies model shards from the network share to the probe host's local SSD.
//
//   node test/benchmarks/model-catalog/stage-models.mjs copy   --models <id,id> [--dest D:\models]
//   node test/benchmarks/model-catalog/stage-models.mjs status
//   node test/benchmarks/model-catalog/stage-models.mjs list
//   node test/benchmarks/model-catalog/stage-models.mjs stop
//
// Reading a 200 GiB model off the SMB share takes tens of minutes, and a
// context sweep reads the same model several times. Copying once to local SSD
// turns every later load into a local read. The copy has to run in the
// logged-on user's session for the same reason the probe does: an SSH session
// does not see the mapped drive.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const command = process.argv[2];
const host = argument('--host', '42mob@192.168.0.29');
const dest = argument('--dest', 'D:\\models');
const inventoryFile = argument('--inventory', path.join('target', 'model-catalog', 'inventory.json'));
const root = argument('--root', 'C:\\Users\\42mob\\p4-remote');
const remoteDir = `${root}\\probe`;
const taskName = `p4-model-stage-copy-${argument('--run-name', 'main')}`;
const user = argument('--user', 'm42-server2\\42mob');
const runTag = argument('--run', new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14));
// Task, launcher and log all carry the run's own name. With one fixed set of
// names a second staging run deletes the first run's scheduled task and
// overwrites the script its cmd is still reading, killing it mid-copy, and a
// leftover cmd keeps the old log open so the next task dies on its redirection.
const launcher = `${remoteDir}\\stage-copy-${runTag}.cmd`;
const logRemote = `${remoteDir}\\stage-copy-${runTag}.log`;
const logGlob = `${remoteDir}\\stage-copy-*.log`;

function ssh(script, timeoutMs = 180_000) {
  const encoded = Buffer.from(`$ProgressPreference = 'SilentlyContinue';\n${script}`, 'utf16le').toString('base64');
  const result = spawnSync('ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', host,
    `powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: timeoutMs });
  return { status: result.status, out: `${result.stdout ?? ''}${result.stderr ?? ''}`.trim() };
}

if (command === 'copy') {
  const ids = (argument('--models', '') || '').split(',').filter(Boolean);
  if (!ids.length) throw new Error('copy needs --models <id,id>');
  const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
  const lines = ['@echo off', `echo START %DATE% %TIME%`];
  let bytes = 0;
  for (const id of ids) {
    const model = inventory.models.find((m) => m.id === id);
    if (!model) throw new Error(`unknown model ${id}`);
    bytes += model.file_bytes ?? 0;
    // One directory per repository under the destination, so a model keeps its
    // shard set together and the plan only has to swap the path prefix.
    const target = `${dest}\\${model.repository}`;
    const names = model.files.map((file) => `"${path.basename(file)}"`).join(' ');
    lines.push(`echo COPY ${model.id}`);
    // /J is unbuffered: measured 112 MB/s against 80 MB/s buffered on this
    // link. Its cost is that a destination file is set to full length before
    // the data arrives, so a run killed mid-file leaves a full-size file with
    // partial contents that the next run would skip on size. `stop` therefore
    // deletes the directory of whichever model was in flight.
    lines.push(`echo INFLIGHT ${model.repository}`);
    lines.push(`robocopy "${model.directory}" "${target}" ${names} /J /NP /NJH /NJS /R:2 /W:5`);
    lines.push('if %ERRORLEVEL% GEQ 8 echo COPY_FAILED %ERRORLEVEL% ' + model.id);
  }
  lines.push('echo DONE %DATE% %TIME%', '');
  const localCmd = path.join(os.tmpdir(), 'p4-stage-copy.cmd');
  fs.writeFileSync(localCmd, lines.join('\r\n'), 'ascii');
  const copy = spawnSync('scp', ['-o', 'BatchMode=yes', '-q', localCmd, `${host}:${launcher.replace(/\\/g, '/')}`],
    { encoding: 'utf8' });
  if (copy.status !== 0) throw new Error(`launcher copy failed: ${copy.stderr ?? ''}`);
  // Deleting a scheduled task does not kill the cmd it already started, so a
  // second run would leave two robocopy chains writing the same destination
  // files. Refuse instead, unless the caller says to take over.
  if (!process.argv.includes('--force')) {
    const busy = ssh("@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'cmd.exe' -and $_.CommandLine -like '*stage-copy-*.cmd*' -and $_.CommandLine -notlike '*powershell*' }).Count").out.trim();
    if (busy !== '0') {
      process.stderr.write(`a staging copy is already running (${busy} process(es)); stop it first or pass --force
`);
      process.exit(1);
    }
  }
  const start = [
    `$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c ${launcher} > ${logRemote} 2>&1' -WorkingDirectory '${remoteDir}';`,
    `$principal = New-ScheduledTaskPrincipal -UserId '${user}' -LogonType Interactive -RunLevel Limited;`,
    '$settings = New-ScheduledTaskSettingsSet -Hidden -ExecutionTimeLimit ([TimeSpan]::FromHours(24));',
    `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
    `Register-ScheduledTask -TaskName ${taskName} -Action $action -Principal $principal -Settings $settings | Out-Null;`,
    `Start-ScheduledTask -TaskName ${taskName};`,
    'Start-Sleep -Seconds 3;',
    `Write-Output ('STARTED ' + (Get-ScheduledTask -TaskName ${taskName}).State)`,
  ].join(' ');
  const { out } = ssh(start);
  process.stdout.write(`${JSON.stringify({ models: ids.length, gib: Number((bytes / 2 ** 30).toFixed(1)) })}\n${out}\n`);
} else if (command === 'status') {
  const script = [
    `$state = (Get-ScheduledTask -TaskName ${taskName} -ErrorAction SilentlyContinue).State;`,
    "Write-Output ('task=' + $state);",
    `$log = Get-ChildItem -Path '${logGlob}' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1;`,
    "if ($log) { Write-Output ('log=' + $log.Name + ' age_s=' + [math]::Round(((Get-Date) - $log.LastWriteTime).TotalSeconds)); Get-Content -LiteralPath $log.FullName -Tail 6 };",
    `$size = (Get-ChildItem -LiteralPath '${dest}' -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Sum Length);`,
    "Write-Output ('staged_files=' + $size.Count + ' staged_gib=' + [math]::Round($size.Sum/1GB,1));",
    "$d = Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='D:'\"; Write-Output ('dest_free_gib=' + [math]::Round($d.FreeSpace/1GB))",
  ].join(' ');
  process.stdout.write(`${ssh(script).out}\n`);
} else if (command === 'remove') {
  // Reclaim the host's SSD as soon as a model's loads are recorded. The copy
  // walks its list once and never returns to a model it has passed, so
  // deleting behind it is safe while it is still running.
  const ids = (argument('--models', '') || '').split(',').filter(Boolean);
  if (!ids.length) throw new Error('remove needs --models <id,id>');
  const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
  const dirs = ids.map((id) => {
    const model = inventory.models.find((m) => m.id === id);
    if (!model) throw new Error('unknown model ' + id);
    return path.join(dest, model.repository);
  });
  // One SSH command line has a length limit, so remove in small batches.
  const script = dirs.slice(0, 4).map((dir) => "$d = '" + dir + "'; if (Test-Path -LiteralPath $d) { $n = (Get-ChildItem -LiteralPath $d -Recurse -File | Measure-Object -Sum Length); Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue; Write-Output ('REMOVED ' + $d + ' gib=' + [math]::Round($n.Sum/1GB,1)) };").join(' ')
    + " $f = (Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='D:'\").FreeSpace; Write-Output ('dest_free_gib=' + [math]::Round($f/1GB))";
  process.stdout.write(`${ssh(script).out}\n`);
} else if (command === 'list') {
  const script = `Get-ChildItem -LiteralPath '${dest}' -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName + ' ' + $_.Length }`;
  process.stdout.write(`${ssh(script).out}\n`);
} else if (command === 'stop') {
  const script = [
    `schtasks.exe /end /tn ${taskName} *> $null;`,
    `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
    // Whatever model was mid-copy has a full-size but partly written file, and
    // a later run would skip it on size. Remove that model's directory so the
    // next run copies it again from the start.
    `$log = Get-ChildItem -Path '${logGlob}' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1;`,
    "$inflight = if ($log) { (Select-String -LiteralPath $log.FullName -Pattern 'INFLIGHT (.+)' | Select-Object -Last 1).Matches.Groups[1].Value.Trim() } else { $null };",
    `if ($inflight) { $dir = Join-Path '${dest}' $inflight; if (Test-Path $dir) { Remove-Item $dir -Recurse -Force; Write-Output ('DISCARDED ' + $dir) } };`,
    'Get-Process robocopy -ErrorAction SilentlyContinue | Stop-Process -Force;',
    "Get-CimInstance Win32_Process -Filter \"Name='cmd.exe'\" | Where-Object { $_.CommandLine -like '*stage-copy-*.cmd*' -and $_.CommandLine -notlike '*powershell*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue };",
    "Write-Output 'STOPPED'",
  ].join(' ');
  process.stdout.write(`${ssh(script).out}\n`);
} else {
  process.stderr.write('usage: stage-models.mjs copy|status|list|stop [options]\n');
  process.exitCode = 2;
}
