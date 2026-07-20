import { useEffect, useRef, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { Card, MetricRow, Meter, CopyButton } from '../components'
import { QR } from '../qr'
import { Staking } from '../services'
import { api, isElectron, NODE_GUIDE_URL } from '../api'

type BackendId = 'metal' | 'cuda' | 'rocm' | 'cpu'
type Mode = 'local_shard' | 'rpc_worker'

const osLabel = (os: string) => ({ macos: 'macOS', windows: 'Windows', linux: 'Linux' }[os] || os)

const BACKEND_META: Record<BackendId, { label: string; unit: string }> = {
  metal: { label: 'GPU · Metal', unit: 'GPU · Apple Silicon (Metal/MLX)' },
  cuda: { label: 'GPU · CUDA', unit: 'GPU · NVIDIA (CUDA)' },
  rocm: { label: 'GPU · ROCm', unit: 'GPU · AMD (ROCm/HIP)' },
  cpu: { label: 'CPU', unit: '' },
}
// Which GPU backends to offer. The Electron app IS the node, so filter by its OS
// (macOS = Metal; Windows/Linux = CUDA/ROCm). The web app is a control surface —
// the node's hardware is arbitrary and unrelated to the browser's OS — so offer
// all of them (incl. AMD ROCm) and let the operator pick their node's hardware.
const gpuBackends = (os: string): BackendId[] =>
  isElectron ? (os === 'macos' ? ['metal'] : ['cuda', 'rocm']) : ['metal', 'cuda', 'rocm']

function profile(id: BackendId, mode: Mode, os: string) {
  const local = mode === 'local_shard'
  if (id !== 'cpu') {
    // rough 0.5B Q8 decode estimates: CUDA highest, ROCm close behind, Metal lower.
    const tps = id === 'metal' ? (local ? 130 : 60) : id === 'rocm' ? (local ? 200 : 85) : (local ? 220 : 90)
    return { unit: BACKEND_META[id].unit, id, mem: 0.6, thermal: 0.5, perf: local ? 0.96 : 0.55, tps, note: local ? 'Runs the layer shard on the GPU locally — highest throughput.' : 'GPU tensor ops over RPC — network latency dominates.' }
  }
  return { unit: `CPU · ${os === 'macos' ? 'Apple Silicon' : 'x86-64'}`, id: 'cpu' as const, mem: 0.35, thermal: 0.6, perf: local ? 0.7 : 0.45, tps: local ? 60 : 30, note: 'Stable small-model decode; sustained load heats the CPU.' }
}

export function NodeSettingsScreen() {
  const { t } = useI18n()
  const { address, stakingUrl, os, arch } = useWallet()
  const [backendId, setBackendId] = useState<BackendId>('cuda')
  const [mode, setMode] = useState<Mode>('local_shard')
  const [live, setLive] = useState(false)
  const [connectMsg, setConnectMsg] = useState<string | null>(null)
  const timer = useRef<any>(null)
  // Keep the selection valid for the detected OS (mac → metal; others → cuda/rocm/cpu).
  useEffect(() => { setBackendId((prev) => (prev === 'cpu' || gpuBackends(os).includes(prev)) ? prev : gpuBackends(os)[0]) }, [os])
  const p = profile(backendId, mode, os)
  const nodeId = `desktop-${(address ?? 'dev').slice(0, 8)}`
  const cmd = `LINKCPP_SERVICE=${stakingUrl} LINKCPP_OWNER=${address ?? ''} node connect.js`

  // Track whether THIS machine currently hosts the gateway → its node earns the reward bonus.
  const hostsGw = useRef(false)
  useEffect(() => {
    const poll = () => api.gateway.status().then((s) => { hostsGw.current = !!s.managed }).catch(() => {})
    poll(); const id = setInterval(poll, 3000); return () => clearInterval(id)
  }, [])

  useEffect(() => {
    const svc = new Staking(stakingUrl)
    // Only the Electron app can actually run compute on this machine — the web
    // app must never register the browser itself as a (phantom) node.
    if (isElectron && live && address) {
      svc.registerNode({ nodeId, owner: address, os, deviceKind: 'desktop', accelerator: backendId === 'cpu' ? 'cpu' : 'gpu', label: `${osLabel(os)} · ${p.id} · ${mode}`, perfScore: p.tps, backend: p.id, mode, hostsGateway: hostsGw.current }).catch(() => {})
      timer.current = setInterval(() => svc.heartbeat(nodeId, hostsGw.current).catch(() => {}), 3000)
    }
    return () => { if (timer.current) clearInterval(timer.current) }
  }, [live, address, stakingUrl, backendId, mode, os])

  const connectThis = async () => {
    if (!address) return
    try {
      await new Staking(stakingUrl).registerNode({ nodeId, owner: address, os, deviceKind: 'desktop', accelerator: backendId === 'cpu' ? 'cpu' : 'gpu', label: `This PC · ${osLabel(os)} · ${p.id}`, perfScore: p.tps, backend: p.id, mode, hostsGateway: hostsGw.current })
      setConnectMsg(`✓ ${nodeId} · ${osLabel(os)} (${arch})`)
    } catch (e: any) { setConnectMsg(String(e?.message ?? e)) }
  }

  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr 1fr', alignItems: 'start' }}>
      {/* col 1 — compute */}
      <div className="grid">
        <Card>
          <div className="spread"><div className="label">{t('ns.backend')}</div><span className="chip">{osLabel(os)} · {arch}</span></div>
          <div className="muted small" style={{ margin: '4px 0 12px' }}>{t('ns.backendDesc')}</div>
          <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
            {gpuBackends(os).map((b) => (
              <button key={b} className={`chip ${backendId === b ? 'on' : ''}`} onClick={() => setBackendId(b)}>{BACKEND_META[b].label}</button>
            ))}
            <button className={`chip ${backendId === 'cpu' ? 'on' : ''}`} onClick={() => setBackendId('cpu')}>{BACKEND_META.cpu.label}</button>
          </div>
        </Card>
        <Card>
          <div className="label" style={{ marginBottom: 10 }}>{t('ns.mode')}</div>
          <ModeRow title={t('ns.localShard')} desc={t('ns.localShardDesc')} on={mode === 'local_shard'} onClick={() => setMode('local_shard')} />
          <div style={{ height: 8 }} />
          <ModeRow title={t('ns.rpcWorker')} desc={t('ns.rpcWorkerDesc')} on={mode === 'rpc_worker'} onClick={() => setMode('rpc_worker')} />
        </Card>
      </div>

      {/* col 2 — impact + live */}
      <div className="grid">
        <Card>
          <div className="spread">
            <div className="label">{t('ns.resImpact')}</div>
            <div style={{ textAlign: 'right' }}>
              <div className="grad-text" style={{ fontSize: 24, fontWeight: 800 }}>{p.tps}</div>
              <div className="small muted">tok/s (0.5B Q8)</div>
            </div>
          </div>
          <div style={{ color: 'var(--blue)', fontWeight: 600, fontSize: 13, marginBottom: 8 }}>{p.unit}</div>
          <MetricRow label={t('ns.mem')} value={p.mem} valueText={`${Math.round(p.mem * 100)}%`} color="var(--info)" />
          <MetricRow label={t('ns.thermal')} value={p.thermal} valueText={p.thermal >= 0.75 ? 'high' : p.thermal >= 0.5 ? 'med' : 'low'} color="var(--warn)" />
          <MetricRow label={t('ns.performance')} value={p.perf} valueText={`${Math.round(p.perf * 100)}%`} color="var(--good)" />
          <div className="small muted" style={{ marginTop: 8 }}>※ {p.note}</div>
        </Card>
        {isElectron && (
          <Card>
            <label className="spread">
              <span style={{ fontWeight: 600 }}>{t('ns.runLive')}</span>
              <input type="checkbox" style={{ width: 44 }} checked={live} onChange={(e) => setLive(e.target.checked)} />
            </label>
            {live && (
              <div style={{ marginTop: 14 }}>
                <div className="row" style={{ marginBottom: 10 }}><span className="badge-dot" style={{ background: 'var(--good)' }} /><span className="small" style={{ marginLeft: 8 }}>{t('ns.live')} · {nodeId} · {osLabel(os)}</span></div>
                <MetricRow label={p.id.toUpperCase()} value={p.perf} valueText="est" color="var(--pink)" />
                <div style={{ height: 8 }} /><Meter value={p.mem} color="var(--info)" />
              </div>
            )}
          </Card>
        )}
      </div>

      {/* col 3 — account + device connection (merged from Devices) */}
      <div className="grid">
        <Card style={{ textAlign: 'center' }}>
          <div className="label" style={{ textAlign: 'left', marginBottom: 6 }}>{t('dev.account')}</div>
          <div className="small muted" style={{ textAlign: 'left', marginBottom: 14 }}>{t('dev.accountHint')}</div>
          {address && <QR text={address} size={150} />}
          <div className="mono small muted" style={{ margin: '12px 0', wordBreak: 'break-all' }}>{address}</div>
          {address && <CopyButton text={address} />}
        </Card>
        {isElectron ? (
          <Card>
            <div className="label" style={{ marginBottom: 10 }}>{t('dev.connectThis')}</div>
            <button className="btn block" onClick={connectThis}>{t('dev.connectThis')}</button>
            {connectMsg && <div className="small" style={{ marginTop: 10, color: connectMsg.startsWith('✓') ? 'var(--good)' : 'var(--danger)' }}>{connectMsg}</div>}
          </Card>
        ) : (
          // Web mode: the browser can't run a node — hand off to the native apps.
          // Phone scans the QR → homepage node-operator guide (store links);
          // desktop users follow the same guide for the Electron installer.
          <Card style={{ textAlign: 'center' }}>
            <div className="label" style={{ textAlign: 'left', marginBottom: 6 }}>{t('dev.getApp')}</div>
            <div className="small muted" style={{ textAlign: 'left', marginBottom: 14 }}>{t('dev.getAppHint')}</div>
            <QR text={NODE_GUIDE_URL} size={150} />
            <div className="small muted" style={{ margin: '12px 0' }}>{t('dev.scanQr')}</div>
            <button className="btn block" onClick={() => api.openExternal(NODE_GUIDE_URL)}>{t('dev.downloadDesktop')}</button>
          </Card>
        )}
        <Card>
          <div className="label" style={{ marginBottom: 6 }}>{t('dev.otherHint')}</div>
          <div className="mono small" style={{ background: 'var(--surface-2)', padding: 12, borderRadius: 10, wordBreak: 'break-all', marginBottom: 10 }}>{cmd}</div>
          <CopyButton text={cmd} />
        </Card>
      </div>
    </div>
  )
}

function ModeRow({ title, desc, on, onClick }: { title: string; desc: string; on: boolean; onClick: () => void }) {
  return (
    <button className="nav-item" style={{ alignItems: 'flex-start', background: on ? 'color-mix(in srgb, var(--pink) 10%, transparent)' : 'var(--surface-2)', padding: 12 }} onClick={onClick}>
      <span style={{ width: 20, height: 20, borderRadius: 50, background: on ? 'var(--pink)' : 'var(--stroke)', display: 'grid', placeItems: 'center', color: '#fff', fontSize: 12 }}>{on ? '✓' : ''}</span>
      <span style={{ flex: 1, textAlign: 'left' }}>
        <div style={{ fontWeight: 600, color: 'var(--text)' }}>{title}</div>
        <div className="small muted">{desc}</div>
      </span>
    </button>
  )
}
