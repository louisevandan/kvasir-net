import { useEffect, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { api, fmt } from '../api'
import { Card, Stat } from '../components'
import { Staking, StakingConfig, StakePosition, NodeStatus } from '../services'

/** Staking + node-reward panel, embedded on the main dashboard. */
export function StakingPanel() {
  const { t } = useI18n()
  const { address, stakingUrl, treasuryOwner, network, refresh } = useWallet()
  const svc = new Staking(stakingUrl)
  const [config, setConfig] = useState<StakingConfig | null>(null)
  const [pos, setPos] = useState<StakePosition | null>(null)
  const [nodes, setNodes] = useState<NodeStatus | null>(null)
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)

  const load = async () => {
    if (!address) return
    try {
      setConfig(await svc.config()); setPos(await svc.position(address)); setNodes(await svc.nodeStatus(address)); setMsg(null)
    } catch (e: any) { setMsg(String(e?.message ?? e)) }
  }
  useEffect(() => { load() }, [address, stakingUrl])

  const stake = async () => {
    if (!address) return
    setBusy(true); setMsg(null)
    try {
      const sig = await api.solana.sendToken({ to: treasuryOwner, amount: Number(amount), network })
      setPos(await svc.stake(address, Number(amount), sig)); setAmount(''); await refresh()
    } catch (e: any) { setMsg(String(e?.message ?? e)) } finally { setBusy(false) }
  }
  const online = (nodes?.totals.online ?? 0) > 0
  const unstake = async () => { if (!address) return; setBusy(true); try { await svc.unstake(address); await load(); await refresh() } catch (e: any) { setMsg(String(e?.message ?? e)) } finally { setBusy(false) } }
  const claim = async () => { if (!address) return; setBusy(true); try { await svc.claim(address); await load(); await refresh() } catch (e: any) { setMsg(String(e?.message ?? e)) } finally { setBusy(false) } }

  return (
    <Card>
      <div className="spread" style={{ marginBottom: 14 }}>
        <span className="label">{t('staking.title')}</span>
        <span className="small muted">{config ? `${config.aprPercent}% ${t('staking.apr')}` : ''}</span>
      </div>
      <div className="grid cols-3">
        <Stat grad v={fmt(pos?.principal ?? 0)} k={t('staking.staked')} />
        <Stat grad v={fmt(pos?.rewards ?? 0)} k={t('staking.rewards')} />
        <Stat grad v={fmt(nodes?.totals.pending ?? 0)} k={t('staking.nodeRewards')} />
      </div>
      <hr className="divider" />
      {/* Staking is reserved for online contributing nodes — the backend rejects
          a stake from an owner with no online node, so gate the button too. */}
      {!online && <div className="small" style={{ color: 'var(--warn)', marginBottom: 10 }}>⚠ {t('staking.needOnline')}</div>}
      <input value={amount} onChange={(e) => setAmount(e.target.value)} placeholder={t('staking.amount')} inputMode="decimal" />
      <div className="row" style={{ gap: 10, marginTop: 12 }}>
        <button className="btn block" disabled={busy || !Number(amount) || !online} onClick={stake} title={!online ? t('staking.needOnline') : undefined}>{t('staking.stake')}</button>
        <button className="btn ghost block" disabled={busy || !(pos?.principal)} onClick={unstake}>{t('staking.unstake')}</button>
        <button className="btn secondary block" disabled={busy || !(nodes?.totals.pending)} onClick={claim}>{t('staking.claim')}</button>
      </div>
      {msg && <div className="small" style={{ color: 'var(--danger)', marginTop: 12 }}>{msg}</div>}
    </Card>
  )
}
