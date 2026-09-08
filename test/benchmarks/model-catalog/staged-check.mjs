// Lists which models are completely staged on the probe host.
//
//   node test/benchmarks/model-catalog/staged-check.mjs [--dest D:\models] [--exclude-inflight]
//
// A model counts as staged only when every shard is present at exactly its
// source length and no copy is writing it. Loading a shard the copy still holds
// open fails with a permission error, and an unbuffered copy leaves a
// full-length file with partial contents until it finishes.

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const host = argument('--host', '42mob@192.168.0.29');
const dest = argument('--dest', 'D:\\models');
const inventoryFile = argument('--inventory', path.join('target', 'model-catalog', 'inventory.json'));
const root = argument('--root', 'C:\\Users\\42mob\\p4-remote');
const remoteDir = `${root}\\probe`;
const out = argument('--out', null);

function ssh(script) {
  const encoded = Buffer.from(`$ProgressPreference = 'SilentlyContinue';\n${script}`, 'utf16le').toString('base64');
  const result = spawnSync('ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', host,
    `powershell -NoProfile -NonInteractive -EncodedCommand ${encoded}`],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return `${result.stdout ?? ''}`.trim();
}

const listing = ssh([
  `Get-ChildItem -LiteralPath '${dest}' -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { 'F ' + $_.FullName + '|' + $_.Length };`,
  `$log = Get-ChildItem -Path '${remoteDir}\\stage-copy-*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1;`,
  "if ($log) { Get-Content -LiteralPath $log.FullName | Where-Object { $_ -match '^(COPY|DONE)' } | ForEach-Object { Write-Output ('L ' + $_.Trim()) } };",
  "$running = @(Get-Process robocopy -ErrorAction SilentlyContinue).Count; Write-Output ('ROBOCOPY ' + $running)",
].join(' '));

const sizes = new Map();
const logLines = [];
let copying = false;
for (const line of listing.split(/\r?\n/)) {
  if (line.startsWith('F ')) {
    const [full, length] = line.slice(2).split('|');
    sizes.set(full.toLowerCase(), Number(length));
  } else if (line.startsWith('L ')) logLines.push(line.slice(2).trim());
  else if (line.startsWith('ROBOCOPY ')) copying = Number(line.slice(9).trim()) > 0;
}

// The last COPY line is the model still being written; every earlier COPY line
// finished, unless it reported COPY_FAILED.
const copyOrder = logLines.filter((l) => l.startsWith('COPY ') && !l.startsWith('COPY_FAILED')).map((l) => l.slice(5).trim());
const failed = new Set(logLines.filter((l) => l.startsWith('COPY_FAILED')).map((l) => l.split(/\s+/).pop()));
const finished = logLines.some((l) => l.startsWith('DONE'));
const completedIds = new Set(finished ? copyOrder : copyOrder.slice(0, -1));
const inflight = finished ? null : copyOrder[copyOrder.length - 1] ?? null;

const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
const staged = [];
const missing = [];
for (const model of inventory.models) {
  if (!model.files?.length) continue;
  let total = 0;
  let complete = true;
  for (const file of model.files) {
    const target = `${dest}\\${model.repository}\\${path.basename(file)}`.toLowerCase();
    const length = sizes.get(target);
    if (length == null) { complete = false; break; }
    total += length;
  }
  const done = completedIds.has(model.id) && !failed.has(model.id);
  if (complete && total === model.file_bytes && done) staged.push(model.id);
  else missing.push({ id: model.id, present: complete, copy_logged_done: done, bytes: total, expected: model.file_bytes });
}

const report = { dest, copying, inflight, staged, missing_count: missing.length };
if (out) fs.writeFileSync(out, `${JSON.stringify({ ...report, missing }, null, 2)}\n`);
process.stdout.write(`${JSON.stringify(report)}\n`);
