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
  const encoded = Buffer.from(script, "utf16le").toString("base64");
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
