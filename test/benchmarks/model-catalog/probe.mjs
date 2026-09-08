#!/usr/bin/env node
// Stage-load probe. Runs on the machine that can see the model share.
//   node probe.mjs <jobs.json> <out.jsonl> [logdir]
// Each job: { id, mode: "plan"|"load", stages:[{device, plan:[..tokens]}] }
// Every stage of a job runs concurrently, one process per stage, so the host
// memory a real multi-stage load needs is what gets measured.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const [jobsFile, outFile, logDirArg] = process.argv.slice(2);
const EXE = String.raw`C:\Users\42mob\p4-remote\staged\p4_staged_server.exe`;
const logDir = logDirArg ?? String.raw`C:\Users\42mob\p4-remote\probe-logs`;
fs.mkdirSync(logDir, { recursive: true });
const jobs = JSON.parse(fs.readFileSync(jobsFile, 'utf8'));

const memory = () => ({ free: os.freemem(), total: os.totalmem() });

function runStage(job, stage, index) {
  return new Promise((resolve) => {
    const planText = stage.plan.join(' ');
    const body = Buffer.from(planText, 'utf8');
    const head = Buffer.alloc(4);
    head.writeUInt32LE(body.length, 0);
    const args = ['--port', String(42200 + index), '--bind', '127.0.0.1',
      ...(job.mode === 'plan' ? ['--inspect-memory-plan'] : [])];
    const started = Date.now();
    const child = spawn(EXE, args, {
      stdio: ['pipe', 'ignore', 'pipe'],
      env: { ...process.env, CUDA_VISIBLE_DEVICES: String(stage.device), CUDA_DEVICE_ORDER: 'PCI_BUS_ID' },
    });
    let err = '';
    let settled = false;
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
        arch: lines(/^print_info: *arch *=|^load: |^llama_model_loader: - kv +\d+: +general\.(architecture|name)/).slice(0, 4),
        error: lines(/error|failed|differ|unsupported|unknown|assert|exceeds|not supported/i).slice(0, 12),
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
    // MEMORY_ACTUAL is printed just before the server compares it with the plan
    // and refuses the load if they differ, so seeing the line is not yet a
    // successful load. Give the process a few seconds to fail before calling it
    // loaded; if it exits first, the exit handler reports that instead.
    let settleAt = 0;
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
  const before = memory();
  process.stderr.write(`PROBE_START ${job.id} mode=${job.mode} stages=${job.stages.length} ${new Date().toISOString()}\n`);
  const stages = await Promise.all(job.stages.map((s, i) => runStage(job, s, i)));
  const after = memory();
  out.write(`${JSON.stringify({
    id: job.id,
    mode: job.mode,
    at: new Date().toISOString(),
    host_free_before: before.free,
    host_free_after: after.free,
    host_total: before.total,
    stages,
  })}\n`);
  process.stderr.write(`PROBE_DONE ${job.id} ${stages.map((s) => s.outcome).join(',')} ${Math.round(Math.max(...stages.map((s) => s.elapsed_ms)) / 1000)}s\n`);
  await new Promise((r) => setTimeout(r, 5000));
}
out.end();
process.stderr.write('PROBE_ALL_DONE\n');
