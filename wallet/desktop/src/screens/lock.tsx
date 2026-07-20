import { ReactNode, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { Card } from '../components'
import { shorten } from '../api'

// Shared validation for a new passphrase (create / import / upgrade / change).
export function passphraseError(pass: string, confirm: string, t: (k: string) => string): string | null {
  if (pass.length < 8) return t('lock.tooShort')
  if (pass !== confirm) return t('lock.mismatch')
  return null
}

export function PassphraseFields({ pass, confirm, setPass, setConfirm, onEnter, autoFocus }: {
  pass: string; confirm: string; setPass: (s: string) => void; setConfirm: (s: string) => void
  onEnter?: () => void; autoFocus?: boolean
}) {
  const { t } = useI18n()
  const enter = (e: React.KeyboardEvent) => { if (e.key === 'Enter') onEnter?.() }
  return (
    <div className="grid" style={{ gap: 10 }}>
      <input type="password" placeholder={t('lock.passphrase')} value={pass} autoFocus={autoFocus}
        onChange={(e) => setPass(e.target.value)} onKeyDown={enter} />
      <input type="password" placeholder={t('lock.confirm')} value={confirm}
        onChange={(e) => setConfirm(e.target.value)} onKeyDown={enter} />
    </div>
  )
}

function Centered({ children }: { children: ReactNode }) {
  return (
    <div style={{ display: 'grid', placeItems: 'center', height: '100vh', padding: 24 }}>
      <div style={{ width: 420, maxWidth: '92vw' }}>{children}</div>
    </div>
  )
}

export function UnlockScreen() {
  const { t } = useI18n()
  const { unlock, address, logout } = useWallet()
  const [pass, setPass] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirmReset, setConfirmReset] = useState(false)
  const submit = async () => {
    if (!pass || busy) return
    setBusy(true); setErr(null)
    try { await unlock(pass) } catch { setErr(t('lock.wrong')); setBusy(false); setPass('') }
  }
  return (
    <Centered>
      <div style={{ textAlign: 'center', marginBottom: 20 }}>
        <div className="brand-mark" style={{ width: 56, height: 56, margin: '0 auto 12px', fontSize: 24 }}>🔒</div>
        <div style={{ fontSize: 22, fontWeight: 800 }}>{t('lock.title')}</div>
        {address && <div className="muted small mono" style={{ marginTop: 6 }}>{shorten(address)}</div>}
      </div>
      <Card>
        <div className="muted small" style={{ marginBottom: 12 }}>{t('lock.prompt')}</div>
        <input type="password" autoFocus placeholder={t('lock.passphrase')} value={pass}
          onChange={(e) => setPass(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') submit() }} />
        {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{err}</div>}
        <div style={{ height: 14 }} />
        <button className="btn block" disabled={!pass || busy} onClick={submit}>{busy ? t('common.busy') : t('lock.unlock')}</button>
        {/* Escape hatch: a locked-out user can reset and re-import with their recovery phrase. */}
        <div style={{ marginTop: 16, textAlign: 'center' }}>
          {!confirmReset ? (
            <button className="btn ghost small" onClick={() => setConfirmReset(true)}>{t('lock.forgot')}</button>
          ) : (
            <>
              <div className="small" style={{ color: 'var(--warn)', marginBottom: 10 }}>{t('lock.resetConfirm')}</div>
              <div className="row" style={{ gap: 8, justifyContent: 'center' }}>
                <button className="btn ghost" style={{ color: 'var(--danger)' }} onClick={() => logout()}>{t('lock.reset')}</button>
                <button className="btn ghost" onClick={() => setConfirmReset(false)}>{t('common.cancel')}</button>
              </div>
            </>
          )}
        </div>
      </Card>
    </Centered>
  )
}

export function UpgradeScreen() {
  const { t } = useI18n()
  const { upgrade } = useWallet()
  const [pass, setPass] = useState('')
  const [confirm, setConfirm] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    const ve = passphraseError(pass, confirm, t); if (ve) { setErr(ve); return }
    setBusy(true); setErr(null)
    try { await upgrade(pass) } catch (e: any) { setErr(String(e?.message ?? e)); setBusy(false) }
  }
  return (
    <Centered>
      <div style={{ textAlign: 'center', marginBottom: 20 }}>
        <div className="brand-mark" style={{ width: 56, height: 56, margin: '0 auto 12px', fontSize: 24 }}>🛡</div>
        <div style={{ fontSize: 22, fontWeight: 800 }}>{t('lock.upgradeTitle')}</div>
      </div>
      <Card>
        <div className="muted small" style={{ marginBottom: 14 }}>{t('lock.upgradeHint')}</div>
        <PassphraseFields pass={pass} confirm={confirm} setPass={setPass} setConfirm={setConfirm} onEnter={submit} autoFocus />
        {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{err}</div>}
        <div style={{ height: 14 }} />
        <button className="btn block" disabled={busy} onClick={submit}>{busy ? t('common.busy') : t('lock.setPass')}</button>
      </Card>
    </Centered>
  )
}
