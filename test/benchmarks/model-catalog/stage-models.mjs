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
const taskName = 'p4-model-stage-copy';
const user = argument('--user', 'm42-server2\\42mob');
const launcher = `${remoteDir}\\stage-copy.cmd`;
// A run gets its own log. A previous run's cmd can outlive its scheduled task
// and keep the old file open, and then the next task dies instantly because its
// output redirection cannot be created.
const runTag = argument('--run', new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14));
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
    lines.push(`robocopy "${model.directory}" "${target}" ${names} /J /NP /NJH /NJS /R:2 /W:5`);
    lines.push('if %ERRORLEVEL% GEQ 8 echo COPY_FAILED %ERRORLEVEL% ' + model.id);
  }
  lines.push('echo DONE %DATE% %TIME%', '');
  const localCmd = path.join(os.tmpdir(), 'p4-stage-copy.cmd');
  fs.writeFileSync(localCmd, lines.join('\r\n'), 'ascii');
  const copy = spawnSync('scp', ['-o', 'BatchMode=yes', '-q', localCmd, `${host}:${launcher.replace(/\\/g, '/')}`],
    { encoding: 'utf8' });
  if (copy.status !== 0) throw new Error(`launcher copy failed: ${copy.stderr ?? ''}`);
  const start = [
    // A leftover robocopy from an earlier run would both share the link and
    // hold the previous log open, so clear it before registering the task.
    'Get-Process robocopy -ErrorAction SilentlyContinue | Stop-Process -Force;',
    "Get-CimInstance Win32_Process -Filter \"Name='cmd.exe'\" | Where-Object { $_.CommandLine -like '*stage-copy*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue };",
    'Start-Sleep -Seconds 2;',
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
} else if (command === 'list') {
  const script = `Get-ChildItem -LiteralPath '${dest}' -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName + ' ' + $_.Length }`;
  process.stdout.write(`${ssh(script).out}\n`);
} else if (command === 'stop') {
  const script = [
    `schtasks.exe /end /tn ${taskName} *> $null;`,
    `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
    'Get-Process robocopy -ErrorAction SilentlyContinue | Stop-Process -Force;',
    "Get-CimInstance Win32_Process -Filter \"Name='cmd.exe'\" | Where-Object { $_.CommandLine -like '*stage-copy*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue };",
    "Write-Output 'STOPPED'",
  ].join(' ');
  process.stdout.write(`${ssh(script).out}\n`);
} else {
  process.stderr.write('usage: stage-models.mjs copy|status|list|stop [options]\n');
  process.exitCode = 2;
}
