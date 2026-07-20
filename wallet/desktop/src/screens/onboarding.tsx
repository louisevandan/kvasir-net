import { useState } from 'react'
import { api, isElectron } from '../api'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { Card } from '../components'
import { PassphraseFields, passphraseError } from './lock'

export function Onboarding() {
  const { t } = useI18n()
  const { reload } = useWallet()
  const [mode, setMode] = useState<'welcome' | 'create' | 'import'>('welcome')

  return (
    <div style={{ display: 'grid', placeItems: 'center', height: '100vh', padding: 24 }}>
      <div style={{ width: 460, maxWidth: '92vw' }}>
        <div style={{ textAlign: 'center', marginBottom: 22 }}>
          <div className="brand-mark" style={{ width: 60, height: 60, margin: '0 auto 14px', fontSize: 26 }}>K</div>
          <div style={{ fontSize: 24, fontWeight: 800 }}>Kvasir Wallet</div>
          <div className="muted" style={{ marginTop: 4 }}>{t('ob.subtitle')}</div>
        </div>
        {mode === 'welcome' && (
          <Card>
            <button className="btn block" onClick={() => setMode('create')}>{t('ob.create')}</button>
            <div style={{ height: 12 }} />
            <button className="btn secondary block" onClick={() => setMode('import')}>{t('ob.import')}</button>
          </Card>
        )}
        {mode === 'create' && <Create onDone={reload} onBack={() => setMode('welcome')} />}
        {mode === 'import' && <Import onDone={reload} onBack={() => setMode('welcome')} />}
      </div>
    </div>
  )
}

function Create({ onDone, onBack }: { onDone: () => void; onBack: () => void }) {
  const { t } = useI18n()
  const [words] = useState<Promise<string[]>>(() => api.wallet.preview(12).then((r) => r.mnemonic.split(' ')))
  const [list, setList] = useState<string[]>([])
  useState(() => { words.then(setList) })
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)
  const [pass, setPass] = useState('')
  const [confirm, setConfirm] = useState('')
  const [err, setErr] = useState<string | null>(null)

  const start = async () => {
    const ve = passphraseError(pass, confirm, t); if (ve) { setErr(ve); return }
    setBusy(true); setErr(null)
    try { await api.wallet.commit(list.join(' '), pass); onDone() }
    catch (e: any) { setErr(String(e?.message ?? e)); setBusy(false) }
  }

  return (
    <Card>
      <div className="spread"><div style={{ fontWeight: 700, fontSize: 17 }}>{t('ob.createTitle')}</div>
        <button className="btn ghost" onClick={onBack}>←</button></div>
      <div className="muted small" style={{ margin: '6px 0 14px' }}>{t('ob.createHint')}</div>
      <div className="grid cols-3" style={{ gap: 10 }}>
        {list.map((w, i) => (
          <div key={i} className="row" style={{ background: 'var(--surface-2)', borderRadius: 10, padding: '8px 10px' }}>
            <span className="grad-text" style={{ fontWeight: 700, fontSize: 12, width: 18 }}>{i + 1}</span>
            <span style={{ fontWeight: 600 }}>{w}</span>
          </div>
        ))}
      </div>
      <label className="row" style={{ margin: '16px 0', gap: 8 }}>
        <input type="checkbox" style={{ width: 18 }} checked={saved} onChange={(e) => setSaved(e.target.checked)} />
        <span>{t('ob.saved')}</span>
      </label>
      {saved && (
        <div style={{ marginBottom: 14 }}>
          <div className="label" style={{ marginBottom: 6 }}>{t('lock.createPassTitle')}</div>
          <div className="muted small" style={{ marginBottom: 10 }}>{t('lock.createPassHint')}</div>
          <PassphraseFields pass={pass} confirm={confirm} setPass={setPass} setConfirm={setConfirm} onEnter={start} />
        </div>
      )}
      {err && <div className="small" style={{ color: 'var(--danger)', marginBottom: 10 }}>{err}</div>}
      <button className="btn block" disabled={!saved || busy || !list.length} onClick={start}>{t('ob.start')}</button>
    </Card>
  )
}

function Import({ onDone, onBack }: { onDone: () => void; onBack: () => void }) {
  const { t } = useI18n()
  const [text, setText] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [pass, setPass] = useState('')
  const [confirm, setConfirm] = useState('')

  const restore = async () => {
    if (!text.trim()) return
    const ve = passphraseError(pass, confirm, t); if (ve) { setErr(ve); return }
    setBusy(true); setErr(null)
    try { await api.wallet.import(text, pass); onDone() }
    catch (e: any) { setErr(String(e?.message ?? e)); setBusy(false) }
  }

  return (
    <Card>
      <div className="spread"><div style={{ fontWeight: 700, fontSize: 17 }}>{t('ob.importTitle')}</div>
        <button className="btn ghost" onClick={onBack}>←</button></div>
      <div className="muted small" style={{ margin: '6px 0 14px' }}>
        {t('ob.importHint')}
        {isElectron && <> {t('ob.importRawHint')}</>}
      </div>
      <textarea className="mono" value={text} onChange={(e) => setText(e.target.value)} rows={4} />
      <div style={{ height: 14 }} />
      <div className="label" style={{ marginBottom: 6 }}>{t('lock.createPassTitle')}</div>
      <div className="muted small" style={{ marginBottom: 10 }}>{t('lock.createPassHint')}</div>
      <PassphraseFields pass={pass} confirm={confirm} setPass={setPass} setConfirm={setConfirm} onEnter={restore} />
      {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{err}</div>}
      <div style={{ height: 14 }} />
      <button className="btn block" disabled={busy || !text.trim()} onClick={restore}>{t('ob.restore')}</button>
    </Card>
  )
}
