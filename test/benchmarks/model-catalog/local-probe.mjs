// Runs the stage-load probe on this machine.
//
//   node test/benchmarks/model-catalog/local-probe.mjs --jobs <file> --out <file> [--exe <path>]
//
// Same probe as remote-probe drives on the RTX 3090 x2 host, without SSH or a
// scheduled task: here the model share and the local disks are already visible
// to this session. Use it when the probe host is unavailable, and record which
// machine a measurement came from - the two have different cards.

import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import os from 'node:os';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const jobsFile = argument('--jobs', null);
const outFile = argument('--out', null);
const exe = argument('--exe', path.resolve('target/p4-staged-cuda/p4_staged_server.exe'));
const logDir = argument('--logs', path.resolve('target/model-catalog/local-logs'));
if (!jobsFile || !outFile) throw new Error('needs --jobs and --out');
fs.mkdirSync(logDir, { recursive: true });

const jobs = JSON.parse(fs.readFileSync(jobsFile, 'utf8'));

function runStage(job, stage, index) {
  return new Promise((resolve) => {
    const body = Buffer.from(stage.plan.join(' '), 'utf8');
    const head = Buffer.alloc(4);
    head.writeUInt32LE(body.length, 0);
    const args = ['--port', String(42300 + index), '--bind', '127.0.0.1',
      ...(job.mode === 'plan' ? ['--inspect-memory-plan'] : [])];
    const started = Date.now();
    const child = spawn(exe, args, {
      stdio: ['pipe', 'ignore', 'pipe'],
      env: { ...process.env, CUDA_VISIBLE_DEVICES: String(stage.device), CUDA_DEVICE_ORDER: 'PCI_BUS_ID' },
    });
    let err = '';
    let settled = false;
    let settleAt = 0;
    const finish = (outcome) => {
      if (settled) return;
      settled = true;
      clearInterval(poll);
      clearTimeout(deadline);
      const log = path.join(logDir, `${job.id}-s${index}.log`);
      try { fs.writeFileSync(log, err, 'utf8'); } catch {}
      const grab = (tag) => {
        const m = err.match(new RegExp(`^${tag} (\\{.*)$`, 'm'));
        try { return m ? JSON.parse(m[1]) : null; } catch { return null; }
      };
      const lines = (re) => err.split(/\r?\n/).filter((l) => re.test(l)).map((l) => l.trim());
      resolve({
        stage: index,
        device: stage.device,
        outcome,
        elapsed_ms: Date.now() - started,
        plan: grab('MEMORY_PLAN'),
        actual: grab('MEMORY_ACTUAL'),
        buffers: lines(/buffer size/),
        graph: lines(/graph nodes|graph splits|worst-case/),
        error: lines(/error|failed|differ|unsupported|assert|exceeds|not supported/i).slice(0, 12),
        log,
      });
      try { child.kill(); } catch {}
    };
    child.stderr.on('data', (c) => {
      err += c.toString();
      if (err.length > 40e6) err = err.slice(-20e6);
    });
    child.on('exit', (code, signal) => finish(`exit:${code ?? signal}`));
    child.on('error', (e) => { err += `spawn error: ${e.message}\n`; finish('spawn_error'); });
    const poll = setInterval(() => {
      if (job.mode === 'plan') return;
      if (!settleAt && /^MEMORY_ACTUAL /m.test(err)) settleAt = Date.now() + 5000;
      if (settleAt && Date.now() >= settleAt) finish('loaded');
    }, 500);
    const deadline = setTimeout(() => finish('timeout'), (job.timeout_s ?? 5400) * 1000);
    child.stdin.write(head);
    child.stdin.write(body);
    child.stdin.end();
  });
}

const out = fs.createWriteStream(outFile, { flags: 'a' });
for (const job of jobs) {
  const before = os.freemem();
  process.stderr.write(`PROBE_START ${job.id} mode=${job.mode} stages=${job.stages.length} ${new Date().toISOString()}\n`);
  const stages = await Promise.all(job.stages.map((s, i) => runStage(job, s, i)));
  out.write(`${JSON.stringify({
    id: job.id,
    mode: job.mode,
    at: new Date().toISOString(),
    machine: os.hostname(),
    host_free_before: before,
    host_free_after: os.freemem(),
    host_total: os.totalmem(),
    stages,
  })}\n`);
  process.stderr.write(`PROBE_DONE ${job.id} ${stages.map((s) => s.outcome).join(',')} ${Math.round(Math.max(...stages.map((s) => s.elapsed_ms)) / 1000)}s\n`);
  // A stage that pinned 90 GiB of host memory does not give it back the
  // instant the process dies, and the next job then fails its pinned
  // allocation with 'resource already mapped'. Wait for the memory to come
  // back before starting the next one.
  for (let waited = 0; waited < 300; waited += 5) {
    if (os.freemem() >= before * 0.9) break;
    await new Promise((r) => setTimeout(r, 5000));
  }
  await new Promise((r) => setTimeout(r, 5000));
}
out.end();
process.stderr.write('PROBE_ALL_DONE\n');
