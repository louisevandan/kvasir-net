import { ReactNode, useState } from 'react'
import { useWallet } from '../state'
import { Card } from '../components'
import { QR } from '../qr'
import { operatorEnroll, operatorConfirm, operatorDisable, EnrollInfo } from '../operatorAuth'

function Centered({ children }: { children: ReactNode }) {
  return (
    <div style={{ display: 'grid', placeItems: 'center', height: '100vh', padding: 24 }}>
      <div style={{ width: 420, maxWidth: '92vw' }}>{children}</div>
    </div>
  )
}

// Blocking screen shown after wallet unlock when an operator wallet has 2FA
// enrolled — the second factor of the login. Until it passes, the app is gated.
export function Operator2FAGate() {
  const { operatorSubmit2fa, operatorSignOut, address } = useWallet()
  const [code, setCode] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    if (!code || busy) return
    setBusy(true); setErr(null)
    try { await operatorSubmit2fa(code.trim()) } catch (e: any) { setErr(String(e?.message ?? e)); setBusy(false); setCode('') }
  }
  return (
    <Centered>
      <div style={{ textAlign: 'center', marginBottom: 20 }}>
        <div className="brand-mark" style={{ width: 56, height: 56, margin: '0 auto 12px', fontSize: 24 }}>🔐</div>
        <div style={{ fontSize: 22, fontWeight: 800 }}>Operator verification</div>
        <div className="muted small" style={{ marginTop: 6 }}>Two-factor authentication required</div>
      </div>
      <Card>
        <div className="muted small" style={{ marginBottom: 12 }}>Enter the 6-digit code from your authenticator app (or a backup code).</div>
        <input inputMode="numeric" autoComplete="one-time-code" autoFocus placeholder="123456" value={code}
          onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') submit() }} />
        {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{err}</div>}
        <div style={{ height: 14 }} />
        <button className="btn block" disabled={!code || busy} onClick={submit}>{busy ? '…' : 'Verify'}</button>
        <div style={{ marginTop: 14, textAlign: 'center' }}>
          <button className="btn ghost small" onClick={() => operatorSignOut()}>Cancel</button>
        </div>
      </Card>
    </Centered>
  )
}

// Operator 2FA management, shown in Settings for an operator (admin) wallet.
export function OperatorPanel() {
  const { operator, stakingUrl, operatorRefresh, operatorSignOut } = useWallet()
  const [enroll, setEnroll] = useState<EnrollInfo | null>(null)
  const [code, setCode] = useState('')
  const [disableCode, setDisableCode] = useState('')
  const [msg, setMsg] = useState<{ t: string; ok?: boolean } | null>(null)
  const [busy, setBusy] = useState(false)
  const url = () => stakingUrl

  if (!operator.enabled) return null // gateway has no admin allowlist
  if (operator.phase === 'not-admin') return null // ordinary wallet — nothing to manage

  const startEnroll = async () => {
    setBusy(true); setMsg(null)
    try { setEnroll(await operatorEnroll(url())) } catch (e: any) { setMsg({ t: String(e?.message ?? e) }) } finally { setBusy(false) }
  }
  const confirm = async () => {
    setBusy(true); setMsg(null)
    try { await operatorConfirm(url(), code.trim()); setEnroll(null); setCode(''); setMsg({ t: 'Two-factor auth enabled.', ok: true }); await operatorRefresh() }
    catch (e: any) { setMsg({ t: String(e?.message ?? e) }) } finally { setBusy(false) }
  }
  const disable = async () => {
    setBusy(true); setMsg(null)
    try { await operatorDisable(url(), disableCode.trim()); setDisableCode(''); setMsg({ t: 'Two-factor auth disabled.', ok: true }); await operatorRefresh() }
    catch (e: any) { setMsg({ t: String(e?.message ?? e) }) } finally { setBusy(false) }
  }

  return (
    <Card>
      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <div style={{ fontWeight: 700 }}>Operator security</div>
        <span className="chip">{operator.twofa ? '2FA on' : '2FA off'}</span>
      </div>
      <div className="muted small" style={{ margin: '6px 0 12px' }}>
        This wallet operates the gateway. Protect operator sign-in with an authenticator app.
      </div>

      {!operator.twofa && !enroll && (
        <button className="btn" disabled={busy} onClick={startEnroll}>Enable 2FA (authenticator app)</button>
      )}

      {enroll && (
        <div className="grid" style={{ gap: 10 }}>
          <div className="muted small">Scan with Google Authenticator, or enter the setup key manually.</div>
          <div style={{ display: 'flex', justifyContent: 'center' }}><QR text={enroll.otpauth_uri} size={180} /></div>
          <div><div className="muted small">Setup key</div><div className="mono small" style={{ wordBreak: 'break-all' }}>{enroll.secret}</div></div>
          <div>
            <div className="muted small">Backup codes — save these; each works once:</div>
            <pre style={{ background: 'var(--panel-2, #0e0f12)', borderRadius: 8, padding: 10, fontSize: 12, whiteSpace: 'pre', overflowX: 'auto', margin: '4px 0 0' }}>{enroll.backup_codes.join('\n')}</pre>
          </div>
          <input inputMode="numeric" autoComplete="one-time-code" placeholder="6-digit code" value={code}
            onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') confirm() }} />
          <button className="btn" disabled={busy || !code} onClick={confirm}>Confirm & enable</button>
        </div>
      )}

      {operator.twofa && (
        <div className="grid" style={{ gap: 10 }}>
          <input inputMode="numeric" autoComplete="one-time-code" placeholder="authenticator or backup code" value={disableCode}
            onChange={(e) => setDisableCode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') disable() }} />
          <button className="btn ghost" style={{ color: 'var(--danger)' }} disabled={busy || !disableCode} onClick={disable}>Disable 2FA</button>
        </div>
      )}

      <div style={{ marginTop: 12 }}>
        <button className="btn ghost small" onClick={() => operatorSignOut()}>Sign out operator session</button>
      </div>
      {msg && <div className="small" style={{ marginTop: 8, color: msg.ok ? 'var(--good)' : 'var(--danger)' }}>{msg.t}</div>}
    </Card>
  )
}
