// Runs the stage-load probe on the RTX 3090 x2 host and brings the results back.
//
//   node test/benchmarks/model-catalog/remote-probe.mjs run   --jobs <file> [--tag <name>]
//   node test/benchmarks/model-catalog/remote-probe.mjs status [--tag <name>]
//   node test/benchmarks/model-catalog/remote-probe.mjs fetch  --out <file> [--tag <name>]
//   node test/benchmarks/model-catalog/remote-probe.mjs stop   [--tag <name>]
//
// The model share is a mapped drive that only a session owned by the logged-on
// user can see, so an SSH command cannot open a model. The probe therefore runs
// as an interactive scheduled task, the same mechanism remote-agent.mjs uses for
// the harness agent, and SSH is used only to start it and read its files.

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
const tag = argument('--tag', 'catalog');
const root = argument('--root', 'C:\\Users\\42mob\\p4-remote');
const remoteDir = `${root}\\probe`;
const taskName = `p4-model-probe-${tag}`;
const user = argument('--user', 'm42-server2\\42mob');
const jobsRemote = `${remoteDir}\\${tag}-jobs.json`;
const outRemote = `${remoteDir}\\${tag}-results.jsonl`;
const logRemote = `${remoteDir}\\${tag}-probe.log`;
const launcher = `${remoteDir}\\${tag}-run.cmd`;

function ssh(script, timeoutMs = 120_000) {
  const encoded = Buffer.from(`$ProgressPreference = 'SilentlyContinue';\n${script}`, 'utf16le').toString('base64');
  const result = spawnSync('ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', host,
    `powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: timeoutMs });
  return { status: result.status, out: `${result.stdout ?? ''}${result.stderr ?? ''}`.trim() };
}

function copy(localFile, remoteFile) {
  const result = spawnSync('scp', ['-o', 'BatchMode=yes', '-q', localFile, `${host}:${remoteFile.replace(/\\/g, '/')}`],
    { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`copy failed: ${result.stderr ?? ''}`);
}

const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\//, ''));

if (command === 'run') {
  const jobsFile = argument('--jobs', null);
  if (!jobsFile) throw new Error('run needs --jobs <file>');
  ssh(`New-Item -ItemType Directory -Force -Path '${remoteDir}' | Out-Null; Write-Output ready`);
  copy(jobsFile, jobsRemote);
  copy(path.join(here, 'probe.mjs'), `${remoteDir}\\probe.mjs`);
  const cmd = [
    '@echo off',
    `cd /d ${remoteDir}`,
    `node "${remoteDir}\\probe.mjs" "${jobsRemote}" "${outRemote}" "${remoteDir}\\logs" > "${logRemote}" 2>&1`,
    '',
  ].join('\r\n');
  const localCmd = path.join(os.tmpdir(), `${taskName}.cmd`);
  fs.writeFileSync(localCmd, cmd, 'ascii');
  copy(localCmd, launcher);
  const start = [
    `$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c ${launcher}' -WorkingDirectory '${remoteDir}';`,
    `$principal = New-ScheduledTaskPrincipal -UserId '${user}' -LogonType Interactive -RunLevel Limited;`,
    '$settings = New-ScheduledTaskSettingsSet -Hidden -ExecutionTimeLimit ([TimeSpan]::FromHours(24));',
    `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
    `Register-ScheduledTask -TaskName ${taskName} -Action $action -Principal $principal -Settings $settings | Out-Null;`,
    `Start-ScheduledTask -TaskName ${taskName};`,
    'Start-Sleep -Seconds 3;',
    `Write-Output ('STARTED ' + (Get-ScheduledTask -TaskName ${taskName}).State)`,
  ].join(' ');
  const { status, out } = ssh(start);
  process.stdout.write(`${out}\n`);
  process.exitCode = status === 0 ? 0 : 1;
} else if (command === 'status') {
  const script = [
    `$state = (Get-ScheduledTask -TaskName ${taskName} -ErrorAction SilentlyContinue).State;`,
    "Write-Output ('task=' + $state);",
    `if (Test-Path '${outRemote}') { Write-Output ('records=' + (Get-Content -LiteralPath '${outRemote}' | Measure-Object -Line).Lines) } else { Write-Output 'records=0' };`,
    `if (Test-Path '${logRemote}') { Write-Output '--- log tail ---'; Get-Content -LiteralPath '${logRemote}' -Tail 12 };`,
    "$m = Get-CimInstance Win32_OperatingSystem;",
    "Write-Output ('host_free_gib=' + [math]::Round($m.FreePhysicalMemory/1MB,1));",
    "$g = & nvidia-smi --query-gpu=index,memory.used --format=csv,noheader; Write-Output ('gpu=' + ($g -join ' | '))",
  ].join(' ');
  const { out } = ssh(script);
  process.stdout.write(`${out}\n`);
} else if (command === 'fetch') {
  const out = argument('--out', null);
  if (!out) throw new Error('fetch needs --out <file>');
  const result = spawnSync('scp', ['-o', 'BatchMode=yes', '-q', `${host}:${outRemote.replace(/\\/g, '/')}`, out],
    { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`fetch failed: ${result.stderr ?? ''}`);
  const lines = fs.readFileSync(out, 'utf8').trim().split(/\r?\n/).filter(Boolean);
  process.stdout.write(`${JSON.stringify({ out, records: lines.length })}\n`);
} else if (command === 'stop') {
  const script = [
    `schtasks.exe /end /tn ${taskName} *> $null;`,
    `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
    "$stopped = @(Get-Process node, p4_staged_server -ErrorAction SilentlyContinue);",
    '$stopped | Stop-Process -Force -ErrorAction SilentlyContinue;',
    "Write-Output ('STOPPED processes=' + $stopped.Count)",
  ].join(' ');
  const { out } = ssh(script);
  process.stdout.write(`${out}\n`);
} else {
  process.stderr.write('usage: remote-probe.mjs run|status|fetch|stop [options]\n');
  process.exitCode = 2;
}
