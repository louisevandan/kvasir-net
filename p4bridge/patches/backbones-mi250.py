import sys
p = sys.argv[1]
s = open(p).read()
if "P4_BRIDGE_BACKBONES" in s:
    print("already patched"); sys.exit(0)

def sub(old, new):
    global s
    assert s.count(old) == 1, old[:70]
    s = s.replace(old, new, 1)

sub("const PORT_BASE = 52_970;\nconst PORT_SPAN = 60;\n",
"""const PORT_BASE = 52_970;
const PORT_SPAN = 60;

/**
 * Backbones other than the default ring, by the id a wallet sends as `backbone`.
 *
 * P4_BRIDGE_BACKBONES is JSON: {"mi250-02": {"model": "step-3.7-flash-mi250",
 * "port_base": 53970, "port_span": 16, "token_sha256": "<hex>"}}. A session a
 * worker claims for one of these is listed under that backbone's catalog model,
 * so only that backbone's overlay (which filters on model) opens a listener for
 * it, and its port comes from that backbone's range, which the backbone tunnel
 * forwards. The bridge still dials 127.0.0.1. `token_sha256` lets that backbone
 * list ITS sessions with a token of its own; only the hash lives here.
 * An unknown or missing `backbone` is the default ring, as before.
 */
const BACKBONES = (() => {
  try {
    const raw = JSON.parse(process.env.P4_BRIDGE_BACKBONES || '{}');
    const out = {};
    for (const [id, b] of Object.entries(raw)) {
      if (!/^[a-z0-9-]{1,32}$/.test(id) || !b || typeof b.model !== 'string') continue;
      const base = Number(b.port_base), span = Number(b.port_span ?? 16);
      if (!Number.isInteger(base) || !Number.isInteger(span) || span < 1 || span > 64) continue;
      out[id] = { model: b.model, portBase: base, portSpan: span,
        tokenSha256: typeof b.token_sha256 === 'string' ? b.token_sha256.toLowerCase() : '' };
    }
    return out;
  } catch (e) {
    console.log(`P4_BRIDGE_BACKBONES ignored: ${e.message}`);
    return {};
  }
})();
""")

sub("""  portFor(workerId) {
    if (this.ports.has(workerId)) return this.ports.get(workerId);
    const taken = new Set(this.ports.values());
    for (let i = 0; i < PORT_SPAN; i += 1) {
      const port = PORT_BASE + i;
      if (!taken.has(port)) { this.ports.set(workerId, port); return port; }
    }
    return null;
  }
""", """  portFor(workerId, backbone = null) {
    const bb = backbone ? BACKBONES[backbone] : null;
    const key = bb ? `${workerId}@${backbone}` : workerId;
    if (this.ports.has(key)) return this.ports.get(key);
    const taken = new Set(this.ports.values());
    const base = bb ? bb.portBase : PORT_BASE;
    const span = bb ? bb.portSpan : PORT_SPAN;
    for (let i = 0; i < span; i += 1) {
      const port = base + i;
      if (!taken.has(port)) { this.ports.set(key, port); return port; }
    }
    return null;
  }
""")

sub("""        session = relaySession;
        listenPort = this.portFor(workerId);
        if (listenPort) {
          const prior = this.relayTargets.get(session);
          this.relayTargets.set(session, {
            host: '127.0.0.1',
            port: listenPort,
            model: model.split('/').pop(),
""", """        session = relaySession;
        const backbone = BACKBONES[String(body?.backbone ?? '')] ? String(body.backbone) : null;
        listenPort = this.portFor(workerId, backbone);
        if (listenPort) {
          const prior = this.relayTargets.get(session);
          this.relayTargets.set(session, {
            host: '127.0.0.1',
            port: listenPort,
            model: backbone ? BACKBONES[backbone].model : model.split('/').pop(),
            backbone,
""")

sub("""      if (who.kind !== 'service') {
        return fail(res, 403, 'a service token is required to list relay sessions'), true;
      }
""", """      // A backbone's own token lists only that backbone's sessions.
      let scope = null;
      if (who.kind !== 'service') {
        const presented = String(req.headers['x-kvasir-service-token'] ?? '').trim();
        const digest = presented ? crypto.createHash('sha256').update(presented).digest('hex') : '';
        for (const [id, b] of Object.entries(BACKBONES)) {
          if (b.tokenSha256 && digest && timingSafeEqual(digest, b.tokenSha256)) scope = id;
        }
        if (!scope) return fail(res, 403, 'a service token is required to list relay sessions'), true;
      }
""")

sub("""      const sessions = [...this.relayTargets.entries()].map(([session, t]) => {
        const stats = this.relayStats.get(session);""", """      const sessions = [...this.relayTargets.entries()]
        .filter(([, t]) => !scope || t.backbone === scope)
        .map(([session, t]) => {
        const stats = this.relayStats.get(session);""")

sub("""          model: t.model ?? null,
          layer: t.layer ?? null,""", """          model: t.model ?? null,
          backbone: t.backbone ?? null,
          layer: t.layer ?? null,""")

open(p, "w").write(s)
print("patched")
