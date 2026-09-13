// Groups the model share's GGUF files into logical models and reads each one's
// header. Run on a machine that can see the share.
//
//   node test/benchmarks/model-catalog/inventory.mjs [--root S:\models] [--out <file>]

import fs from 'node:fs';
import path from 'node:path';
import { describeModel } from '../../../tools/model-loading/src/gguf-header.mjs';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const root = argument('--root', 'S:\\models');
const out = argument('--out', path.join(process.cwd(), 'target', 'model-catalog', 'inventory.json'));

/// A split GGUF is one model: `name-00001-of-00005.gguf` and its siblings.
/// An mmproj is a separate, non-generating artifact and is listed apart.
function walk(directory, found = []) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) walk(full, found);
    else if (/\.gguf$/i.test(entry.name)) found.push(full);
  }
  return found;
}

const files = walk(root);
const groups = new Map();
for (const file of files) {
  const directory = path.dirname(file);
  const base = path.basename(file).replace(/-\d{5}-of-\d{5}\.gguf$/i, '').replace(/\.gguf$/i, '');
  const key = `${directory}\u0000${base}`;
  if (!groups.has(key)) groups.set(key, { directory, base, files: [] });
  groups.get(key).files.push(file);
}

const models = [];
const auxiliary = [];
for (const group of groups.values()) {
  group.files.sort();
  const record = {
    id: `${path.basename(group.directory)}/${group.base}`.replace(/[\\/]/g, '_'),
    publisher: path.basename(path.dirname(group.directory)),
    repository: path.basename(group.directory),
    base_name: group.base,
    directory: group.directory,
    files: group.files,
    first_shard: group.files[0],
  };
  if (/mmproj|projector/i.test(group.base)) { auxiliary.push(record); continue; }
  try {
    Object.assign(record, describeModel(group.files));
    models.push(record);
  } catch (error) {
    models.push({ ...record, header_error: error.message });
  }
}

models.sort((a, b) => (a.file_bytes ?? 0) - (b.file_bytes ?? 0));
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, `${JSON.stringify({ root, observed_at: new Date().toISOString(), models, auxiliary }, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ out, models: models.length, auxiliary: auxiliary.length })}\n`);
