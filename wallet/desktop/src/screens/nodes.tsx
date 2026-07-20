import { useEffect, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { useNav } from '../nav'
import { fmt } from '../api'
import { Card, Stat, TierBadge, tierColor, StatusPill } from '../components'
import { Staking, NodeStatus, NodeStatusItem } from '../services'

const TIERS: [string, string, string][] = [['S', '≥ 90 tok/s', '×1.5'], ['A', '60–89 tok/s', '×1.25'], ['B', '30–59 tok/s', '×1.0'], ['C', '< 30 tok/s', '×0.7']]
const osIcon = (os?: string) => ({ macos: '🖥', ios: '📱', android: '🤖', windows: '🪟', linux: '🐧' }[(os || '').toLowerCase()] || '💻')
const modeLabel = (m?: string) => m === 'local_shard' ? 'local shard' : m === 'rpc_worker' ? 'RPC worker' : (m || '')

export function NodesScreen() {
  const { t } = useI18n()
  const { go } = useNav()
  const { address, stakingUrl } = useWallet()
  const [status, setStatus] = useState<NodeStatus | null>(null)
  const [msg, setMsg] = useState<string | null>(null)

  const load = () => {
    if (!address) return
    new Staking(stakingUrl).nodeStatus(address).then(setStatus).catch((e) => setMsg(String(e?.message ?? e)))
  }
  useEffect(() => { load() }, [address, stakingUrl])

  const remove = async (nodeId: string) => {
    if (!address || !window.confirm(t('node.removeConfirm'))) return
    try { await new Staking(stakingUrl).removeNode(nodeId, address); load() } catch (e: any) { setMsg(String(e?.message ?? e)) }
  }

  const totals = status?.totals
  return (
    <div className="grid" style={{ gap: 18 }}>
      <div className="grid cols-4">
        <Card><Stat grad v={totals?.nodes ?? 0} k={t('node.nodes')} /></Card>
        <Card><Stat grad v={totals?.online ?? 0} k={t('common.online')} /></Card>
        <Card><Stat grad v={fmt(totals?.effectiveUnits ?? 0)} k={t('node.effContrib')} /></Card>
        <Card><Stat grad v={fmt(totals?.pending ?? 0)} k={t('node.claimable')} /></Card>
      </div>

      <div className="grid" style={{ gridTemplateColumns: '1fr 2fr', alignItems: 'start' }}>
        <Card>
          <div className="label" style={{ marginBottom: 4 }}>{t('node.tier')}</div>
          <div className="small muted" style={{ marginBottom: 12 }}>{t('node.tierDesc')}</div>
          {TIERS.map(([tier, range, mult]) => (
            <div key={tier} className="row" style={{ padding: '5px 0' }}>
              <TierBadge tier={tier} size={24} />
              <span className="small muted" style={{ flex: 1 }}>{range}</span>
              <span className="mono" style={{ color: tierColor(tier), fontWeight: 700 }}>{mult}</span>
            </div>
          ))}
        </Card>

        <div className="grid">
          {(!status || status.nodes.length === 0) ? (
            <Card style={{ textAlign: 'center', padding: 34 }}>
              <div style={{ fontWeight: 700 }}>{t('node.empty')}</div>
              <div className="muted small" style={{ marginTop: 4 }}>{t('node.emptyHint')}</div>
            </Card>
          ) : status.nodes.map((n) => <NodeCard key={n.nodeId} n={n} t={t} onOpen={() => go('nodeSettings')} onRemove={() => remove(n.nodeId)} />)}
        </div>
      </div>
      {msg && <div className="small" style={{ color: 'var(--danger)' }}>{msg}</div>}
    </div>
  )
}

function NodeCard({ n, t, onOpen, onRemove }: { n: NodeStatusItem; t: (k: string) => string; onOpen: () => void; onRemove: () => void }) {
  const tier = n.tier ?? '—'
  const mult = n.perfMultiplier ?? 1
  const eff = n.effectiveUnits ?? n.contributedUnits
  return (
    <Card>
      <div className="spread">
        <div className="row" style={{ cursor: 'pointer' }} onClick={onOpen} title={t('node.openSettings')}>
          <span style={{ fontSize: 20 }}>{osIcon(n.os)}</span>
          <div>
            <div style={{ fontWeight: 600 }}>{n.label ?? n.nodeId} <span className="muted" style={{ fontWeight: 400 }}>›</span></div>
            <div className="small muted">{(n.os ?? 'unknown')} · {(n.accelerator ?? 'cpu').toUpperCase()}</div>
          </div>
        </div>
        <div className="row" style={{ gap: 8 }}>
          <StatusPill status={n.status} />
          <button className="menu-btn" style={{ width: 32, height: 32, fontSize: 14 }} title={t('node.remove')} onClick={onRemove}>🗑</button>
        </div>
      </div>

      <div className="row" style={{ marginTop: 12, gap: 12, background: `color-mix(in srgb, ${tierColor(tier)} 10%, transparent)`, borderRadius: 12, padding: 12 }}>
        <TierBadge tier={tier} size={36} />
        <div style={{ flex: 1 }}>
          <div style={{ color: tierColor(tier), fontWeight: 700 }}>{t('node.tier')} {tier} · ×{fmt(mult)}</div>
          <div className="small muted">{n.backend ? n.backend.toUpperCase() + ' · ' : ''}{n.perfScore ? Math.round(n.perfScore) + ' tok/s' : ''}{n.mode ? ' · ' + modeLabel(n.mode) : ''}</div>
        </div>
      </div>

      <div className="small" style={{ margin: '10px 0' }}>
        <span className="muted">{t('node.rawContrib')} {fmt(n.contributedUnits)}</span>
        <span style={{ color: tierColor(tier), fontWeight: 700 }}>  ×{fmt(mult)}  </span>
        <span style={{ fontWeight: 600 }}>→ {t('node.effContrib')} {fmt(eff)}</span>
      </div>
      <div className="grid cols-3">
        <Stat grad v={fmt(eff)} k={t('node.effContrib')} />
        <Stat grad v={fmt(n.pendingRewards)} k={t('node.claimable')} />
        <Stat grad v={fmt(n.claimedTotal)} k={t('node.received')} />
      </div>
    </Card>
  )
}
