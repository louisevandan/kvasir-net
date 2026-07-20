#!/usr/bin/env node
// linkcpp node client — connect a compute device (desktop/laptop) to a linkcpp
// account (wallet address) so it appears in the wallet's node monitoring.
//
// Zero dependencies (uses Node's built-in fetch, Node 18+).
//
//   LINKCPP_SERVICE=http://your-gateway-host:8791 LINKCPP_OWNER=<account_pubkey> node connect.js
//   node connect.js <serviceUrl> <ownerPubkey>
'use strict';

const os = require('os');
const { execSync } = require('child_process');

const SERVICE = process.env.LINKCPP_SERVICE || process.argv[2];
const OWNER = process.env.LINKCPP_OWNER || process.argv[3];
if (!SERVICE || !OWNER) {
  console.error('usage: LINKCPP_SERVICE=http://<host>:8791 LINKCPP_OWNER=<account_pubkey> node connect.js');
  console.error('   or: node connect.js <serviceUrl> <ownerPubkey>');
  process.exit(1);
}

function detectOs() {
  switch (os.platform()) {
    case 'darwin': return 'macos';
    case 'win32': return 'windows';
    case 'linux': return 'linux';
    default: return 'unknown';
  }
}
function cmdOk(cmd) { try { execSync(cmd, { stdio: 'ignore' }); return true; } catch { return false; } }
function detectAccel() {
  if (process.env.LINKCPP_ACCEL) return process.env.LINKCPP_ACCEL.toLowerCase();
  if (cmdOk('nvidia-smi -L')) return 'gpu';        // CUDA GPU
  if (os.platform() === 'darwin') return 'gpu';    // Apple GPU (Metal); ANE/NPU also present
  return 'cpu';
}

const OSCAT = detectOs();
const ACCEL = detectAccel();
const DEVICE_KIND = process.env.LINKCPP_DEVICE_KIND || 'computer';
const NODE_ID = (process.env.LINKCPP_NODE_ID || os.hostname()).replace(/[^A-Za-z0-9_.-]/g, '-');
const LABEL = process.env.LINKCPP_LABEL || os.hostname();
const base = SERVICE.replace(/\/$/, '');

async function post(path, body) {
  const r = await fetch(base + path, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`${path} -> ${r.status}: ${text}`);
  return JSON.parse(text);
}

async function main() {
  console.log(`connecting node "${NODE_ID}"  os=${OSCAT}  accel=${ACCEL}  kind=${DEVICE_KIND}`);
  console.log(`account: ${OWNER}`);
  console.log(`service: ${base}`);
  await post('/api/node/register', {
    nodeId: NODE_ID, owner: OWNER, os: OSCAT, accelerator: ACCEL, deviceKind: DEVICE_KIND, label: LABEL,
  });
  console.log('registered ✓  sending heartbeats every 30s (Ctrl+C to stop)');
  const beat = async () => {
    try { await post('/api/node/heartbeat', { nodeId: NODE_ID }); process.stdout.write('.'); }
    catch (e) { console.error('\nheartbeat failed:', e.message); }
  };
  await beat();
  setInterval(beat, 30000);
}

main().catch((e) => { console.error(e.message); process.exit(1); });
