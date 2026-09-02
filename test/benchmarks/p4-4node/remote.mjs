// Remote-run plumbing: an SSH forward whose ownership is proved, and a GPU
// sampler on the far side.
//
// Both exist because a remote run's evidence has to name the machine it came
// from. A forward that silently failed to bind leaves a *previous* forward -
// or a local agent - holding the same port, and the run then records
// `target: remote` while having talked to something else entirely. Sampling
// only the local devices has the same shape of problem: it produces a GPU
// trace that says nothing about the GPUs that did the work.

import { spawn, spawnSync } from "node:child_process";

/// Runs one PowerShell script on the remote host. Passed base64-encoded
/// because SSH concatenates its remote command with the login shell, which
/// re-splits pipes and quotes before PowerShell ever sees them.
export function remotePowerShell(host, script, timeoutMs = 60_000) {
  // Progress records become CLIXML on stderr over a non-interactive SSH
  // channel, which buries the real output; silencing them at the source is
  // cheaper than filtering a multi-kilobyte block back out.
  const encoded = Buffer.from(`$ProgressPreference = 'SilentlyContinue';
${script}`, "utf16le").toString("base64");
  const result = spawnSync("ssh", ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=20", host,
    `powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: timeoutMs });
  const noise = /CLIXML|<Objs|<Obj |progress/;
  return {
    status: result.status,
    out: (result.stdout ?? "").trim(),
    err: (result.stderr ?? "").split(/\r?\n/).filter((l) => l.trim() && !noise.test(l)).join("\n"),
  };
}

/// Opens the forward and proves this process owns it.
///
/// Binding is not enough on its own: `ssh -L` on a taken port logs "Address
/// already in use" and keeps running, so the caller would see a live child and
/// a reachable port that belong to someone else. `ExitOnForwardFailure` turns
/// that into an exit, and the identity probe below closes the remaining gap by
/// asking the far side to confirm the connection arrived over this tunnel.
export async function openTunnel({ host, localPort, remotePort }) {
  const child = spawn("ssh", ["-N", "-T", "-o", "BatchMode=yes",
    "-o", "ExitOnForwardFailure=yes", "-o", "ServerAliveInterval=15",
    "-o", "ServerAliveCountMax=240", "-o", "TCPKeepAlive=yes",
    "-L", `${localPort}:127.0.0.1:${remotePort}`, host],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });

  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk.toString("utf8"); });

  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`ssh forward exited ${child.exitCode}: ${stderr.trim() || "no output"}`);
    }
    if (/Address already in use|cannot listen|Could not request/i.test(stderr)) {
      child.kill();
      throw new Error(`ssh forward could not bind ${localPort}: ${stderr.trim()}`);
    }
    if (await reachable(localPort)) return { child, stderr: () => stderr };
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  child.kill();
  throw new Error(`ssh forward to ${host} never became reachable: ${stderr.trim()}`);
}

async function reachable(port) {
  const net = await import("node:net");
  return new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port, timeout: 1_500 });
    const done = (value) => { socket.destroy(); resolve(value); };
    socket.on("connect", () => done(true));
    socket.on("error", () => done(false));
    socket.on("timeout", () => done(false));
  });
}

/// Confirms the port really reaches the intended host's agent, by matching the
/// remote agent process against the connection the far side sees.
export function proveTunnelIdentity(host, remotePort) {
  const script = [
    // Identity is the process listening on the port, not a count of
    // established connections: the probe runs before the drive connects.
    `$own = @(Get-NetTCPConnection -State Listen -LocalPort ${remotePort} -ErrorAction SilentlyContinue | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique);`,
    `$agent = @($own | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue } | Where-Object { $_.ProcessName -eq 'p4-agent' });`,
    `Write-Output ('agent_pids=' + (($agent | ForEach-Object { $_.Id }) -join ','));`,
    `Write-Output ('host=' + $env:COMPUTERNAME);`,
    `$g = & nvidia-smi --query-gpu=index,name,uuid --format=csv,noheader;`,
    `Write-Output ('gpus=' + ($g -join ' ; '))`,
  ].join(" ");
  const { out, err, status } = remotePowerShell(host, script);
  if (status !== 0) throw new Error(`tunnel identity probe failed: ${err || out}`);
  const identity = Object.fromEntries(out.split(/\r?\n/).filter(Boolean).map((line) => {
    const index = line.indexOf("=");
    return [line.slice(0, index), line.slice(index + 1)];
  }));
  if (!identity.agent_pids) {
    throw new Error(`no agent process on ${host}; the port may belong to something else`);
  }
  return identity;
}

/// The launcher the remote agent was started from, verbatim, with its digest.
///
/// The agent's policy knobs - the coalescing threshold above all - live in
/// its environment, and an environment is not reconstructible after the fact:
/// a later restart with different flags leaves no trace of what the measured
/// run was configured with. A throughput number whose policy cannot be
/// recovered from its own evidence is not a measurement of anything.
export function remoteLauncher({ host, remotePort, root }) {
  const file = `${root}\\run-agent-${remotePort}.cmd`;
  const script = [
    `$p = '${file}';`,
    "if (Test-Path -LiteralPath $p) {",
    "  Write-Output ('launcher_sha256=' + (Get-FileHash -Algorithm SHA256 -LiteralPath $p).Hash.ToLower());",
    "  Write-Output 'launcher_begin';",
    "  Get-Content -LiteralPath $p;",
    "}",
  ].join("\n");
  const { out, err, status } = remotePowerShell(host, script);
  if (status !== 0) throw new Error(`remote launcher read failed: ${err || out}`);
  const begin = out.indexOf("launcher_begin");
  if (begin < 0) throw new Error(`no launcher at ${file}`);
  const digest = /launcher_sha256=([0-9a-f]{64})/.exec(out);
  return {
    path: file,
    sha256: digest ? digest[1] : null,
    // Verbatim, because the knobs are `set` lines in it and a summary would
    // be a second thing to keep in step with the first.
    text: out.slice(begin + "launcher_begin".length).replace(/^\r?\n/, ""),
  };
}

/// Hashes the files the remote agent is actually running, on the remote host.
///
/// Hashing the local `target/` copies proves what this machine built, not
/// what the far side executed. The two agree only because a person ran scp
/// between them, and "a person ran scp" is not evidence. So the agent's own
/// image path is taken from the live process, and the stage runtime beside it
/// is hashed too - by the host that owns them.
export function remoteImageDigests({ host, remotePort, root }) {
  const script = [
    `$own = @(Get-NetTCPConnection -State Listen -LocalPort ${remotePort} -ErrorAction SilentlyContinue | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique);`,
    "$agent = @($own | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue } | Where-Object { $_.ProcessName -eq 'p4-agent' })[0];",
    "if ($agent) { Write-Output ('agent_path=' + $agent.Path); Write-Output ('agent_sha256=' + (Get-FileHash -Algorithm SHA256 -LiteralPath $agent.Path).Hash.ToLower()) }",
    `$staged = '${root}\\staged';`,
    "foreach ($name in @('p4_staged_server.exe','ggml-cuda.dll','llama.dll','ggml-base.dll')) {",
    "  $file = Join-Path $staged $name;",
    "  if (Test-Path -LiteralPath $file) { Write-Output ($name + '_sha256=' + (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLower()) }",
    "}",
  ].join("\n");
  const { out, err, status } = remotePowerShell(host, script);
  if (status !== 0) throw new Error(`remote image digest failed: ${err || out}`);
  const digests = Object.fromEntries(out.split(/\r?\n/).filter(Boolean).map((line) => {
    const index = line.indexOf("=");
    return [line.slice(0, index), line.slice(index + 1)];
  }));
  if (!digests.agent_sha256) {
    throw new Error(`no running agent image to hash on ${host}`);
  }
  return digests;
}

/// Samples the remote GPUs for the duration of the run. Returns a handle whose
/// `stop()` yields the CSV, so a remote run carries the same evidence a local
/// one does.
export function startRemoteGpuSampler(host) {
  const child = spawn("ssh", ["-T", "-o", "BatchMode=yes", host,
    "nvidia-smi --query-gpu=timestamp,index,name,utilization.gpu,memory.used,power.draw"
    + " --format=csv,noheader,nounits -lms 250"],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let csv = "";
  child.stdout.on("data", (chunk) => { csv += chunk.toString("utf8"); });
  return {
    async stop() {
      child.kill();
      await new Promise((resolve) => setTimeout(resolve, 300));
      return csv;
    },
  };
}

/// Pulls the remote agent's stderr so a remote run's evidence names what the
/// far side logged, not only what the drive saw. The session-key trace lives
/// here and nowhere else: the key an OUTER mints is never echoed on the wire.
///
/// Throws when the log cannot be read at all: an unreadable log and an adapter
/// that dropped the field produce the same empty string, and only one of those
/// is a run this harness may pass.
/// Byte length of the remote agent log right now.
///
/// The agent appends across runs and the smoke scenario reuses one request
/// id, so a line another run wrote would otherwise satisfy this run's check.
/// Taking the length first and reading from it makes the evidence this run's.
export function remoteAgentLogLength({ host, remotePort, root }) {
  const { status, out } = remotePowerShell(host,
    `$p = '${root}\\agent-${remotePort}.err.log'; if (Test-Path -LiteralPath $p) { Write-Output ((Get-Item -LiteralPath $p).Length) } else { Write-Output 0 }`);
  // A failed probe used to become Number("") = 0, which reads the whole file
  // from the start - the offset silently doing the opposite of its job.
  const value = Number(out.trim());
  if (status !== 0 || out.trim() === "" || !Number.isInteger(value) || value < 0) {
    throw new Error(`could not measure the remote agent log: ${out.slice(0, 200) || "no output"}`);
  }
  return value;
}

/// The agent's record file: the evidence channel, with no other writers.
/// Exactly the bytes a run appended: `[fromByte, toByte)`.
///
/// Reading to the end of the file instead would fold in whatever the agent
/// wrote after the run's closing length was taken - the closing offset would
/// name a boundary without being one.
export function fetchRemoteRecord({ host, remotePort, root, fromByte = 0, toByte = null }) {
  return readRemoteRange(host, recordPath(root, remotePort), fromByte, toByte);
}

/// The record file's length right now.
///
/// The harness marks a run by taking this before the drive starts and again
/// after it ends: the run's records are exactly the bytes between. Writing a
/// fence into the file itself would have made the harness a second writer of
/// a file declared to have one, and nothing orders an external append against
/// the agent's own.
/// Whether the agent marked its record channel as failed, and why.
///
/// Read from a file beside the record file rather than from stderr: the
/// announcement must not travel on the channel four stage servers share,
/// which is the one that tore a record in half.
export function remoteRecordFailure({ host, remotePort, root }) {
  const marker = `${recordPath(root, remotePort)}.failed`;
  const { status, out } = remotePowerShell(host,
    `$p = '${marker}';`
    + " if (Test-Path -LiteralPath $p) { Write-Output ('P4_RECORD_FAILED ' + (Get-Content -LiteralPath $p -Raw)) }"
    + " else { Write-Output 'P4_RECORD_OK' }");
  if (status !== 0 || (!out.includes("P4_RECORD_OK") && !out.includes("P4_RECORD_FAILED"))) {
    throw new Error(`could not read the record channel's status: ${out.slice(0, 200) || "no output"}`);
  }
  return out.includes("P4_RECORD_FAILED")
    ? out.slice(out.indexOf("P4_RECORD_FAILED")).trim()
    : null;
}

/// Clears a previous run's failure marker, so this run's status is its own.
export function clearRemoteRecordFailure({ host, remotePort, root }) {
  const marker = `${recordPath(root, remotePort)}.failed`;
  remotePowerShell(host, `Remove-Item -LiteralPath '${marker}' -Force -ErrorAction SilentlyContinue`);
}

export function remoteRecordLength({ host, remotePort, root }) {
  const { status, out } = remotePowerShell(host,
    `$p = '${recordPath(root, remotePort)}';`
    + " if (Test-Path -LiteralPath $p) { Write-Output ((Get-Item -LiteralPath $p).Length) }"
    + " else { Write-Output 0 }");
  const value = Number(out.trim());
  if (status !== 0 || out.trim() === "" || !Number.isInteger(value) || value < 0) {
    throw new Error(`could not measure the remote record file: ${out.slice(0, 200) || "no output"}`);
  }
  return value;
}

function recordPath(root, remotePort) {
  return `${root}\\agent-${remotePort}.record.log`;
}

export function fetchRemoteAgentLog({ host, remotePort, root, fromByte = 0 }) {
  return readRemoteFile(host, `${root}\\agent-${remotePort}.err.log`, fromByte);
}

/// Reads one remote file from a byte offset.
///
/// Through an explicitly shared handle: the agent holds these open for
/// writing and Get-Content would fail on the sharing mode. The outcome is
/// reported on stdout because a PowerShell error record reaches this side as
/// CLIXML on stderr, where it is indistinguishable from noise - a silent
/// empty read once looked exactly like the dropped field it was investigating.
function readRemoteFile(host, file, fromByte) {
  const { out } = remotePowerShell(host, [
    `$p = '${file}'`,
    "try {",
    "  $s = [System.IO.File]::Open($p, 'Open', 'Read', 'ReadWrite')",
    // Equality means this run appended nothing, which must read as nothing -
    // not as the whole file. Shrinking means the file was replaced under us,
    // which no offset can describe.
    `  if ($s.Length -lt ${fromByte}) { throw 'record shrank below the recorded offset' }`,
    `  $null = $s.Seek(${fromByte}, 'Begin')`,
    "  $r = New-Object System.IO.StreamReader($s)",
    "  $t = $r.ReadToEnd(); $r.Close(); $s.Close()",
    "  Write-Output 'P4_REMOTE_LOG_BEGIN'; Write-Output $t",
    "} catch { Write-Output ('P4_REMOTE_LOG_ERROR ' + $_.Exception.Message) }",
  ].join("\n"), 120_000);
  const begin = out.indexOf("P4_REMOTE_LOG_BEGIN");
  if (begin < 0) {
    throw new Error(`remote file unreadable: ${out.slice(0, 200) || "no output"}`);
  }
  return out.slice(begin + "P4_REMOTE_LOG_BEGIN".length).replace(/^\r?\n/, "");
}

/// Reads one byte range of a remote file.
///
/// `toByte` of null means to the end. A range that does not end on a line
/// boundary is refused rather than truncated mid-record: half a record is
/// not evidence, and silently dropping it would hide the very interleaving
/// this channel exists to avoid.
function readRemoteRange(host, file, fromByte, toByte) {
  const length = toByte === null ? -1 : toByte - fromByte;
  if (length !== -1 && length < 0) {
    throw new Error(`record range ends before it begins: ${fromByte}..${toByte}`);
  }
  if (length === 0) return { text: "", endsOnRecord: true };
  const { out } = remotePowerShell(host, [
    `$p = '${file}'`,
    "try {",
    "  $s = [System.IO.File]::Open($p, 'Open', 'Read', 'ReadWrite')",
    `  if ($s.Length -lt ${fromByte}) { throw 'record shrank below the opening offset' }`,
    `  $null = $s.Seek(${fromByte}, 'Begin')`,
    `  $want = ${length}`,
    "  if ($want -lt 0) { $want = $s.Length - $s.Position }",
    "  if ($s.Position + $want -gt $s.Length) { throw 'record range runs past the file' }",
    "  $buffer = New-Object byte[] $want",
    "  $read = 0",
    "  while ($read -lt $want) {",
    "    $step = $s.Read($buffer, $read, $want - $read)",
    "    if ($step -le 0) { throw 'record range ended early' }",
    "    $read += $step",
    "  }",
    "  $s.Close()",
    // Asked here, where the bytes are still intact: the transport trims
    // trailing whitespace, so a newline cannot be checked on this side.
    "  $whole = $want -eq 0 -or $buffer[$want - 1] -eq 10",
    "  Write-Output ('P4_REMOTE_RANGE_WHOLE=' + [int]$whole)",
    "  Write-Output 'P4_REMOTE_LOG_BEGIN'",
    "  Write-Output ([System.Text.Encoding]::UTF8.GetString($buffer))",
    "} catch { Write-Output ('P4_REMOTE_LOG_ERROR ' + $_.Exception.Message) }",
  ].join("\n"), 120_000);
  const begin = out.indexOf("P4_REMOTE_LOG_BEGIN");
  if (begin < 0) {
    throw new Error(`remote record range unreadable: ${out.slice(0, 200) || "no output"}`);
  }
  return {
    text: out.slice(begin + "P4_REMOTE_LOG_BEGIN".length).replace(/^\r?\n/, ""),
    endsOnRecord: out.includes("P4_REMOTE_RANGE_WHOLE=1"),
  };
}
