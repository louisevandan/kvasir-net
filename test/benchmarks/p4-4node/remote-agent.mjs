#!/usr/bin/env node
// Starts and stops the four-node harness's agent on a remote Windows host.
//
//   node remote-agent.mjs start|stop|status [--host USER@HOST] [--port N]
//
// The agent must run as an interactive scheduled task rather than under the
// SSH session: a non-interactive SSH logon does not see the host's mapped
// network drives, so a model on S: is invisible to anything SSH launches while
// being perfectly visible to a process the logged-on user owns. This is the
// same launch shape run-ssh-forwarded-real-four-node.ps1 uses.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const [command, ...rest] = process.argv.slice(2);
const argument = (name, fallback) => {
  const index = rest.indexOf(name);
  return index >= 0 && rest[index + 1] ? rest[index + 1] : fallback;
};

const host = argument("--host", "42mob@192.168.0.29");
const port = Number(argument("--port", "42003"));
const user = argument("--user", "m42-server2\\42mob");
const root = argument("--root", "C:\\Users\\42mob\\p4-remote");
const taskName = `p4-4node-agent-${port}`;

if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("--port must be a TCP port");
}

const launcher = `${root}\\run-agent-${port}.cmd`;
const descriptor = `${root}\\agent-${port}.descriptor.json`;
const agent = `${root}\\p4-agent.exe`;
const log = `${root}\\agent-${port}.log`;
const err = `${root}\\agent-${port}.err.log`;
// Records read back as evidence go here, alone. The agent inherits its stage
// servers' stderr so a load failure is not swallowed, which means four
// unsynchronised writers share that file and can tear a record in half.
const record = `${root}\\agent-${port}.record.log`;
// The agent must advertise the address the drive uses to reach it, which is
// the tunnel entrance on the driving machine, not the remote LAN address:
// event endpoints are matched by value, so an agent that calls itself
// 192.168.0.29 while the drive addressed 127.0.0.1 rejects its own traffic.
// The stage servers it spawns are all local to the remote host, so they are
// unaffected by this choice.
const advertised = argument("--advertise", "127.0.0.1");

// Batch coalescing is an adapter policy read from the environment at start,
// so an A/B over it is an agent restart rather than a config edit. 0 or 1
// leaves the scheduler planning whatever is ready; see the batching layers
// document for why that is the default.
const minBatchRows = Number(argument("--min-batch-rows", "0"));
if (!Number.isInteger(minBatchRows) || minBatchRows < 0) {
  throw new Error("--min-batch-rows must be a non-negative integer");
}
// The other issue policy: hold the head while this many batches are in the
// pipeline, so rows that would queue at the tail merge at the head instead.
// 0 leaves it off.
const maxOpenBatches = Number(argument("--max-open-batches", "0"));
if (!Number.isInteger(maxOpenBatches) || maxOpenBatches < 0) {
  throw new Error("--max-open-batches must be a non-negative integer");
}
// The width side of the same question: cap the rows one issued batch may
// carry so the ready set travels as several batches. 0 leaves it off.
const maxIssueRows = Number(argument("--max-issue-rows", "0"));
if (!Number.isInteger(maxIssueRows) || maxIssueRows < 0) {
  throw new Error("--max-issue-rows must be a non-negative integer");
}

// One line per generated token, on a stderr four stage servers already share.
// It is what separated "the adapter never produced this position" from "the
// adapter produced it and something downstream ate it", so it stays available
// - but a run that is not chasing a gap should not pay for it.
const tracePositions = rest.includes("--trace-positions");

// Exists so the harness can be tested against an adapter that records
// nothing. A run in this state must fail: before the fence it passed, by
// reading an earlier run's records for the same request ids.
const noSessionKeyTrace = rest.includes("--no-session-key-trace");

// The script is passed base64-encoded. SSH concatenates its remote command
// with the login shell in between, so pipes, quotes and semicolons in a
// PowerShell one-liner are re-split before PowerShell ever sees them;
// -EncodedCommand is the only form that survives both layers intact.
function ssh(script) {
  // Progress records become CLIXML on stderr over a non-interactive SSH
  // channel, which buries the real output; silencing them at the source is
  // cheaper than filtering a multi-kilobyte block back out.
  const encoded = Buffer.from(`$ProgressPreference = 'SilentlyContinue';
${script}`, "utf16le").toString("base64");
  const result = spawnSync("ssh", ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=20", host,
    `powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
  return { status: result.status, out: `${result.stdout ?? ""}${result.stderr ?? ""}`.trim() };
}

// Written locally and copied rather than echoed through PowerShell: quotes
// survive neither the SSH command line nor PowerShell quoting intact, and a
// mangled quote produces a launcher that cannot find its own executable.
function copyLauncher() {
  const text = [
    "@echo off",
    `cd /d ${root}`,
    // Stage-server stderr is inherited so a load failure on the remote side
    // lands in this log instead of being discarded into the null device.
    "set P4_STAGED_LLAMA_INHERIT_STDERR=1",
    "set P4_AGENT_STATS=1",
    // The conversation key an OUTER mints is not echoed anywhere on the wire,
    // so without this trace a run can only observe that the adapter did not
    // reject it. The log this writes is collected as run evidence.
    ...(noSessionKeyTrace ? [] : ["set P4_STAGED_TRACE_SESSION_KEY=1"]),
    `set P4_RECORD_FILE=${record}`,
    // Presence is what the adapter tests, so an unwanted trace has to be
    // absent rather than set to zero.
    ...(tracePositions ? ["set P4_STAGED_TRACE_OUTPUT_POSITION=1"] : []),
    `set P4_STAGED_MIN_BATCH_ROWS=${minBatchRows}`,
    `set P4_STAGED_MAX_OPEN_BATCHES=${maxOpenBatches}`,
    `set P4_STAGED_MAX_ISSUE_ROWS=${maxIssueRows}`,
    // Redirections go first: cmd strips them in place and leaves the gap,
    // which reaches the program as an extra empty argument - the advertised
    // address then parsed as blank and the agent refused every connection.
    `1>"${log}" 2>"${err}" "${agent}" 0.0.0.0:${port} tcp://${advertised}:${port}`,
    "",
  ].join("\r\n");
  const local = path.join(os.tmpdir(), `p4-4node-run-agent-${port}.cmd`);
  fs.writeFileSync(local, text, "ascii");
  const destination = `${host}:${launcher.replace(/\\/g, "/")}`;
  const copy = spawnSync("scp", ["-o", "BatchMode=yes", "-q", local, destination],
    { encoding: "utf8" });
  if (copy.status !== 0) throw new Error(`launcher copy failed: ${copy.stderr ?? ""}`);
}

const START = [
  // The launcher path has no spaces by construction, so it needs no inner
  // quoting - and quoting it here is what made cmd exit 1 while the very same
  // command line worked when run by hand.
  `$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c ${launcher}' -WorkingDirectory '${root}';`,
  `$principal = New-ScheduledTaskPrincipal -UserId '${user}' -LogonType Interactive -RunLevel Limited;`,
  `$settings = New-ScheduledTaskSettingsSet -Hidden;`,
  `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
  `Register-ScheduledTask -TaskName ${taskName} -Action $action -Principal $principal -Settings $settings | Out-Null;`,
  `Start-ScheduledTask -TaskName ${taskName};`,
  `$deadline = (Get-Date).AddSeconds(30);`,
  `do {`,
  `  $ready = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -eq ${port} });`,
  `  if ($ready.Count -eq 0) { Start-Sleep -Milliseconds 250 }`,
  `} while ($ready.Count -eq 0 -and (Get-Date) -lt $deadline);`,
  `if ($ready.Count -eq 0) { Get-Content -LiteralPath '${err}' -ErrorAction SilentlyContinue | Select-Object -Last 20; throw 'remote agent did not listen' }`,
  // A descriptor records what this harness started, so stop identifies its
  // own process instead of inferring ownership from whoever holds the port
  // later. Measured: 'schtasks /end' does leave the process alive long
  // enough for a port lookup to work, but depending on that ordering is
  // fragile and says nothing about whose agent it is.
  `$own = @(Get-NetTCPConnection -State Listen -LocalPort ${port} -ErrorAction SilentlyContinue | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique);`,
  `$proc = @($own | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue } | Where-Object { $_.ProcessName -eq 'p4-agent' })[0];`,
  `if (-not $proc) { throw 'no p4-agent owns the port after start' };`,
  `$desc = [ordered]@{ task = '${taskName}'; port = ${port}; pid = $proc.Id; path = $proc.Path; started_at = $proc.StartTime.ToString('o'); host = $env:COMPUTERNAME };`,
  `$desc | ConvertTo-Json -Compress | Set-Content -LiteralPath '${descriptor}' -Encoding ascii;`,
  `Write-Output ('REMOTE_AGENT_LISTENING pid=' + $proc.Id + ' host=' + $env:COMPUTERNAME)`,
].join(" ");

// Stops only what this harness started. Killing every p4-agent and
// p4_staged_server on the host would take down anyone else using the machine,
// which is not a stop this tool is entitled to make - so the agent is found by
// the port it was told to bind, and the stage servers by being its children.
const STOP = [
  `schtasks.exe /end /tn ${taskName} *> $null;`,
  `schtasks.exe /delete /tn ${taskName} /f *> $null;`,
  // Ownership comes from the descriptor written at start and is re-verified
  // against the live process: a pid alone can have been recycled onto
  // someone else's program, and killing that would be a stop this harness
  // is not entitled to make.
  `$agents = @();`,
  `if (Test-Path '${descriptor}') {`,
  `  $d = Get-Content -LiteralPath '${descriptor}' -Raw | ConvertFrom-Json;`,
  `  $p = Get-Process -Id $d.pid -ErrorAction SilentlyContinue;`,
  `  if ($p -and $p.Path -eq $d.path -and $p.StartTime.ToString('o') -eq $d.started_at) { $agents = @($p) }`,
  `}`,
  `$stages = @();`,
  `foreach ($a in $agents) { $stages += @(Get-CimInstance Win32_Process -Filter \"ParentProcessId=$($a.Id)\" -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'p4_staged_server.exe' }) }`,
  `foreach ($s in $stages) { Stop-Process -Id $s.ProcessId -Force -ErrorAction SilentlyContinue }`,
  `foreach ($a in $agents) { Stop-Process -Id $a.Id -Force -ErrorAction SilentlyContinue }`,
  `Write-Output ('REMOTE_AGENT_STOPPED agents=' + $agents.Count + ' stages=' + $stages.Count)`,
].join(" ");

const STATUS = [
  `$listen = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -eq ${port} }).Count;`,
  `$procs = @(Get-Process p4-agent, p4_staged_server -ErrorAction SilentlyContinue).Count;`,
  `Write-Output ('listening=' + $listen + ' processes=' + $procs);`,
  `if (Test-Path '${err}') { Write-Output '--- stderr tail ---'; Get-Content -LiteralPath '${err}' -Tail 15 }`,
].join(" ");

const scripts = { start: START, stop: STOP, status: STATUS };
if (!scripts[command]) {
  process.stderr.write("usage: remote-agent.mjs start|stop|status [--host USER@HOST] [--port N]\n");
  process.exitCode = 1;
} else {
  if (command === "start") copyLauncher();
  const { status, out } = ssh(scripts[command]);
  process.stdout.write(`${out}\n`);
  if (status !== 0) process.exitCode = 1;
}
