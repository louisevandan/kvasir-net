import { useEffect, useRef, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { Card, MetricRow, CopyButton } from '../components'
import { QR } from '../qr'
import { Staking } from '../services'
import { api, capacityForBudget, isElectron, NODE_GUIDE_URL } from '../api'
import type { NodeStatus } from '../api'

const osLabel = (os: string) => ({ macos: 'macOS', windows: 'Windows', linux: 'Linux' }[os] || os)
const BACKEND_LABEL: Record<string, string> = { metal: 'GPU · Metal', cuda: 'GPU · CUDA', rocm: 'GPU · ROCm', cpu: 'CPU' }
const gib = (bytes: number | null | undefined) => (bytes ? `${(bytes / 1024 ** 3).toFixed(0)} GiB` : '—')
// The reward tiers the settlement service applies. Shown so an operator can see
// where a measured number lands instead of guessing.
const tierOf = (tps: number | null) => (tps === null ? '—' : tps >= 90 ? 'S' : tps >= 60 ? 'A' : tps >= 30 ? 'B' : 'C')

// VRAM is shown to one decimal: 8 GiB cards are common and whole-GiB rounding
// hides the difference between "fits" and "does not".
const gib1 = (bytes: number | null | undefined) => (bytes == null ? '—' : `${(bytes / 1024 ** 3).toFixed(1)} GiB`)
const VRAM_STEP = 256 * 1024 ** 2

/** Which engines can actually compute here — "a GPU" is not the same as "able to work". */
function ComputeCard({ status }: { status: NodeStatus | null }) {
  const { t } = useI18n()
  const compute = status?.compute
  return (
    <Card>
      <div className="spread"><div className="label">{t('ns.compute')}</div>
        <span className="chip" style={{ color: compute?.summary.canServeExperts ? 'var(--good)' : 'var(--muted)' }}>
          {compute?.summary.canServeExperts ? t('ns.exReady') : t('ns.exMissing')}
        </span>
      </div>
      <div className="muted small" style={{ margin: '4px 0 12px' }}>{t('ns.computeDesc')}</div>
      {(compute?.executors ?? []).map((ex) => {
        // Three states, not two: "installed but broken" is the one that used
        // to hide behind a node that looked idle.
        const state = ex.runnable ? 'ok' : ex.found ? 'broken' : 'missing'
        const color = state === 'ok' ? 'var(--good)' : state === 'broken' ? 'var(--danger)' : 'var(--muted)'
        return (
          <div key={ex.id} style={{ padding: '6px 0', borderTop: '1px solid var(--line)' }}>
            <div className="spread small">
              <span className="mono">{ex.id}</span>
              <span style={{ color, fontWeight: 600 }}>
                {state === 'ok' ? t('ns.exReady') : state === 'broken' ? t('ns.exBroken') : t('ns.exMissing')}
              </span>
            </div>
            <div className="small muted">{ex.purpose}</div>
            {state === 'broken' && <div className="small" style={{ color: 'var(--danger)' }}>{ex.reason}</div>}
          </div>
        )
      })}
      {compute?.gpu.ready === false && (
        <div className="small muted" style={{ marginTop: 6 }}>{t('ns.vramNoGpu')}</div>
      )}
      <CudaPackRow status={status} />
    </Card>
  )
}

const mb = (bytes: number) => `${Math.round(bytes / 1e6).toLocaleString()} MB`

/**
 * The expert engine for NVIDIA GPUs is not in the installer — nobody without
 * such a GPU should download it. Offered only when there is an NVIDIA GPU and
 * no working expert engine yet.
 */
function CudaPackRow({ status }: { status: NodeStatus | null }) {
  const { t } = useI18n()
  const pack = status?.cudaPack
  const compute = status?.compute
  const expert = compute?.executors.find((e) => e.id === 'linkcpp-expert-worker')
  // pack.version is null where no CUDA pack exists (a Mac uses Metal, bundled).
  if (!pack || !pack.version || !compute?.gpu.ready || expert?.runnable) return null
  const busy = pack.phase === 'downloading' || pack.phase === 'verifying' || pack.phase === 'installing'
  return (
    <div style={{ marginTop: 10, paddingTop: 10, borderTop: '1px solid var(--line)' }}>
      <div className="small muted" style={{ marginBottom: 8 }}>{t('ns.packDesc')}</div>
      {!pack.available && <div className="small muted">{t('ns.packUnpublished')}</div>}
      {pack.available && !busy && (
        <button className="btn" onClick={() => { void api.node?.installCudaPack() }}>{t('ns.packGet', mb(pack.bytes))}</button>
      )}
      {pack.phase === 'downloading' && (
        <>
          <div style={{ height: 6, background: 'var(--surface-2)', borderRadius: 3, overflow: 'hidden', margin: '4px 0' }}>
            <div style={{ height: '100%', width: `${pack.total ? (pack.received / pack.total) * 100 : 0}%`, background: 'var(--pink)' }} />
          </div>
          <div className="spread small">
            <span className="muted">{t('ns.packProgress', mb(pack.received), mb(pack.total))}</span>
            <button className="btn ghost" onClick={() => { void api.node?.cancelCudaPack() }}>{t('ns.packCancel')}</button>
          </div>
        </>
      )}
      {(pack.phase === 'verifying' || pack.phase === 'installing') && <div className="small muted">{t('ns.packVerifying')}</div>}
      {pack.phase === 'failed' && pack.error && (
        <div className="small" style={{ color: 'var(--danger)', marginTop: 6 }}>{t('ns.packFailed', pack.error)}</div>
      )}
    </div>
  )
}

/**
 * How much GPU memory the node may use. The GPU is usually shared with other
 * software, so this is the operator's decision, not the app's — and it takes
 * effect immediately, as a cap on how many experts the bridge will assign.
 */
function VramCard({ status, onChange }: { status: NodeStatus | null; onChange: (bytes: number) => void }) {
  const { t } = useI18n()
  const gpu = status?.compute?.gpu.gpus?.[0]
  const [value, setValue] = useState<number | null>(null)
  // Follow the saved budget until the operator starts dragging.
  const saved = status?.vramBudgetBytes ?? null
  const current = value ?? saved ?? (gpu ? Math.floor(gpu.totalBytes / 2 / VRAM_STEP) * VRAM_STEP : 0)
  if (!gpu) {
    return (
      <Card>
        <div className="label">{t('ns.vram')}</div>
        <div className="muted small" style={{ marginTop: 4 }}>{t('ns.vramNoGpu')}</div>
      </Card>
    )
  }
  const max = Math.floor(gpu.totalBytes / VRAM_STEP) * VRAM_STEP
  // Same conversion the app uses for its offer: the budget, capped by what is
  // actually free (our own worker's share added back), through the executor's
  // measured memory model.
  const own = status?.ownWorkerGpuBytes ?? 0
  const available = Math.max(0, gpu.freeBytes + own - (status?.vramReserveBytes ?? 0))
  // Total across slots, not one shard's window: a machine lending 4 GiB holds
  // several shards, and the slider should say what it will actually take.
  // A null model means the app could not tell whether this GPU's memory is the
  // machine's memory, and the two answers differ by ~320 MiB per slot. The
  // node offers nothing in that state, so the slider must not promise a number
  // either — showing the preview default here would be the one place the UI
  // and the offer disagree.
  const knowsCost = status?.expertMemoryModel != null
  const experts = capacityForBudget(current, status?.expertMemoryModel, available).experts
  const usedByOthers = Math.max(0, gpu.usedBytes - own)
  const overFree = current > available
  const pct = (b: number) => `${Math.min(100, Math.max(0, (b / gpu.totalBytes) * 100))}%`
  return (
    <Card>
      <div className="spread"><div className="label">{t('ns.vram')}</div>
        <span className="chip mono">{gib1(current)} / {gib1(gpu.totalBytes)}</span>
      </div>
      <div className="muted small" style={{ margin: '4px 0 12px' }}>{t('ns.vramDesc')}</div>
      {/* Live picture of the card: what others hold now, and what this node would take. */}
      <div style={{ position: 'relative', height: 10, background: 'var(--surface-2)', borderRadius: 5, overflow: 'hidden', marginBottom: 8 }}
        title={`${gpu.name} · used ${gib1(usedByOthers)} · free ${gib1(gpu.freeBytes)}`}>
        <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: pct(usedByOthers), background: 'var(--muted)', opacity: 0.45 }} />
        <div style={{ position: 'absolute', left: pct(usedByOthers), top: 0, bottom: 0, width: pct(current), background: overFree ? 'var(--danger)' : 'var(--pink)' }} />
      </div>
      <label htmlFor="vram-budget" className="sr-only">{t('ns.vram')}</label>
      <input id="vram-budget" type="range" min={0} max={max} step={VRAM_STEP} value={Math.min(current, max)}
        onChange={(e) => setValue(Number(e.target.value))}
        // Commit on release, not on every pixel of the drag.
        onMouseUp={() => { if (value != null) onChange(value) }}
        onKeyUp={() => { if (value != null) onChange(value) }}
        onTouchEnd={() => { if (value != null) onChange(value) }}
        style={{ width: '100%', padding: 0 }} />
      <div className="small" style={{ marginTop: 6 }}>
        {current === 0 ? t('ns.vramNone')
          : knowsCost ? t('ns.vramExperts', experts)
          : (status?.memoryTopologyReason || t('ns.vramUnknownCost'))}
      </div>
      {overFree && current > 0 && (
        <div className="small" style={{ color: 'var(--danger)', marginTop: 4 }}>{t('ns.vramOverFree', gib1(available))}</div>
      )}
      <div className="small muted mono" style={{ marginTop: 6 }}>
        {/*
          Only what this card actually knows. The line used to read
          "<name> · <driver> · CUDA <version> · <util>%" for every machine, so a
          Mac showed "Apple M4 Pro · · CUDA — · %" — a CUDA version on a Metal
          GPU, with three empty fields around it. nvidia-smi supplies the driver,
          CUDA version and utilisation; nothing does on Apple Silicon, and an
          empty field is worse than an absent one.
        */}
        {[
          gpu.name,
          gpu.driver || null,
          status?.compute?.gpu.cudaVersion ? `CUDA ${status.compute.gpu.cudaVersion}` : null,
          gpu.utilizationPct != null ? `${gpu.utilizationPct}%` : null,
        ].filter(Boolean).join(' · ')}
      </div>
    </Card>
  )
}

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

  // The slider commits on release; the main process caps the next volunteer
  // request with it, so it takes effect without restarting the node.
  const saveVram = async (bytes: number) => {
    if (!api.node) return
    try {
      await api.node.setVramBudget(bytes)
      setStatus(await api.node.status())
    } catch { /* the next poll will show the saved value */ }
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

        {isElectron && <ComputeCard status={status} />}
        {isElectron && <VramCard status={status} onChange={saveVram} />}

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
