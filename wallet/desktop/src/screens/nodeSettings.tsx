import { useEffect, useRef, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { Card, MetricRow, CopyButton } from '../components'
import { QR } from '../qr'
import { Staking } from '../services'
import { api, isElectron, NODE_GUIDE_URL } from '../api'
import type { NodeStatus } from '../api'

const osLabel = (os: string) => ({ macos: 'macOS', windows: 'Windows', linux: 'Linux' }[os] || os)
const BACKEND_LABEL: Record<string, string> = { metal: 'GPU · Metal', cuda: 'GPU · CUDA', rocm: 'GPU · ROCm', cpu: 'CPU' }
const gib = (bytes: number | null | undefined) => (bytes ? `${(bytes / 1024 ** 3).toFixed(0)} GiB` : '—')
// The reward tiers the settlement service applies. Shown so an operator can see
// where a measured number lands instead of guessing.
const tierOf = (tps: number | null) => (tps === null ? '—' : tps >= 90 ? 'S' : tps >= 60 ? 'A' : tps >= 30 ? 'B' : 'C')

export function NodeSettingsScreen() {
  const { t } = useI18n()
  const { address, stakingUrl, os, arch } = useWallet()
  const [status, setStatus] = useState<NodeStatus | null>(null)
  const [busy, setBusy] = useState<'start' | 'stop' | 'measure' | null>(null)
  const [connectMsg, setConnectMsg] = useState<string | null>(null)
  const timer = useRef<any>(null)
  const nodeId = `desktop-${(address ?? 'dev').slice(0, 8)}`
  const cmd = `KVASIR_SERVICE=${stakingUrl} KVASIR_OWNER=${address ?? ''} p4-agent 127.0.0.1:42031 tcp://127.0.0.1:42031`

  // Poll the agent. `inspect` asks it what it is holding; that is the only place
  // the stage list can come from, because placement is not the app's decision.
  useEffect(() => {
    if (!api.node) return
    let alive = true
    const poll = () => api.node!.status({ inspect: true }).then((s) => { if (alive) setStatus(s) }).catch(() => {})
    poll()
    const id = setInterval(poll, 5000)
    return () => { alive = false; clearInterval(id) }
  }, [])

  // Track whether THIS machine currently hosts the gateway → its node earns the reward bonus.
  const hostsGw = useRef(false)
  useEffect(() => {
    const poll = () => api.gateway.status().then((s) => { hostsGw.current = !!s.managed }).catch(() => {})
    poll(); const id = setInterval(poll, 3000); return () => clearInterval(id)
  }, [])

  const capability = status?.capability
  const backend = capability?.backend ?? 'cpu'
  const measured = status?.measured ?? null
  const running = !!status?.running

  // Register only while an agent is actually running: a heartbeat from a machine
  // that runs nothing is what this screen used to send, and it earned rewards.
  const nodeBody = () => ({
    nodeId, owner: address as string, os, deviceKind: 'desktop',
    accelerator: backend === 'cpu' ? 'cpu' : 'gpu',
    label: `${osLabel(os)} · ${capability?.gpus[0]?.name ?? capability?.cpu.brand ?? backend}`,
    // Deliberately not sending perfScore. The settlement service honours it only
    // from a trusted reporter, because a node that can assert its own tier can
    // mint its own rewards — so a number sent from here is discarded, and
    // sending it anyway would only make this screen look like it set the tier.
    // The tier comes from throughput the network measured, reported by the
    // bridge in its contribution feed.
    backend, mode: 'p4_agent', hostsGateway: hostsGw.current,
  })

  useEffect(() => {
    const svc = new Staking(stakingUrl)
    if (isElectron && running && address) {
      svc.registerNode(nodeBody()).catch(() => {})
      timer.current = setInterval(() => svc.heartbeat(nodeId, hostsGw.current).catch(() => {}), 3000)
    }
    return () => { if (timer.current) clearInterval(timer.current) }
  }, [running, address, stakingUrl, backend, measured?.tps])

  const toggleAgent = async () => {
    if (!api.node) return
    setBusy(running ? 'stop' : 'start')
    try { setStatus(running ? await api.node.stop() : await api.node.start()) }
    finally { setBusy(null) }
  }

  const measure = async () => {
    if (!api.node) return
    setBusy('measure')
    try {
      const r = await api.node.benchmark(64)
      setStatus(await api.node.status())
      if (!r.ok) setConnectMsg(r.error ?? 'measurement failed')
    } finally { setBusy(null) }
  }

  const connectThis = async () => {
    if (!address) return
    try {
      await new Staking(stakingUrl).registerNode(nodeBody())
      setConnectMsg(`✓ ${nodeId} · ${osLabel(os)} (${arch})`)
    } catch (e: any) { setConnectMsg(String(e?.message ?? e)) }
  }

  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr 1fr', alignItems: 'start' }}>
      {/* col 1 — what this machine actually is */}
      <div className="grid">
        <Card>
          <div className="spread"><div className="label">{t('ns.machine')}</div><span className="chip">{osLabel(os)} · {arch}</span></div>
          <div className="muted small" style={{ margin: '4px 0 12px' }}>{t('ns.machineDesc')}</div>
          <div className="row" style={{ gap: 8, flexWrap: 'wrap', marginBottom: 12 }}>
            <span className="chip on">{BACKEND_LABEL[backend] ?? backend}</span>
            <span className="chip">{capability?.cpu.cores ?? '—'} cores</span>
            <span className="chip">{gib(capability?.ramBytes)} RAM</span>
          </div>
          <div className="small muted" style={{ marginBottom: 4 }}>{capability?.cpu.brand}</div>
          {(capability?.gpus ?? []).map((gpu, i) => (
            <div key={i} className="spread small" style={{ padding: '4px 0' }}>
              <span>{gpu.name}</span><span className="muted">{gib(gpu.memoryBytes)}</span>
            </div>
          ))}
        </Card>

        <Card>
          <div className="spread"><div className="label">{t('ns.agent')}</div>
            <span className="chip" style={{ color: running ? 'var(--good)' : 'var(--muted)' }}>{running ? t('ns.running') : t('ns.stopped')}</span>
          </div>
          <div className="muted small" style={{ margin: '4px 0 12px' }}>{t('ns.agentDesc')}</div>
          {isElectron ? (
            <>
              <button className="btn block" disabled={busy !== null || (!running && !status?.binary)} onClick={toggleAgent}>
                {running ? t('ns.stop') : t('ns.start')}
              </button>
              {!status?.binary && <div className="small" style={{ marginTop: 10, color: 'var(--warn)' }}>{t('ns.noBinary')}</div>}
              {running && (
                <div className="mono small muted" style={{ marginTop: 10 }}>
                  pid {status?.pid} · {status?.address} · {Math.round((status?.uptimeMs ?? 0) / 1000)}s
                </div>
              )}
              {/* Running is not the same as reachable. The agent binds loopback,
                  so until the tunnel has an address the network cannot dial this
                  machine and no work will ever arrive. Say which it is. */}
              {running && (
                <div className="mono small" style={{ marginTop: 6, color: status?.relay?.registered ? 'var(--ok)' : 'var(--warn)' }}>
                  {status?.relay?.registered
                    ? `${t('ns.reachable')} ${status.relay.advertise}`
                    : status?.relay?.lastError
                      ? `${t('ns.notReachable')} — ${status.relay.lastError}`
                      : t('ns.reachPending')}
                </div>
              )}
              {status?.lastError && !running && <div className="small" style={{ marginTop: 10, color: 'var(--danger)' }}>{status.lastError}</div>}
            </>
          ) : (
            <div className="small muted">{t('dev.getAppHint')}</div>
          )}
        </Card>

        <Card>
          <div className="label" style={{ marginBottom: 8 }}>{t('ns.stages')}</div>
          {(status?.nodes.length ?? 0) === 0
            ? <div className="small muted">{t('ns.noStages')}</div>
            : status!.nodes.map((n) => (
              <div key={n.nodeId} className="spread small" style={{ padding: '4px 0' }}>
                <span className="mono">{n.nodeId}</span>
                <span className="muted">{n.state} · gen {n.generation}{n.adapterKind ? ` · ${n.adapterKind}` : ''}</span>
              </div>
            ))}
        </Card>
      </div>

      {/* col 2 — measured throughput, and what it is worth */}
      <div className="grid">
        <Card>
          <div className="spread">
            <div className="label">{t('ns.throughput')}</div>
            <div style={{ textAlign: 'right' }}>
              <div className="grad-text" style={{ fontSize: 24, fontWeight: 800 }}>
                {measured ? measured.tps.toFixed(1) : '—'}
              </div>
              <div className="small muted">tok/s measured here</div>
            </div>
          </div>
          <div className="small muted" style={{ margin: '8px 0 12px' }}>{t('ns.measureHint')}</div>
          <div className="small muted" style={{ margin: '0 0 12px' }}>
            {t('ns.tierNote')} {measured ? `(${tierOf(measured.tps)})` : ''}
          </div>
          {isElectron && (
            <button className="btn block" disabled={busy !== null} onClick={measure}>
              {busy === 'measure' ? t('ns.measuring') : t('ns.measure')}
            </button>
          )}
          {measured
            ? <div className="small muted" style={{ marginTop: 10 }}>{measured.model} · {measured.tokens} tok · {(measured.elapsedMs / 1000).toFixed(1)}s</div>
            : <div className="small muted" style={{ marginTop: 10 }}>{t('ns.notMeasured')}</div>}
        </Card>

        {isElectron && running && (
          <Card>
            <div className="row" style={{ marginBottom: 10 }}>
              <span className="badge-dot" style={{ background: 'var(--good)' }} />
              <span className="small" style={{ marginLeft: 8 }}>{t('ns.live')} · {nodeId}</span>
            </div>
            <MetricRow label={BACKEND_LABEL[backend] ?? backend} value={Math.min(1, (measured?.tps ?? 0) / 120)}
              valueText={measured ? `${Math.round(measured.tps)} tok/s` : t('ns.notMeasured')} color="var(--pink)" />
          </Card>
        )}

        {isElectron && (status?.log.length ?? 0) > 0 && (
          <Card>
            <div className="label" style={{ marginBottom: 8 }}>{t('ns.agent')}</div>
            <div className="mono small muted" style={{ maxHeight: 160, overflow: 'auto', whiteSpace: 'pre-wrap' }}>
              {status!.log.slice(-12).join('\n')}
            </div>
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
