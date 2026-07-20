// linkcpp off-chain KVR staking settlement service (devnet MVP).
//
// Model: a user stakes by transferring KVR to the vault (treasury ATA) from the
// wallet app, then POSTs the tx signature here. This service verifies the
// on-chain transfer, records the principal, and accrues rewards at a fixed APR.
// On unstake it pays principal + rewards back from the vault using the admin key.
//
// NOTE: devnet MVP. The vault == treasury and the admin key is the settlement
// authority, so staked funds are custodial here. This is the deliberate
// off-chain-first step; an on-chain program replaces it later.
'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');

// Real OS of the machine hosting this gateway/node. The served web wallet reports
// THIS as the node's OS (the node runs here), not the viewing browser's platform.
const HOST_OS = (() => {
  const p = os.platform();
  return p === 'darwin' ? 'macos' : p === 'win32' ? 'windows' : 'linux';
})();
const express = require('express');
const gwauth = require('./gatewayAuth');
const {
  Connection, Keypair, PublicKey,
} = require('@solana/web3.js');
const {
  getOrCreateAssociatedTokenAccount, getAssociatedTokenAddress, transferChecked,
} = require('@solana/spl-token');

const PORT = Number(process.env.PORT || 8791);
const APR = Number(process.env.STAKING_APR || 0.12); // 12% APR
const SECONDS_PER_YEAR = 365 * 24 * 60 * 60;
// Node operator reward: KVR paid per contribution "unit" (e.g. 1k tokens served).
const REWARD_PER_UNIT = Number(process.env.LINKCPP_REWARD_PER_UNIT || 0.01);
// Extra reward multiplier for a node that also HOSTS the gateway/settlement service
// (it keeps the network's coordination point online). Applied on top of the perf tier.
const GATEWAY_BONUS = Number(process.env.KVR_GATEWAY_BONUS || 1.5);

// Infrastructure UPTIME rewards. The hub (control plane / orchestrator) and the
// gateway (public entry / settlement) do not themselves produce inference "units",
// so a perf-tier multiplier can never pay them. Instead they earn KVR per hour of
// uptime, accrued on each heartbeat, INDEPENDENT of inference and SUMMED on top of
// it — so an all-in-one host earns hub + gateway + inference together. The hub is
// the most critical role, so its default rate is the highest.
const HUB_UPTIME_PER_HOUR = Number(process.env.KVR_HUB_UPTIME_PER_HOUR || 2.0);
const GATEWAY_UPTIME_PER_HOUR = Number(process.env.KVR_GATEWAY_UPTIME_PER_HOUR || 1.0);
// Cap the time credited per report so a heartbeat after an offline gap doesn't pay
// for downtime (continuous ~3s heartbeats accrue fully; long gaps are clamped).
const UPTIME_MAX_GAP_SEC = Number(process.env.KVR_UPTIME_MAX_GAP_SEC || 120);

// Performance tiers. A node reports its measured decode throughput (tok/s); that
// sets a reward multiplier so faster / more-capable nodes earn proportionally more
// for the same raw work. Contribution is re-scored as rawUnits * multiplier.
const PERF_TIERS = [
  { tier: 'S', minTps: 90, mult: 1.5 },
  { tier: 'A', minTps: 60, mult: 1.25 },
  { tier: 'B', minTps: 30, mult: 1.0 },
  { tier: 'C', minTps: 0, mult: 0.7 },
];
function perfTier(tps) {
  const t = Number(tps) || 0;
  return PERF_TIERS.find((x) => t >= x.minTps) || PERF_TIERS[PERF_TIERS.length - 1];
}

// Credit infra-role uptime rewards for the elapsed time since the last accrual,
// clamped so an offline gap isn't paid. Call on every register/heartbeat/contribution.
// Uptime rewards SUM with inference rewards into the same pendingRewards balance.
function accrueUptime(n, now) {
  const elapsed = Math.max(0, Math.min(now - (n.lastUptimeAt || now), UPTIME_MAX_GAP_SEC));
  let credit = 0;
  if (n.hostsHub) credit += elapsed * (HUB_UPTIME_PER_HOUR / 3600);
  if (n.hostsGateway) credit += elapsed * (GATEWAY_UPTIME_PER_HOUR / 3600);
  if (credit > 0) {
    n.uptimeRewards = (n.uptimeRewards || 0) + credit;
    n.pendingRewards = (n.pendingRewards || 0) + credit;
  }
  n.lastUptimeAt = now;
  return credit;
}

const SPEC_PATH = process.env.KVR_TOKEN_SPEC || path.resolve(__dirname, '../../wallet/shared-spec/token.devnet.json');
const SPEC = JSON.parse(fs.readFileSync(SPEC_PATH, 'utf8'));
// Canonical public URL clients should use (set when deployed behind a public IP / domain).
const PUBLIC_URL = process.env.KVR_PUBLIC_URL || null;
const RPC = process.env.LINKCPP_RPC_URL || SPEC.rpcUrl;
const MINT = new PublicKey(SPEC.token.mint);
const DECIMALS = SPEC.token.decimals;
const VAULT_ATA = new PublicKey(SPEC.treasury.ata);
const TREASURY_OWNER = SPEC.treasury.owner; // inference-payment recipient (Phase 2)
// Settlement admin key. Injected securely at deploy time: inline JSON array via
// KVR_ADMIN_KEY, or a mounted file via KVR_ADMIN_KEY_FILE; falls back to the local
// devnet key for development. NEVER bake this into an image or reuse on mainnet.
function loadAdminSecret() {
  if (process.env.KVR_ADMIN_KEY) return JSON.parse(process.env.KVR_ADMIN_KEY);
  const f = process.env.KVR_ADMIN_KEY_FILE || path.resolve(__dirname, '../token/.keys/admin.json');
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}
const ADMIN = Keypair.fromSecretKey(Uint8Array.from(loadAdminSecret()));

const DATA_DIR = process.env.KVR_DATA_DIR || path.resolve(__dirname, 'data');
const DB_FILE = path.join(DATA_DIR, 'positions.json');
const DB_TMP = DB_FILE + '.tmp';
const DB_BAK = DB_FILE + '.bak';
fs.mkdirSync(DATA_DIR, { recursive: true });

const conn = new Connection(RPC, 'confirmed');

// ---- storage ---------------------------------------------------------------
// Tolerate an empty/truncated DB (e.g. a crash mid-write): fall back to the last
// good backup, then to an empty store. NEVER throws — a throw here would 500 every
// DB-backed endpoint (node status, staking, inference payment).
function loadDB() {
  const empty = { positions: {}, usedSignatures: {}, nodes: {}, requests: {} };
  for (const f of [DB_FILE, DB_BAK]) {
    try {
      if (!fs.existsSync(f)) continue;
      const raw = fs.readFileSync(f, 'utf8');
      if (!raw.trim()) continue;
      return JSON.parse(raw);
    } catch (e) { console.error(`loadDB: ${f} unreadable (${e.message})`); }
  }
  return empty;
}
// Atomic write: write to a temp file, snapshot the current good copy to .bak, then
// rename into place. A crash can't leave a half-written positions.json behind.
function saveDB(db) {
  fs.writeFileSync(DB_TMP, JSON.stringify(db, null, 2));
  try { if (fs.existsSync(DB_FILE)) fs.copyFileSync(DB_FILE, DB_BAK); } catch { /* best-effort backup */ }
  fs.renameSync(DB_TMP, DB_FILE);
}

const toBase = (whole) => BigInt(Math.round(Number(whole) * 10 ** DECIMALS));
const toWhole = (base) => Number(base) / 10 ** DECIMALS;

// Serialize read-modify-write sections that straddle an await (on-chain payout /
// tx verification). loadDB/saveDB is a lock-free full-file RMW, so two concurrent
// requests could both read the same pending/principal, both await payout, then both
// write — paying out (or crediting a signature) twice. withLock chains same-key
// sections so each runs only after the previous settles. Keyed by owner/requestId;
// the map self-cleans when a key's chain drains, so it stays bounded.
const _lockTail = new Map();
function withLock(key, fn) {
  const k = key || '*';
  const prev = _lockTail.get(k) || Promise.resolve();
  const next = prev.then(() => fn(), () => fn()); // run fn once prev settles (either way)
  const guard = next.then(() => {}, () => {});    // swallow so the chain never wedges
  _lockTail.set(k, guard);
  guard.finally(() => { if (_lockTail.get(k) === guard) _lockTail.delete(k); });
  return next;
}

// Operator token-gate: registering/operating a node requires the owner wallet to
// hold at least this many KVR on-chain (0 disables the gate). Mirrors the hub.
const MIN_OPERATOR_KVR = Number(process.env.LINKCPP_MIN_OPERATOR_KVR || 0);
async function kvrBalance(owner) {
  try {
    const ata = await getAssociatedTokenAddress(MINT, new PublicKey(owner));
    const bal = await conn.getTokenAccountBalance(ata);
    return Number((bal && bal.value && bal.value.uiAmount) || 0);
  } catch { return 0; } // missing ATA / bad address => treat as 0
}

// Accrue rewards up to `nowMs` and mutate the position in place.
function accrue(pos, nowMs) {
  const now = Math.floor(nowMs / 1000);
  const dt = Math.max(0, now - (pos.lastUpdate || now));
  pos.accruedRewards = (pos.accruedRewards || 0) + pos.principal * APR * dt / SECONDS_PER_YEAR;
  pos.lastUpdate = now;
  return pos;
}

function emptyPosition() {
  const now = Math.floor(nowMsSafe() / 1000);
  return { principal: 0, accruedRewards: 0, stakedAt: now, lastUpdate: now };
}

// Date.now is fine in this standalone service (unlike the workflow sandbox).
function nowMsSafe() { return Date.now(); }
function nowSec() { return Math.floor(nowMsSafe() / 1000); }

// A node counts as "online" while its last report is within this window — the
// same threshold /api/node/status uses to label a node online.
const NODE_ONLINE_SEC = 300;
function ownerHasOnlineNode(db, owner) {
  const now = nowSec();
  return Object.values(db.nodes || {}).some(
    (n) => n.owner === owner && n.lastReport != null && (now - n.lastReport) < NODE_ONLINE_SEC,
  );
}

// ---- on-chain verification -------------------------------------------------
async function verifyStakeTransfer(signature, ownerStr, amountWhole) {
  // The client confirms the payment against ITS RPC, but this gateway's RPC can lag
  // (getParsedTransaction trails confirmation + node-to-node propagation). Failing on
  // the first null caused intermittent "transaction not found" 400s (mobile
  // responseError). Poll briefly (~12s) so a freshly-confirmed tx is found. This stays
  // well under the mobile client's 60s request timeout, even with generation after.
  let tx = null;
  for (let attempt = 0; attempt < 8; attempt++) {
    tx = await conn.getParsedTransaction(signature, {
      maxSupportedTransactionVersion: 0, commitment: 'confirmed',
    });
    if (tx) break;
    await new Promise((r) => setTimeout(r, 1500));
  }
  if (!tx) throw new Error('transaction not found (not yet confirmed?)');
  if (tx.meta && tx.meta.err) throw new Error('transaction failed on-chain');

  const keys = tx.transaction.message.accountKeys.map((k) => (k.pubkey ? k.pubkey.toString() : k.toString()));
  const vault = VAULT_ATA.toString();
  const mint = MINT.toString();
  const pre = tx.meta.preTokenBalances || [];
  const post = tx.meta.postTokenBalances || [];
  const balAt = (list) => {
    const b = (list || []).find((x) => x.mint === mint && keys[x.accountIndex] === vault);
    return b ? BigInt(b.uiTokenAmount.amount) : 0n;
  };
  const need = toBase(amountWhole);
  const delta = balAt(post) - balAt(pre);
  if (delta < need) {
    throw new Error(`vault received ${toWhole(delta)} KVR, expected >= ${amountWhole}`);
  }
  // Sender binding: the KVR must have been DEBITED from a token account owned by
  // `ownerStr`. Solana signatures are public, so without this an attacker could
  // replay a victim's vault-funding signature under their OWN wallet and be credited
  // the principal (then unstake it) — direct theft. Empty ownerStr (inference path)
  // skips this; that flow is bound by the private requestId + one-shot signature.
  if (ownerStr) {
    const amtAt = (list, idx) => {
      const b = (list || []).find((x) => x.accountIndex === idx && x.mint === mint);
      return b ? BigInt(b.uiTokenAmount.amount) : 0n;
    };
    const ownedIdx = new Set();
    for (const b of [...pre, ...post]) if (b.mint === mint && b.owner === ownerStr) ownedIdx.add(b.accountIndex);
    let sent = 0n;
    for (const idx of ownedIdx) { const d = amtAt(pre, idx) - amtAt(post, idx); if (d > 0n) sent += d; }
    if (sent < need) {
      throw new Error(`payment was not sent by ${ownerStr} (debited ${toWhole(sent)} KVR, need ${amountWhole})`);
    }
  }
  // Identify the payer (owner debited the most for this mint) so a failed
  // inference can be refunded to them. Best-effort — null if not derivable.
  const idxOwner = new Map();
  for (const b of [...pre, ...post]) if (b.mint === mint && b.owner) idxOwner.set(b.accountIndex, b.owner);
  const debitAt = (idx) => {
    const at = (list) => { const b = (list || []).find((x) => x.accountIndex === idx && x.mint === mint); return b ? BigInt(b.uiTokenAmount.amount) : 0n; };
    return at(pre) - at(post);
  };
  let payer = null, maxDebit = 0n;
  for (const [idx, owner] of idxOwner) { const d = debitAt(idx); if (d > maxDebit) { maxDebit = d; payer = owner; } }
  return { ok: true, payer };
}

// ---- payout ----------------------------------------------------------------
async function payout(ownerStr, amountWhole) {
  const owner = new PublicKey(ownerStr);
  const dest = await getOrCreateAssociatedTokenAccount(conn, ADMIN, MINT, owner);
  const source = await getAssociatedTokenAddress(MINT, ADMIN.publicKey);
  const sig = await transferChecked(
    conn, ADMIN, source, MINT, dest.address, ADMIN, toBase(amountWhole), DECIMALS,
  );
  return sig;
}

// ---- API -------------------------------------------------------------------
const app = express();
// 2 MB body cap: the model now serves up to a 128K-token context, and a prompt
// that large is well over the old 100kb express default (which 413'd ~112 KB
// coding-agent prompts before they reached the hub). Still bounded to keep the
// unauthenticated surface from accepting unbounded payloads.
app.use(express.json({ limit: '2mb' }));

// Permissive CORS so the desktop (Electron renderer / web) client can call the
// service cross-origin. This is a trusted-LAN devnet service; the mobile apps use
// native HTTP (no CORS). Handles the preflight for JSON POSTs.
app.use((req, res, next) => {
  res.header('Access-Control-Allow-Origin', '*');
  res.header('Access-Control-Allow-Headers', 'Content-Type');
  res.header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  if (req.method === 'OPTIONS') return res.sendStatus(204);
  next();
});

app.get('/api/config', (_req, res) => {
  res.json({
    cluster: SPEC.cluster, rpcUrl: RPC, mint: MINT.toString(),
    decimals: DECIMALS, vault: VAULT_ATA.toString(),
    vaultOwner: SPEC.treasury.owner,
    symbol: SPEC.token.symbol, aprPercent: APR * 100,
    rewardPerUnit: REWARD_PER_UNIT,
    perfTiers: PERF_TIERS,
    gatewayBonus: GATEWAY_BONUS,
    hubUptimePerHour: HUB_UPTIME_PER_HOUR,
    gatewayUptimePerHour: GATEWAY_UPTIME_PER_HOUR,
    publicUrl: PUBLIC_URL,
    hostOs: HOST_OS,
    minOperatorKvr: MIN_OPERATOR_KVR,
  });
});

function positionView(owner, pos) {
  return {
    owner,
    principal: pos.principal,
    rewards: pos.accruedRewards,
    total: pos.principal + pos.accruedRewards,
    aprPercent: APR * 100,
    stakedAt: pos.stakedAt,
  };
}

app.get('/api/positions/:owner', (req, res) => {
  const db = loadDB();
  const pos = db.positions[req.params.owner];
  if (!pos) return res.json(positionView(req.params.owner, emptyPosition()));
  accrue(pos, nowMsSafe());
  saveDB(db);
  res.json(positionView(req.params.owner, pos));
});

app.post('/api/stake', async (req, res) => {
  try {
    const { owner, amount, signature } = req.body || {};
    if (!owner || !amount || !signature) return res.status(400).json({ error: 'owner, amount, signature required' });
    if (Number(amount) <= 0) return res.status(400).json({ error: 'amount must be > 0' });
    const view = await withLock(owner, async () => {
      const db = loadDB();
      if (db.usedSignatures[signature]) { const e = new Error('signature already used'); e.status = 409; throw e; }
      await verifyStakeTransfer(signature, owner, amount);
      // Staking is reserved for contributing operators: the owner must have a
      // node that is currently online. This is checked AFTER the transfer is
      // verified but BEFORE the signature is consumed, so if a node is briefly
      // offline the deposit isn't lost — the same call retries and succeeds
      // once the node reports online again.
      if (!ownerHasOnlineNode(db, owner)) {
        const e = new Error('staking requires an online node — register this device as a node and keep it online, then retry');
        e.status = 403; throw e;
      }
      const pos = db.positions[owner] || emptyPosition();
      accrue(pos, nowMsSafe());
      pos.principal += Number(amount);
      db.positions[owner] = pos;
      db.usedSignatures[signature] = { owner, amount: Number(amount), at: Math.floor(nowMsSafe() / 1000) };
      saveDB(db);
      return positionView(owner, pos);
    });
    res.json(view);
  } catch (e) {
    res.status(e.status || 400).json({ error: String(e.message || e) });
  }
});

app.post('/api/unstake', async (req, res) => {
  try {
    const { owner, amount } = req.body || {};
    if (!owner) return res.status(400).json({ error: 'owner required' });
    const out = await withLock(owner, async () => {
      const db = loadDB();
      const pos = db.positions[owner];
      if (!pos || pos.principal <= 0) { const e = new Error('no active stake'); e.status = 400; throw e; }
      accrue(pos, nowMsSafe());

      const principalOut = amount ? Math.min(Number(amount), pos.principal) : pos.principal;
      const frac = pos.principal > 0 ? principalOut / pos.principal : 1;
      const rewardsOut = pos.accruedRewards * frac;
      const total = principalOut + rewardsOut;

      // Concurrency is already prevented by withLock(owner), so pay out FIRST: if the
      // on-chain payout throws (vault/network), we never debit and the stake is intact
      // — safer than debiting first and losing the balance on a failed payout.
      const sig = await payout(owner, total);

      pos.principal -= principalOut;
      pos.accruedRewards -= rewardsOut;
      if (pos.principal <= 1e-9) delete db.positions[owner];
      else db.positions[owner] = pos;
      saveDB(db);
      return {
        signature: sig,
        principalReturned: principalOut,
        rewardsPaid: rewardsOut,
        total,
        position: db.positions[owner] ? positionView(owner, db.positions[owner]) : positionView(owner, emptyPosition()),
      };
    });
    res.json(out);
  } catch (e) {
    res.status(e.status || 400).json({ error: String(e.message || e) });
  }
});

// ---- node operator rewards -------------------------------------------------
// A node operator links their wallet to a node, the hub reports the node's
// contribution (inference telemetry), and the operator claims accrued KVR.

// Constant-time string compare (avoids leaking token length/prefix via timing).
function ctEq(a, b) {
  const ba = Buffer.from(String(a == null ? '' : a));
  const bb = Buffer.from(String(b == null ? '' : b));
  return ba.length === bb.length && crypto.timingSafeEqual(ba, bb);
}
// A reporter trusted to assert REWARD-AFFECTING facts: contribution units, infra
// roles (hostsHub/hostsGateway), and the perf tier. ONLY the M2M service token (used
// by the hub contribution poll) or an authenticated admin qualifies — NEVER an
// anonymous client, even in open LAN mode, because these facts directly mint
// claimable KVR. The legit sources are in-process (reportInfraNodes) or the
// service-token hub poll (reportInferenceContribution); a wallet linking its own
// node still registers freely, it just can't self-assert rewards.
function trustedReporter(req) {
  if (HUB_SERVICE_TOKEN && ctEq(req.headers['x-linkcpp-service-token'], HUB_SERVICE_TOKEN)) return true;
  return !!adminWallet(req);
}

// OS categories the UI displays: macos | ios | android | windows | linux | unknown
const OS_CATEGORIES = ['macos', 'ios', 'android', 'windows', 'linux', 'unknown'];
const normOs = (v) => (OS_CATEGORIES.includes(String(v || '').toLowerCase()) ? String(v).toLowerCase() : 'unknown');
const normAccel = (v) => (['cpu', 'gpu', 'npu'].includes(String(v || '').toLowerCase()) ? String(v).toLowerCase() : 'cpu');

app.post('/api/node/register', async (req, res) => {
  const { nodeId, owner, os, deviceKind, accelerator, label, perfScore, backend, mode, hostsGateway, hostsHub } = req.body || {};
  if (!nodeId || !owner) return res.status(400).json({ error: 'nodeId, owner required' });
  if (MIN_OPERATOR_KVR > 0) {
    const bal = await kvrBalance(owner);
    if (bal < MIN_OPERATOR_KVR) {
      return res.status(403).json({ error: `operator wallet needs >= ${MIN_OPERATOR_KVR} KVR to run a node (has ${bal})` });
    }
  }
  const db = loadDB();
  db.nodes = db.nodes || {};
  const n = db.nodes[nodeId] || {
    owner, contributedUnits: 0, effectiveUnits: 0, pendingRewards: 0, claimedTotal: 0,
    hostsGateway: false, hostsHub: false, uptimeRewards: 0,
    registeredAt: nowSec(), lastReport: null, lastUptimeAt: nowSec(),
  };
  n.owner = owner;
  n.os = normOs(os != null ? os : n.os);
  n.deviceKind = deviceKind || n.deviceKind || 'unknown';
  n.accelerator = normAccel(accelerator != null ? accelerator : n.accelerator);
  n.label = label || n.label || nodeId;
  // Reward-affecting fields (perf tier, infra roles) are honored ONLY from a trusted
  // reporter — otherwise a node could self-assert S-tier / hub-host and mint rewards.
  const trusted = trustedReporter(req);
  // Performance re-scoring: node advertises its measured decode throughput.
  if (trusted && perfScore != null) n.perfScore = Number(perfScore);
  if (backend != null) n.backend = String(backend);
  if (mode != null) n.mode = String(mode);
  accrueUptime(n, nowSec()); // credit elapsed infra uptime under the CURRENT roles first
  if (trusted && hostsGateway != null) n.hostsGateway = !!hostsGateway;
  if (trusted && hostsHub != null) n.hostsHub = !!hostsHub;
  const pt = perfTier(n.perfScore);
  n.tier = pt.tier;
  n.perfMultiplier = pt.mult;
  db.nodes[nodeId] = n;
  saveDB(db);
  res.json({
    nodeId, owner: n.owner, os: n.os, deviceKind: n.deviceKind, accelerator: n.accelerator,
    label: n.label, perfScore: n.perfScore || 0, backend: n.backend || null, mode: n.mode || null,
    tier: n.tier, perfMultiplier: n.perfMultiplier,
    hostsGateway: !!n.hostsGateway, gatewayBonus: n.hostsGateway ? GATEWAY_BONUS : 1,
    hostsHub: !!n.hostsHub, hubUptimePerHour: HUB_UPTIME_PER_HOUR, gatewayUptimePerHour: GATEWAY_UPTIME_PER_HOUR,
    uptimeRewards: n.uptimeRewards || 0, pendingRewards: n.pendingRewards || 0,
  });
});

// Liveness heartbeat from a connected device.
app.post('/api/node/heartbeat', (req, res) => {
  const { nodeId, hostsGateway, hostsHub } = req.body || {};
  if (!nodeId) return res.status(400).json({ error: 'nodeId required' });
  const db = loadDB();
  const n = (db.nodes || {})[nodeId];
  if (!n) return res.status(404).json({ error: 'node not registered' });
  const now = nowSec();
  accrueUptime(n, now); // credit uptime for the interval that just elapsed, then update roles
  // Infra roles drive uptime KVR, so only a trusted reporter may change them here.
  if (trustedReporter(req)) {
    if (hostsGateway != null) n.hostsGateway = !!hostsGateway; // reflect current gateway-host state
    if (hostsHub != null) n.hostsHub = !!hostsHub;             // reflect current hub-host state
  }
  n.lastReport = now;
  db.nodes[nodeId] = n;
  saveDB(db);
  res.json({
    nodeId, lastReport: n.lastReport,
    hostsGateway: !!n.hostsGateway, hostsHub: !!n.hostsHub,
    uptimeRewards: n.uptimeRewards || 0, pendingRewards: n.pendingRewards || 0,
  });
});

// Remove (unregister) a node. Only the owning account may remove it.
app.post('/api/node/remove', (req, res) => {
  const { nodeId, owner } = req.body || {};
  if (!nodeId || !owner) return res.status(400).json({ error: 'nodeId, owner required' });
  const db = loadDB();
  const n = (db.nodes || {})[nodeId];
  if (!n) return res.status(404).json({ error: 'node not found' });
  if (n.owner !== owner) return res.status(403).json({ error: 'not owner of this node' });
  delete db.nodes[nodeId];
  saveDB(db);
  res.json({ removed: nodeId });
});

// Called by the linkcpp hub (or a reporter) to credit a node's contribution.
app.post('/api/node/contribution', (req, res) => {
  // Contribution units mint claimable KVR, so only the hub/reporter (service token)
  // or an admin may credit them — never an anonymous client.
  if (!trustedReporter(req)) return res.status(401).json({ error: 'service token or admin required' });
  const { nodeId, units } = req.body || {};
  if (!nodeId || units == null) return res.status(400).json({ error: 'nodeId, units required' });
  const db = loadDB();
  db.nodes = db.nodes || {};
  const n = db.nodes[nodeId];
  if (!n) return res.status(404).json({ error: 'node not registered' });
  accrueUptime(n, nowSec()); // infra uptime accrues alongside inference (summed)
  // Re-score raw work by the node's performance multiplier, then apply the gateway-host
  // bonus if this node also keeps the gateway online, before crediting reward.
  const mult = n.perfMultiplier || perfTier(n.perfScore).mult;
  const gwBonus = n.hostsGateway ? GATEWAY_BONUS : 1;
  const raw = Number(units);
  const eff = raw * mult * gwBonus;
  n.contributedUnits += raw;
  n.effectiveUnits = (n.effectiveUnits || 0) + eff;
  n.pendingRewards += eff * REWARD_PER_UNIT;
  n.lastReport = nowSec();
  db.nodes[nodeId] = n;
  saveDB(db);
  res.json({
    nodeId, contributedUnits: n.contributedUnits, effectiveUnits: n.effectiveUnits,
    perfMultiplier: mult, tier: n.tier || perfTier(n.perfScore).tier,
    hostsGateway: !!n.hostsGateway, gatewayBonus: gwBonus,
    pendingRewards: n.pendingRewards, lastReport: n.lastReport,
  });
});

function operatorRewards(db, owner) {
  const nodes = Object.entries(db.nodes || {})
    .filter(([, n]) => n.owner === owner)
    .map(([nodeId, n]) => ({ nodeId, contributedUnits: n.contributedUnits, pendingRewards: n.pendingRewards }));
  const pending = nodes.reduce((s, n) => s + n.pendingRewards, 0);
  return { owner, pending, rewardPerUnit: REWARD_PER_UNIT, nodes };
}

app.get('/api/node/rewards/:owner', (req, res) => {
  const db = loadDB();
  res.json(operatorRewards(db, req.params.owner));
});

// Operational monitoring for an operator's nodes.
app.get('/api/node/status/:owner', (req, res) => {
  const db = loadDB();
  const now = nowSec();
  const nodes = Object.entries(db.nodes || {})
    .filter(([, n]) => n.owner === req.params.owner)
    .map(([nodeId, n]) => {
      const last = n.lastReport;
      let status = 'offline';
      if (last == null) status = 'registered';
      else if (now - last < 300) status = 'online';
      else if (now - last < 3600) status = 'idle';
      const pt = perfTier(n.perfScore);
      return {
        nodeId, status,
        os: n.os || 'unknown',
        deviceKind: n.deviceKind || 'unknown',
        accelerator: n.accelerator || 'cpu',
        label: n.label || nodeId,
        perfScore: n.perfScore || 0,
        backend: n.backend || null,
        mode: n.mode || null,
        tier: n.tier || pt.tier,
        perfMultiplier: n.perfMultiplier || pt.mult,
        hostsGateway: !!n.hostsGateway,
        gatewayBonus: n.hostsGateway ? GATEWAY_BONUS : 1,
        hostsHub: !!n.hostsHub,
        uptimeRewards: n.uptimeRewards || 0,
        contributedUnits: n.contributedUnits,
        effectiveUnits: n.effectiveUnits || 0,
        pendingRewards: n.pendingRewards,
        claimedTotal: n.claimedTotal || 0,
        registeredAt: n.registeredAt || null,
        lastReport: last,
      };
    });
  const totals = {
    nodes: nodes.length,
    online: nodes.filter((n) => n.status === 'online').length,
    contributedUnits: nodes.reduce((s, n) => s + n.contributedUnits, 0),
    effectiveUnits: nodes.reduce((s, n) => s + n.effectiveUnits, 0),
    pending: nodes.reduce((s, n) => s + n.pendingRewards, 0),
    claimedTotal: nodes.reduce((s, n) => s + n.claimedTotal, 0),
    lifetimeRewards: nodes.reduce((s, n) => s + n.pendingRewards + n.claimedTotal, 0),
  };
  res.json({ owner: req.params.owner, totals, nodes });
});

// Whole-network node view (all owners) for the desktop world map. Owner is masked.
app.get('/api/node/all', (_req, res) => {
  const db = loadDB();
  const now = nowSec();
  const nodes = Object.entries(db.nodes || {}).map(([nodeId, n]) => {
    const last = n.lastReport;
    let status = 'offline';
    if (last == null) status = 'registered';
    else if (now - last < 300) status = 'online';
    else if (now - last < 3600) status = 'idle';
    const pt = perfTier(n.perfScore);
    const owner = String(n.owner || '');
    return {
      nodeId, status,
      os: n.os || 'unknown',
      deviceKind: n.deviceKind || 'unknown',
      accelerator: n.accelerator || 'cpu',
      backend: n.backend || null,
      label: n.label || nodeId,
      perfScore: n.perfScore || 0,
      tier: n.tier || pt.tier,
      perfMultiplier: n.perfMultiplier || pt.mult,
      hostsGateway: !!n.hostsGateway,
      hostsHub: !!n.hostsHub,
      effectiveUnits: n.effectiveUnits || 0,
      uptimeRewards: n.uptimeRewards || 0,
      pendingRewards: n.pendingRewards || 0,
      ownerShort: owner ? `${owner.slice(0, 4)}…${owner.slice(-4)}` : '',
    };
  });
  const byTier = {}; const byOs = {};
  for (const n of nodes) { byTier[n.tier] = (byTier[n.tier] || 0) + 1; byOs[n.os] = (byOs[n.os] || 0) + 1; }
  res.json({
    totals: {
      nodes: nodes.length,
      online: nodes.filter((n) => n.status === 'online').length,
      owners: new Set(Object.values(db.nodes || {}).map((n) => n.owner)).size,
      effectiveUnits: nodes.reduce((s, n) => s + n.effectiveUnits, 0),
      byTier, byOs,
    },
    nodes,
  });
});

app.post('/api/node/claim', async (req, res) => {
  try {
    const { owner } = req.body || {};
    if (!owner) return res.status(400).json({ error: 'owner required' });
    const out = await withLock(owner, async () => {
      const db = loadDB();
      const summary = operatorRewards(db, owner);
      if (summary.pending <= 1e-9) { const e = new Error('no rewards to claim'); e.status = 400; throw e; }

      // Serialized by withLock(owner): pay out first, then zero the claimed rewards.
      // Without the lock two concurrent claims both read `pending` and both pay it.
      const sig = await payout(owner, summary.pending);

      for (const [nodeId, n] of Object.entries(db.nodes || {})) {
        if (n.owner === owner) {
          n.claimedTotal = (n.claimedTotal || 0) + n.pendingRewards;
          n.pendingRewards = 0;
          db.nodes[nodeId] = n;
        }
      }
      saveDB(db);
      return { signature: sig, claimed: summary.pending, rewards: operatorRewards(db, owner) };
    });
    res.json(out);
  } catch (e) {
    res.status(e.status || 400).json({ error: String(e.message || e) });
  }
});

// ---- devnet KVR faucet -----------------------------------------------------
// Dispenses a fixed amount of devnet KVR to any address, rate-limited per
// address. Devnet only — KVR has no real value. Powers the /docs/api page's
// "request test KVR" widget so developers can fund a wallet and try the
// pay-per-inference flow. Pays from the treasury (ADMIN), same as claim/payout.
const FAUCET_AMOUNT = Number(process.env.KVR_FAUCET_AMOUNT || 100);
const FAUCET_COOLDOWN_MS = Number(process.env.KVR_FAUCET_COOLDOWN_SEC || 86400) * 1000;

app.post('/api/faucet', async (req, res) => {
  try {
    const { address } = req.body || {};
    if (!address || typeof address !== 'string') return res.status(400).json({ error: 'address required' });
    let pubkey;
    try { pubkey = new PublicKey(address.trim()); } catch { return res.status(400).json({ error: 'invalid Solana address' }); }
    const addr = pubkey.toBase58();

    // Serialize per-address so two concurrent requests can't both pass the
    // cooldown check and double-dispense.
    const out = await withLock(`faucet:${addr}`, async () => {
      const db = loadDB();
      db.faucet = db.faucet || {};
      const now = Date.now();
      const last = db.faucet[addr] || 0;
      const wait = last + FAUCET_COOLDOWN_MS - now;
      if (wait > 0) {
        const e = new Error(`rate limited — already funded, try again in ~${Math.ceil(wait / 3600000)}h`);
        e.status = 429; throw e;
      }
      const sig = await payout(addr, FAUCET_AMOUNT);
      db.faucet[addr] = now;
      saveDB(db);
      return { address: addr, amount: FAUCET_AMOUNT, symbol: SPEC.token.symbol, signature: sig, cluster: SPEC.cluster };
    });
    res.json(out);
  } catch (e) {
    res.status(e.status || 400).json({ error: String(e.message || e) });
  }
});

// ---- Phase 2: inference usage payment (Solana Pay style) -------------------
// The wallet pays KVR to the treasury for an inference request; this gateway
// verifies the on-chain payment and returns the result. Inference itself is a
// devnet mock — in production the linkcpp hub serves the model after payment.

const MODELS = [
  { id: 'linkcpp-fast', name: 'linkcpp Fast', basePrice: 0.5, perToken: 0.01, estOut: 160 },
  { id: 'linkcpp-pro', name: 'linkcpp Pro', basePrice: 2.0, perToken: 0.03, estOut: 320 },
];
const round6 = (v) => Math.round(v * 1e6) / 1e6;
// ~4 chars/token heuristic (matches typical BPE for mixed text).
function estimateTokens(text) { return Math.max(1, Math.ceil((text || '').length / 4)); }
/** Estimated quote (예상 견적): prompt tokens are exact, completion is a model nominal. */
function quoteFor(m, prompt) {
  const estPromptTokens = estimateTokens(prompt);
  const estCompletionTokens = m.estOut;
  const estTotalTokens = estPromptTokens + estCompletionTokens;
  return { priceToken: round6(m.basePrice + estTotalTokens * m.perToken), estPromptTokens, estCompletionTokens, estTotalTokens };
}
/** Actual usage after generation: real prompt + completion token counts. */
function usageFor(m, prompt, result) {
  const promptTokens = estimateTokens(prompt);
  const completionTokens = estimateTokens(result);
  const totalTokens = promptTokens + completionTokens;
  return { promptTokens, completionTokens, totalTokens, costToken: round6(m.basePrice + totalTokens * m.perToken) };
}
function mockInfer(modelId, prompt) {
  const m = MODELS.find((x) => x.id === modelId) || MODELS[0];
  // Markdown so the chat client can render it richly. Devnet mock.
  return `### ${m.name}\n\n`
    + `> ${String(prompt).slice(0, 160)}\n\n`
    + `요청을 접수해 처리했습니다. 주요 포인트는 다음과 같습니다:\n\n`
    + `- **온체인 결제 확인됨** — KVR 전송이 트레저리에서 검증되었습니다.\n`
    + `- **분산 추론 실행** — linkcpp 허브가 노드에 작업을 분배합니다.\n`
    + `- **사용량 정산** — 실제 사용 토큰 기준으로 청구됩니다.\n\n`
    + '```python\n# linkcpp inference (mock)\nresult = linkcpp.run(model="' + m.id + '", prompt=...)\n```\n\n'
    + '*현재는 결제·정산 흐름 검증용 목업입니다. 실제 응답은 결제 확인 후 linkcpp 허브가 생성합니다.*';
}

// --- real linkcpp hub bridge (multi-hub aggregation) ------------------------
// Inference hubs advertise their served models through this gateway. The
// statically-configured LINKCPP_HUB_URL is always included; other hubs (incl.
// on different networks) join by POSTing /api/pay/hub/register periodically.
// The model list aggregates every reachable hub; each model routes inference to
// its own hub. Falls back to the static demo catalog when no hub is reachable.
const LINKCPP_HUB_URL = (process.env.LINKCPP_HUB_URL || '').replace(/\/+$/, '');
// Shared secret so the gateway can reach a hub that has SIWS auth enabled (M2M).
const HUB_SERVICE_TOKEN = (process.env.LINKCPP_HUB_SERVICE_TOKEN || '').trim();
const hubHeaders = (extra) => Object.assign(HUB_SERVICE_TOKEN ? { 'X-Linkcpp-Service-Token': HUB_SERVICE_TOKEN } : {}, extra || {});
// One-time bootstrap seed only. Runtime pricing lives in db.pricing (DB), which
// only the genesis wallet may change (SIWS + fresh TOTP, from the desktop app).
const DEFAULT_PRICING = { basePrice: 0.5, perToken: 0.01, estOut: 256 };
const HUB_TTL_MS = Number(process.env.LINKCPP_HUB_TTL_MS || 90000);

// ---- model pricing store (genesis-governed, DB-backed) ---------------------
// The genesis (governance) wallet is the only principal allowed to change
// per-model inference pricing. Not merely an admin allowlist member: pricing
// is an economic parameter, so it is locked to genesis + fresh 2FA.
// Kept SEPARATE from the treasury key (SPEC.governance.wallet) so the money
// key never signs interactive SIWS logins; specs that predate the field fall
// back to the treasury owner. Deliberately file-pinned — no env override, so
// repointing governance requires a spec change + redeploy, not an env tweak.
const GENESIS_WALLET = (SPEC.governance && SPEC.governance.wallet) || SPEC.treasury.owner;
function loadPricing(db) {
  if (!db.pricing || typeof db.pricing !== 'object') {
    db.pricing = { default: { ...DEFAULT_PRICING }, perModel: {}, updatedAt: 0, updatedBy: null };
  }
  db.pricing.default = { ...DEFAULT_PRICING, ...(db.pricing.default || {}) };
  db.pricing.perModel = db.pricing.perModel || {};
  return db.pricing;
}
// Effective pricing for a model id: perModel override merged over default.
function pricingFrom(pricing, modelId) {
  const eff = { ...pricing.default, ...((modelId && pricing.perModel[modelId]) || {}) };
  return { basePrice: Number(eff.basePrice), perToken: Number(eff.perToken), estOut: Number(eff.estOut) };
}
function pricingFor(modelId) { return pricingFrom(loadPricing(loadDB()), modelId); }

// ---- pricing propagation --------------------------------------------------
// The genesis wallet is connected locally to ONE gateway (the "source") — that
// gateway is the single source of truth for pricing. Because that machine may be
// a laptop behind NAT (not reachable by remote gateways), propagation is push +
// pull chained through a public relay:
//
//   source gateway  --push-->  public relay gateway  <--poll--  follower gateways
//   (genesis wallet)           (e.g. gate.kvasir-ai.net)        (other remote hubs)
//
// Role is chosen by env; the SOURCE is the default (the genesis wallet's local
// gateway — no propagation env at all):
//   KVR_GENESIS_GATEWAY_URL public URL to poll for pricing   → FOLLOWER
//   KVR_PRICING_PUSH_SECRET shared secret; accepts pushes    → RELAY
//   (neither set)                                            → SOURCE (genesis)
// The source's relay list lives in db.pricingRelays and is edited from the
// desktop pricing page (genesis wallet + TOTP) — NOT from env — so the operator
// can add/remove relays like MI250 with genesis authentication. Only the source
// accepts admin pricing/relay writes; relays and followers mirror it.
const GENESIS_GATEWAY_URL = (process.env.KVR_GENESIS_GATEWAY_URL || '').replace(/\/+$/, '');
const PRICING_PUSH_SECRET = process.env.KVR_PRICING_PUSH_SECRET || '';
const IS_FOLLOWER = !!GENESIS_GATEWAY_URL;
const RECEIVES_PUSH = !!PRICING_PUSH_SECRET;
const IS_SOURCE = !IS_FOLLOWER && !RECEIVES_PUSH;
const IS_WRITE_LOCKED = !IS_SOURCE;
const PRICING_SYNC_MS = Number(process.env.KVR_PRICING_SYNC_MS || 60000);

// Relay list (source only) — [{ url, secret, label, addedAt, addedBy }].
function loadRelays(db) {
  if (!Array.isArray(db.pricingRelays)) db.pricingRelays = [];
  return db.pricingRelays;
}

// FOLLOWER: poll the public relay and adopt newer pricing.
async function syncPricingFromGenesis() {
  if (!GENESIS_GATEWAY_URL) return;
  try {
    const r = await fetch(`${GENESIS_GATEWAY_URL}/api/pay/pricing`, { signal: AbortSignal.timeout(5000) });
    if (!r.ok) return;
    const remote = await r.json();
    if (!remote || typeof remote !== 'object' || !remote.default) return;
    const db = loadDB();
    const local = loadPricing(db);
    if (Number(remote.updatedAt || 0) <= Number(local.updatedAt || 0)) return; // already current
    local.default = { ...local.default, ...remote.default };
    local.perModel = (remote.perModel && typeof remote.perModel === 'object') ? remote.perModel : {};
    local.updatedAt = Number(remote.updatedAt) || Math.floor(Date.now() / 1000);
    local.updatedBy = `genesis-sync:${GENESIS_GATEWAY_URL}`;
    saveDB(db);
    console.log(`pricing synced from ${GENESIS_GATEWAY_URL} (updatedAt=${local.updatedAt})`);
  } catch { /* best-effort; retry next tick */ }
}
if (GENESIS_GATEWAY_URL) { syncPricingFromGenesis(); setInterval(syncPricingFromGenesis, PRICING_SYNC_MS); }

// SOURCE: push the current pricing to every relay configured in db.pricingRelays.
// Best-effort and idempotent (relays adopt only when updatedAt is newer), re-run
// on a timer so a relay that was offline during the change heals on the next tick.
async function pushPricingToRelays() {
  if (!IS_SOURCE) return;
  const db = loadDB();
  const relays = loadRelays(db);
  if (!relays.length) return;
  const p = loadPricing(db);
  const body = JSON.stringify({ default: p.default, perModel: p.perModel, updatedAt: p.updatedAt || 0, genesis: GENESIS_WALLET });
  await Promise.all(relays.map(async (relay) => {
    try {
      const r = await fetch(`${relay.url}/api/pay/pricing/push`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-pricing-push-key': relay.secret || '' },
        body, signal: AbortSignal.timeout(5000),
      });
      if (!r.ok) console.warn(`pricing push to ${relay.url} → HTTP ${r.status}`);
    } catch (e) { console.warn(`pricing push to ${relay.url} failed: ${e.message}`); }
  }));
}
if (IS_SOURCE) { pushPricingToRelays(); setInterval(pushPricingToRelays, PRICING_SYNC_MS); }

const hubRegistry = new Map(); // hubUrl -> { hubUrl, name, expiresAt }
function hubKey(url) { return crypto.createHash('sha1').update(url).digest('hex').slice(0, 8); }
function activeHubUrls() {
  const urls = [];
  if (LINKCPP_HUB_URL) urls.push(LINKCPP_HUB_URL);
  const now = Date.now();
  for (const [url, h] of hubRegistry) {
    if (h.expiresAt > now) { if (!urls.includes(url)) urls.push(url); }
    else hubRegistry.delete(url); // expired heartbeat
  }
  return urls;
}

async function fetchModelsFrom(hubUrl) {
  try {
    const r = await fetch(`${hubUrl}/api/controllers`, { headers: hubHeaders(), signal: AbortSignal.timeout(5000) });
    if (!r.ok) return [];
    const d = await r.json();
    const ctrls = Array.isArray(d) ? d : (d.controllers || []);
    const pricing = loadPricing(loadDB());
    return ctrls
      .filter((c) => c && c.runtime_loaded && (c.serving || c.active_model))
      .map((c) => {
        const id = `${hubKey(hubUrl)}:${c.id}`; // hub-qualified so ids never collide
        return {
          id,
          name: String(c.active_model || c.serving).replace(/\.gguf$/i, ''),
          hubUrl, cid: c.id, hubModel: c.active_model || c.serving,
          ...pricingFrom(pricing, id), // genesis-governed per-model pricing (DB)
        };
      });
  } catch { return []; }
}

// Aggregate served models across every reachable hub (bootstrap + registered).
async function fetchHubModels() {
  const urls = activeHubUrls();
  if (!urls.length) return null;
  const lists = await Promise.all(urls.map(fetchModelsFrom));
  const models = lists.flat();
  return models.length ? models : null;
}

// Active model pool: only the real models the swarm is actually serving. When no
// hub is serving anything we return an empty pool rather than the demo catalog —
// showing non-functional "linkcpp Fast/Pro" placeholders in the production app is
// misleading (they route to a mock, not real inference). The demo MODELS constant
// is retained only for the legacy mock path; it is never surfaced as available.
async function resolveModels() {
  const real = await fetchHubModels();
  return (real && real.length) ? real : [];
}

// ---- infrastructure operator rewards (hub/gateway have no wallet) ----------
// A hub (and a standalone gateway) is headless — it has no wallet UI, so there's
// no way for it to earn or for the operator to SEE it. The operator designates the
// wallet that owns these roles via env; the gateway then auto-registers + heartbeats
// a node on their behalf. Result: the hub's uptime reward accrues to that wallet AND
// the node shows up in that wallet's node monitor. Empty owner => role not reported.
const INFRA_OWNER = (process.env.KVR_INFRA_OWNER || '').trim();
const GATEWAY_OWNER = (process.env.KVR_GATEWAY_OWNER || INFRA_OWNER).trim();
const HUB_OWNER = (process.env.KVR_HUB_OWNER || INFRA_OWNER).trim();
const INFRA_REPORT_MS = Number(process.env.KVR_INFRA_REPORT_MS || 30000);

// Register-or-heartbeat an infra node in-process (same uptime accrual as the HTTP paths).
function upsertInfraNode(nodeId, owner, roles, label, deviceKind) {
  if (!owner) return;
  const db = loadDB();
  db.nodes = db.nodes || {};
  const now = nowSec();
  const n = db.nodes[nodeId] || {
    owner, contributedUnits: 0, effectiveUnits: 0, pendingRewards: 0, claimedTotal: 0,
    hostsGateway: false, hostsHub: false, uptimeRewards: 0,
    registeredAt: now, lastReport: null, lastUptimeAt: now,
  };
  n.owner = owner;
  n.deviceKind = deviceKind;
  n.label = label;
  n.os = n.os || HOST_OS;
  n.accelerator = n.accelerator || 'cpu';
  accrueUptime(n, now); // credit elapsed uptime before refreshing role flags
  n.hostsGateway = !!roles.hostsGateway;
  n.hostsHub = !!roles.hostsHub;
  n.tier = n.tier || perfTier(0).tier;
  n.perfMultiplier = n.perfMultiplier || perfTier(0).mult;
  n.lastReport = now;
  db.nodes[nodeId] = n;
  saveDB(db);
}

// Periodic reporter: this gateway host, plus each reachable hub (only credited while
// the hub actually answers, so a downed hub stops accruing).
async function reportInfraNodes() {
  try {
    if (GATEWAY_OWNER) upsertInfraNode('infra-gateway', GATEWAY_OWNER, { hostsGateway: true }, 'Gateway (this host)', 'gateway');
    if (LINKCPP_HUB_URL) {
      // The hub owner is set by the operator in the HUB UI (advertised at /api/runtime).
      // That takes priority; KVR_HUB_OWNER is only a fallback for older hubs.
      let owner = HUB_OWNER, up = false;
      try {
        const r = await fetch(`${LINKCPP_HUB_URL}/api/runtime`, { headers: hubHeaders(), signal: AbortSignal.timeout(4000) });
        up = r.ok;
        if (r.ok) { const d = await r.json().catch(() => null); if (d && d.operator_wallet) owner = String(d.operator_wallet); }
      } catch { up = false; }
      if (up && owner) upsertInfraNode(`infra-hub-${hubKey(LINKCPP_HUB_URL)}`, owner, { hostsHub: true }, `Hub ${LINKCPP_HUB_URL}`, 'hub');
    }
  } catch { /* best-effort */ }
}
if (GATEWAY_OWNER || HUB_OWNER || LINKCPP_HUB_URL) { reportInfraNodes(); setInterval(reportInfraNodes, INFRA_REPORT_MS); }

// Register (on the operator's behalf) a hub inference node and credit the DELTA of
// its cumulative contribution units since the last poll. Monotonic + restart-safe:
// if the hub's cumulative decreases (hub restarted, counters reset), we rebaseline
// instead of re-crediting from zero.
function upsertInferenceNode(nodeId, owner, label, info) {
  const db = loadDB();
  db.nodes = db.nodes || {};
  const now = nowSec();
  const n = db.nodes[nodeId] || {
    owner, contributedUnits: 0, effectiveUnits: 0, pendingRewards: 0, claimedTotal: 0,
    hostsGateway: false, hostsHub: false, uptimeRewards: 0,
    creditedUnits: 0, registeredAt: now, lastReport: null, lastUptimeAt: now,
  };
  n.owner = owner;
  n.label = label || n.label || nodeId;
  n.deviceKind = info.deviceKind || n.deviceKind || 'node';
  n.os = info.os || n.os || HOST_OS;
  n.accelerator = info.accelerator || n.accelerator || 'gpu';
  if (info.backend != null) n.backend = String(info.backend);
  if (info.perfScore != null) { n.perfScore = Number(info.perfScore); const pt = perfTier(n.perfScore); n.tier = pt.tier; n.perfMultiplier = pt.mult; }
  else { n.tier = n.tier || perfTier(0).tier; n.perfMultiplier = n.perfMultiplier || perfTier(0).mult; }
  const cumulative = Number(info.units || 0);
  let credited = Number(n.creditedUnits || 0);
  if (cumulative < credited) credited = 0; // hub counters reset -> rebaseline
  const delta = Math.max(0, cumulative - credited);
  if (delta > 0) {
    const mult = n.perfMultiplier || perfTier(n.perfScore).mult;
    const gwBonus = n.hostsGateway ? GATEWAY_BONUS : 1;
    const eff = delta * mult * gwBonus;
    n.contributedUnits = (n.contributedUnits || 0) + delta;
    n.effectiveUnits = (n.effectiveUnits || 0) + eff;
    n.pendingRewards = (n.pendingRewards || 0) + eff * REWARD_PER_UNIT;
    n.lastReport = now;
  }
  n.creditedUnits = cumulative;
  db.nodes[nodeId] = n;
  saveDB(db);
}

// Poll the hub for per-node inference contribution and credit each participating node
// (the nodes that actually ran the model). Complements the infra uptime reporter.
async function reportInferenceContribution() {
  if (!LINKCPP_HUB_URL) return;
  try {
    const r = await fetch(`${LINKCPP_HUB_URL}/api/contributions`, { headers: hubHeaders(), signal: AbortSignal.timeout(4000) });
    if (!r.ok) return;
    const d = await r.json().catch(() => null);
    for (const c of ((d && d.contributions) || [])) {
      if (!c.node_id || !c.owner) continue;
      upsertInferenceNode(`infer-${hubKey(LINKCPP_HUB_URL)}-${c.node_id}`, String(c.owner), c.node_name || c.node_id, {
        units: c.units, backend: c.backend, os: c.os,
        accelerator: c.accelerator || 'gpu', deviceKind: c.device_kind || 'node', perfScore: c.perf_tps,
      });
    }
  } catch { /* best-effort */ }
}
if (LINKCPP_HUB_URL) { reportInferenceContribution(); setInterval(reportInferenceContribution, INFRA_REPORT_MS); }

// Run a real completion on a specific hub's OpenAI-compatible gateway.
async function hubInfer(hubUrl, cid, hubModel, prompt) {
  const r = await fetch(`${hubUrl}/c/${cid}/v1/chat/completions`, {
    method: 'POST',
    headers: hubHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({
      model: hubModel,
      messages: [{ role: 'user', content: prompt }],
      // Reasoning models (Qwen3.x etc.) emit a hidden "thinking" pass into
      // reasoning_content before message.content. With thinking ON, the token budget
      // can be fully consumed by thinking, leaving content EMPTY (blank/failed
      // response, yet KVR is charged). enable_thinking=false is the real fix
      // (verified: content filled, finish=stop). max_tokens is headroom for a
      // complete answer; kept at 1024 (not higher) because the mobile client times
      // out at 60s and generation runs ~15 tok/s, so a much larger cap would let a
      // long answer time out — reintroducing the failure via a different path.
      max_tokens: 1024,
      temperature: 0.7,
      chat_template_kwargs: { enable_thinking: false },
    }),
    // Up to 3 minutes: very large / paging-backed models (a 428B MoE served from
    // disk runs ~1 tok/s) need well past 60s for a full answer. Matched by the
    // mobile client's request timeout.
    signal: AbortSignal.timeout(180000),
  });
  if (!r.ok) throw new Error(`hub inference failed (${r.status})`);
  const d = await r.json();
  const msg = (d && d.choices && d.choices[0] && d.choices[0].message) || {};
  const content = msg.content || msg.reasoning_content || '';
  // An empty answer must be an ERROR, not a 200 "success" — otherwise the user is
  // charged KVR for a blank reply. (Should not happen with thinking disabled.)
  if (!content.trim()) throw new Error('hub returned empty content (thinking overflow?)');
  return { content, usage: (d && d.usage) || null };
}

// A hub advertises itself (and refreshes its TTL) so its served models appear in
// the aggregated catalog. Gated when admin auth is on: either an admin session or
// the shared M2M service token (so external hubs self-register unattended) is
// required — otherwise anyone could poison the catalog with arbitrary hub URLs.
function adminOrServiceToken(req) {
  if (!gwauth.authEnabled()) return true; // open in trusted-LAN mode (no allowlist)
  if (HUB_SERVICE_TOKEN && req.headers['x-linkcpp-service-token'] === HUB_SERVICE_TOKEN) return true;
  return !!adminWallet(req);
}
app.post('/api/pay/hub/register', (req, res) => {
  if (!adminOrServiceToken(req)) return res.status(401).json({ error: 'admin or service token required' });
  const { hubUrl, name } = req.body || {};
  if (!hubUrl || !/^https?:\/\//i.test(String(hubUrl))) return res.status(400).json({ error: 'hubUrl (http/https) required' });
  const url = String(hubUrl).replace(/\/+$/, '');
  hubRegistry.set(url, { hubUrl: url, name: name || '', expiresAt: Date.now() + HUB_TTL_MS });
  res.json({ ok: true, hubUrl: url, ttlMs: HUB_TTL_MS, hubs: activeHubUrls().length });
});

app.get('/api/pay/hubs', (_req, res) => {
  res.json({ ttlMs: HUB_TTL_MS, hubs: activeHubUrls() });
});

// Public read of the governed pricing so follower gateways can adopt it (pricing
// is not secret — it is already derivable from /api/pay/quote). Followers poll
// this on the genesis gateway; see syncPricingFromGenesis.
app.get('/api/pay/pricing', (_req, res) => {
  const p = loadPricing(loadDB());
  res.json({ default: p.default, perModel: p.perModel, updatedAt: p.updatedAt || 0, genesis: GENESIS_WALLET });
});

// RELAY: receive a pricing push from the genesis source gateway. Authenticated
// by a shared secret (KVR_PRICING_PUSH_SECRET) rather than a desktop session, so
// the NAT-bound source can reach a public relay. Adopts only a newer, same-
// genesis payload; idempotent, so the source can safely re-push on a timer.
app.post('/api/pay/pricing/push', (req, res) => {
  if (!RECEIVES_PUSH) return res.status(404).json({ error: 'this gateway does not accept pricing pushes' });
  const key = req.get('x-pricing-push-key') || '';
  if (key !== PRICING_PUSH_SECRET) return res.status(401).json({ error: 'bad push key' });
  const b = req.body || {};
  if (!b.default || typeof b.default !== 'object') return res.status(400).json({ error: 'missing default pricing' });
  if (b.genesis && b.genesis !== GENESIS_WALLET) return res.status(409).json({ error: 'genesis wallet mismatch' });
  const db = loadDB();
  const local = loadPricing(db);
  if (Number(b.updatedAt || 0) <= Number(local.updatedAt || 0)) {
    return res.json({ ok: true, adopted: false, updatedAt: local.updatedAt || 0 }); // already current
  }
  local.default = { ...local.default, ...b.default };
  local.perModel = (b.perModel && typeof b.perModel === 'object') ? b.perModel : {};
  local.updatedAt = Number(b.updatedAt);
  local.updatedBy = 'genesis-push';
  saveDB(db);
  console.log(`pricing adopted via push (updatedAt=${local.updatedAt})`);
  res.json({ ok: true, adopted: true, updatedAt: local.updatedAt });
});

app.get('/api/pay/models', async (_req, res) => {
  const pool = await resolveModels();
  res.json({
    recipient: TREASURY_OWNER, mint: MINT.toString(), symbol: SPEC.token.symbol,
    models: pool.map((m) => ({ id: m.id, name: m.name })),
  });
});

app.post('/api/pay/quote', async (req, res) => {
  try {
    const { model, prompt } = req.body || {};
    if (!model || prompt == null) return res.status(400).json({ error: 'model, prompt required' });
    const pool = await resolveModels();
    const m = pool.find((x) => x.id === model);
    if (!m) return res.status(400).json({ error: 'unknown model' });
    const q = quoteFor(m, prompt);
    const requestId = crypto.randomUUID();
    const db = loadDB();
    db.requests = db.requests || {};
    db.requests[requestId] = {
      model: m.id, name: m.name, hubUrl: m.hubUrl || null, cid: m.cid || null, hubModel: m.hubModel || null,
      basePrice: m.basePrice, perToken: m.perToken, estOut: m.estOut,
      prompt, priceToken: q.priceToken, recipient: TREASURY_OWNER, paid: false, createdAt: nowSec(),
    };
    saveDB(db);
    res.json({
      requestId, model: m.id, priceToken: q.priceToken, recipient: TREASURY_OWNER,
      mint: MINT.toString(), symbol: SPEC.token.symbol,
      estimated: true,
      estPromptTokens: q.estPromptTokens, estCompletionTokens: q.estCompletionTokens, estTotalTokens: q.estTotalTokens,
    });
  } catch (e) {
    res.status(400).json({ error: String(e.message || e) });
  }
});

app.post('/api/inference', async (req, res) => {
  try {
    const { requestId, signature } = req.body || {};
    if (!requestId || !signature) return res.status(400).json({ error: 'requestId, signature required' });
    // Serialize by requestId: two concurrent submissions of the same request must not
    // both verify + run inference (double compute / double-consume). The second waits,
    // then sees r.paid and returns the same result idempotently.
    const out = await withLock(`req:${requestId}`, async () => {
      const db = loadDB();
      const r = (db.requests || {})[requestId];
      if (!r) { const e = new Error('unknown requestId'); e.status = 404; throw e; }
      if (r.paid) {
        // Idempotent replay. A failed-and-refunded request must not masquerade as a
        // successful one — report the failure/refund instead of an empty result.
        if (r.failed) { const e = new Error(r.refunded ? `inference failed; ${r.priceToken} KVR refunded (${r.refundSig})` : 'inference failed; refund pending'); e.status = 502; throw e; }
        return { requestId, paid: true, signature: r.signature, model: r.model, priceToken: r.priceToken, result: r.result, usage: r.usage };
      }
      if (db.usedSignatures[signature]) { const e = new Error('signature already used'); e.status = 409; throw e; }

      // verify the KVR payment landed in the treasury (same vault ATA); capture the payer for refunds
      const vr = await verifyStakeTransfer(signature, '', r.priceToken);
      const payer = vr && typeof vr === 'object' ? vr.payer : null;

      r.paid = true;
      r.signature = signature;
      r.payer = payer;
      db.usedSignatures[signature] = { kind: 'payment', requestId, amount: r.priceToken, at: nowSec() };
      if (r.hubUrl && r.cid) {
        // Real inference on the model's own hub; bill on the hub's actual token usage.
        // The KVR already settled on-chain (client-signed transfer) BEFORE this call,
        // so a hub failure (5xx / empty) would otherwise charge for nothing. Refund
        // the payer from the treasury and surface a 502 so the client isn't billed.
        let res2;
        try {
          res2 = await hubInfer(r.hubUrl, r.cid, r.hubModel, r.prompt);
        } catch (ie) {
          let refundSig = null;
          try { if (payer) refundSig = await payout(payer, r.priceToken); } catch (re) { console.error(`refund FAILED for ${requestId} payer=${payer}: ${re.message}`); }
          r.failed = true; r.refunded = !!refundSig; r.refundSig = refundSig; r.error = String(ie.message || ie);
          db.usedSignatures[signature].refunded = !!refundSig;
          db.requests[requestId] = r;
          saveDB(db);
          const e = new Error(refundSig
            ? `inference failed (${r.error}); ${r.priceToken} KVR refunded to ${payer} (${refundSig})`
            : `inference failed (${r.error}); refund could not be issued automatically${payer ? '' : ' (payer not derivable)'}`);
          e.status = 502; throw e;
        }
        r.result = res2.content;
        const pt = res2.usage && res2.usage.prompt_tokens != null ? res2.usage.prompt_tokens : estimateTokens(r.prompt);
        const ct = res2.usage && res2.usage.completion_tokens != null ? res2.usage.completion_tokens : estimateTokens(r.result);
        r.usage = { promptTokens: pt, completionTokens: ct, totalTokens: pt + ct, costToken: round6(r.basePrice + (pt + ct) * r.perToken) };
      } else {
        r.result = mockInfer(r.model, r.prompt);
        r.usage = usageFor({ basePrice: r.basePrice, perToken: r.perToken }, r.prompt, r.result); // actual tokens used
      }
      db.requests[requestId] = r;
      saveDB(db);
      return { requestId, paid: true, signature, model: r.model, priceToken: r.priceToken, result: r.result, usage: r.usage };
    });
    res.json(out);
  } catch (e) {
    res.status(e.status || 400).json({ error: String(e.message || e) });
  }
});

// ---- gateway admin auth (SIWS + TOTP 2FA) ---------------------------------
// A wallet-address owner from the KVR_ADMIN_WALLETS allowlist proves ownership by
// signing a nonce (first factor), then a TOTP code if enrolled (second factor).
// This gates the gateway's administrative surface (network node registry, hub
// catalog). User settlement flows (stake/claim/register) are intentionally NOT
// gated. Admin 2FA enrollment is persisted in the DB under `adminAuth`.
const ADMIN_COOKIE = 'kvr_admin_session';

function loadAuthStore() { const db = loadDB(); db.adminAuth = db.adminAuth || {}; return db; }
function twofaEnabled(db, wallet) { return !!((db.adminAuth[wallet] || {}).enabled); }

function adminWallet(req) {
  const tok = gwauth.parseCookies(req)[ADMIN_COOKIE] || '';
  const w = tok ? gwauth.verifySession(tok) : null;
  return w && gwauth.isAdmin(w) ? w : null;
}

function setAdminCookie(res, wallet) {
  res.cookie(ADMIN_COOKIE, gwauth.makeSession(wallet), {
    httpOnly: true, sameSite: 'lax', maxAge: 86400 * 1000, path: '/',
  });
}

function requireAdmin(req, res, next) {
  if (!gwauth.authEnabled()) return res.status(503).json({ error: 'admin auth not configured' });
  const w = adminWallet(req);
  if (!w) return res.status(401).json({ error: 'admin sign-in required' });
  req.adminWallet = w;
  next();
}

app.get('/api/admin/status', (req, res) => {
  if (!gwauth.authEnabled()) return res.json({ enabled: false, authenticated: false, wallet: null });
  const w = adminWallet(req);
  const db = loadAuthStore();
  res.json({ enabled: true, authenticated: !!w, wallet: w, twofa: w ? twofaEnabled(db, w) : false });
});

app.post('/api/admin/challenge', (req, res) => {
  if (!gwauth.authEnabled()) return res.status(503).json({ error: 'admin auth not configured' });
  const wallet = String((req.body || {}).wallet || '').trim();
  if (!gwauth.isAdmin(wallet)) return res.status(403).json({ error: 'wallet is not a gateway admin' });
  res.json(gwauth.newChallenge(wallet));
});

app.post('/api/admin/verify', (req, res) => {
  const { wallet, nonce, signature } = req.body || {};
  const w = String(wallet || '').trim();
  if (!gwauth.verifyLogin(w, nonce, signature)) return res.status(401).json({ error: 'signature verification failed' });
  if (!gwauth.isAdmin(w)) return res.status(403).json({ error: 'wallet is not a gateway admin' });
  const db = loadAuthStore();
  if (twofaEnabled(db, w)) {
    return res.json({ wallet: w, twofa_required: true, pre_auth: gwauth.makeToken(w, 'pre2fa', 300) });
  }
  setAdminCookie(res, w);
  res.json({ wallet: w });
});

app.post('/api/admin/2fa/login', (req, res) => {
  const { pre_auth, code } = req.body || {};
  const w = gwauth.verifyToken(pre_auth || '', 'pre2fa');
  if (!w) return res.status(401).json({ error: '2FA challenge expired — sign in again' });
  const db = loadAuthStore();
  const ent = db.adminAuth[w] || {};
  if (!ent.enabled) { setAdminCookie(res, w); return res.json({ wallet: w }); }
  if (gwauth.verifyCode(ent.secret, code)) { setAdminCookie(res, w); return res.json({ wallet: w }); }
  const remaining = gwauth.verifyAndConsumeBackup(ent.backup || [], code);
  if (remaining) {
    ent.backup = remaining; db.adminAuth[w] = ent; saveDB(db);
    setAdminCookie(res, w); return res.json({ wallet: w });
  }
  res.status(401).json({ error: 'invalid authenticator or backup code' });
});

// ---- model pricing governance (genesis wallet + fresh 2FA, desktop-only) ---
// Reject requests that arrived through the public reverse proxy (Cloudflare /
// caddy set x-forwarded-*). Only a direct request from the gateway host — the
// Electron desktop app that hosts this gateway — reaches the pricing writer.
function requireLocalDesktop(req, res, next) {
  if (req.headers['x-forwarded-for'] || req.headers['x-forwarded-host']) {
    return res.status(403).json({ error: 'pricing changes are only allowed from the gateway desktop app' });
  }
  next();
}
// Genesis wallet only — not merely an admin-allowlist member.
function requireGenesis(req, res, next) {
  if (req.adminWallet !== GENESIS_WALLET) {
    return res.status(403).json({ error: 'pricing can only be changed by the genesis wallet' });
  }
  next();
}
// A fresh TOTP code (2FA must be enrolled+enabled for genesis, and code valid now).
function requireFreshTotp(req, res, next) {
  const db = loadAuthStore();
  const ent = db.adminAuth[GENESIS_WALLET] || {};
  if (!ent.enabled) return res.status(403).json({ error: '2FA must be enabled for the genesis wallet' });
  if (!gwauth.verifyCode(ent.secret, String((req.body || {}).totp || ''))) {
    return res.status(401).json({ error: 'invalid 2FA code' });
  }
  next();
}

app.get('/api/admin/pricing', requireAdmin, (req, res) => {
  const p = loadPricing(loadDB());
  res.json({
    default: p.default, perModel: p.perModel, updatedAt: p.updatedAt, updatedBy: p.updatedBy,
    genesis: GENESIS_WALLET, isGenesis: req.adminWallet === GENESIS_WALLET, symbol: SPEC.token.symbol,
    isSource: IS_SOURCE, writeLocked: IS_WRITE_LOCKED,
  });
});

app.post('/api/admin/pricing', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  if (IS_WRITE_LOCKED) {
    return res.status(409).json({ error: 'this gateway mirrors the genesis source; change pricing on the genesis (source) gateway' });
  }
  const { modelId, basePrice, perToken, estOut } = req.body || {};
  const num = (v) => (v === undefined || v === null || v === '' || !Number.isFinite(Number(v)) || Number(v) < 0 ? null : Number(v));
  const bp = num(basePrice), pt = num(perToken), eo = num(estOut);
  if (bp === null || pt === null || eo === null) {
    return res.status(400).json({ error: 'basePrice, perToken, estOut must be non-negative numbers' });
  }
  const db = loadDB();
  const p = loadPricing(db);
  const target = (modelId && modelId !== 'default') ? String(modelId) : 'default';
  const prev = target === 'default' ? { ...p.default } : { ...(p.perModel[target] || p.default) };
  const nextVal = { basePrice: bp, perToken: pt, estOut: eo };
  if (target === 'default') p.default = nextVal; else p.perModel[target] = nextVal;
  p.updatedAt = nowSec(); p.updatedBy = GENESIS_WALLET;
  db.pricingAudit = db.pricingAudit || [];
  db.pricingAudit.push({ at: nowSec(), by: GENESIS_WALLET, target, prev, next: nextVal });
  if (db.pricingAudit.length > 500) db.pricingAudit = db.pricingAudit.slice(-500);
  saveDB(db);
  pushPricingToRelays(); // propagate to configured relays immediately (best-effort)
  res.json({ ok: true, target, pricing: nextVal, updatedAt: p.updatedAt });
});

app.get('/api/admin/pricing/audit', requireAdmin, (_req, res) => {
  res.json({ audit: (loadDB().pricingAudit || []).slice(-100).reverse() });
});

// ---- pricing relays (source only): the genesis operator manages which public
// gateways receive pushed pricing, from the desktop pricing page. Secrets are
// never returned to the client — only whether one is set.
function relayView(r) { return { url: r.url, label: r.label || '', hasSecret: !!r.secret, addedAt: r.addedAt || 0, addedBy: r.addedBy || '' }; }

app.get('/api/admin/pricing/relays', requireAdmin, (_req, res) => {
  res.json({ isSource: IS_SOURCE, relays: loadRelays(loadDB()).map(relayView) });
});

app.post('/api/admin/pricing/relays', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  if (IS_WRITE_LOCKED) return res.status(409).json({ error: 'only the genesis (source) gateway manages relays' });
  const { url, secret, label } = req.body || {};
  let clean;
  try { const u = new URL(String(url)); if (!/^https?:$/.test(u.protocol)) throw 0; clean = u.origin; }
  catch { return res.status(400).json({ error: 'url must be a valid http(s) URL' }); }
  const db = loadDB();
  const relays = loadRelays(db);
  const existing = relays.find((r) => r.url === clean);
  if (existing) {
    if (typeof secret === 'string' && secret) existing.secret = secret;
    if (label !== undefined) existing.label = String(label || '');
  } else {
    relays.push({ url: clean, secret: String(secret || ''), label: String(label || ''), addedAt: nowSec(), addedBy: GENESIS_WALLET });
  }
  saveDB(db);
  pushPricingToRelays(); // seed the new/updated relay right away
  res.json({ ok: true, relays: relays.map(relayView) });
});

app.post('/api/admin/pricing/relays/remove', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  if (IS_WRITE_LOCKED) return res.status(409).json({ error: 'only the genesis (source) gateway manages relays' });
  const { url } = req.body || {};
  let clean; try { clean = new URL(String(url)).origin; } catch { clean = String(url || ''); }
  const db = loadDB();
  db.pricingRelays = loadRelays(db).filter((r) => r.url !== clean);
  saveDB(db);
  res.json({ ok: true, relays: db.pricingRelays.map(relayView) });
});

// ==== prepaid credits + OpenAI-compatible endpoint (whitelist + prepaid) ======
// A whitelisted wallet prepays KVR (genesis grant or on-chain deposit) into a
// credit balance, mints an API key by signing (SIWS), and calls a native
// OpenAI-compatible endpoint that streams straight from the hub (stream + tools).
// Each call debits the balance by the governed per-token price — no per-request
// quote/pay handshake. This is the reporter's "whitelist / fund our wallet" path.
const CREDIT_WHITELIST = (process.env.KVR_CREDIT_WHITELIST || '')
  .split(',').map((s) => s.trim()).filter(Boolean);
const CREDIT_MIN_BALANCE = Number(process.env.KVR_CREDIT_MIN_BALANCE || 0);
// Open self-registration: any wallet that proves ownership (SIWS) adds itself to
// the whitelist. Prepaid credits remain the real spend gate. OFF => operator
// approves wallets via /api/admin/credits/whitelist (genesis).
const CREDIT_OPEN_REGISTER = /^(1|true|yes)$/i.test(process.env.KVR_CREDIT_OPEN_REGISTER || '');
// DB-backed whitelist (managed from the UI/admin) unions with the env bootstrap
// list, so wallets can be added without a restart.
function loadCreditWhitelist(db) { if (!Array.isArray(db.creditWhitelist)) db.creditWhitelist = []; return db.creditWhitelist; }
function creditWhitelisted(w) {
  if (CREDIT_WHITELIST.length === 0) return true;               // empty env => open (legacy)
  if (CREDIT_WHITELIST.includes(w)) return true;
  try { return loadCreditWhitelist(loadDB()).includes(w); } catch { return false; }
}
function loadCredits(db) { if (!db.credits || typeof db.credits !== 'object') db.credits = {}; return db.credits; }
function creditAcct(db, wallet) {
  const c = loadCredits(db);
  if (!c[wallet]) c[wallet] = { balance: 0, granted: 0, deposited: 0, spent: 0, createdAt: nowSec(), updatedAt: nowSec() };
  return c[wallet];
}
function loadApiKeys(db) { if (!db.apiKeys || typeof db.apiKeys !== 'object') db.apiKeys = {}; return db.apiKeys; }
function hashKey(k) { return crypto.createHash('sha256').update(String(k)).digest('hex'); }
function bearerWallet(req) {
  const m = String(req.get('authorization') || '').match(/^Bearer\s+(.+)$/i);
  if (!m) return null;
  const ent = loadApiKeys(loadDB())[hashKey(m[1].trim())];
  return ent ? ent.wallet : null;
}
function creditAudit(db, entry) {
  db.creditAudit = db.creditAudit || [];
  db.creditAudit.push({ at: nowSec(), ...entry });
  if (db.creditAudit.length > 1000) db.creditAudit = db.creditAudit.slice(-1000);
}
// Debit a wallet's balance by the governed cost of one completion's token usage.
function debitCredits(wallet, modelId, usage) {
  const db = loadDB();
  const a = creditAcct(db, wallet);
  const pr = pricingFor(modelId);
  const pt = Number((usage && usage.prompt_tokens) || 0);
  const ct = Number((usage && usage.completion_tokens) || 0);
  const cost = round6(pr.basePrice + (pt + ct) * pr.perToken);
  a.balance = round6(a.balance - cost);
  a.spent = round6((a.spent || 0) + cost);
  a.updatedAt = nowSec();
  creditAudit(db, { wallet, kind: 'debit', model: modelId, cost, prompt_tokens: pt, completion_tokens: ct, balance: a.balance });
  saveDB(db);
  return cost;
}

// SIWS-mint an API key bound to a wallet (returned once; only its hash is stored).
app.post('/api/credits/challenge', (req, res) => {
  const wallet = String((req.body || {}).wallet || '').trim();
  if (!wallet) return res.status(400).json({ error: 'wallet required' });
  if (!creditWhitelisted(wallet)) return res.status(403).json({ error: 'wallet is not whitelisted for API access' });
  res.json(gwauth.newChallenge(wallet));
});
// Self-register: nonce for the registration signature. Deliberately NOT
// whitelist-gated (an unregistered wallet must be able to get a nonce to prove
// ownership and register); registration itself is gated by CREDIT_OPEN_REGISTER.
app.post('/api/credits/register/challenge', (req, res) => {
  const wallet = String((req.body || {}).wallet || '').trim();
  if (!wallet) return res.status(400).json({ error: 'wallet required' });
  res.json(gwauth.newChallenge(wallet));
});
app.post('/api/credits/register', (req, res) => {
  if (!CREDIT_OPEN_REGISTER) return res.status(403).json({ error: 'self-registration is disabled; ask the operator to whitelist your wallet' });
  const { wallet, nonce, signature } = req.body || {};
  const w = String(wallet || '').trim();
  if (!gwauth.verifyLogin(w, nonce, signature)) return res.status(401).json({ error: 'signature verification failed' });
  const db = loadDB();
  const wl = loadCreditWhitelist(db);
  const added = !wl.includes(w);
  if (added) { wl.push(w); creditAcct(db, w); creditAudit(db, { wallet: w, kind: 'whitelist-register' }); saveDB(db); }
  res.json({ ok: true, wallet: w, whitelisted: true, added });
});
app.post('/api/credits/apikey', (req, res) => {
  const { wallet, nonce, signature, label } = req.body || {};
  const w = String(wallet || '').trim();
  if (!gwauth.verifyLogin(w, nonce, signature)) return res.status(401).json({ error: 'signature verification failed' });
  if (!creditWhitelisted(w)) return res.status(403).json({ error: 'wallet is not whitelisted' });
  const key = 'kvr-' + crypto.randomBytes(24).toString('base64url');
  const db = loadDB();
  loadApiKeys(db)[hashKey(key)] = { wallet: w, label: String(label || ''), createdAt: nowSec() };
  creditAcct(db, w);
  saveDB(db);
  res.json({ apiKey: key, wallet: w, note: 'store this key now; it is not shown again' });
});
app.get('/api/credits/balance', (req, res) => {
  const w = bearerWallet(req);
  if (!w) return res.status(401).json({ error: 'invalid or missing API key' });
  const a = creditAcct(loadDB(), w);
  res.json({ wallet: w, balance: round6(a.balance), spent: round6(a.spent || 0), symbol: SPEC.token.symbol });
});
// Genesis funds a wallet's balance directly (whitelist + prepaid, operator side).
app.post('/api/admin/credits/grant', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  const { wallet, amount } = req.body || {};
  const w = String(wallet || '').trim();
  const amt = Number(amount);
  if (!w || !Number.isFinite(amt) || amt <= 0) return res.status(400).json({ error: 'wallet and positive amount required' });
  const db = loadDB();
  const a = creditAcct(db, w);
  a.balance = round6(a.balance + amt);
  a.granted = round6((a.granted || 0) + amt);
  a.updatedAt = nowSec();
  creditAudit(db, { by: GENESIS_WALLET, wallet: w, kind: 'grant', amount: amt, balance: a.balance });
  saveDB(db);
  res.json({ ok: true, wallet: w, balance: a.balance, symbol: SPEC.token.symbol });
});
// Operator-managed whitelist (genesis, desktop-local). env KVR_CREDIT_WHITELIST
// entries are permanent bootstrap and are shown but not removable here.
app.get('/api/admin/credits/whitelist', requireAdmin, (_req, res) => {
  res.json({ env: CREDIT_WHITELIST, db: loadCreditWhitelist(loadDB()), openRegister: CREDIT_OPEN_REGISTER });
});
app.post('/api/admin/credits/whitelist', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  const w = String((req.body || {}).wallet || '').trim();
  if (!w) return res.status(400).json({ error: 'wallet required' });
  const db = loadDB();
  const wl = loadCreditWhitelist(db);
  const added = !wl.includes(w);
  if (added) { wl.push(w); creditAcct(db, w); creditAudit(db, { by: GENESIS_WALLET, wallet: w, kind: 'whitelist-add' }); saveDB(db); }
  res.json({ ok: true, wallet: w, added, db: wl });
});
app.post('/api/admin/credits/whitelist/remove', requireAdmin, requireLocalDesktop, requireGenesis, requireFreshTotp, (req, res) => {
  const w = String((req.body || {}).wallet || '').trim();
  const db = loadDB();
  db.creditWhitelist = loadCreditWhitelist(db).filter((x) => x !== w);
  creditAudit(db, { by: GENESIS_WALLET, wallet: w, kind: 'whitelist-remove' });
  saveDB(db);
  res.json({ ok: true, wallet: w, db: db.creditWhitelist });
});
// Self-serve: prove an on-chain KVR transfer (>= amount) from `wallet` to the
// treasury, credit that amount. Reuses the payment verifier's sender binding.
app.post('/api/credits/deposit', async (req, res) => {
  try {
    const { wallet, amount, signature } = req.body || {};
    const w = String(wallet || '').trim();
    const amt = Number(amount);
    if (!w || !signature || !Number.isFinite(amt) || amt <= 0) return res.status(400).json({ error: 'wallet, amount, signature required' });
    if (!creditWhitelisted(w)) return res.status(403).json({ error: 'wallet is not whitelisted' });
    const out = await withLock(`credit-deposit:${signature}`, async () => {
      const db = loadDB();
      if (db.usedSignatures[signature]) { const e = new Error('signature already used'); e.status = 409; throw e; }
      await verifyStakeTransfer(signature, w, amt); // verifies w sent >= amt to the vault
      const a = creditAcct(db, w);
      a.balance = round6(a.balance + amt);
      a.deposited = round6((a.deposited || 0) + amt);
      a.updatedAt = nowSec();
      db.usedSignatures[signature] = { kind: 'credit-deposit', wallet: w, amount: amt, at: nowSec() };
      creditAudit(db, { wallet: w, kind: 'deposit', amount: amt, balance: a.balance, signature });
      saveDB(db);
      return { ok: true, wallet: w, credited: amt, balance: a.balance };
    });
    res.json(out);
  } catch (e) { res.status(e.status || 400).json({ error: String(e.message || e) }); }
});

// OpenAI-compatible model list (API-key auth).
app.get('/v1/models', async (req, res) => {
  if (!bearerWallet(req)) return res.status(401).json({ error: { message: 'invalid API key', type: 'invalid_request_error' } });
  const pool = await resolveModels();
  res.json({ object: 'list', data: pool.map((m) => ({ id: m.id, object: 'model', owned_by: 'kvasir', name: m.name })) });
});
// OpenAI-compatible chat completions: streaming + tools passthrough, credit-debited.
// ---- ring resilience: auto-recover a crashed serving ring -------------------
// A ring stage/coordinator can die under load; the hub keeps phase=running and
// every inference then 500s until a manual reload (this is what banya-agent saw
// as intermittent "500 {}"). Detect an upstream outage, return a *typed 503*
// (not empty {}), and trigger a debounced reload of the ring's last-served
// config so it self-heals in ~1 min instead of staying down.
const RING_RELOAD = {}; // cid -> { at, inflight }
const RING_RELOAD_COOLDOWN_MS = Number(process.env.KVR_RING_RELOAD_COOLDOWN_MS || 120000);
async function reloadRing(hubUrl, cid) {
  const st = RING_RELOAD[cid] || (RING_RELOAD[cid] = { at: 0, inflight: false });
  const now = Date.now();
  if (st.inflight || (now - st.at) < RING_RELOAD_COOLDOWN_MS) return false;
  st.inflight = true; st.at = now;
  try {
    const cr = await (await fetch(`${hubUrl}/api/controllers?full=1`, { headers: hubHeaders(), signal: AbortSignal.timeout(8000) })).json();
    const c = (Array.isArray(cr) ? cr : (cr.controllers || [])).find((x) => x.id === cid);
    const ll = c && c.last_load;
    if (!ll || !ll.model) { console.warn(`ring reload ${cid}: no last_load`); return false; }
    const serveBody = {
      model: ll.model, ctx: ll.ctx || 4096, parallel: ll.parallel || 1,
      kv_bits: ll.kv_bits || 16, cache_type_k: ll.cache_type_k || 'f16', cache_type_v: ll.cache_type_v || 'f16',
      no_cpu_offload: !!ll.no_cpu_offload, reserve_mib: ll.reserve_mib || 1024,
      placement_strategy: ll.placement_strategy || 'ring-stage-vram-weighted',
      runtime_mode: 'ring_proxy', batch: ll.batch || 2048, ubatch: ll.ubatch || 512,
    };
    await fetch(`${hubUrl}/api/controllers/${cid}/unload`, { method: 'POST', headers: hubHeaders({ 'Content-Type': 'application/json' }), body: '{}', signal: AbortSignal.timeout(30000) }).catch(() => {});
    await new Promise((r) => setTimeout(r, 2000));
    const r = await fetch(`${hubUrl}/api/controllers/${cid}/serve`, { method: 'POST', headers: hubHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify(serveBody), signal: AbortSignal.timeout(30000) });
    console.log(`ring auto-reload ${cid}: serve ${r.status} ctx=${serveBody.ctx}`);
    return r.ok;
  } catch (e) { console.warn(`ring auto-reload ${cid} failed: ${e.message}`); return false; }
  finally { setTimeout(() => { st.inflight = false; }, 60000); } // hold ~ load time
}
function ringOutage(res, m, detail) {
  if (m && m.hubUrl && m.cid) reloadRing(m.hubUrl, m.cid); // fire-and-forget, debounced
  return res.status(503).json({ error: {
    message: 'the model is temporarily unavailable — the serving ring is recovering; retry in ~60s',
    type: 'hub_unavailable', code: 'ring_recovering', detail: String(detail || '').slice(0, 200),
  } });
}
// A hub "ring down" shows up either as a non-2xx, or as a 200 SSE whose FIRST
// chunk is a data:{"error":...} — treat both as an outage.
function sseFirstError(text) {
  for (const line of String(text || '').split('\n')) {
    const t = line.trim();
    if (!t.startsWith('data:')) continue;
    const p = t.slice(5).trim();
    if (p === '[DONE]') return null;
    try { const o = JSON.parse(p); return o && o.error ? (o.error.message || 'upstream error') : null; } catch { return null; }
  }
  return null;
}

app.post('/v1/chat/completions', async (req, res) => {
 // Wrap the whole handler: any unforeseen throw must become a typed error, never
 // an Express default 500 with an empty {} body (the exact symptom banya-agent
 // reported). `m` may be undefined early, so guard the outage helper on it.
 let m;
 try {
  const w = bearerWallet(req);
  if (!w) return res.status(401).json({ error: { message: 'invalid API key', type: 'invalid_request_error' } });
  if (!creditWhitelisted(w)) return res.status(403).json({ error: { message: 'wallet is not whitelisted', type: 'access_denied' } });
  if (creditAcct(loadDB(), w).balance <= CREDIT_MIN_BALANCE) {
    return res.status(402).json({ error: { message: 'insufficient credit balance; top up', type: 'insufficient_quota' } });
  }
  const body = req.body || {};
  const pool = await resolveModels().catch(() => []);
  m = pool.find((x) => x.id === body.model) || pool.find((x) => x.name === body.model);
  if (!m || !m.hubUrl || !m.cid) {
    // No model matched: distinguish "the model isn't served right now" (ring
    // reloading / down) from a genuinely bad name, so the client can retry vs fix.
    const served = pool.some((x) => x.hubUrl && x.cid);
    return res.status(served ? 400 : 503).json({ error: {
      message: served ? `unknown model '${body.model}'` : 'no model is currently served — the ring is starting or recovering; retry in ~60s',
      type: served ? 'invalid_request_error' : 'hub_unavailable',
      code: served ? 'model_not_served' : 'ring_recovering',
    } });
  }
  const wantStream = !!body.stream;
  const fwd = { ...body, model: m.hubModel };
  // Reasoning models (Qwen3.x, MiniMax-M3, ...) emit a hidden "thinking" pass into
  // reasoning_content that can consume the whole token budget and leave content
  // EMPTY — a blank reply, or a slow crawl toward the timeout. The pay path
  // (hubInfer) already disables it; do the same here so the OpenAI endpoint the
  // app uses does not error on M3. An explicit caller value still wins.
  fwd.chat_template_kwargs = { enable_thinking: false, ...(body.chat_template_kwargs || {}) };
  if (wantStream) fwd.stream_options = { ...(body.stream_options || {}), include_usage: true };
  let upstream;
  try {
    upstream = await fetch(`${m.hubUrl}/c/${m.cid}/v1/chat/completions`, {
      method: 'POST', headers: hubHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify(fwd), signal: AbortSignal.timeout(300000),
    });
  } catch (e) { return ringOutage(res, m, `upstream unreachable: ${e.message}`); }

  if (!wantStream) {
    const raw = await upstream.text().catch(() => '');
    if (!upstream.ok) return ringOutage(res, m, raw || `upstream ${upstream.status}`);
    let d; try { d = JSON.parse(raw); } catch { return ringOutage(res, m, 'non-JSON upstream response'); }
    if (!d || !d.choices) return ringOutage(res, m, 'upstream returned no choices');
    markRingOk(m.cid);
    if (d.usage) debitCredits(w, m.id, d.usage);
    return res.json(d);
  }

  if (!upstream.ok || !upstream.body) {
    const t = await upstream.text().catch(() => '');
    return ringOutage(res, m, t || `upstream ${upstream.status}`);
  }
  // Peek the first chunk: a down ring answers 200 with data:{"error":...}. Catch
  // it BEFORE committing SSE headers so we can return a typed 503 + auto-reload.
  const reader = upstream.body.getReader();
  const dec = new TextDecoder();
  let first;
  try { first = await reader.read(); } catch (e) { return ringOutage(res, m, `stream read: ${e.message}`); }
  const firstText = first && first.value ? dec.decode(first.value, { stream: true }) : '';
  const errMsg = sseFirstError(firstText);
  if (errMsg) return ringOutage(res, m, errMsg);

  res.status(200);
  res.setHeader('Content-Type', 'text/event-stream; charset=utf-8');
  res.setHeader('Cache-Control', 'no-cache');
  res.setHeader('Connection', 'keep-alive');
  let usage = null, buf = '';
  const scan = (text) => {
    buf += text;
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim(); buf = buf.slice(nl + 1);
      if (!line.startsWith('data:')) continue;
      const payload = line.slice(5).trim();
      if (payload === '[DONE]') continue;
      try { const obj = JSON.parse(payload); if (obj && obj.usage) usage = obj.usage; } catch { /* partial */ }
    }
  };
  try {
    if (firstText) { res.write(firstText); scan(firstText); }
    if (!(first && first.done)) {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        const text = dec.decode(value, { stream: true });
        res.write(text);
        scan(text);
      }
    }
  } catch { /* client/stream aborted */ }
  markRingOk(m.cid);
  res.end();
  if (usage) debitCredits(w, m.id, usage);
 } catch (e) {
  // Last-resort guard: never leak an Express default 500 {}. If we haven't
  // committed a response yet, return a typed error the agent can branch on.
  if (!res.headersSent) {
    return res.status(502).json({ error: {
      message: 'gateway error handling the completion; retry shortly',
      type: 'gateway_error', code: 'internal', detail: String(e && e.message || e).slice(0, 200),
    } });
  }
  try { res.end(); } catch { /* already closed */ }
 }
});

// Health: real lightweight probe of the current serving ring (cached ~5s).
// Liveness WITHOUT consuming a decode slot. A chat-completion probe queues behind
// in-flight work when the ring is served --parallel 1, so a busy-but-healthy ring
// (a legitimate 20K-token prefill runs ~26s > the old 20s probe timeout) looked
// "dead" and got reloaded — and the reload WAS the outage banya-agent saw. The
// hub already knows the controller phase; asking it costs no slot and never
// false-times-out. Returns true (serving), false (down), or null (unknown/unreachable).
async function ringServing(hubUrl, cid) {
  try {
    const r = await fetch(`${hubUrl}/api/controllers`, { headers: hubHeaders(), signal: AbortSignal.timeout(5000) });
    if (!r.ok) return null;
    const d = await r.json();
    const ctrls = Array.isArray(d) ? d : (d.controllers || []);
    const c = ctrls.find((x) => x.id === cid);
    if (!c) return false;
    if (c.phase && !['running', 'loading', 'serving'].includes(String(c.phase))) return false;
    return !!(c.runtime_loaded && (c.serving || c.active_model));
  } catch { return null; }
}

// Real successful traffic is the strongest liveness signal — a ring that just
// answered a client request is alive, so the watchdog skips it entirely.
const RING_LAST_OK = {}; // cid -> ts of last successful client completion
const markRingOk = (cid) => { if (cid) RING_LAST_OK[cid] = Date.now(); };

let HEALTH_CACHE = { at: 0, body: null, code: 200 };
app.get(['/v1/health'], async (_req, res) => {
  const now = Date.now();
  if (HEALTH_CACHE.body && (now - HEALTH_CACHE.at) < 5000) return res.status(HEALTH_CACHE.code).json(HEALTH_CACHE.body);
  const pool = await resolveModels().catch(() => []);
  const m = pool[0];
  const set = (code, b) => { HEALTH_CACHE = { at: Date.now(), body: b, code }; return res.status(code).json(b); };
  if (!m || !m.hubUrl || !m.cid) return set(503, { status: 'no_model', detail: 'no model is currently served' });
  // Recent real traffic OR a serving controller phase = healthy. No slot-consuming
  // probe, so a busy ring is never reported unhealthy (and never reloaded).
  if (RING_LAST_OK[m.cid] && (now - RING_LAST_OK[m.cid]) < RING_WATCHDOG_MS) {
    return set(200, { status: 'ok', model: m.id, name: m.name });
  }
  const serving = await ringServing(m.hubUrl, m.cid);
  if (serving === true || serving === null) return set(200, { status: 'ok', model: m.id, name: m.name });
  reloadRing(m.hubUrl, m.cid);
  return set(503, { status: 'unhealthy', model: m.id, detail: 'controller not serving', recovering: true });
});

// Proactive self-heal: periodically confirm the ring is still SERVING (via the
// hub's controller phase — no decode slot consumed) and reload ONLY a genuinely
// down ring, never a busy one. Requires two consecutive "down" reads to debounce
// a transient hub blip, and skips a ring that served real traffic this interval.
const RING_WATCHDOG_MS = Number(process.env.KVR_RING_WATCHDOG_MS || 30000);
const RING_DOWN_STREAK = {}; // cid -> consecutive "not serving" reads
async function ringWatchdog() {
  const models = await resolveModels().catch(() => []);
  const seen = new Set();
  for (const m of models) {
    if (!m || !m.hubUrl || !m.cid) continue;
    const key = `${m.hubUrl}|${m.cid}`;
    if (seen.has(key)) continue;
    seen.add(key);
    await probeModel(m);
  }
}
async function probeModel(m) {
  // Real traffic proves liveness — don't probe a ring that just served a request.
  if (RING_LAST_OK[m.cid] && (Date.now() - RING_LAST_OK[m.cid]) < RING_WATCHDOG_MS) { RING_DOWN_STREAK[m.cid] = 0; return; }
  const serving = await ringServing(m.hubUrl, m.cid);
  if (serving !== false) { RING_DOWN_STREAK[m.cid] = 0; return; } // serving or unknown -> leave it alone
  RING_DOWN_STREAK[m.cid] = (RING_DOWN_STREAK[m.cid] || 0) + 1;
  if (RING_DOWN_STREAK[m.cid] >= 2) {
    console.warn(`ring watchdog: ${m.id} not serving x${RING_DOWN_STREAK[m.cid]} -> reload`);
    reloadRing(m.hubUrl, m.cid);
    RING_DOWN_STREAK[m.cid] = 0;
  }
}
if (LINKCPP_HUB_URL) setInterval(ringWatchdog, RING_WATCHDOG_MS);

app.post('/api/admin/logout', (req, res) => {
  res.clearCookie(ADMIN_COOKIE, { path: '/' });
  res.json({ ok: true });
});

app.get('/api/admin/2fa/status', requireAdmin, (req, res) => {
  const db = loadAuthStore();
  res.json({ enabled: twofaEnabled(db, req.adminWallet) });
});

app.post('/api/admin/2fa/enroll', requireAdmin, (req, res) => {
  const secret = gwauth.newSecret();
  const codes = gwauth.newBackupCodes();
  const db = loadAuthStore();
  db.adminAuth[req.adminWallet] = { secret, backup: codes.map(gwauth.hashBackup), enabled: false };
  saveDB(db);
  res.json({ secret, otpauth_uri: gwauth.otpauthUri(secret, req.adminWallet), backup_codes: codes });
});

app.post('/api/admin/2fa/confirm', requireAdmin, (req, res) => {
  const db = loadAuthStore();
  const ent = db.adminAuth[req.adminWallet] || {};
  if (!ent.secret) return res.status(400).json({ error: 'no enrollment in progress' });
  if (!gwauth.verifyCode(ent.secret, (req.body || {}).code)) return res.status(400).json({ error: 'invalid authenticator code' });
  ent.enabled = true; db.adminAuth[req.adminWallet] = ent; saveDB(db);
  res.json({ enabled: true });
});

app.post('/api/admin/2fa/disable', requireAdmin, (req, res) => {
  const db = loadAuthStore();
  const ent = db.adminAuth[req.adminWallet] || {};
  if (!ent.enabled) return res.status(400).json({ error: '2FA is not enabled' });
  const code = (req.body || {}).code;
  let ok = gwauth.verifyCode(ent.secret, code);
  if (!ok) ok = gwauth.verifyAndConsumeBackup(ent.backup || [], code) !== null;
  if (!ok) return res.status(400).json({ error: 'invalid authenticator or backup code' });
  delete db.adminAuth[req.adminWallet]; saveDB(db);
  res.json({ enabled: false });
});

// Admin-only network views + management.
app.get('/api/admin/nodes', requireAdmin, (_req, res) => {
  const db = loadDB();
  const now = nowSec();
  const nodes = Object.entries(db.nodes || {}).map(([nodeId, n]) => {
    const last = n.lastReport;
    let status = 'offline';
    if (last == null) status = 'registered';
    else if (now - last < 300) status = 'online';
    else if (now - last < 3600) status = 'idle';
    return {
      nodeId, status, owner: n.owner, os: n.os || 'unknown', label: n.label || nodeId,
      accelerator: n.accelerator || 'cpu', backend: n.backend || null,
      tier: n.tier || perfTier(n.perfScore).tier, perfScore: n.perfScore || 0,
      hostsGateway: !!n.hostsGateway, hostsHub: !!n.hostsHub,
      effectiveUnits: n.effectiveUnits || 0, pendingRewards: n.pendingRewards || 0,
      claimedTotal: n.claimedTotal || 0, registeredAt: n.registeredAt || null, lastReport: last,
    };
  });
  res.json({ count: nodes.length, nodes });
});

app.post('/api/admin/node/remove', requireAdmin, (req, res) => {
  const nodeId = String((req.body || {}).nodeId || '');
  const db = loadDB();
  if (!(db.nodes || {})[nodeId]) return res.status(404).json({ error: 'node not found' });
  delete db.nodes[nodeId]; saveDB(db);
  res.json({ removed: nodeId });
});

app.get('/api/admin/hubs', requireAdmin, (_req, res) => {
  const list = Array.from(hubRegistry.values()).map((h) => ({
    hubUrl: h.hubUrl, name: h.name || '', expiresAt: h.expiresAt, active: h.expiresAt > Date.now(),
  }));
  res.json({ hubs: list });
});

app.post('/api/admin/hub/remove', requireAdmin, (req, res) => {
  const url = String((req.body || {}).hubUrl || '').replace(/\/+$/, '');
  const had = hubRegistry.delete(url);
  res.json({ removed: had ? url : null });
});

// NOTE: the admin UI is NOT a separate page — the operator login + 2FA live inside
// the served wallet app (it signs the SIWS challenge with the unlocked wallet and
// prompts for the TOTP code as part of its own login flow). These /api/admin/*
// endpoints back that flow. See wallet/desktop/src/operatorAuth.ts.

app.get('/health', (_req, res) => res.json({ ok: true }));

// Serve the web app (same UI + features as the desktop) so the gateway doubles as a
// hosted web application. Set KVR_WEB_DIR to the built bundle (wallet/desktop `build:web`).
const WEB_DIR = process.env.KVR_WEB_DIR || path.resolve(__dirname, '../../wallet/desktop/dist');
const WEB_ENABLED = fs.existsSync(path.join(WEB_DIR, 'index.html'));
if (WEB_ENABLED) {
  // Hashed assets (index-<hash>.js/.css) are immutable; index.html must always be
  // revalidated so a new deploy's bundle is picked up instead of a stale cache.
  const noCacheIndex = (res, filePath) => {
    if (filePath.endsWith('index.html')) res.setHeader('Cache-Control', 'no-cache, must-revalidate');
    else if (/\/assets\//.test(filePath)) res.setHeader('Cache-Control', 'public, max-age=31536000, immutable');
  };
  app.use(express.static(WEB_DIR, { setHeaders: noCacheIndex }));
  // SPA fallback: serve index.html for client-side routes (never for /api or /health).
  app.use((req, res, next) => {
    if (req.method !== 'GET' || req.path.startsWith('/api/') || req.path === '/health') return next();
    res.setHeader('Cache-Control', 'no-cache, must-revalidate');
    res.sendFile(path.join(WEB_DIR, 'index.html'));
  });
}

// Final backstop: any error that escapes a route handler (sync throw, next(err))
// returns a typed JSON body, never Express's default empty 500. Must be last.
app.use((err, _req, res, _next) => {
  if (res.headersSent) return res.end();
  res.status(500).json({ error: {
    message: 'internal gateway error; retry shortly',
    type: 'gateway_error', code: 'internal', detail: String(err && err.message || err).slice(0, 200),
  } });
});

const server = app.listen(PORT, '0.0.0.0', () => {
  console.log(`staking-service on :${PORT}  cluster=${SPEC.cluster}  vault=${VAULT_ATA.toString()}  APR=${APR * 100}%`);
  console.log(`gatewayBonus=${GATEWAY_BONUS}  webUI=${WEB_ENABLED ? WEB_DIR : 'disabled'}  publicUrl=${PUBLIC_URL || '(unset)'}`);
});

// ---- hub participation API pass-through ---------------------------------------
// Remote expert workers (NAT) reach the LAN-only hub's participation surface
// through the gateway: market calls + shard download. The caller's OWN token
// forwards untouched — the hub enforces auth, the gateway grants nothing.
app.all(['/api/expert-demand', '/api/expert-volunteer', '/api/expert-coverage',
         '/api/proxy/models/:model/expert-shard'], async (req, res) => {
  if (!LINKCPP_HUB_URL) return res.status(503).json({ error: 'no hub configured' });
  try {
    const qs = req.originalUrl.includes('?') ? req.originalUrl.slice(req.originalUrl.indexOf('?')) : '';
    const headers = {};
    for (const h of ['x-linkcpp-service-token', 'authorization', 'content-type']) {
      if (req.headers[h]) headers[h] = req.headers[h];
    }
    const r = await fetch(LINKCPP_HUB_URL + req.path + qs, {
      method: req.method, headers,
      body: ['GET', 'HEAD'].includes(req.method) ? undefined : JSON.stringify(req.body || {}),
    });
    res.status(r.status);
    for (const h of ['content-type', 'content-length']) {
      const v = r.headers.get(h);
      if (v) res.setHeader(h, v);
    }
    require('stream').Readable.fromWeb(r.body).pipe(res);
  } catch (e) {
    res.status(502).json({ error: String(e.message || e) });
  }
});

// ---- hub relay WS pass-through ----------------------------------------------
// NAT/remote workers dial wss://gate/api/{expert,ring}-relay over 443; the hub
// (LAN-only) does the actual WS handshake, auth, and bridging. The gateway just
// splices the raw upgraded socket to the hub — it never parses WS frames, so
// relay traffic is opaque to it. Anything else on the upgrade port is dropped.
const net = require('net');
const RELAY_WS_PATHS = ['/api/expert-relay', '/api/ring-relay'];
server.on('upgrade', (req, socket, head) => {
  let pathname = '';
  try { pathname = new URL(req.url, 'http://x').pathname; } catch { /* fall through */ }
  if (!LINKCPP_HUB_URL || !RELAY_WS_PATHS.includes(pathname)) { socket.destroy(); return; }
  let hub;
  try { hub = new URL(LINKCPP_HUB_URL); } catch { socket.destroy(); return; }
  const up = net.connect(Number(hub.port || 80), hub.hostname, () => {
    let raw = `${req.method} ${req.url} HTTP/1.1\r\n`;
    for (let i = 0; i < req.rawHeaders.length; i += 2) raw += `${req.rawHeaders[i]}: ${req.rawHeaders[i + 1]}\r\n`;
    raw += '\r\n';
    up.write(raw);
    if (head && head.length) up.write(head);
    socket.pipe(up);
    up.pipe(socket);
  });
  up.setNoDelay(true);
  socket.setNoDelay(true);
  up.on('error', () => socket.destroy());
  socket.on('error', () => up.destroy());
  console.log(`relay ws pass-through: ${pathname} -> ${hub.hostname}:${hub.port || 80}`);
});
