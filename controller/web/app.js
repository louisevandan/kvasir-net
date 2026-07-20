// linkcpp controller UI
const $ = (s) => document.querySelector(s);
const api = async (m, p, b) => {
  const r = await fetch(p, {method: m, headers: {"Content-Type": "application/json"},
                           body: b ? JSON.stringify(b) : undefined});
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.detail || j.error || r.statusText);
  return j;
};

async function refreshNodes() {
  const {nodes} = await api("GET", "/api/nodes");
  const body = $("#nodes-body");
  body.innerHTML = "";
  for (const n of nodes) {
    const tr = document.createElement("tr");
    tr.dataset.testid = "node-row";
    tr.innerHTML = `<td>${n.id}</td><td data-testid="node-gpu">${n.gpu||""}</td>
      <td>${n.vram_budget_gib??""}</td><td>${n.ram_budget_gib??""}</td>
      <td>${n.cores??""}</td><td>${n.bound_to?"bound":"free"}</td>
      <td><button data-del="${n.id}">remove</button></td>`;
    body.appendChild(tr);
  }
}

async function refreshModels() {
  const {models, downloads} = await api("GET", "/api/models");
  const ul = $("#models-list"); ul.innerHTML = "";
  const sel = $('[data-testid="serve-model"]'); const cur = sel.value; sel.innerHTML = "";
  for (const m of models) {
    const li = document.createElement("li");
    li.dataset.testid = "model-item";
    li.textContent = m.label || `${m.name} (${m.size_gib} GiB)`;
    ul.appendChild(li);
    const o = document.createElement("option"); o.value = m.name; o.textContent = m.label || m.name;
    sel.appendChild(o);
  }
  if (cur) sel.value = cur;
  for (const [name, d] of Object.entries(downloads || {})) {
    if (d.status !== "done") {
      const li = document.createElement("li");
      const pct = d.total ? Math.round(100 * d.done / d.total) : 0;
      li.textContent = `${name} — ${d.status} ${pct}%`;
      ul.appendChild(li);
    }
  }
}

let STATUS = {phase: "idle"};
async function refreshStatus() {
  const s = await api("GET", "/api/status");
  STATUS = s;
  const el = $("#status");
  el.dataset.phase = s.phase;
  if (s.phase === "running") el.textContent = `running: ${s.serving} (parallel ${s.parallel})`;
  else if (s.phase === "loading") el.textContent = `loading: ${s.serving} — ${s.detail}…`;
  else if (s.phase === "error") el.textContent = `error: ${s.detail}`;
  else el.textContent = "idle";

  // prominent live banner in the Serve section
  const live = $("#serve-live"), txt = $("#serve-live-text");
  if (live) {
    live.className = "live " + s.phase;
    if (s.phase === "running") txt.textContent = `● Serving ${s.serving} — ready (parallel ${s.parallel})`;
    else if (s.phase === "loading") txt.textContent = `Loading ${s.serving} — ${s.detail || "starting"}…`;
    else if (s.phase === "error") txt.textContent = `Failed: ${s.detail}`;
    else txt.textContent = "idle — no model served";
  }
}

$("#node-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = e.target;
  try {
    await api("POST", "/api/nodes", {addr: f.addr.value});
    f.addr.value = "";
    await refreshNodes();
  } catch (err) { alert("add node: " + err.message); }
});

$("#nodes-body").addEventListener("click", async (e) => {
  const id = e.target.getAttribute("data-del");
  if (id) { await api("DELETE", "/api/nodes/" + encodeURIComponent(id)); await refreshNodes(); }
});

$("#dl-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = e.target;
  await api("POST", "/api/models/download", {url: f.url.value, name: f.name.value});
  setTimeout(refreshModels, 500);
});

function serveReq() {
  const f = $("#serve-form");
  return {model: f.model.value, ctx: +f.ctx.value, parallel: +f.parallel.value};
}

$("#plan-btn").addEventListener("click", async () => {
  const v = $("#plan-verdict");
  v.textContent = "planning…";
  try {
    const r = await api("POST", "/api/plan", serveReq());
    v.dataset.feasible = r.feasible;
    v.textContent = r.feasible
      ? `FEASIBLE — nodes ${r.nodes_used}, split ${JSON.stringify(r.tensor_split)}, ` +
        `weights ${r.total_weight_gib} GiB, KV ${r.kv_total_gib} GiB\n` +
        r.placement.filter(p=>p.n_layers).map(p=>`node${p.node}: layers ${p.layers[0]}..${p.layers[1]} ` +
        `VRAM ${p.vram_used_gib}/${p.vram_budget_gib} GiB${p.ot?" -ot "+p.ot:""}`).join("\n")
      : `INFEASIBLE — ${r.reason}\n` + (r.suggestions||[]).map(s=>"• "+s).join("\n");
  } catch (err) { v.textContent = "plan error: " + err.message; }
});

$("#serve-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const live = $("#serve-live"), txt = $("#serve-live-text");
  live.className = "live loading"; txt.textContent = "Loading — starting…";
  $("#plan-verdict").textContent = "";
  try {
    await api("POST", "/api/serve", serveReq());
  } catch (err) {
    live.className = "live error"; txt.textContent = "serve error: " + err.message;
  }
  await refreshStatus();
});

$("#chat-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const out = $("#chat-output");
  // don't fire a request that will just 503 — tell the user what's happening
  if (STATUS.phase !== "running") {
    out.textContent = STATUS.phase === "loading"
      ? `⏳ Model is still loading — ${STATUS.detail || "please wait"}…`
      : "No model is being served yet. Pick a model above and click Serve.";
    return;
  }
  out.textContent = "…";
  try {
    const r = await api("POST", "/v1/chat/completions",
      {messages: [{role: "user", content: e.target.prompt.value}], max_tokens: 512});
    out.textContent = r.choices[0].message.content;
  } catch (err) { out.textContent = "chat error: " + err.message; }
});

async function tick() { try { await refreshStatus(); } catch {} }
refreshNodes(); refreshModels(); refreshStatus();
setInterval(() => { refreshNodes(); refreshModels(); refreshStatus(); }, 4000);
