import { useEffect, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { useNav } from '../nav'
import { api, fmt, shorten, explorerTxUrl } from '../api'
import { Card, Stat, CopyButton, TierBadge, tierColor, StatusPill } from '../components'
import { Staking, NodeStatus } from '../services'
import { StakingPanel } from './staking'

export function Overview() {
  const { t } = useI18n()
  const { go } = useNav()
  const { address, balances, txs, network, stakingUrl } = useWallet()
  const [status, setStatus] = useState<NodeStatus | null>(null)
  const totals = status?.totals ?? null

  useEffect(() => {
    if (!address) return
    new Staking(stakingUrl).nodeStatus(address).then(setStatus).catch(() => setStatus(null))
  }, [address, stakingUrl])

  return (
    <div className="grid" style={{ gridTemplateColumns: '1.4fr 1fr', alignItems: 'start' }}>
      {/* hero + stats */}
      <div className="grid">
        <Card>
          <div className="eyebrow">{t('ov.lkcBalance')}</div>
          <div className="hero-num grad-text" style={{ margin: '6px 0 2px' }}>{balances ? fmt(balances.token) : '—'}</div>
          <div className="muted">{balances ? `${fmt(balances.sol)} ${t('ov.solBalance')}` : t('ov.notIssued')}</div>
          <hr className="divider" />
          <div className="spread">
            <span className="mono small muted" title={address ?? ''}>{address ? shorten(address, 10, 10) : ''}</span>
            {address && <CopyButton text={address} />}
          </div>
          <div className="row" style={{ marginTop: 14, gap: 10 }}>
            <button className="btn block" onClick={() => go('wallet')}>{t('common.send')}</button>
            <button className="btn secondary block" onClick={() => go('wallet')}>{t('common.receive')}</button>
          </div>
        </Card>

        <div className="grid cols-3">
          <Card><Stat grad v={totals?.online ?? 0} k={t('ov.nodesOnline')} /></Card>
          <Card><Stat grad v={fmt(totals?.pending ?? 0)} k={t('ov.pendingRewards')} /></Card>
          <Card><Stat grad v={fmt(totals?.effectiveUnits ?? 0)} k={t('ov.effContribution')} /></Card>
        </div>

        <StakingPanel />

        <Card>
          <div className="label" style={{ marginBottom: 10 }}>{t('ov.recentTx')}</div>
          {txs.length === 0 ? <div className="muted small">{t('ov.noTx')}</div> : (
            <div className="grid" style={{ gap: 8 }}>
              {txs.slice(0, 6).map((tx) => (
                <div key={tx.signature} className="spread" style={{ padding: '6px 0' }}>
                  <div className="row">
                    <span className="badge-dot" style={{ background: tx.failed ? 'var(--danger)' : 'var(--good)' }} />
                    <span className="mono small">{shorten(tx.signature)}</span>
                  </div>
                  <a className="small" onClick={() => api.openExternal(explorerTxUrl(tx.signature, network))}>{t('common.explorer')} ↗</a>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      {/* right column: node status */}
      <div className="grid">
        <Card>
          <div className="spread" style={{ marginBottom: 12 }}>
            <span className="label">{t('node.title')}</span>
            <a className="small" onClick={() => go('node')}>{t('node.tier')} ›</a>
          </div>
          {(!status || status.nodes.length === 0) ? (
            <div className="muted small">{t('node.empty')}</div>
          ) : (
            <div className="grid" style={{ gap: 10 }}>
              {status.nodes.slice(0, 6).map((n) => (
                <div key={n.nodeId} className="row" style={{ gap: 10 }}>
                  <TierBadge tier={n.tier ?? '—'} size={30} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="spread"><span style={{ fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{n.label ?? n.nodeId}</span><StatusPill status={n.status} /></div>
                    <div className="small muted">{(n.os ?? '')} · {fmt(n.effectiveUnits ?? n.contributedUnits)} · <span style={{ color: tierColor(n.tier) }}>×{fmt(n.perfMultiplier ?? 1)}</span> · {fmt(n.pendingRewards)} KVR</div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </div>
  )
}
