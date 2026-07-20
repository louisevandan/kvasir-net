// linkcpp hub single-page control app.
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
const el = (html) => {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
};
const esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;"
}[c]));
const RENDER_CACHE = new WeakMap();
function setHtmlIfChanged(target, html) {
  if (!target) return false;
  if (RENDER_CACHE.get(target) === html) return false;
  target.innerHTML = html;
  RENDER_CACHE.set(target, html);
  return true;
}
function setTextIfChanged(target, text) {
  if (!target) return;
  const next = String(text ?? "");
  if (target.textContent !== next) target.textContent = next;
  RENDER_CACHE.delete(target);
}
const api = async (method, path, body) => {
  const r = await fetch(path, {
    method,
    headers: {"Content-Type": "application/json"},
    body: body ? JSON.stringify(body) : undefined,
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.detail || j.error || r.statusText);
  return j;
};
const pct = (used, total) => total > 0 ? Math.min(100, Math.round(used / total * 100)) : 0;
const wholeMax = (v) => Math.max(0, Math.floor(Number(v) || 0));
const DEFAULT_CHAT_MAX_TOKENS = 512;
const UI_CHAT_MAX_TOKENS = 4096;
const clampWhole = (v, max) => {
  const n = Math.max(0, Math.round(Number(v) || 0));
  return max > 0 ? Math.min(n, max) : n;
};

let sel = {type: null, id: null};
// Pristine Welcome panel, restored whenever the session locks.
const WELCOME_HTML = document.querySelector("#main") ? document.querySelector("#main").innerHTML : "";
let CACHE = {nodes: [], ctrls: [], gpus: [], system: {ram_total_gib: 0, cpu_cores: 0}, runtime: null};
let CTRL_TAB = "node";
let CURRENT_CTRL = null;
let LIVE_INFERENCE_FAST_POLL = null;
const METAL_SLOT_DRAFT = {};

const isMetalSlot = (node) => node.kind === "agent" && node.backend?.backend_kind === "metal";
const metalMode = () => CACHE.nodes.some(isMetalSlot);
const visibleNodes = () => metalMode()
  ? CACHE.nodes.filter((node) => node.kind !== "local")
  : CACHE.nodes;
const slotName = (node) => isMetalSlot(node)
  ? `Slot ${node.logical_slot || "?"}`
  : node.name;

function runtimeText(runtime) {
  if (!runtime) return "protocol not reported";
  return `unit ${runtime.unit_version || "unknown"} / pack ${runtime.runtime_pack_version || runtime.unit_version || "unknown"} / llama.cpp ${runtime.llama_cpp_version || "unknown"} / rpc ${runtime.rpc_abi || "unknown"}`;
}

function backendText(backend) {
  if (!backend) return "backend not reported";
  const kind = backend.backend_kind || backend.llama_cpp_backend || "unknown";
  const runtime = backend.backend_runtime_version ? ` / runtime ${backend.backend_runtime_version}` : "";
  const driver = backend.backend_driver_version ? ` / driver ${backend.backend_driver_version}` : "";
  const device = backend.backend_device ? ` / ${backend.backend_device}` : "";
  return `${kind}${runtime}${driver}${device}`;
}

function platformText(host) {
  if (!host) return "unknown";
  const system = host.system || "unknown";
  const machine = host.machine ? ` / ${host.machine}` : "";
  const hostname = host.hostname ? ` / ${host.hostname}` : "";
  return `${system}${machine}${hostname}`;
}

function compatText(report) {
  if (!report) return "unknown";
  if (report.not_applicable) return "not applicable";
  if (report.compatible) {
    const drift = (report.warnings || []).map((m) => m.field).join(", ");
    return drift ? `compatible (version drift: ${drift})` : "compatible";
  }
  return (report.mismatches || []).map((m) => m.field).join(", ") || "mismatch";
}

async function loadGpus(selectEl) {
  const data = await api("GET", "/api/gpus");
  const gpus = data.gpus || [];
  CACHE.gpus = gpus;
  CACHE.system = data.system || CACHE.system;
  selectEl.innerHTML = "";
  for (const g of gpus) {
    selectEl.appendChild(el(`<option value="${esc(g.uuid)}">${esc(g.name)} (${g.vram_total_gib} GiB)</option>`));
  }
  if (!gpus.length) selectEl.appendChild(el(`<option value="">No local GPUs found</option>`));
}

function selectedGpu(selectEl) {
  return CACHE.gpus.find((g) => g.uuid === selectEl.value) || null;
}

function resourceMax(kind, nd, selectEl) {
  if (kind === "vram") return wholeMax(selectedGpu(selectEl)?.vram_total_gib || nd.vram);
  if (kind === "ram") return wholeMax(CACHE.system?.ram_total_gib || nd.ram);
  if (kind === "cores") return wholeMax(CACHE.system?.cpu_cores || nd.cores);
  return 0;
}

function setFillMeter(input, value, max) {
  const p = max > 0 ? Math.max(0, Math.min(100, Math.round(value / max * 100))) : 0;
  input.style.setProperty("--fill-pct", `${p}%`);
}

function wireBudgetControl(card, testid, value, max, locked, writeValue) {
  const input = $(`[data-testid="${testid}"]`, card);
  const slider = $(`[data-testid="${testid}-slider"]`, card);
  if (!input || !slider) return;
  const current = writeValue ? value : input.value;
  const next = clampWhole(current, max);
  input.min = "0";
  input.step = "1";
  input.max = max > 0 ? String(max) : "";
  slider.min = "0";
  slider.step = "1";
  slider.max = String(max);
  input.value = String(next);
  slider.value = String(next);
  input.disabled = locked;
  slider.disabled = locked || max <= 0;
  setFillMeter(input, next, max);
  const sync = (source) => {
    const synced = clampWhole(source.value, max);
    input.value = String(synced);
    slider.value = String(synced);
    setFillMeter(input, synced, max);
  };
  input.oninput = () => sync(input);
  slider.oninput = () => sync(slider);
}

async function refreshSidebar() {
  const [nodes, ctrls, runtime] = await Promise.all([
    api("GET", "/api/nodes"),
    api("GET", "/api/controllers"),
    api("GET", "/api/runtime"),
  ]);
  CACHE.nodes = nodes.nodes;
  CACHE.ctrls = ctrls.controllers;
  CACHE.runtime = runtime;
  const version = $("#unit-version");
  setTextIfChanged(version, runtimeText(runtime.runtime));

  const nl = $("#node-list");
  const nodeHtml = visibleNodes().map((nd) => {
    const isDockerSlot = nd.kind === "local";
    const isMetalSlice = nd.kind === "agent" && nd.backend?.backend_kind === "metal";
    const remoteBusy = nd.kind === "remote_unit_node" && nd.remote_controller_id;
    const statusClass = nd.bound_to ? "on" : remoteBusy ? "info" : nd.assigned ? (nd.kind === "remote" ? "info" : "") : "";
    const slot = nd.slot ? `Docker port ${nd.rpc_port}` : (nd.rpc_endpoint || "");
    const subtitle = isDockerSlot && !nd.assigned
      ? `${slot} - Docker GPU not assigned`
      : isMetalSlice
      ? `Apple Metal - ${nd.vram} GiB / ${nd.ram} GiB / ${nd.cores} cores${nd.bound_to_name ? " - " + nd.bound_to_name : " - free"}`
      : nd.kind === "remote"
      ? `${nd.rpc_endpoint}${nd.bound_to_name ? " - " + nd.bound_to_name : " - free"}`
      : remoteBusy
      ? `${nd.gpu_name} - ${nd.remote_unit_name || "remote unit"}:${nd.remote_controller_name || nd.remote_controller_id}`
      : `${nd.gpu_name}${nd.bound_to_name ? " - " + nd.bound_to_name : " - free"}`;
    const selected = sel.type === "node" && sel.id === nd.id ? " sel" : "";
    return `<li class="${selected.trim()}" data-testid="node-item" data-id="${esc(nd.id)}" data-kind="${esc(nd.kind)}" data-assigned="${nd.assigned ? "true" : "false"}">
      <span class="dot ${statusClass}"></span>
      <span><span class="name">${esc(slotName(nd))}</span><br><span class="sub">${esc(subtitle)}</span></span>
    </li>`;
  }).join("");
  if (setHtmlIfChanged(nl, nodeHtml)) {
    $$("[data-testid=\"node-item\"]", nl).forEach((li) => {
      li.onclick = () => select("node", li.dataset.id);
    });
  }

  const cl = $("#ctrl-list");
  const ctrlHtml = ctrls.controllers.map((cd) => {
    const cls = cd.phase === "running" ? "on" : cd.phase === "loading" ? "load" : cd.phase === "error" ? "err" : "";
    const selected = sel.type === "ctrl" && sel.id === cd.id ? " sel" : "";
    return `<li class="${selected.trim()}" data-testid="ctrl-item" data-id="${esc(cd.id)}">
      <span class="dot ${cls}"></span>
      <span><span class="name">${esc(cd.name)}</span><br><span class="sub">${cd.nodes.length} node(s)${cd.serving ? " - " + esc(cd.phase) : ""}</span></span>
    </li>`;
  }).join("");
  if (setHtmlIfChanged(cl, ctrlHtml)) {
    $$("[data-testid=\"ctrl-item\"]", cl).forEach((li) => {
      li.onclick = () => select("ctrl", li.dataset.id);
    });
  }
}

$('[data-testid="add-ctrl-btn"]').onclick = () => { if (uiLocked()) { walletSetOpen(true); return; } $("#add-ctrl").classList.toggle("hidden"); };
$('[data-testid="ctrl-create-btn"]').onclick = async () => {
  if (uiLocked()) { walletSetOpen(true); return; }
  try {
    const cd = await api("POST", "/api/controllers", {name: $('[data-testid="ctrl-name"]').value});
    $("#add-ctrl").classList.add("hidden");
    $('[data-testid="ctrl-name"]').value = "";
    await refreshSidebar();
    select("ctrl", cd.id);
  } catch (e) {
    alert("create controller: " + e.message);
  }
};
$("#firewall-menu").onclick = () => select("firewall", "guide");
$("#runtime-menu").onclick = () => select("runtime", "overview");

// ---- Sign-In With Solana (only when the hub enables an admin allowlist) ----
// Fail closed: assume auth is required until /api/auth/status proves otherwise,
// so no menu can be opened in the window before the first status round-trip.
let AUTH_ENABLED = true, AUTHED = false, SESSION_WALLET = null;
function uiLocked() { return AUTH_ENABLED && !AUTHED; }
async function checkAuth() {
  let st;
  try { st = await api("GET", "/api/auth/status"); } catch (e) { return !uiLocked(); } // status unreachable => keep current lock state
  AUTH_ENABLED = !!st.enabled;
  const authed = !st.enabled || st.authenticated;
  AUTHED = authed; SESSION_WALLET = st.wallet || null;
  const gate = $("#auth-gate");
  if (gate) gate.classList.toggle("hidden", authed || !AUTH_ENABLED);
  // Lock the whole app shell (sidebar + main) when sign-in is required. The header
  // sign-in UI stays usable (it lives outside .app). UX / defense-in-depth only —
  // the real boundary is the server (every control API returns 401 unauthenticated).
  const app = document.querySelector(".app");
  if (app) app.classList.toggle("locked", AUTH_ENABLED && !authed);
  if (AUTH_ENABLED && !authed && sel.type) {
    // Session ended (or never started): close any open panel and return to Welcome.
    sel = {type: null, id: null};
    const main = $("#main");
    if (main) main.innerHTML = WELCOME_HTML;
  }
  hwInit();
  hwRenderAuthState();
  if (AUTH_ENABLED && !authed) walletSetOpen(true); // nudge the operator to sign in
  return authed;
}
const b64FromBytes = (u8) => btoa(String.fromCharCode.apply(null, u8));
// After a wallet signature verifies, the server either issues a session (reload)
// or, if the wallet has 2FA enabled, returns {twofa_required, pre_auth} — in which
// case we show the code prompt and defer the reload to the 2FA step.
function handleVerifyResult(res) {
  if (res && res.twofa_required) { twofaLoginShow(res.pre_auth); return; }
  location.reload();
}
let PRE_AUTH = null;
function twofaLoginShow(preAuth) {
  PRE_AUTH = preAuth;
  ["hw-setup", "hw-unlock", "hw-account"].forEach((id) => { const el = $("#" + id); if (el) el.classList.add("hidden"); });
  const ext = $("#auth-external"); if (ext) ext.classList.add("hidden");
  const box = $("#hw-2fa-login"); if (box) box.classList.remove("hidden");
  hwMsg("");
  const c = $("#hw-2fa-login-code"); if (c) { c.value = ""; c.focus(); }
}
async function twofaLoginSubmit() {
  const code = ($("#hw-2fa-login-code").value || "").trim();
  if (!code) return;
  try {
    await api("POST", "/api/auth/2fa/login", { pre_auth: PRE_AUTH, code });
    location.reload();
  } catch (e) { hwMsg("2FA failed: " + (e.message || e)); }
}
function twofaLoginCancel() { PRE_AUTH = null; const box = $("#hw-2fa-login"); if (box) box.classList.add("hidden"); const ext = $("#auth-external"); if (ext) ext.classList.remove("hidden"); hwInit(); }
async function siwsSign() {
  const msg = $("#auth-msg");
  const provider = window.solana || (window.phantom && window.phantom.solana);
  try {
    let wallet = $("#auth-wallet").value.trim();
    if (provider) {
      const res = await provider.connect();
      wallet = res.publicKey.toString();
      $("#auth-wallet").value = wallet;
    }
    if (!wallet) { msg.textContent = "Enter your admin wallet address."; return; }
    msg.textContent = "Requesting challenge…";
    const ch = await api("POST", "/api/auth/challenge", { wallet });
    if (provider && provider.signMessage) {
      const signed = await provider.signMessage(new TextEncoder().encode(ch.message), "utf8");
      const sig = signed.signature ? new Uint8Array(signed.signature) : new Uint8Array(signed);
      const res = await api("POST", "/api/auth/verify", { wallet, nonce: ch.nonce, signature: b64FromBytes(sig) });
      handleVerifyResult(res);
    } else {
      // No injected wallet: show the message to sign externally + paste the signature.
      $("#auth-message").value = ch.message;
      const m = $("#auth-manual");
      m.dataset.nonce = ch.nonce; m.dataset.wallet = wallet;
      m.classList.remove("hidden");
      msg.textContent = "Sign the message with your wallet, then paste the base64 signature.";
    }
  } catch (e) { msg.textContent = "Sign-in failed: " + (e.message || e); }
}
async function siwsVerifyManual() {
  const m = $("#auth-manual"), msg = $("#auth-msg");
  try {
    const res = await api("POST", "/api/auth/verify", { wallet: m.dataset.wallet, nonce: m.dataset.nonce, signature: $("#auth-sig").value.trim() });
    handleVerifyResult(res);
  } catch (e) { msg.textContent = "Verify failed: " + (e.message || e); }
}
if ($("#auth-connect")) $("#auth-connect").onclick = siwsSign;
if ($("#auth-verify")) $("#auth-verify").onclick = siwsVerifyManual;

// ---- built-in non-custodial hub wallet (window.LinkcppHubWallet) -----------
// Create/import a Kvasir-compatible wallet in the browser, see its KVR balance,
// and sign the SIWS challenge locally — no extension needed.
const HW_LS = "linkcpp_hub_wallet";
const HW = { mnemonic: null, address: null };
const hwEnvelope = () => { try { return JSON.parse(localStorage.getItem(HW_LS) || "null"); } catch (e) { return null; } };
const hwMsg = (t) => { const m = $("#auth-msg"); if (m) m.textContent = t; };
function hwShow(which) {
  ["hw-setup", "hw-unlock", "hw-account"].forEach((id) => { const el = $("#" + id); if (el) el.classList.toggle("hidden", id !== which); });
}
function hwInit() {
  if (!window.LinkcppHubWallet) { hwMsg("wallet module failed to load"); return; }
  if (HW.mnemonic) return hwShow("hw-account");
  hwShow(hwEnvelope() ? "hw-unlock" : "hw-setup");
}
function hwTab(which) {
  $("#hw-tab-create").classList.toggle("on", which === "create");
  $("#hw-tab-import").classList.toggle("on", which === "import");
  $("#hw-create-pane").classList.toggle("hidden", which !== "create");
  $("#hw-import-pane").classList.toggle("hidden", which !== "import");
}
async function hwLoadBalance() {
  const b = $("#hw-balance"), g = $("#hw-gate");
  b.textContent = "…"; g.textContent = "";
  try {
    const r = await api("GET", "/api/auth/kvr-balance?wallet=" + encodeURIComponent(HW.address));
    b.textContent = (r.balance == null ? "—" : r.balance) + " KVR" + (r.min ? " (need " + r.min + ")" : "");
    if (r.ok) { g.textContent = r.admin ? "✓ admin wallet — may operate" : "✓ meets the KVR requirement"; g.style.color = "#3cbf8e"; }
    else { g.textContent = r.gated ? ("✗ needs ≥ " + r.min + " KVR to operate") : "✗ not authorized"; g.style.color = "#e66"; }
  } catch (e) { b.textContent = "?"; }
}
async function hwUnlockWith(mnemonic) {
  HW.mnemonic = mnemonic;
  HW.address = window.LinkcppHubWallet.addressFromMnemonic(mnemonic);
  $("#hw-address").textContent = HW.address;
  hwShow("hw-account");
  hwRenderAuthState();
  await hwLoadBalance();
}
function hwGenerate() { $("#hw-mnemonic-new").value = window.LinkcppHubWallet.generateMnemonic(); }
async function hwSave() {
  try {
    const importing = !$("#hw-import-pane").classList.contains("hidden");
    const mnemonic = (importing ? $("#hw-mnemonic-in").value : $("#hw-mnemonic-new").value).trim();
    if (!window.LinkcppHubWallet.validateMnemonic(mnemonic)) return hwMsg("Invalid recovery phrase.");
    const pass = $("#hw-pass-new").value;
    if (!pass || pass.length < 8) return hwMsg("Passphrase must be at least 8 characters.");
    localStorage.setItem(HW_LS, JSON.stringify(window.LinkcppHubWallet.seal(mnemonic, pass)));
    hwMsg(""); await hwUnlockWith(mnemonic);
  } catch (e) { hwMsg("Save failed: " + (e.message || e)); }
}
async function hwUnlock() {
  try {
    const env = hwEnvelope(); if (!env) return hwShow("hw-setup");
    const mnemonic = window.LinkcppHubWallet.open(env, $("#hw-pass").value);
    hwMsg(""); await hwUnlockWith(mnemonic);
  } catch (e) { hwMsg("Wrong passphrase."); }
}
function hwForget() { if (confirm("Remove this wallet from this device? Make sure you saved the recovery phrase.")) { localStorage.removeItem(HW_LS); HW.mnemonic = null; HW.address = null; hwShow("hw-setup"); } }
function hwLock() { HW.mnemonic = null; hwShow("hw-unlock"); }
async function hwSignIn() {
  try {
    if (!HW.mnemonic) return;
    hwMsg("Requesting challenge…");
    const ch = await api("POST", "/api/auth/challenge", { wallet: HW.address });
    const sig = window.LinkcppHubWallet.signMessage(HW.mnemonic, new TextEncoder().encode(ch.message));
    const res = await api("POST", "/api/auth/verify", { wallet: HW.address, nonce: ch.nonce, signature: b64FromBytes(sig) });
    handleVerifyResult(res);
  } catch (e) { hwMsg("Sign-in failed: " + (e.message || e)); }
}
if ($("#hw-tab-create")) $("#hw-tab-create").onclick = () => hwTab("create");
if ($("#hw-tab-import")) $("#hw-tab-import").onclick = () => hwTab("import");
if ($("#hw-generate")) $("#hw-generate").onclick = hwGenerate;
if ($("#hw-save")) $("#hw-save").onclick = hwSave;
if ($("#hw-unlock-btn")) $("#hw-unlock-btn").onclick = hwUnlock;
// Enter in a passphrase field submits (setup -> save, unlock -> unlock).
if ($("#hw-pass-new")) $("#hw-pass-new").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); hwSave(); } });
if ($("#hw-pass")) $("#hw-pass").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); hwUnlock(); } });
if ($("#hw-forget")) $("#hw-forget").onclick = hwForget;
if ($("#hw-lock")) $("#hw-lock").onclick = hwLock;
if ($("#hw-signin")) $("#hw-signin").onclick = hwSignIn;

// ---- wallet header dropdown -----------------------------------------------
function walletSetOpen(open) { const dd = $("#wallet-dropdown"); if (dd) dd.classList.toggle("hidden", !open); }
function walletToggle() { const dd = $("#wallet-dropdown"); if (dd) dd.classList.toggle("hidden"); }
function hwRenderAuthState() {
  const signedIn = AUTH_ENABLED && AUTHED;
  const toggle = $("#wallet-toggle"); if (toggle) toggle.classList.toggle("connected", signedIn);
  const label = $("#wallet-label");
  const addr = HW.address || SESSION_WALLET || "";
  if (label) label.textContent = addr ? (addr.slice(0, 4) + "…" + addr.slice(-4)) : "";
  const row = $("#hw-signin-row"), done = $("#hw-signedin");
  if (row) row.classList.toggle("hidden", signedIn);
  if (done) done.classList.toggle("hidden", !signedIn);
  if (signedIn) twofaRefresh();
}

// ---- 2FA enrollment / management (signed-in only) -------------------------
// The auth state re-renders every ~2s (the polling tick calls hwRenderAuthState).
// While an enrollment is in progress the panel shows the secret + backup codes the
// user must save, so this flag stops the periodic re-render from hiding it.
let TWOFA_ENROLLING = false;
async function twofaRefresh() {
  try {
    const st = await api("GET", "/api/auth/2fa/status");
    twofaRender(!!st.enabled);
  } catch (e) { /* not signed in yet */ }
}
function twofaRender(enabled) {
  if (enabled) TWOFA_ENROLLING = false;   // enrollment finished (or already on)
  if (TWOFA_ENROLLING) return;            // don't clobber the in-progress enroll panel
  const state = $("#hw-2fa-state"); if (state) { state.textContent = enabled ? "✓ on" : "off"; state.style.color = enabled ? "#3cbf8e" : ""; }
  const enable = $("#hw-2fa-enable"); if (enable) enable.classList.toggle("hidden", enabled);
  const enroll = $("#hw-2fa-enroll"); if (enroll) enroll.classList.add("hidden");
  const dis = $("#hw-2fa-disable-row"); if (dis) dis.classList.toggle("hidden", !enabled);
}
// Render the otpauth URI as a scannable QR into the enroll canvas. Bundled
// generator (window.QRCode from /web/vendor/qrcode.js); degrades to the setup key
// if it isn't available.
function twofaRenderQR(uri) {
  const cv = $("#hw-2fa-qr");
  if (!cv || !window.QRCode) { if (cv) cv.style.display = "none"; return; }
  window.QRCode.toCanvas(cv, uri, { margin: 1, width: 200, color: { dark: "#0c0d10", light: "#ffffff" } }, (err) => {
    if (err) { cv.style.display = "none"; }
  });
}
async function twofaEnroll() {
  try {
    const r = await api("POST", "/api/auth/2fa/enroll", {});
    $("#hw-2fa-secret").textContent = r.secret;
    $("#hw-2fa-uri").textContent = r.otpauth_uri;
    $("#hw-2fa-codes").textContent = (r.backup_codes || []).join("\n");
    twofaRenderQR(r.otpauth_uri);
    $("#hw-2fa-enable").classList.add("hidden");
    $("#hw-2fa-enroll").classList.remove("hidden");
    TWOFA_ENROLLING = true;   // keep the panel open against the 2s refresh
    $("#hw-2fa-confirm-code").focus();
  } catch (e) { hwMsg("Enroll failed: " + (e.message || e)); }
}
async function twofaConfirm() {
  const code = ($("#hw-2fa-confirm-code").value || "").trim();
  try {
    await api("POST", "/api/auth/2fa/confirm", { code });
    TWOFA_ENROLLING = false;
    hwMsg("Two-factor auth enabled.");
    twofaRender(true);
  } catch (e) { hwMsg("Confirm failed: " + (e.message || e)); }
}
async function twofaDisable() {
  const code = ($("#hw-2fa-disable-code").value || "").trim();
  try {
    await api("POST", "/api/auth/2fa/disable", { code });
    hwMsg("Two-factor auth disabled.");
    twofaRender(false);
  } catch (e) { hwMsg("Disable failed: " + (e.message || e)); }
}
if ($("#hw-2fa-enable")) $("#hw-2fa-enable").onclick = twofaEnroll;
if ($("#hw-2fa-confirm")) $("#hw-2fa-confirm").onclick = twofaConfirm;
if ($("#hw-2fa-disable")) $("#hw-2fa-disable").onclick = twofaDisable;
if ($("#hw-2fa-login-btn")) $("#hw-2fa-login-btn").onclick = twofaLoginSubmit;
if ($("#hw-2fa-login-cancel")) $("#hw-2fa-login-cancel").onclick = twofaLoginCancel;
if ($("#hw-2fa-login-code")) $("#hw-2fa-login-code").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); twofaLoginSubmit(); } });
if ($("#hw-2fa-confirm-code")) $("#hw-2fa-confirm-code").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); twofaConfirm(); } });

async function hwLogout() { try { await api("POST", "/api/auth/logout", {}); } catch (e) {} location.reload(); }
if ($("#wallet-toggle")) $("#wallet-toggle").onclick = (e) => { e.stopPropagation(); walletToggle(); };
if ($("#hw-logout")) $("#hw-logout").onclick = hwLogout;
document.addEventListener("click", (e) => {
  const menu = $(".wallet-menu");
  if (menu && !menu.contains(e.target)) walletSetOpen(false);
});

function select(type, id) {
  if (uiLocked()) { walletSetOpen(true); return; }  // no menu opens before wallet+2FA sign-in
  sel = {type, id};
  refreshSidebar();
  renderMain();
}

async function renderMain() {
  if (uiLocked()) return;  // defense in depth: nothing renders into #main while locked
  const main = $("#main");
  if (sel.type === "node") main.innerHTML = nodeShell();
  else if (sel.type === "ctrl") main.innerHTML = ctrlShell();
  else if (sel.type === "firewall") {
    main.innerHTML = firewallShell();
    wireFirewall();
    return;
  } else if (sel.type === "runtime") {
    main.innerHTML = runtimeShell();
    renderRuntime();
    wireRuntime();
    return;
  } else return;
  await updateDetail();
  if (sel.type === "ctrl") wireCtrl();
}

function nodeShell() {
  return `<div data-testid="node-detail">
    <div class="detail-head">
      <div class="title-edit">
        <h2 id="nd-title">node</h2>
        <input id="nd-name-input" class="title-name-input hidden" data-testid="node-name" placeholder="optional"/>
        <button type="button" class="icon-btn" id="nd-name-edit" data-testid="node-name-edit" title="Edit slot name" aria-label="Edit slot name">&#9998;</button>
      </div>
      <span class="badge" id="nd-slot"></span>
    </div>
    <div class="card" id="nd-config-card" data-testid="node-config">
      <h3>Slot assignment</h3>
      <div class="node-form">
        <label>GPU <select data-testid="node-gpu"></select></label>
        <label class="resource-control">VRAM
          <span class="budget-control">
            <input class="fill-meter" data-testid="node-vram" type="number" value="0"/>
            <input class="range-meter" data-testid="node-vram-slider" type="range" value="0"/>
          </span>
        </label>
        <label class="resource-control">RAM
          <span class="budget-control">
            <input class="fill-meter ram" data-testid="node-ram" type="number" value="0"/>
            <input class="range-meter ram" data-testid="node-ram-slider" type="range" value="0"/>
          </span>
        </label>
        <label class="resource-control">Cores
          <span class="budget-control">
            <input class="fill-meter cores" data-testid="node-cores" type="number" value="0"/>
            <input class="range-meter cores" data-testid="node-cores-slider" type="range" value="0"/>
          </span>
        </label>
      </div>
      <p class="muted tight" id="nd-edit-note"></p>
      <div class="actions start">
        <button type="button" data-testid="node-save-btn" id="nd-save">Save slot</button>
        <button type="button" class="danger" data-testid="node-clear-btn" id="nd-clear">Clear assignment</button>
      </div>
    </div>
    <div class="card hidden" id="nd-metal-config-card" data-testid="metal-node-config">
      <h3>Mac Metal — Slot 1 resources</h3>
      <div class="node-form">
        <label class="resource-control">RAM / Metal budget GiB
          <span class="budget-control">
            <input class="fill-meter ram" data-testid="metal-node-ram" type="number" min="1" step="1"/>
            <input class="range-meter ram" data-testid="metal-node-ram-slider" type="range" min="1" step="1"/>
          </span>
        </label>
        <label class="resource-control">CPU cores
          <span class="budget-control">
            <input class="fill-meter cores" data-testid="metal-node-cores" type="number" min="1" step="1"/>
            <input class="range-meter cores" data-testid="metal-node-cores-slider" type="range" min="1" step="1"/>
          </span>
        </label>
      </div>
      <p class="muted tight" data-testid="metal-node-note"></p>
      <div class="actions start"><button type="button" data-testid="metal-node-save">Apply Slot 1</button></div>
    </div>
    <div class="card"><h3>Resources</h3>
      <div class="kv"><b>Type</b><span id="nd-kind"></span></div>
      <div class="kv"><b>Controller</b><span id="nd-bound"></span></div>
      <div class="kv"><b>GPU</b><span id="nd-gpu"></span></div>
      <div class="kv"><b>RPC endpoint</b><span id="nd-rpc"></span></div>
      <div class="kv"><b>Protocol</b><span id="nd-runtime"></span></div>
      <div class="kv"><b>Protocol check</b><span id="nd-runtime-compat"></span></div>
      <div class="kv"><b>Backend</b><span id="nd-backend"></span></div>
      <div class="kv"><b>Host</b><span id="nd-host"></span></div>
      <p class="muted tight">VRAM (used / budget)</p>
      <div class="bar" data-testid="node-vram-bar"><span id="nd-vram-fill"></span><label id="nd-vram-lbl"></label></div>
      <p class="muted tight">RAM for CPU offload (used / budget)</p>
      <div class="bar ram" data-testid="node-ram-bar"><span id="nd-ram-fill"></span><label id="nd-ram-lbl"></label></div>
    </div>
    <div class="card"><h3>Live log <span class="muted">(load and inference)</span></h3>
      <div class="log" id="nd-log" data-testid="node-log"></div></div>
  </div>`;
}

async function updateNode() {
  const nd = CACHE.nodes.find((x) => x.id === sel.id);
  if (!nd) {
    sel = {type: null, id: null};
    $("#main").innerHTML = "";
    return;
  }
  const detail = await api("GET", "/api/nodes/" + sel.id + "/logs").catch(() => null);
  if (!detail) return;
  const nameInput = $('[data-testid="node-name"]');
  const nodeDetail = $('[data-testid="node-detail"]');
  const writingDetail = !nodeDetail?.contains(document.activeElement);
  if (writingDetail) {
    if (nameInput.dataset.dirty === "true") {
      $("#nd-title").textContent = nameInput.value.trim() || nd.name;
    } else {
      $("#nd-title").textContent = slotName(nd);
      nameInput.value = slotName(nd);
    }
  }
  const isMetalSlice = isMetalSlot(nd);
  $("#nd-slot").textContent = nd.slot ? `Docker slot / port ${nd.rpc_port}` : `Logical slot ${nd.logical_slot || "?"}`;
  $("#nd-kind").textContent = isMetalSlice ? "Apple Metal"
    : nd.kind === "agent" ? "Native managed node"
    : nd.kind === "remote" ? "Remote RPC node"
    : nd.kind === "remote_unit_node" ? "Remote unit node"
    : "Docker-local GPU slot";
  $("#nd-bound").textContent = nd.bound_to_name
    || (nd.remote_controller_id ? `${nd.remote_unit_name || "remote unit"}:${nd.remote_controller_name || nd.remote_controller_id}` : "not bound");
  $("#nd-gpu").textContent = nd.assigned ? `${nd.gpu_name}${nd.gpu_uuid ? " - " + nd.gpu_uuid : ""}` : "not assigned";
  $("#nd-rpc").textContent = nd.rpc_endpoint;
  $("#nd-runtime").textContent = runtimeText(nd.runtime);
  $("#nd-runtime-compat").textContent = compatText(nd.runtime_compatibility);
  $("#nd-backend").textContent = backendText(nd.backend);
  $("#nd-host").textContent = platformText(nd.host_platform);
  const vp = pct(detail.vram_used_gib, nd.vram);
  $("#nd-vram-fill").style.width = vp + "%";
  $("#nd-vram-lbl").textContent = `${detail.vram_used_gib} / ${nd.vram} GiB (${vp}%)`;
  const rp = pct(detail.ram_used_gib, nd.ram || 1);
  $("#nd-ram-fill").style.width = (nd.ram ? rp : 0) + "%";
  $("#nd-ram-lbl").textContent = `${detail.ram_used_gib} / ${nd.ram} GiB`;
  const log = $("#nd-log");
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
  log.textContent = detail.log || "(no activity yet)";
  if (atBottom) log.scrollTop = log.scrollHeight;
  await wireNodeConfig(nd);
  wireMetalNodeConfig(nd);
}

function wireMetalNodeConfig(nd) {
  const card = $("#nd-metal-config-card");
  if (!card) return;
  const metal = isMetalSlot(nd);
  card.classList.toggle("hidden", !metal);
  if (!metal) return;
  const ram = $("[data-testid=metal-node-ram]", card);
  const ramSlider = $("[data-testid=metal-node-ram-slider]", card);
  const cores = $("[data-testid=metal-node-cores]", card);
  const coresSlider = $("[data-testid=metal-node-cores-slider]", card);
  const save = $("[data-testid=metal-node-save]", card);
  const note = $("[data-testid=metal-node-note]", card);
  const controller = nd.bound_to;
  const draft = METAL_SLOT_DRAFT[nd.id] || {};
  const controllerState = CACHE.ctrls.find((item) => item.id === controller);
  const locked = !controller || (controllerState && controllerState.phase !== "idle" && controllerState.phase !== "error");
  const ramMax = wholeMax(nd.resources?.ram_total_gib || nd.ram || 1);
  const coresMax = wholeMax(nd.resources?.cores_total || nd.cores || 1);
  const configure = (input, slider, value, max, field) => {
    const next = Math.max(1, Math.min(max, Math.round(Number(value) || 1)));
    input.max = String(max); slider.max = String(max);
    input.value = String(next); slider.value = String(next);
    setFillMeter(input, next, max);
    const sync = (source) => {
      const current = Math.max(1, Math.min(max, Math.round(Number(source.value) || 1)));
      input.value = String(current); slider.value = String(current);
      setFillMeter(input, current, max);
    };
    const updateDraft = (source) => {
      sync(source);
      METAL_SLOT_DRAFT[nd.id] = {...(METAL_SLOT_DRAFT[nd.id] || {}), [field]: Number(input.value)};
      note.textContent = "Unsaved change — click Apply Slot 1 to save.";
    };
    input.oninput = () => updateDraft(input);
    slider.oninput = () => updateDraft(slider);
  };
  configure(ram, ramSlider, draft.ram_budget_gib ?? nd.ram, ramMax, "ram_budget_gib");
  configure(cores, coresSlider, draft.cores_budget ?? nd.cores, coresMax, "cores_budget");
  ram.disabled = locked;
  ramSlider.disabled = locked;
  cores.disabled = locked;
  coresSlider.disabled = locked;
  save.disabled = locked;
  note.textContent = !controller ? "Bind Slot 1 to a controller before setting its budget."
    : locked ? "Unload the controller before changing this budget."
    : Object.keys(draft).length ? "Unsaved change — click Apply Slot 1 to save."
    : "One unified-memory budget is used for both RAM and Metal planning.";
  const apply = async () => {
    const ram_budget_gib = Number(ram.value || 0);
    const cores_budget = Number(cores.value || 0);
    if (!(ram_budget_gib > 0 && cores_budget > 0)) return alert("Set positive RAM and CPU-core budgets.");
    save.disabled = true;
    note.textContent = "Applying Slot 1 budget…";
    try {
      await api("POST", `/api/controllers/${controller}/metal-slot/resources`, {
        ram_budget_gib,
        cores_budget: Math.floor(cores_budget),
        node_id: nd.id,
      });
      delete METAL_SLOT_DRAFT[nd.id];
      note.textContent = "Slot 1 budget applied.";
      await refreshSidebar();
    } catch (error) {
      note.textContent = "Could not apply the budget.";
      alert(error.message);
    } finally {
      save.disabled = locked;
    }
  };
  save.onclick = apply;
}

async function wireNodeConfig(nd) {
  const card = $("#nd-config-card");
  if (!card) return;
  const isLocal = nd.kind === "local";
  card.classList.toggle("hidden", !isLocal);
  if (!isLocal) return;

  const select = $('[data-testid="node-gpu"]', card);
  if (!select.options.length) await loadGpus(select);

  const formFields = $$("input, select", card);
  const locked = Boolean(nd.bound_to);
  for (const f of formFields) f.disabled = locked;
  const nameInput = $('[data-testid="node-name"]');
  const nameEdit = $('[data-testid="node-name-edit"]');
  if (nameInput) nameInput.disabled = locked;
  if (nameEdit) nameEdit.disabled = locked;
  $("#nd-save").disabled = locked;
  $("#nd-clear").disabled = locked || !nd.assigned;
  $("#nd-edit-note").textContent = locked
    ? "Unbind this node before changing its name or resource allocation."
    : "Resource allocation is editable while the slot is unbound.";

  const detailEl = $('[data-testid="node-detail"]');
  const writeValue = !detailEl?.contains(document.activeElement);
  if (writeValue) {
    select.value = nd.gpu_uuid || select.value;
  }
  wireBudgetControl(card, "node-vram", nd.vram || 0, resourceMax("vram", nd, select), locked, writeValue);
  wireBudgetControl(card, "node-ram", nd.ram || 0, resourceMax("ram", nd, select), locked, writeValue);
  wireBudgetControl(card, "node-cores", nd.cores || 0, resourceMax("cores", nd, select), locked, writeValue);

  select.onchange = () => {
    wireBudgetControl(card, "node-vram", $('[data-testid="node-vram"]', card).value, resourceMax("vram", nd, select), locked, false);
  };

  if (nameEdit && nameInput) {
    nameEdit.onclick = () => {
      $("#nd-title").classList.add("hidden");
      nameInput.classList.remove("hidden");
      nameInput.focus();
      nameInput.select();
    };
    nameInput.onblur = () => {
      $("#nd-title").textContent = nameInput.value.trim() || nd.name;
      nameInput.classList.add("hidden");
      $("#nd-title").classList.remove("hidden");
    };
    nameInput.oninput = () => {
      nameInput.dataset.dirty = "true";
    };
    nameInput.onkeydown = (e) => {
      if (e.key === "Enter") nameInput.blur();
      if (e.key === "Escape") {
        nameInput.value = nd.name;
        nameInput.dataset.dirty = "false";
        nameInput.blur();
      }
    };
  }

  $("#nd-save").onclick = async () => {
    try {
      const gpu = select.value;
      if (!gpu) throw new Error("no local GPU selected");
      const saved = await api("POST", "/api/nodes", {
        slot_id: nd.id,
        gpu_uuid: gpu,
        name: ($('[data-testid="node-name"]')?.value || "").trim(),
        vram: +$('[data-testid="node-vram"]', card).value,
        ram: +$('[data-testid="node-ram"]', card).value,
        cores: +$('[data-testid="node-cores"]', card).value,
      });
      const nameInput = $('[data-testid="node-name"]');
      if (nameInput) nameInput.dataset.dirty = "false";
      await refreshSidebar();
      sel = {type: "node", id: saved.id};
      await updateNode();
    } catch (e) {
      alert("save node slot: " + e.message);
    }
  };
  $("#nd-clear").onclick = async () => {
    if (!confirm("Clear this node slot assignment?")) return;
    try {
      await api("DELETE", "/api/nodes/" + nd.id);
      await refreshSidebar();
      await updateNode();
    } catch (e) {
      alert("clear node slot: " + e.message);
    }
  };
}

function ctrlShell() {
  return `<div data-testid="ctrl-detail">
    <div class="detail-head">
      <h2 id="cd-title">controller</h2>
      <button class="danger" id="cd-del">Delete controller</button>
    </div>
    <div class="top-tabs" data-testid="ctrl-tabs">
      <button type="button" data-ctrl-tab="node" data-testid="ctrl-tab-node">Node</button>
      <button type="button" data-ctrl-tab="inference" data-testid="ctrl-tab-inference">Inference</button>
    </div>

    <section id="ctrl-node-tab" data-testid="node-tab">
      <div class="card" data-testid="remote-unit-section">
        <h3>Import unit nodes</h3>
        <div class="row">
          <label class="wide">Unit URL <input data-testid="remote-unit-url" placeholder="http://192.168.1.50:19000"/></label>
          <label>Name <input data-testid="remote-unit-name" placeholder="studio-a"/></label>
          <button type="button" data-testid="remote-unit-create">Import nodes</button>
        </div>
        <p class="muted">Importing a unit makes its available nodes appear below. Bind or unbind them exactly like local nodes; ownership is reserved in the source unit.</p>
        <div id="remote-units" data-testid="remote-units"></div>
      </div>

      <div class="card" data-testid="local-node-section">
        <h3>Nodes</h3>
        <div id="cd-nodes" data-testid="ctrl-nodes"></div>
      </div>
    </section>

    <section id="ctrl-inference-tab" class="hidden" data-testid="inference-tab">
      <details class="card collapsible-card" data-testid="api-endpoints-section">
        <summary>
          <h3>API endpoints</h3>
        </summary>
        <div id="cd-endpoints" data-testid="api-endpoints"></div>
      </details>
      <div class="card" data-testid="model-load-section">
        <h3>Model</h3>
        <p class="muted tight" id="cd-runtime-check"></p>
        <div class="runtime-mode-block" data-testid="serve-runtime-picker">
          <div class="runtime-mode-heading">
            <b>Execution mode</b>
            <span id="serve-runtime-mode-note">Stock llama.cpp master/worker RPC.</span>
          </div>
          <div class="runtime-mode-options">
            <label class="runtime-mode-option active" data-runtime-mode-option="llama_rpc">
              <input type="radio" name="serve-runtime-mode" value="llama_rpc" data-testid="serve-runtime-rpc" checked/>
              <span><b>llama.cpp RPC</b><small>Stable - master controls RPC workers</small></span>
            </label>
            <label class="runtime-mode-option" data-runtime-mode-option="ring_proxy">
              <input type="radio" name="serve-runtime-mode" value="ring_proxy" data-testid="serve-runtime-proxy"/>
              <span><b>linkcpp Proxy</b><small>Node-local layers - hidden-state ring</small></span>
            </label>
          </div>
        </div>
        <div class="row">
          <label class="wide">Model <select data-testid="serve-model"></select></label>
          <label>Context <input data-testid="serve-ctx" value="4096" size="6"/></label>
          <label>Parallel <input data-testid="serve-parallel" value="1" size="4"/></label>
          <button data-testid="serve-btn">Load</button>
          <button class="ghost hidden" data-testid="stop-btn">Unload</button>
        </div>
        <div class="row kv-cache-row">
          <label class="check locked-check">Flash Attention <input data-testid="serve-flash-attn" type="checkbox" checked disabled/></label>
          <label>K Cache Quantization Type <select data-testid="serve-cache-type-k"></select></label>
          <label>V Cache Quantization Type <select data-testid="serve-cache-type-v"></select></label>
        </div>
        <details class="perf-options">
          <summary>Distributed decode tuning</summary>
          <div class="row">
            <label>Speculation
              <select data-testid="serve-spec-type">
                <option value="none">none</option>
                <option value="ngram-cache">ngram-cache</option>
                <option value="ngram-mod">ngram-mod</option>
                <option value="ngram-simple">ngram-simple</option>
                <option value="draft-simple">draft-simple</option>
                <option value="draft-mtp">draft-mtp</option>
              </select>
            </label>
            <label>Draft model <input data-testid="serve-spec-draft-model" size="32" placeholder="/models/.../draft.gguf"/></label>
            <label>Draft max <input data-testid="serve-spec-draft-n-max" value="" size="4"/></label>
            <label>Draft min <input data-testid="serve-spec-draft-n-min" value="" size="4"/></label>
          </div>
          <div class="row">
            <label>Batch <input data-testid="serve-batch" value="" size="5"/></label>
            <label>uBatch <input data-testid="serve-ubatch" value="" size="5"/></label>
            <label>Poll <input data-testid="serve-poll" value="" size="4"/></label>
            <label>Cache reuse <input data-testid="serve-cache-reuse" value="" size="5"/></label>
            <label class="check">Continuous batching <input data-testid="serve-cont-batching" type="checkbox" checked/></label>
          </div>
          <div class="row">
            <label>Ngram min <input data-testid="serve-spec-ngram-mod-n-min" value="" size="4"/></label>
            <label>Ngram max <input data-testid="serve-spec-ngram-mod-n-max" value="" size="4"/></label>
            <label>Ngram match <input data-testid="serve-spec-ngram-mod-n-match" value="" size="4"/></label>
            <label>p-min <input data-testid="serve-spec-draft-p-min" value="" size="4"/></label>
            <label>p-split <input data-testid="serve-spec-draft-p-split" value="" size="4"/></label>
          </div>
        </details>
      </div>
      <div class="card" data-testid="loading-plan-section">
        <h3>Loading plan and activity</h3>
        <div class="chatout plan-report" data-testid="plan-verdict"></div>
        <div id="cd-load-status" data-testid="ctrl-load-status" class="load-status"></div>
        <div id="cd-ops" data-testid="ctrl-operations" class="ops"></div>
      </div>
      <div class="card" data-testid="live-inference-section">
        <h3>Live inference requests</h3>
        <div id="cd-live-inference" data-testid="live-inference" class="live-inference"></div>
      </div>
      <div class="card" data-testid="request-test-section">
        <h3>Request test</h3>
        <textarea data-testid="chat-input" rows="2" placeholder="Ask something" style="width:100%"></textarea>
        <div class="actions start">
          <button data-testid="chat-send">Send</button>
          <label>Max tokens <input data-testid="chat-max-tokens" value="512" size="5"/></label>
        </div>
        <div class="muted tight" data-testid="chat-meta"></div>
        <div class="chatout" data-testid="chat-output"></div>
      </div>
    </section>
  </div>`;
}

async function updateCtrl() {
  const c = await api("GET", "/api/controllers/" + sel.id).catch(() => null);
  if (!c) {
    sel = {type: null, id: null};
    CURRENT_CTRL = null;
    $("#main").innerHTML = "";
    return;
  }
  CURRENT_CTRL = c;
  setTextIfChanged($("#cd-title"), c.name);
  setCtrlTab(CTRL_TAB);
  renderLoadControls(c);
  renderRemoteUnits(c);
  renderCtrlNodes(c);
  const rtc = $("#cd-runtime-check");
  setTextIfChanged(rtc, c.runtime_compatibility?.compatible
    ? `Protocol check: ${runtimeText(c.runtime)}`
    : `Protocol mismatch: ${(c.runtime_compatibility?.nodes || []).map((n) => n.name || n.id).join(", ")}`);
  renderPlan(c.plan, c);
  renderOperations(c);
  if (!LIVE_INFERENCE_FAST_POLL) renderInferenceActivity(c);
  renderEndpoints(c);
}

function renderCtrlNodes(c) {
  const box = $("#cd-nodes");
  if (!box) return;
  const bound = new Set(c.nodes);
  const rows = (c.available_node_items || []).map((nd) => {
    const mine = bound.has(nd.id);
    const remoteBusy = nd.kind === "remote_unit_node" && nd.remote_controller_id;
    const busy = (nd.bound_to && nd.bound_to !== c.id) || remoteBusy;
    const unassigned = nd.kind === "local" && !nd.assigned;
    const owner = mine
      ? "this controller"
      : remoteBusy
        ? `${nd.remote_unit_name || "remote unit"}:${nd.remote_controller_name || nd.remote_controller_id}`
        : busy ? (nd.bound_to_name || nd.bound_to) : "unbound";
    const address = (nd.rpc_host || "").toString();
    const port = nd.rpc_port || "";
    const desired = nd.desired_load || {};
    const loadState = desired.model
      ? `${shortTarget(desired.model)}${desired.layers ? ` ${desired.layers.join("..")}` : ""}`
      : nd.worker_running ? "worker running" : "-";
    const status = unassigned ? "unassigned" : mine ? "bound" : busy ? `connected to ${owner}` : "unbound";
    const runtimeOk = !nd.runtime_compatibility || nd.runtime_compatibility.compatible;
    const bindAction = unassigned
      ? `<span class="badge">configure first</span>`
      : mine
      ? `<button class="ghost" data-unbind="${esc(nd.id)}">unbind</button>`
      : busy
        ? `<span class="badge warn">${esc(status)}</span>`
        : !runtimeOk
          ? `<span class="badge warn">protocol mismatch</span>`
        : `<button data-bind="${esc(nd.id)}">bind</button>`;
    return `<tr data-testid="node-bind-row">
      <td>${esc(slotName(nd))}</td>
      <td>${esc(nd.gpu_name)}</td>
      <td>${esc(nd.vram)} GiB</td>
      <td>${esc(nd.ram)} GiB</td>
      <td>${esc(nd.cores)}</td>
      <td>${esc(nd.remote_unit_name || "local")}</td>
      <td>${esc(nd.remote_controller_name || "-")}</td>
      <td>${esc(platformText(nd.host_platform))}</td>
      <td>${esc(runtimeText(nd.runtime))}</td>
      <td>${esc(backendText(nd.backend))}</td>
      <td>${esc(compatText(nd.runtime_compatibility))}</td>
      <td>${esc(address || "-")}</td>
      <td>${esc(port || "-")}</td>
      <td title="${esc(loadState)}">${esc(loadState)}</td>
      <td>${esc(status)}</td>
      <td class="row-actions"><span class="row-action-wrap">${bindAction}</span></td>
    </tr>`;
  }).join("");
  const html = `<table class="ctrl-node-table">
    <thead><tr><th>Name</th><th>GPU</th><th>VRAM</th><th>RAM</th><th>CPU cores</th><th>Unit</th><th>Remote controller</th><th>Host</th><th>Protocol</th><th>Backend</th><th>Check</th><th>Address</th><th>Port</th><th>Load</th><th>Status</th><th>Action</th></tr></thead>
    <tbody data-testid="bind-rows">${rows || '<tr><td colspan="16" class="muted">no nodes yet</td></tr>'}</tbody>
  </table>`;
  if (!setHtmlIfChanged(box, html)) return;
  $$("[data-bind]", box).forEach((b) => b.onclick = async () => {
    try {
      await api("POST", `/api/controllers/${sel.id}/bind`, {node_id: b.dataset.bind});
      await Promise.all([refreshSidebar(), updateCtrl()]);
    } catch (e) {
      alert(e.message);
    }
  });
  $$("[data-unbind]", box).forEach((b) => b.onclick = async () => {
    await api("POST", `/api/controllers/${sel.id}/unbind`, {node_id: b.dataset.unbind});
    await Promise.all([refreshSidebar(), updateCtrl()]);
  });
}

function renderRemoteUnits(c) {
  const box = $("#remote-units");
  if (!box) return;
  const units = c.remote_units || [];
  if (!units.length) {
    setHtmlIfChanged(box, '<p class="muted">No remote units registered.</p>');
    return;
  }
  const html = `<table><tbody>${units.map((u) => `
    <tr data-testid="remote-unit-row">
      <td>${esc(u.name)}</td>
      <td><code>${esc(u.unit_url || u.base_url)}</code></td>
      <td>${esc(u.node_count)} node(s)</td>
      <td class="row-actions">
        <button class="ghost" data-refresh-unit="${esc(u.id)}">refresh</button>
        <button class="danger" data-delete-unit="${esc(u.id)}">delete</button>
      </td>
    </tr>`).join("")}</tbody></table>`;
  if (!setHtmlIfChanged(box, html)) return;
  $$("[data-refresh-unit]", box).forEach((b) => b.onclick = async () => {
    await api("POST", `/api/controllers/${sel.id}/remote-units/${b.dataset.refreshUnit}/refresh`);
    await refreshSidebar();
    await updateCtrl();
  });
  $$("[data-delete-unit]", box).forEach((b) => b.onclick = async () => {
    if (!confirm("Delete remote unit?")) return;
    await api("DELETE", `/api/controllers/${sel.id}/remote-units/${b.dataset.deleteUnit}`);
    await Promise.all([refreshSidebar(), updateCtrl()]);
  });
}

async function fillModels() {
  const detail = $('[data-testid="ctrl-detail"]');
  if (!detail) return;
  const select = $('[data-testid="serve-model"]', detail);
  if (!select) return;
  const current = select.value;
  const {models} = await api("GET", "/api/models");
  select.innerHTML = "";
  for (const m of models) {
    select.appendChild(el(`<option value="${esc(m.id || m.relative_path || m.name)}">${esc(m.label || m.name)}</option>`));
  }
  if (!models.length) select.appendChild(el(`<option value="">No downloaded models found</option>`));
  const remembered = detail.dataset.loadDirty === "true" ? "" : (CURRENT_CTRL?.last_load?.model || "");
  const desired = remembered || current;
  if (desired && Array.from(select.options).some((o) => o.value === desired)) select.value = desired;
}

function fillKvCacheTypes() {
  const detail = $('[data-testid="ctrl-detail"]');
  if (!detail) return;
  const types = CACHE.runtime?.kv_cache_types?.length
    ? CACHE.runtime.kv_cache_types
    : ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"];
  for (const testid of ["serve-cache-type-k", "serve-cache-type-v"]) {
    const select = $(`[data-testid="${testid}"]`, detail);
    if (!select) continue;
    const current = select.value || "f16";
    select.innerHTML = "";
    for (const t of types) {
      select.appendChild(el(`<option value="${esc(t)}">${esc(t.toUpperCase())}</option>`));
    }
    select.value = types.includes(current) ? current : (types.includes("f16") ? "f16" : types[0]);
  }
}

function setCtrlTab(tab) {
  CTRL_TAB = tab || "node";
  $$("[data-ctrl-tab]").forEach((b) => b.classList.toggle("active", b.dataset.ctrlTab === CTRL_TAB));
  $("#ctrl-node-tab")?.classList.toggle("hidden", CTRL_TAB !== "node");
  $("#ctrl-inference-tab")?.classList.toggle("hidden", CTRL_TAB !== "inference");
}

function renderLoadControls(c) {
  const detail = $('[data-testid="ctrl-detail"]');
  if (!detail) return;
  const loadBtn = $('[data-testid="serve-btn"]', detail);
  const unloadBtn = $('[data-testid="stop-btn"]', detail);
  const modelInput = $('[data-testid="serve-model"]', detail);
  const ctxInput = $('[data-testid="serve-ctx"]', detail);
  const parallelInput = $('[data-testid="serve-parallel"]', detail);
  const cacheKInput = $('[data-testid="serve-cache-type-k"]', detail);
  const cacheVInput = $('[data-testid="serve-cache-type-v"]', detail);
  const isLoading = c.phase === "loading" || Boolean(c.can_cancel_load);
  const canUnload = Boolean(c.can_unload) && !isLoading;
  const remembered = c.last_load || {};
  if (detail.dataset.loadDirty !== "true" && !isLoading && !canUnload) {
    if (remembered.model && modelInput && Array.from(modelInput.options).some((o) => o.value === remembered.model)) {
      modelInput.value = remembered.model;
    }
    if (remembered.ctx && ctxInput) ctxInput.value = remembered.ctx;
    if (remembered.parallel && parallelInput) parallelInput.value = remembered.parallel;
    setRuntimeMode(detail, remembered.runtime_mode || c.runtime_mode || "llama_rpc");
    if (remembered.cache_type_k && cacheKInput && Array.from(cacheKInput.options).some((o) => o.value === remembered.cache_type_k)) {
      cacheKInput.value = remembered.cache_type_k;
    }
    if (remembered.cache_type_v && cacheVInput && Array.from(cacheVInput.options).some((o) => o.value === remembered.cache_type_v)) {
      cacheVInput.value = remembered.cache_type_v;
    }
    const perf = remembered.performance || {};
    const setPerf = (testId, value) => {
      const input = $(`[data-testid="${testId}"]`, detail);
      if (input && value !== undefined && value !== null) input.value = value;
    };
    setPerf("serve-spec-type", perf.spec_type || "none");
    setPerf("serve-spec-draft-model", perf.spec_draft_model || "");
    setPerf("serve-spec-draft-n-max", perf.spec_draft_n_max || "");
    setPerf("serve-spec-draft-n-min", perf.spec_draft_n_min || "");
    setPerf("serve-batch", perf.batch || "");
    setPerf("serve-ubatch", perf.ubatch || "");
    setPerf("serve-poll", perf.poll || "");
    setPerf("serve-cache-reuse", perf.cache_reuse || "");
    setPerf("serve-spec-ngram-mod-n-min", perf.spec_ngram_mod_n_min || "");
    setPerf("serve-spec-ngram-mod-n-max", perf.spec_ngram_mod_n_max || "");
    setPerf("serve-spec-ngram-mod-n-match", perf.spec_ngram_mod_n_match || "");
    setPerf("serve-spec-draft-p-min", perf.spec_draft_p_min ?? "");
    setPerf("serve-spec-draft-p-split", perf.spec_draft_p_split ?? "");
    const contBatchingInput = $('[data-testid="serve-cont-batching"]', detail);
    if (contBatchingInput) contBatchingInput.checked = perf.cont_batching !== false;
  }
  loadBtn.classList.toggle("hidden", canUnload);
  loadBtn.dataset.mode = isLoading ? "cancel" : "load";
  loadBtn.textContent = isLoading ? "Cancel" : "Load";
  loadBtn.classList.toggle("danger", isLoading);
  loadBtn.disabled = c.phase === "unloading";
  unloadBtn.classList.toggle("hidden", !canUnload);
  unloadBtn.disabled = c.phase === "unloading";
  unloadBtn.textContent = c.phase === "unloading" ? "Unloading..." : "Unload";
  modelInput.disabled = canUnload || isLoading;
  ctxInput.disabled = canUnload || isLoading;
  parallelInput.disabled = canUnload || isLoading;
  $$('[name="serve-runtime-mode"]', detail).forEach((input) => { input.disabled = canUnload || isLoading; });
  if (cacheKInput) cacheKInput.disabled = canUnload || isLoading;
  if (cacheVInput) cacheVInput.disabled = canUnload || isLoading;
  $$('[data-testid^="serve-spec-"], [data-testid="serve-batch"], [data-testid="serve-ubatch"], [data-testid="serve-poll"], [data-testid="serve-cache-reuse"], [data-testid="serve-cont-batching"]', detail)
    .forEach((input) => { input.disabled = canUnload || isLoading; });
}

function selectedRuntimeMode(detail) {
  return $('[name="serve-runtime-mode"]:checked', detail)?.value || "llama_rpc";
}

function setRuntimeMode(detail, mode) {
  const selected = mode === "ring_proxy" ? "ring_proxy" : "llama_rpc";
  $$('[data-runtime-mode-option]', detail).forEach((option) => {
    const active = option.dataset.runtimeModeOption === selected;
    option.classList.toggle("active", active);
    const input = $('input[type="radio"]', option);
    if (input) input.checked = active;
  });
  const note = $("#serve-runtime-mode-note", detail);
  setTextIfChanged(note, selected === "ring_proxy"
    ? "Loads each node's local layer window and sends only boundary hidden states around the ring."
    : "Uses stock llama-server master and ggml-rpc-server workers.");
}

function renderPlan(plan, c) {
  const verdict = $('[data-testid="plan-verdict"]');
  if (!verdict) return;
  if (!plan) {
    verdict.dataset.feasible = "";
    setTextIfChanged(verdict, c.phase === "idle" ? "No loading plan yet." : `Current status: ${c.phase}\nDetail: ${c.detail || ""}`);
    return;
  }
  if (!plan.feasible) {
    verdict.dataset.feasible = "false";
    renderInfeasiblePlan(verdict, plan, c);
    return;
  }
  verdict.dataset.feasible = "true";
  setHtmlIfChanged(verdict, planReportHtml(plan, c));
}

function fmtGiB(v) {
  const n = Number(v);
  return Number.isFinite(n) ? `${n.toFixed(n % 1 ? 2 : 0)} GiB` : "-";
}

function fmtPct(used, budget) {
  const u = Number(used);
  const b = Number(budget);
  return Number.isFinite(u) && Number.isFinite(b) && b > 0 ? `${Math.round(u / b * 100)}%` : "-";
}

function offloadMethod(p) {
  const bits = [];
  if ((p.kv_ram_gib || 0) > 0) bits.push("KV -> host RAM (--no-kv-offload)");
  if ((p.layer_body_ram_gib || 0) > 0) bits.push("body tensors -> host RAM (-ot)");
  if ((p.ffn_ram_gib || 0) > 0) bits.push("MoE FFN experts -> host RAM (-ot)");
  return bits.length ? bits.join("; ") : "none";
}

function otSummary(p) {
  const rules = (p.ot || "").split(",").map((x) => x.trim()).filter(Boolean);
  if (!rules.length) return "none";
  const preview = rules.slice(0, 3).join(", ");
  return rules.length > 3 ? `${preview}, ... (${rules.length} rules)` : preview;
}

function shortTarget(v) {
  const text = String(v || "");
  if (!text.includes("/")) return text;
  const parts = text.split("/").filter(Boolean);
  return parts.slice(-2).join("/");
}

function planReportHtml(plan, c) {
  const adaptive = plan.adaptive_load_available ? "available" : `blocked - ${plan.adaptive_load_blocker || "monitoring unavailable"}`;
  const totals = plan.resource_totals || {};
  const cal = plan.calibration || {};
  const modelKind = plan.is_moe ? `MoE / sparse (${plan.model?.n_expert || 0} experts)` : "dense";
  const placementRows = (plan.placement || []).filter((p) => p.n_layers).map((p) => `
    <tr>
      <td><b>${esc(p.node_name || `Node ${p.node}`)}</b><br><span class="muted">${esc(p.gpu_name || p.node_kind || "")}</span></td>
      <td>${esc(`${p.layers?.[0]}..${p.layers?.[1]}`)}<br><span class="muted">${esc(`${p.n_layers} layers`)}</span></td>
      <td>${esc(fmtGiB(p.vram_used_gib))} / ${esc(fmtGiB(p.vram_budget_gib))}<br><span class="muted">${esc(fmtPct(p.vram_used_gib, p.vram_budget_gib))}</span></td>
      <td>${esc(fmtGiB(p.ram_used_gib))}<br><span class="muted">${esc(p.offload_policy || "none")}</span></td>
      <td>VRAM ${esc(fmtGiB(p.kv_vram_gib))}<br>RAM ${esc(fmtGiB(p.kv_ram_gib || 0))}</td>
      <td>VRAM ${esc(fmtGiB(p.layer_body_vram_gib))}<br>RAM ${esc(fmtGiB(p.layer_body_ram_gib || 0))}</td>
      <td>VRAM ${esc(fmtGiB(p.ffn_vram_gib || 0))}<br>RAM ${esc(fmtGiB(p.ffn_ram_gib || 0))}</td>
      <td>${esc(offloadMethod(p))}<br><span class="muted">${esc(otSummary(p))}</span></td>
    </tr>`).join("");
  return `<div class="plan-grid">
    <div class="plan-main">
      <div class="plan-title">
        <b>Final planning report</b>
        <span>${esc(c.phase.toUpperCase())} ${esc(c.serving || plan.model_label || plan.model_ref || "")}</span>
      </div>
      <div class="plan-summary">
        <span>Runtime <b>${esc(plan.runtime_mode === "ring_proxy" ? "linkcpp ring proxy" : "llama.cpp RPC")}</b></span>
        <span>Adaptive load <b>${esc(adaptive)}</b></span>
        <span>Tensor split <b>${esc(JSON.stringify(plan.tensor_split || []))}</b></span>
        <span>Weights <b>${esc(fmtGiB(plan.total_weight_gib))}</b></span>
        <span>Planned VRAM <b>${esc(fmtGiB(totals.planned_vram_gib))}</b></span>
        <span>Planned RAM <b>${esc(fmtGiB(totals.planned_ram_gib))}</b></span>
      </div>
      <table class="plan-table">
        <thead><tr>
          <th>Node</th><th>Layer window</th><th>VRAM</th><th>RAM</th><th>KV cache</th><th>Layer body</th><th>MoE FFN</th><th>Offload</th>
        </tr></thead>
        <tbody>${placementRows || '<tr><td colspan="8" class="muted">no placement</td></tr>'}</tbody>
      </table>
    </div>
    <aside class="plan-basis">
      <h4>Basis</h4>
      <div class="basis-row"><span>Total VRAM</span><b>${esc(fmtGiB(totals.vram_budget_gib))}</b></div>
      <div class="basis-row"><span>Total RAM</span><b>${esc(fmtGiB(totals.ram_budget_gib))}</b></div>
      <div class="basis-row"><span>Model type</span><b>${esc(modelKind)}</b></div>
      <div class="basis-row"><span>KV cache</span><b>${esc(fmtGiB(plan.kv_total_gib))}</b></div>
      <div class="basis-row"><span>KV placement</span><b>${esc(plan.kv_cache_location || "vram")}</b></div>
      <div class="basis-row"><span>KV type</span><b>K ${esc((plan.cache_type_k || "f16").toUpperCase())} / V ${esc((plan.cache_type_v || "f16").toUpperCase())}</b></div>
      <div class="basis-row"><span>Flash attention</span><b>${plan.flash_attention === false ? "off" : "on"}</b></div>
      <div class="basis-row"><span>Calibration</span><b>${esc(cal.source || "metadata")}</b></div>
      <div class="basis-row"><span>Confidence</span><b>${esc(Math.round((cal.confidence ?? 0.75) * 100) + "%")}</b></div>
      <div class="basis-note">KV ${esc(fmtGiB(cal.kv_cache_gib ?? plan.kv_total_gib))}, ctx ${esc(plan.n_ctx)}, parallel ${esc(plan.n_parallel)}</div>
    </aside>
  </div>`;
}

function renderInfeasiblePlan(verdict, plan, c) {
  const totals = plan.resource_totals || {};
  setHtmlIfChanged(verdict, `<div class="plan-grid">
    <div class="plan-main">
      <div class="plan-title error"><b>Plan result: INFEASIBLE</b><span>${esc(c.phase.toUpperCase())}</span></div>
      <p class="tight">Reason: ${esc(plan.reason || "cannot place model")}</p>
      <ul class="plan-suggestions">${(plan.suggestions || []).map((x) => `<li>${esc(x)}</li>`).join("")}</ul>
    </div>
    <aside class="plan-basis">
      <h4>Basis</h4>
      <div class="basis-row"><span>Total VRAM</span><b>${esc(fmtGiB(totals.vram_budget_gib || plan.sum_vram_budget_gib))}</b></div>
      <div class="basis-row"><span>Total RAM</span><b>${esc(fmtGiB(totals.ram_budget_gib || plan.sum_ram_budget_gib))}</b></div>
      <div class="basis-row"><span>Model type</span><b>${esc(plan.is_moe ? "MoE / sparse" : "dense")}</b></div>
      <div class="basis-row"><span>KV cache</span><b>${esc(fmtGiB(plan.kv_total_gib))}</b></div>
      <div class="basis-row"><span>KV placement</span><b>${esc(plan.kv_cache_location || "unknown")}</b></div>
      <div class="basis-row"><span>KV type</span><b>K ${esc((plan.cache_type_k || "f16").toUpperCase())} / V ${esc((plan.cache_type_v || "f16").toUpperCase())}</b></div>
      <div class="basis-row"><span>Need VRAM</span><b>${esc(fmtGiB(plan.need_vram_gib))}</b></div>
    </aside>
  </div>`);
}

function renderUnloadProgress(c) {
  const verdict = $('[data-testid="plan-verdict"]');
  const statusBox = $("#cd-load-status");
  const box = $("#cd-ops");
  const nodes = c?.node_items || [];
  if (verdict) {
    setTextIfChanged(verdict, `Current status: UNLOADING ${c?.active_model || c?.serving || ""}`.trim());
  }
  if (statusBox) {
    setHtmlIfChanged(statusBox, `<div class="load-summary running">
      <b>Current stage</b>
      <span>unload / node sweep / running</span>
      <small>Stopping master and unloading ${nodes.length || c?.nodes?.length || 0} node(s).</small>
    </div>`);
  }
  if (!box) return;
  const rows = nodes.map((n) => `<div class="ops-row">
    <div class="ops-cell">unload</div>
    <div class="ops-cell">queued</div>
    <div class="ops-cell"><span class="status-pill running">running</span></div>
    <div class="progress-cell">
      <span class="mini-bar"><span style="width:0%"></span></span>
      <span>queued</span>
    </div>
    <div class="ops-cell ops-target" title="${esc(n.name || n.id)}">${esc(n.name || n.id)}</div>
    <div class="ops-cell ops-detail" title="${esc(n.kind || "node")} unload pending">${esc(n.kind || "node")} unload pending</div>
  </div>`).join("");
  setHtmlIfChanged(box, `<div class="ops-table">
    <div class="ops-row ops-head">
      <div>Stage</div><div>Phase</div><div>Status</div><div>Progress</div><div>Target</div><div>Detail</div>
    </div>
    ${rows || '<div class="ops-empty muted">no bound nodes to unload</div>'}
  </div>`);
}

function renderOperations(c) {
  const box = $("#cd-ops");
  const statusBox = $("#cd-load-status");
  if (!box) return;
  const ops = (c.operations || []).slice(-12).reverse();
  if (!ops.length) {
    if (statusBox) {
      const phase = c.phase || "idle";
      setHtmlIfChanged(statusBox, `<div class="load-summary ${esc(phase)}">
        <b>Current stage</b>
        <span>${esc(phase === "idle" ? "Idle" : phase)}</span>
        <small>${esc(c.detail || "No load activity has started yet.")}</small>
      </div>`);
    }
    setHtmlIfChanged(box, '<p class="muted">No control activity yet.</p>');
    return;
  }

  const active = ops.find((op) => op.status === "running") || ops[0];
  const activePct = Number.isFinite(+active.progress) ? Math.round(+active.progress) : null;
  const activeMessage = active.message || active.error || c.detail || "";
  const nodeLoading = ops.filter((op) => op.type === "node_load" && op.status === "running");
  const nodeLoadingNote = nodeLoading.length
    ? ` ${nodeLoading.length} node monitor(s): ${nodeLoading.map((op) => `${op.node_id || "node"} ${Math.round(Number(op.progress) || 0)}%`).join(", ")}`
    : "";
  if (statusBox) {
    setHtmlIfChanged(statusBox, `<div class="load-summary ${esc(active.status || c.phase || "idle")}">
      <b>Current stage</b>
      <span>${esc(active.type || c.phase || "unknown")} / ${esc(active.phase || "unknown")} / ${esc(active.status || "unknown")}</span>
      <small>${esc(activePct == null ? `${activeMessage}${nodeLoadingNote}` : `${activePct}% - ${activeMessage}${nodeLoadingNote}`)}</small>
    </div>`);
  }

  setHtmlIfChanged(box, `<div class="ops-table">
    <div class="ops-row ops-head">
      <div>Stage</div><div>Phase</div><div>Status</div><div>Progress</div><div>Target</div><div>Detail</div>
    </div>
    ${ops.map((op) => {
    const pctText = Number.isFinite(+op.progress) ? `${Math.round(+op.progress)}%` : "";
    const pctWidth = Number.isFinite(+op.progress) ? Math.max(0, Math.min(100, +op.progress)) : 0;
    return `<div class="ops-row">
      <div class="ops-cell" title="${esc(op.type)}">${esc(op.type)}</div>
      <div class="ops-cell" title="${esc(op.phase)}">${esc(op.phase)}</div>
      <div class="ops-cell"><span class="status-pill ${esc(op.status)}">${esc(op.status)}</span></div>
      <div class="progress-cell">
        <span class="mini-bar"><span style="width:${pctWidth}%"></span></span>
        <span>${esc(pctText || "-")}</span>
      </div>
      <div class="ops-cell ops-target" title="${esc(op.node_id || op.model || "")}">${esc(shortTarget(op.node_id || op.model || ""))}</div>
      <div class="ops-cell ops-detail" title="${esc(op.message || op.error || "")}">${esc(op.message || op.error || "")}</div>
    </div>`;
  }).join("")}</div>`);
}

function fmtNum(v, digits = 2) {
  const n = Number(v);
  return Number.isFinite(n) ? n.toFixed(digits) : "-";
}

function fmtBytesMiB(v) {
  const n = Number(v);
  return Number.isFinite(n) ? `${n.toFixed(n >= 10 ? 1 : 3)} MiB` : "-";
}

function sparkline(samples) {
  const pts = (samples || []).filter((s) => Number.isFinite(Number(s.transfer_total_mib)));
  if (pts.length < 2) return `<div class="transfer-empty">waiting for samples</div>`;
  const maxX = Math.max(...pts.map((s) => Number(s.elapsed_s) || 0), 1);
  const maxY = Math.max(...pts.map((s) => Number(s.transfer_total_mib) || 0), 0.001);
  const coords = pts.map((s) => {
    const x = ((Number(s.elapsed_s) || 0) / maxX) * 100;
    const y = 36 - ((Number(s.transfer_total_mib) || 0) / maxY) * 34;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  }).join(" ");
  return `<svg class="transfer-spark" viewBox="0 0 100 38" preserveAspectRatio="none" aria-label="cumulative transfer graph">
    <polyline points="${esc(coords)}"></polyline>
  </svg>`;
}

function renderNodeMetric(n) {
  const kv = n.kv_cache || {};
  const rt = n.runtime || {};
  const op = rt.last_operation || {};
  const delta = rt.delta || {};
  const queue = rt.queue || {};
  const layers = (n.layers || []).join("..") || "-";
  const runtimeState = rt.state || "unknown";
  const transferKiB = Number(delta.transfer_bytes || 0) / 1024;
  return `<div class="node-metric ${esc(n.phase || "")}">
    <div class="node-metric-head">
      <b>${esc(n.node_name || n.node_id || "node")}</b>
      <span class="status-pill ${esc(runtimeState)}">${esc(runtimeState)}</span>
    </div>
    <div class="node-metric-sub">${esc(n.gpu_name || "")}</div>
    <div class="node-metric-grid">
      <span>Layers</span><b>${esc(layers)} (${esc(n.n_layers || 0)})</b>
      <span>Queue</span><b>${esc(queue.master_task != null ? `task ${queue.master_task}` : queue.association || "-")}</b>
      <span>Current op</span><b title="${esc(op.line || "")}">${esc(op.op || (rt.worker_running ? "idle" : "offline"))}</b>
      <span>Graph</span><b>${esc(op.n_nodes ? `${op.n_nodes} nodes / ${op.n_tensors || "-"} tensors` : "-")}</b>
      <span>Delta</span><b>${esc(`${delta.graph_compute_count || 0} graph, ${fmtNum(transferKiB, 1)} KiB`)}</b>
      <span>KV cache</span><b>${esc(kv.used ? `${kv.location} ${fmtNum(kv.used_gib, 2)} GiB` : "none")}</b>
      <span>KV type</span><b>${esc(`${kv.cache_type_k || "-"} / ${kv.cache_type_v || "-"}`)}</b>
      <span>Offload</span><b>${esc(n.offload_policy || "none")}</b>
      <span>RPC</span><b title="${esc(n.rpc_endpoint || "")}">${esc(n.remote_unit_url ? `remote ${n.rpc_endpoint || ""}` : n.rpc_endpoint || "-")}</b>
    </div>
  </div>`;
}

function renderTransferEdges(edges) {
  if (!(edges || []).length) return `<div class="muted">no transfer edges</div>`;
  return `<div class="transfer-edges">${edges.map((e) => `<div class="transfer-edge">
    <span title="${esc(`${e.source_endpoint || ""} -> ${e.target_endpoint || ""}`)}">${esc(e.source_node_name || "node")} -> ${esc(e.target_node_name || "target")}</span>
    <b>${esc(fmtBytesMiB(e.cumulative_mib))}</b>
    <em>${esc(e.cross_host ? "cross-host" : "local")}${e.estimated ? " estimated" : ""}</em>
  </div>`).join("")}</div>`;
}

function fmtTime(v) {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) return "-";
  return new Date(n * 1000).toLocaleString();
}

function renderKvRows(obj) {
  const rows = Object.entries(obj || {}).filter(([, v]) => v !== undefined && v !== null && v !== "");
  if (!rows.length) return `<div class="muted">none</div>`;
  return `<div class="history-kv">${rows.map(([k, v]) => `<span>${esc(k)}</span><b title="${esc(typeof v === "object" ? JSON.stringify(v) : v)}">${esc(typeof v === "object" ? JSON.stringify(v) : v)}</b>`).join("")}</div>`;
}

function renderArchiveNodes(nodes) {
  if (!(nodes || []).length) return `<div class="muted">no node snapshot</div>`;
  return `<div class="history-node-list">${nodes.map((n) => `<div class="history-node">
    <div class="node-metric-head">
      <b>${esc(n.name || n.id || "node")}</b>
      <span class="status-pill ${esc(n.kind || "idle")}">${esc(n.kind || "node")}</span>
    </div>
    <div class="node-metric-sub">${esc(n.gpu_name || n.gpu || "")}</div>
    <div class="node-metric-grid">
      <span>Node id</span><b title="${esc(n.id || "")}">${esc(n.id || "-")}</b>
      <span>RPC</span><b title="${esc(n.rpc_endpoint || n.endpoint || "")}">${esc(n.rpc_endpoint || n.endpoint || "-")}</b>
      <span>VRAM</span><b>${esc(fmtGiB(n.vram_budget_gib ?? n.vram ?? n.vram_gib))}</b>
      <span>RAM</span><b>${esc(fmtGiB(n.ram_budget_gib ?? n.ram ?? n.ram_gib))}</b>
    </div>
  </div>`).join("")}</div>`;
}

function renderRpcTopology(topology) {
  if (!(topology || []).length) return `<div class="muted">no rpc topology</div>`;
  return `<div class="history-topology">${topology.map((r) => `<div>
    <b>${esc(r.node_name || r.name || r.node_id || "node")}</b>
    <span title="${esc(`${r.rpc_endpoint || ""} ${r.remote_unit_url || ""}`)}">${esc(r.node_kind || r.kind || "node")} ${esc(r.rpc_endpoint || "")}</span>
  </div>`).join("")}</div>`;
}

function renderHistoryPlan(plan, h) {
  if (!plan || !(plan.placement || []).length) return `<div class="muted">no plan snapshot</div>`;
  return planReportHtml(plan, {
    phase: h.status || "done",
    serving: h.model || "",
  });
}

function renderInferenceRecordDetail(h) {
  const d = h.detail || {};
  if (!Object.keys(d).length) return `<div class="history-detail muted">Detailed report unavailable for this record.</div>`;
  const metrics = d.metrics || {};
  const nodes = metrics.node_metrics || [];
  return `<div class="history-detail">
    <div class="history-detail-grid">
      <section>
        <h4>Run</h4>
        ${renderKvRows({
          request_id: h.request_id,
          model: h.model,
          message: d.message,
          finish_reason: d.finish_reason,
          started: fmtTime(d.started_at),
          finished: fmtTime(d.finished_at),
          archive: h.archive_name,
        })}
      </section>
      <section>
        <h4>Request</h4>
        ${renderKvRows(d.request || {})}
      </section>
      <section>
        <h4>Usage</h4>
        ${renderKvRows(d.usage || {})}
      </section>
      <section>
        <h4>Timings</h4>
        ${renderKvRows(d.timings || {})}
      </section>
    </div>
    ${d.error ? `<div class="history-error">${esc(d.error)}</div>` : ""}
    <div class="history-detail-section">
      <h4>Node execution</h4>
      <div class="node-metrics">${nodes.length ? nodes.map(renderNodeMetric).join("") : renderArchiveNodes(d.nodes || [])}</div>
    </div>
    <div class="history-detail-section">
      <h4>Transfer</h4>
      <div class="transfer-panel">
        ${sparkline(d.samples || [])}
        ${renderTransferEdges(metrics.transfer_edges || [])}
      </div>
    </div>
    <div class="history-detail-section">
      <h4>RPC topology</h4>
      ${renderRpcTopology(d.rpc_topology || [])}
    </div>
    <div class="history-detail-section history-plan">
      <h4>Plan snapshot</h4>
      ${renderHistoryPlan(d.plan || {}, h)}
    </div>
  </div>`;
}

function renderInferenceHistory(history, archiveDir, open = false) {
  const rows = history || [];
  const openAttr = open ? " open" : "";
  if (!rows.length) return `<details class="inference-history"${openAttr}><summary>Previous inference records</summary><div class="muted">No records yet.</div></details>`;
  return `<details class="inference-history"${openAttr}>
    <summary>Previous inference records <span>${esc(rows.length)}</span></summary>
    <div class="history-dir"><code>${esc(archiveDir || "")}</code></div>
    <div class="history-list">${rows.map((h) => `<details class="history-record">
      <summary class="history-row">
        <span class="status-pill ${esc(h.status || "")}">${esc(h.status || "")}</span>
        <b>${esc(h.kind || "request")} #${esc(h.seq || "")}</b>
        <span>${esc(fmtNum(h.duration_s, 1))}s</span>
        <span>${esc(h.output_tokens ?? 0)} tok</span>
        <span>${esc(fmtNum(h.tps, 2))} t/s</span>
        <span>${esc(fmtBytesMiB(h.transfer_total_mib))}</span>
        <code title="${esc(h.archive_path || "")}">${esc(h.archive_name || "")}</code>
      </summary>
      ${renderInferenceRecordDetail(h)}
    </details>`).join("")}</div>
  </details>`;
}

function renderInferenceActivity(c) {
  const box = $("#cd-live-inference");
  if (!box) return;
  if (!box.querySelector("[data-inference-live]")) {
    setHtmlIfChanged(box, `<div data-inference-live></div><div data-inference-history-host></div>`);
  }
  const liveBox = $("[data-inference-live]", box);
  const historyBox = $("[data-inference-history-host]", box);
  const activity = c.inference_activity || {limit: c.parallel || 1, active: 0, queued: 0, items: [], history: []};
  const items = activity.items || [];
  if (!items.length) {
    setHtmlIfChanged(liveBox, `<div class="inference-empty">
      <span>No active inference requests.</span>
      <b>${esc(activity.active || 0)} / ${esc(activity.limit || c.parallel || 1)} running</b>
    </div>`);
    const historyOpen = $(".inference-history", historyBox)?.open || false;
    setHtmlIfChanged(historyBox, renderInferenceHistory(activity.history || [], activity.archive_dir, historyOpen));
    return;
  }
  setHtmlIfChanged(liveBox, `<div class="inference-head">
    <span><b>${esc(activity.active)}</b> running</span>
    <span><b>${esc(activity.queued)}</b> queued</span>
    <span><b>${esc(activity.limit)}</b> parallel slots</span>
  </div>
  <div class="inference-list">${items.map((it) => {
    const pct = Math.max(0, Math.min(100, Number(it.progress) || 0));
    const metrics = it.metrics || {};
    const edges = metrics.transfer_edges || [];
    const nodes = metrics.node_metrics || [];
    const pipe = (it.pipeline || []).map((p) => `<span class="pipe-node ${esc(p.phase || "")}" title="${esc((p.node_name || "") + " " + (p.detail || ""))}">
      <b>${esc(p.node_name || p.node_id || "node")}</b>
      <small>${esc((p.layers || []).join("..") || "")}</small>
      <em>${esc(p.phase || "")}</em>
      <i>${esc(p.handoff_to ? "-> " + p.handoff_to : "")}</i>
    </span>`).join("");
    return `<div class="inference-item">
      <div class="inference-top">
        <span class="status-pill ${esc(it.status)}">${esc(it.status)}</span>
        <b>${esc(it.kind || "request")} #${esc(it.seq || "")}</b>
        <span class="muted">${esc(Math.round(it.elapsed_s || 0))}s</span>
        <span class="muted">${esc(it.stream ? "stream" : "non-stream")}</span>
        <span class="muted">${esc(it.max_tokens ? `max ${it.max_tokens}` : "")}</span>
      </div>
      <div class="inference-progress">
        <span class="mini-bar"><span style="width:${pct}%"></span></span>
        <span>${esc(Math.round(pct))}%</span>
        <span class="ops-cell" title="${esc(it.message || "")}">${esc(it.message || "")}</span>
      </div>
      <div class="inference-metrics">
        <span><b>${esc(metrics.output_tokens ?? 0)}</b> tokens</span>
        <span><b>${esc(fmtNum(metrics.tps, 2))}</b> t/s</span>
        <span><b>${esc(fmtBytesMiB(metrics.transfer_total_mib))}</b> transfer</span>
        <span>${esc(metrics.sample_source || metrics.output_token_source || "")}</span>
      </div>
      <div class="node-metrics">${nodes.map(renderNodeMetric).join("")}</div>
      <div class="transfer-panel">
        ${sparkline(it.samples || [])}
        ${renderTransferEdges(edges)}
      </div>
      <div class="pipeline">${pipe || '<span class="muted">no placement pipeline</span>'}</div>
    </div>`;
  }).join("")}</div>`);
  const historyOpen = $(".inference-history", historyBox)?.open || false;
  setHtmlIfChanged(historyBox, renderInferenceHistory(activity.history || [], activity.archive_dir, historyOpen));
}

function renderEndpoints(c) {
  const box = $("#cd-endpoints");
  if (!box) return;
  const base = `${location.origin}/c/${encodeURIComponent(c.id)}`;
  const endpoints = [
    ["OpenAI Models", `${base}/v1/models`],
    ["OpenAI Chat Completions", `${base}/v1/chat/completions`],
    ["OpenAI Responses", `${base}/v1/responses`],
    ["Anthropic Models", `${base}/anthropic/v1/models`],
    ["Anthropic Messages", `${base}/anthropic/v1/messages`],
  ];
  setHtmlIfChanged(box, `<table><tbody>${endpoints.map(([name, url]) =>
    `<tr><td>${esc(name)}</td><td><code>${esc(url)}</code></td></tr>`).join("")}</tbody></table>`);
}

async function refreshLiveInferenceNow() {
  if (sel.type !== "ctrl" || !sel.id || !CURRENT_CTRL) return;
  const activity = await api("GET", `/api/controllers/${sel.id}/inference-activity`).catch(() => null);
  if (!activity || sel.type !== "ctrl" || !CURRENT_CTRL || CURRENT_CTRL.id !== sel.id) return;
  CURRENT_CTRL.inference_activity = activity;
  renderInferenceActivity(CURRENT_CTRL);
}

function startLiveInferenceFastPoll() {
  stopLiveInferenceFastPoll();
  refreshLiveInferenceNow();
  LIVE_INFERENCE_FAST_POLL = setInterval(refreshLiveInferenceNow, 750);
}

function stopLiveInferenceFastPoll() {
  if (LIVE_INFERENCE_FAST_POLL) clearInterval(LIVE_INFERENCE_FAST_POLL);
  LIVE_INFERENCE_FAST_POLL = null;
}

function wireCtrl() {
  const detail = $('[data-testid="ctrl-detail"]');
  const req = () => ({
    model: $('[data-testid="serve-model"]', detail).value,
    runtime_mode: selectedRuntimeMode(detail),
    ctx: +$('[data-testid="serve-ctx"]', detail).value,
    parallel: +$('[data-testid="serve-parallel"]', detail).value,
    cache_type_k: $('[data-testid="serve-cache-type-k"]', detail)?.value || "f16",
    cache_type_v: $('[data-testid="serve-cache-type-v"]', detail)?.value || "f16",
    batch: +$('[data-testid="serve-batch"]', detail)?.value || 0,
    ubatch: +$('[data-testid="serve-ubatch"]', detail)?.value || 0,
    poll: +$('[data-testid="serve-poll"]', detail)?.value || 0,
    cont_batching: $('[data-testid="serve-cont-batching"]', detail)?.checked !== false,
    cache_reuse: +$('[data-testid="serve-cache-reuse"]', detail)?.value || 0,
    spec_type: $('[data-testid="serve-spec-type"]', detail)?.value || "none",
    spec_draft_model: $('[data-testid="serve-spec-draft-model"]', detail)?.value || "",
    spec_draft_n_max: +$('[data-testid="serve-spec-draft-n-max"]', detail)?.value || 0,
    spec_draft_n_min: +$('[data-testid="serve-spec-draft-n-min"]', detail)?.value || 0,
    spec_draft_p_min: $('[data-testid="serve-spec-draft-p-min"]', detail)?.value || null,
    spec_draft_p_split: $('[data-testid="serve-spec-draft-p-split"]', detail)?.value || null,
    spec_ngram_mod_n_min: +$('[data-testid="serve-spec-ngram-mod-n-min"]', detail)?.value || 0,
    spec_ngram_mod_n_max: +$('[data-testid="serve-spec-ngram-mod-n-max"]', detail)?.value || 0,
    spec_ngram_mod_n_match: +$('[data-testid="serve-spec-ngram-mod-n-match"]', detail)?.value || 0,
  });
  fillModels();
  fillKvCacheTypes();
  setCtrlTab(CTRL_TAB);
  $$('[data-testid="serve-model"], [name="serve-runtime-mode"], [data-testid="serve-ctx"], [data-testid="serve-parallel"], [data-testid="serve-cache-type-k"], [data-testid="serve-cache-type-v"], [data-testid^="serve-spec-"], [data-testid="serve-batch"], [data-testid="serve-ubatch"], [data-testid="serve-poll"], [data-testid="serve-cache-reuse"], [data-testid="serve-cont-batching"]', detail)
    .forEach((input) => input.oninput = input.onchange = () => {
      detail.dataset.loadDirty = "true";
      if (input.name === "serve-runtime-mode") setRuntimeMode(detail, input.value);
    });
  $$("[data-ctrl-tab]", detail).forEach((b) => b.onclick = () => setCtrlTab(b.dataset.ctrlTab));
  $('[data-testid="remote-unit-create"]', detail).onclick = async () => {
    try {
      await api("POST", `/api/controllers/${sel.id}/remote-units`, {
        unit_url: $('[data-testid="remote-unit-url"]', detail).value,
        name: $('[data-testid="remote-unit-name"]', detail).value,
      });
      await Promise.all([refreshSidebar(), updateCtrl()]);
    } catch (e) {
      alert("add remote unit: " + e.message);
    }
  };
  $('[data-testid="serve-btn"]', detail).onclick = async () => {
    const loadBtn = $('[data-testid="serve-btn"]', detail);
    const verdict = $('[data-testid="plan-verdict"]', detail);
    if (loadBtn.dataset.mode === "cancel") {
      loadBtn.disabled = true;
      setTextIfChanged(verdict, "canceling load...");
      try {
        await api("POST", `/api/controllers/${sel.id}/load/cancel`, {reason: "ui"});
      } catch (e) {
        alert("cancel load error: " + e.message);
      }
      await Promise.all([refreshSidebar(), updateCtrl()]);
      return;
    }
    setTextIfChanged(verdict, "planning and loading...");
    try {
      const r = await api("POST", `/api/controllers/${sel.id}/load`, req());
      renderPlan(r.plan, {phase: r.phase, serving: r.accepted});
    } catch (e) {
      setTextIfChanged(verdict, "load error: " + e.message);
    }
    updateCtrl();
  };
  $('[data-testid="stop-btn"]', detail).onclick = async () => {
    const btn = $('[data-testid="stop-btn"]', detail);
    btn.disabled = true;
    btn.textContent = "Unloading...";
    renderUnloadProgress(CURRENT_CTRL);
    try {
      await api("POST", `/api/controllers/${sel.id}/unload`, {reason: "ui"});
      await Promise.all([refreshSidebar(), updateCtrl()]);
    } catch (e) {
      alert("unload error: " + e.message);
      await updateCtrl();
    }
  };
  $('[data-testid="chat-send"]', detail).onclick = async () => {
    const out = $('[data-testid="chat-output"]', detail);
    const meta = $('[data-testid="chat-meta"]', detail);
    const maxTokensEl = $('[data-testid="chat-max-tokens"]', detail);
    const requestedMaxTokens = parseInt(maxTokensEl.value, 10) || DEFAULT_CHAT_MAX_TOKENS;
    const maxTokens = Math.max(1, Math.min(UI_CHAT_MAX_TOKENS, requestedMaxTokens));
    maxTokensEl.value = String(maxTokens);
    meta.textContent = "";
    const st = await api("GET", `/api/controllers/${sel.id}/status`).catch(() => ({phase: "idle"}));
    if (st.phase !== "running") {
      out.textContent = st.phase === "loading" ? `Model is still loading - ${st.detail || "please wait"}` : "No model loaded yet.";
      return;
    }
    out.textContent = "";
    meta.textContent = "Streaming...";
    const startedAt = performance.now();
    let outputEvents = 0;
    let outputChars = 0;
    let usage = {};
    let timings = {};
    startLiveInferenceFastPoll();
    try {
      const resp = await fetch(`/c/${sel.id}/v1/chat/completions`, {
        method: "POST",
        headers: {"Content-Type": "application/json"},
        body: JSON.stringify({
        messages: [{role: "user", content: $('[data-testid="chat-input"]', detail).value}],
        max_tokens: maxTokens,
          stream: true,
        }),
      });
      if (!resp.ok) {
        const j = await resp.json().catch(() => ({}));
        throw new Error(j.detail || j.error || resp.statusText);
      }
      const reader = resp.body?.getReader();
      if (!reader) throw new Error("stream response body unavailable");
      const dec = new TextDecoder();
      let buf = "";
      let text = "";
      let finish = "";
      const usageTokens = () => Number(usage.completion_tokens ?? usage.output_tokens ?? usage.predicted_n ?? timings.predicted_n ?? 0);
      const currentTps = () => {
        const timed = Number(timings.predicted_per_second ?? timings.tokens_per_second ?? timings.tps);
        if (Number.isFinite(timed) && timed > 0) return timed;
        const elapsed = Math.max(0.001, (performance.now() - startedAt) / 1000);
        const tokens = usageTokens() || outputEvents;
        return tokens ? tokens / elapsed : 0;
      };
      while (true) {
        const {done, value} = await reader.read();
        if (done) break;
        buf += dec.decode(value, {stream: true});
        const lines = buf.split(/\r?\n/);
        buf = lines.pop() || "";
        for (const line of lines) {
          if (!line.startsWith("data:")) continue;
          const data = line.slice(5).trim();
          if (!data || data === "[DONE]") continue;
          const chunk = JSON.parse(data);
          if (chunk.usage) usage = chunk.usage;
          if (chunk.timings) timings = chunk.timings;
          const choice = chunk.choices?.[0] || {};
          const delta = choice.delta?.content || choice.message?.content || "";
          if (delta) {
            text += delta;
            outputEvents += 1;
            outputChars += delta.length;
            out.textContent = text;
            meta.textContent = `Streaming... ${usageTokens() || outputEvents} tokens, ${fmtNum(currentTps(), 2)} t/s`;
          }
          if (choice.finish_reason) finish = choice.finish_reason;
        }
      }
      const finalTokens = usageTokens() || outputEvents;
      const tps = currentTps();
      meta.textContent = `${requestedMaxTokens !== maxTokens ? `Max tokens capped to ${maxTokens}. ` : ""}Finish: ${finish || "stream complete"}, output tokens: ${finalTokens}, tps: ${fmtNum(tps, 2)}${usageTokens() ? "" : " (event estimate)"}`;
    } catch (e) {
      out.textContent = "chat error: " + e.message;
      meta.textContent = "";
    } finally {
      stopLiveInferenceFastPoll();
      setTimeout(refreshLiveInferenceNow, 300);
    }
  };
  $("#cd-del", detail).onclick = async () => {
    if (confirm("Delete controller?")) {
      await api("DELETE", "/api/controllers/" + sel.id);
      sel = {type: null, id: null};
      $("#main").innerHTML = "";
      refreshSidebar();
    }
  };
}

function firewallShell() {
  const unitCmd = `docker compose up -d --build`;
  return `<div data-testid="firewall-page">
    <h2>Network access</h2>
    <div class="card">
      <h3>Remote unit</h3>
      <p class="muted">Run another linkcpp unit on the remote GPU computer, then add <code>http://REMOTE_IP:19000</code> in the Node tab.</p>
      <pre class="cmd" data-testid="remote-worker-command">${esc(unitCmd)}</pre>
    </div>
    <div class="card">
      <h3>Windows firewall</h3>
      <ol>
        <li>Open PowerShell as Administrator.</li>
        <li>Allow the unit API and RPC worker port range:</li>
      </ol>
      <pre class="cmd">New-NetFirewallRule -DisplayName "linkcpp unit" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 19000,50052-50056</pre>
    </div>
    <div class="card">
      <h3>macOS firewall</h3>
      <ol>
        <li>System Settings -> Network -> Firewall -> Options.</li>
        <li>Allow Docker Desktop incoming connections.</li>
        <li>Publish the unit API and worker ports with Docker Compose.</li>
      </ol>
      <p class="muted">macOS Application Firewall is app-based; Docker Desktop must accept inbound connections for TCP 19000 and 50052-50056.</p>
    </div>
    <div class="card">
      <h3>Linux firewall</h3>
      <pre class="cmd">sudo ufw allow 19000/tcp
sudo ufw allow 50052:50056/tcp
sudo ufw reload</pre>
      <p class="muted">For firewalld: <code>sudo firewall-cmd --add-port=19000/tcp --add-port=50052-50056/tcp --permanent && sudo firewall-cmd --reload</code></p>
    </div>
    <div class="card">
      <h3>Connectivity check</h3>
      <p class="muted">From the controller computer:</p>
      <pre class="cmd">curl http://REMOTE_IP:19000/api/controllers
nc -vz REMOTE_IP 50052</pre>
    </div>
  </div>`;
}

function wireFirewall() {}

function runtimeShell() {
  return `<div data-testid="runtime-page">
    <h2>Runtime</h2>
    <div class="card">
      <h3>Runtime pack</h3>
      <div class="kv"><b>Unit version</b><span id="rt-unit"></span></div>
      <div class="kv"><b>Pack version</b><span id="rt-pack"></span></div>
      <div class="kv"><b>llama.cpp</b><span id="rt-llama"></span></div>
      <div class="kv"><b>RPC ABI</b><span id="rt-rpc"></span></div>
    </div>
    <div class="card">
      <h3>llama.cpp update</h3>
      <p class="muted">Official upstream and the compatible ring-adapter track are checked separately. Only a reviewed adapter HEAD can be applied here.</p>
      <div class="actions start">
        <button type="button" class="ghost" data-testid="llama-update-check">Check updates</button>
        <button type="button" class="hidden" data-testid="llama-update-apply">Apply compatible adapter</button>
      </div>
      <pre class="cmd" data-testid="llama-update-status">Not checked.</pre>
    </div>
    <div class="card">
      <h3>Local backend</h3>
      <div class="kv"><b>Kind</b><span id="rt-backend-kind"></span></div>
      <div class="kv"><b>Runtime</b><span id="rt-backend-runtime"></span></div>
      <div class="kv"><b>Driver</b><span id="rt-backend-driver"></span></div>
      <div class="kv"><b>Device</b><span id="rt-backend-device"></span></div>
    </div>
    <div class="card">
      <h3>Node compatibility</h3>
      <div id="rt-nodes" data-testid="runtime-nodes"></div>
    </div>
  </div>`;
}

function renderRuntime() {
  const rt = (CACHE.runtime && CACHE.runtime.runtime) || {};
  const be = (CACHE.runtime && CACHE.runtime.backend) || {};
  setTextIfChanged($("#rt-unit"), rt.unit_version || "unknown");
  setTextIfChanged($("#rt-pack"), rt.runtime_pack_version || rt.unit_version || "unknown");
  setTextIfChanged($("#rt-llama"), rt.llama_cpp_version || "unknown");
  setTextIfChanged($("#rt-rpc"), rt.rpc_abi || "unknown");
  setTextIfChanged($("#rt-backend-kind"), be.backend_kind || be.llama_cpp_backend || "unknown");
  setTextIfChanged($("#rt-backend-runtime"), be.backend_runtime_version || "not reported");
  setTextIfChanged($("#rt-backend-driver"), be.backend_driver_version || "not reported");
  setTextIfChanged($("#rt-backend-device"), be.backend_device || "not reported");

  const nodes = CACHE.nodes || [];
  setHtmlIfChanged($("#rt-nodes"), `<table>
    <thead><tr><th>Name</th><th>Kind</th><th>Host</th><th>Protocol</th><th>Backend</th><th>Protocol check</th></tr></thead>
    <tbody>${nodes.map((n) => `<tr>
      <td>${esc(n.name)}</td>
      <td>${esc(n.kind)}</td>
      <td>${esc(platformText(n.host_platform))}</td>
      <td>${esc(runtimeText(n.runtime))}</td>
      <td>${esc(backendText(n.backend))}</td>
      <td>${esc(compatText(n.runtime_compatibility))}</td>
    </tr>`).join("") || '<tr><td colspan="6" class="muted">no nodes</td></tr>'}</tbody>
  </table>`);

}

function wireRuntime() {
  const check = $('[data-testid="llama-update-check"]');
  const apply = $('[data-testid="llama-update-apply"]');
  const out = $('[data-testid="llama-update-status"]');
  let status = null;
  const show = (value) => {
    if (!value?.supported) return setTextIfChanged(out, `Unavailable: ${value?.reason || "unknown error"}`);
    setTextIfChanged(out, [
      `Pinned adapter: ${value.current}`,
      `Compatible adapter HEAD: ${value.adapter_head}`,
      `Official upstream HEAD: ${value.official_head}`,
      value.adapter_update_available ? "Compatible update available." : "Compatible adapter is current.",
      value.official_differs_from_adapter ? "Official upstream differs; adapter rebase review is required." : "Adapter matches official upstream.",
      value.api_apply_enabled ? "Web apply is enabled." : "Web apply is disabled; use the host-side update script.",
    ].join("\n"));
    apply.classList.toggle("hidden", !value.adapter_update_available || value.dirty || !value.api_apply_enabled);
  };
  check.onclick = async () => {
    check.disabled = true;
    setTextIfChanged(out, "Checking GitHub...");
    try { status = await api("GET", "/api/runtime/llama-cpp-update"); show(status); }
    catch (e) { setTextIfChanged(out, `Check failed: ${e.message}`); }
    check.disabled = false;
  };
  apply.onclick = async () => {
    if (!status || !confirm("Advance the llama.cpp submodule to the compatible adapter HEAD?")) return;
    apply.disabled = true;
    try {
      status = await api("POST", "/api/runtime/llama-cpp-update", {
        expected_current: status.current,
        target: status.adapter_head,
      });
      show(status);
    } catch (e) { setTextIfChanged(out, `Update failed: ${e.message}`); }
    apply.disabled = false;
  };
}

async function updateDetail() {
  try {
    if (sel.type === "node") await updateNode();
    else if (sel.type === "ctrl") await updateCtrl();
    else if (sel.type === "runtime") renderRuntime();
  } catch (e) {}
}

async function tick() {
  try {
    if (!(await checkAuth())) return;   // gated: show login, skip polling until signed in
    await refreshSidebar();
    await updateDetail();
  } catch (e) {}
}
checkAuth().then((ok) => { if (ok) refreshSidebar(); });
setInterval(tick, 2000);
