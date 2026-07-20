import { useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { api, fmt, shorten, explorerTxUrl } from '../api'
import { Card, Field, CopyButton } from '../components'
import { QR } from '../qr'

export function WalletScreen() {
  const { t } = useI18n()
  const { address, balances, txs, network, refresh } = useWallet()
  const [tab, setTab] = useState<'send' | 'receive'>('send')
  const [asset, setAsset] = useState<'KVR' | 'SOL'>('KVR')
  const [to, setTo] = useState('')
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)
  const [sig, setSig] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)

  const send = async () => {
    setBusy(true); setErr(null); setSig(null)
    try {
      const amt = Number(amount)
      const s = asset === 'KVR'
        ? await api.solana.sendToken({ to: to.trim(), amount: amt, network })
        : await api.solana.sendSol({ to: to.trim(), amount: amt, network })
      setSig(s); setTo(''); setAmount(''); await refresh()
    } catch (e: any) { setErr(String(e?.message ?? e)) } finally { setBusy(false) }
  }

  // Mobile-like: a narrow, tall single column.
  return (
    <div className="grid" style={{ gap: 16, width: 420, maxWidth: '100%', margin: '0 auto' }}>
      <Card style={{ textAlign: 'center' }}>
        <div className="eyebrow">{t('ov.lkcBalance')}</div>
        <div className="hero-num grad-text" style={{ margin: '6px 0 2px' }}>{balances ? fmt(balances.token) : '—'}</div>
        <div className="muted">{balances ? `${fmt(balances.sol)} ${t('ov.solBalance')}` : t('ov.notIssued')}</div>
        <hr className="divider" />
        <div className="mono small muted" style={{ wordBreak: 'break-all' }}>{address}</div>
        <div style={{ height: 10 }} />
        {address && <CopyButton text={address} />}
      </Card>

      <Card>
        <div className="row" style={{ gap: 8, marginBottom: 14 }}>
          <button className={`chip ${tab === 'send' ? 'on' : ''}`} style={{ flex: 1, textAlign: 'center' }} onClick={() => setTab('send')}>{t('common.send')}</button>
          <button className={`chip ${tab === 'receive' ? 'on' : ''}`} style={{ flex: 1, textAlign: 'center' }} onClick={() => setTab('receive')}>{t('common.receive')}</button>
        </div>

        {tab === 'send' ? (
          <>
            <Field label={t('send.asset')}>
              <div className="row" style={{ gap: 8 }}>
                {(['KVR', 'SOL'] as const).map((a) => <button key={a} className={`chip ${asset === a ? 'on' : ''}`} onClick={() => setAsset(a)}>{a}</button>)}
              </div>
            </Field>
            <div style={{ height: 12 }} />
            <Field label={t('send.recipient')}><input className="mono" value={to} onChange={(e) => setTo(e.target.value)} placeholder="Recipient address" /></Field>
            <div style={{ height: 12 }} />
            <Field label={`${t('send.amount')} (${asset})`}><input value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.0" inputMode="decimal" /></Field>
            <div style={{ height: 16 }} />
            <button className="btn block" disabled={busy || !to.trim() || !Number(amount)} onClick={send}>{busy ? t('common.busy') : t('send.review')}</button>
            {sig && <div className="small" style={{ marginTop: 12 }}><span style={{ color: 'var(--good)' }}>✓ {t('send.sent')}</span>{' · '}<a onClick={() => api.openExternal(explorerTxUrl(sig, network))}>{t('common.explorer')} ↗</a></div>}
            {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 12 }}>{err}</div>}
          </>
        ) : (
          <div style={{ textAlign: 'center' }}>
            {address && <QR text={address} size={190} />}
            <div className="mono small muted" style={{ margin: '12px 0', wordBreak: 'break-all' }}>{address}</div>
            {address && <CopyButton text={address} />}
            <div className="small muted" style={{ marginTop: 10 }}>{t('recv.hint')}</div>
          </div>
        )}
      </Card>

      <Card>
        <div className="label" style={{ marginBottom: 10 }}>{t('ov.recentTx')}</div>
        {txs.length === 0 ? <div className="muted small">{t('ov.noTx')}</div> : txs.map((tx) => (
          <div key={tx.signature} className="spread" style={{ padding: '5px 0' }}>
            <div className="row"><span className="badge-dot" style={{ background: tx.failed ? 'var(--danger)' : 'var(--good)' }} /><span className="mono small">{shorten(tx.signature)}</span></div>
            <a className="small" onClick={() => api.openExternal(explorerTxUrl(tx.signature, network))}>↗</a>
          </div>
        ))}
      </Card>
    </div>
  )
}
