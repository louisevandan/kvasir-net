// Model pricing governance (desktop-only). Only shown when THIS machine hosts
// the gateway: pricing writes must reach it directly — the server rejects any
// request that came through the public reverse proxy (requireLocalDesktop), so
// this screen talks to http://127.0.0.1:<port>, never the public domain.
//
// Auth is the operator SIWS + 2FA flow (operatorAuth) against that local URL,
// and every save additionally carries a fresh TOTP code (requireFreshTotp).
// Reads are allowed for any admin; writes only for the genesis wallet.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { api, fmt, isElectron, shorten } from '../api'
import { Card } from '../components'
import { QR } from '../qr'
import { Gateway, ModelPricing, PayModel, PricingAudit, PricingRelay, PricingView } from '../services'
import { EnrollInfo, OperatorState, operator2faLogin, operatorConfirm, operatorEnroll, operatorSignIn } from '../operatorAuth'
import { PricingSim } from './pricingSim'

// Reference rate for the misentry-guard preview only — display math, not billing.
const USD_PER_KVR = 0.10
const SHORT_CHAT_TOKENS = 257

type Draft = { basePrice: string; perToken: string; estOut: string }
const toDraft = (p: ModelPricing): Draft => ({ basePrice: String(p.basePrice), perToken: String(p.perToken), estOut: String(p.estOut) })
const parseDraft = (d: Draft): ModelPricing | null => {
  const n = (s: string) => { const v = Number(s); return s.trim() !== '' && Number.isFinite(v) && v >= 0 ? v : null }
  const basePrice = n(d.basePrice), perToken = n(d.perToken), estOut = n(d.estOut)
  return basePrice === null || perToken === null || estOut === null ? null : { basePrice, perToken, estOut }
}
const samePricing = (a: ModelPricing, b: ModelPricing) =>
  a.basePrice === b.basePrice && a.perToken === b.perToken && a.estOut === b.estOut

const usd = (v: number) => `$${Number(v.toFixed(6)).toLocaleString(undefined, { maximumFractionDigits: 6 })}`

function Preview({ p, symbol }: { p: ModelPricing | null; symbol: string }) {
  const { t } = useI18n()
  if (!p) return <div className="small" style={{ color: 'var(--danger)' }}>—</div>
  const chatKvr = p.basePrice + SHORT_CHAT_TOKENS * p.perToken
  return (
    <div className="small muted" style={{ display: 'flex', gap: 14, flexWrap: 'wrap' }}>
      <span>$/1M tok <b style={{ color: 'var(--text)' }}>{usd(p.perToken * 1e6 * USD_PER_KVR)}</b></span>
      <span>{t('pricing.perReq')} <b style={{ color: 'var(--text)' }}>{usd(p.basePrice * USD_PER_KVR)}</b></span>
      <span>{t('pricing.shortChat')} <b style={{ color: 'var(--text)' }}>{fmt(chatKvr)} {symbol} ≈ {usd(chatKvr * USD_PER_KVR)}</b></span>
    </div>
  )
}

// Inline genesis 2FA enrollment — pricing writes require a fresh TOTP, so a
// genesis wallet without 2FA must enroll before it can save anything.
function EnrollBlock({ url, onDone }: { url: string; onDone: () => void }) {
  const { t } = useI18n()
  const [enroll, setEnroll] = useState<EnrollInfo | null>(null)
  const [code, setCode] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const start = async () => {
    setBusy(true); setErr(null)
    try { setEnroll(await operatorEnroll(url)) } catch (e: any) { setErr(String(e?.message ?? e)) } finally { setBusy(false) }
  }
  const confirm = async () => {
    setBusy(true); setErr(null)
    try { await operatorConfirm(url, code.trim()); onDone() } catch (e: any) { setErr(String(e?.message ?? e)) } finally { setBusy(false) }
  }
  return (
    <Card>
      <div className="small" style={{ color: 'var(--warn)', marginBottom: 10 }}>{t('pricing.enroll2fa')}</div>
      {!enroll ? (
        <button className="btn" disabled={busy} onClick={start}>{t('pricing.enrollStart')}</button>
      ) : (
        <div className="grid" style={{ gap: 10 }}>
          <div style={{ display: 'flex', justifyContent: 'center' }}><QR text={enroll.otpauth_uri} size={160} /></div>
          <div className="mono small" style={{ wordBreak: 'break-all' }}>{enroll.secret}</div>
          <pre style={{ background: 'var(--surface-2)', borderRadius: 8, padding: 10, fontSize: 12, margin: 0, overflowX: 'auto' }}>{enroll.backup_codes.join('\n')}</pre>
          <input inputMode="numeric" autoComplete="one-time-code" placeholder="123456" value={code}
            onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') confirm() }} />
          <button className="btn" disabled={busy || !code} onClick={confirm}>{t('common.confirm')}</button>
        </div>
      )}
      {err && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{err}</div>}
    </Card>
  )
}

// Propagation relays (source gateway only): the public gateways this genesis
// source pushes pricing to. Add/remove are genesis writes — fresh TOTP each,
// like pricing saves. Shared secrets are write-only (hasSecret badge only).
function RelaysCard({ gw, relays, editable, onChanged }: {
  gw: Gateway; relays: PricingRelay[]; editable: boolean; onChanged: () => Promise<void>
}) {
  const { t } = useI18n()
  const [url, setUrl] = useState('')
  const [label, setLabel] = useState('')
  const [secret, setSecret] = useState('')
  const [totp, setTotp] = useState('')
  const [pending, setPending] = useState<null | { kind: 'add' } | { kind: 'remove'; url: string }>(null)
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<{ ok?: boolean; text: string } | null>(null)

  const submit = async () => {
    if (!pending || !totp) return
    setBusy(true); setMsg(null)
    try {
      if (pending.kind === 'add') {
        await gw.addRelay({ url: url.trim(), ...(secret ? { secret } : {}), ...(label.trim() ? { label: label.trim() } : {}), totp: totp.trim() })
        setUrl(''); setLabel(''); setSecret('')
      } else {
        await gw.removeRelay(pending.url, totp.trim())
      }
      setPending(null)
      await onChanged()
      setMsg({ ok: true, text: t('pricing.saved') })
    } catch (e: any) { setMsg({ text: String(e?.message ?? e) }) }
    finally { setBusy(false); setTotp('') }
  }

  const validUrl = /^https?:\/\/.+/i.test(url.trim())

  return (
    <Card>
      <div className="label" style={{ marginBottom: 6 }}>{t('relays.title')}</div>
      <div className="small muted" style={{ marginBottom: 12 }}>{t('relays.desc')}</div>
      {relays.length === 0 && <div className="small muted" style={{ marginBottom: 8 }}>{t('relays.empty')}</div>}
      {relays.map((r) => (
        <div key={r.url} className="spread" style={{ padding: '6px 0', borderBottom: '1px solid var(--stroke)' }}>
          <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
            <span className="mono small" style={{ fontWeight: 600 }}>{r.url}</span>
            {r.label && <span className="small muted">{r.label}</span>}
            <span className="pill" style={{
              color: r.hasSecret ? 'var(--good)' : 'var(--warn)',
              background: `color-mix(in srgb, ${r.hasSecret ? 'var(--good)' : 'var(--warn)'} 16%, transparent)`,
            }}>
              {r.hasSecret ? '🔑 ' + t('relays.hasSecret') : '△ ' + t('relays.noSecret')}
            </span>
          </div>
          {editable && (
            <button className="btn ghost small" style={{ color: 'var(--danger)' }} disabled={busy}
              onClick={() => { setPending({ kind: 'remove', url: r.url }); setTotp(''); setMsg(null) }}>
              {t('node.remove')}
            </button>
          )}
        </div>
      ))}

      {editable && (
        <div className="row" style={{ gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
          <input className="mono" style={{ flex: 2, minWidth: 220 }} placeholder={t('relays.url')}
            value={url} onChange={(e) => { setUrl(e.target.value); if (pending?.kind === 'add') setPending(null) }} />
          <input style={{ flex: 1, minWidth: 120 }} placeholder={t('relays.label')} value={label} onChange={(e) => setLabel(e.target.value)} />
          <input className="mono" type="password" autoComplete="off" style={{ flex: 1, minWidth: 140 }} placeholder={t('relays.secret')}
            value={secret} onChange={(e) => setSecret(e.target.value)} />
          <button className="btn secondary" disabled={busy || !validUrl}
            onClick={() => { setPending({ kind: 'add' }); setTotp(''); setMsg(null) }}>{t('relays.add')}</button>
        </div>
      )}

      {pending && (
        <div style={{ marginTop: 12, borderTop: '1px solid var(--stroke)', paddingTop: 12 }}>
          <div className="small mono" style={{ marginBottom: 8 }}>
            {pending.kind === 'add' ? `+ ${url.trim()}` : `− ${pending.url}`}
          </div>
          <div className="small muted" style={{ marginBottom: 6 }}>{t('pricing.totpHint')}</div>
          <div className="row" style={{ gap: 8 }}>
            <input className="mono" style={{ width: 120 }} inputMode="numeric" autoComplete="one-time-code" autoFocus placeholder="123456"
              value={totp} onChange={(e) => setTotp(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') submit() }} />
            <button className="btn" disabled={busy || !totp} onClick={submit}>{busy ? '…' : t('common.confirm')}</button>
            <button className="btn ghost" disabled={busy} onClick={() => { setPending(null); setTotp('') }}>{t('common.cancel')}</button>
          </div>
        </div>
      )}
      {msg && <div className="small" style={{ marginTop: 8, color: msg.ok ? 'var(--good)' : 'var(--danger)' }}>{msg.text}</div>}
    </Card>
  )
}

export function PricingScreen() {
  const { t } = useI18n()
  const { address } = useWallet()
  const [gwUrl, setGwUrl] = useState<string | null>(null)
  const [hosted, setHosted] = useState<boolean | null>(null) // null while probing
  const [op, setOp] = useState<OperatorState | null>(null)
  const [loginCode, setLoginCode] = useState('')
  const [view, setView] = useState<PricingView | null>(null)
  const [models, setModels] = useState<PayModel[]>([])
  const [audit, setAudit] = useState<PricingAudit[]>([])
  const [relays, setRelays] = useState<PricingRelay[]>([])
  const [showAudit, setShowAudit] = useState(false)
  const [drafts, setDrafts] = useState<Record<string, Draft>>({})
  const [confirmKey, setConfirmKey] = useState<string | null>(null)
  const [totp, setTotp] = useState('')
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<{ ok?: boolean; text: string } | null>(null)

  const gw = useMemo(() => (gwUrl ? new Gateway(gwUrl) : null), [gwUrl])

  const load = useCallback(async (g: Gateway) => {
    const [pricing, m, a, r] = await Promise.all([
      g.getPricing(),
      g.models().catch(() => ({ recipient: '', symbol: '', models: [] as PayModel[] })),
      g.pricingAudit().catch(() => ({ audit: [] as PricingAudit[] })),
      g.getRelays().catch(() => ({ isSource: false, relays: [] as PricingRelay[] })),
    ])
    setView(pricing)
    setModels(m.models)
    setAudit(a.audit)
    setRelays(r.relays)
    const d: Record<string, Draft> = { default: toDraft(pricing.default) }
    for (const pm of m.models) d[pm.id] = toDraft(pricing.perModel[pm.id] ?? pricing.default)
    for (const id of Object.keys(pricing.perModel)) d[id] = toDraft(pricing.perModel[id])
    setDrafts(d)
    setConfirmKey(null); setTotp('')
  }, [])

  const signIn = useCallback(async (url: string) => {
    setMsg(null)
    try {
      const st = await operatorSignIn(url, address || '')
      // Load BEFORE exposing the new session state, so the screen transitions
      // atomically — no window where a pending reload silently resets drafts
      // under an edit already in progress.
      if (st.phase === 'signed-in') await load(new Gateway(url))
      setOp(st)
    } catch (e: any) { setMsg({ text: String(e?.message ?? e) }) }
  }, [address, load])

  useEffect(() => {
    let alive = true
    api.gateway.status()
      .then((s) => {
        if (!alive) return
        setHosted(!!s.running)
        if (s.running) { const url = `http://127.0.0.1:${s.port}`; setGwUrl(url); signIn(url) }
      })
      .catch(() => { if (alive) setHosted(false) })
    return () => { alive = false }
  }, [signIn])

  const submit2fa = async () => {
    if (!gwUrl || !op?.preAuth || !loginCode) return
    setBusy(true); setMsg(null)
    try { await operator2faLogin(gwUrl, op.preAuth, loginCode.trim()); setLoginCode(''); await signIn(gwUrl) }
    catch (e: any) { setMsg({ text: String(e?.message ?? e) }); setLoginCode('') }
    finally { setBusy(false) }
  }

  const save = async (key: string) => {
    if (!gw || !view) return
    const next = parseDraft(drafts[key])
    if (!next || !totp) return
    setBusy(true); setMsg(null)
    try {
      await gw.setPricing({ ...(key === 'default' ? {} : { modelId: key }), ...next, totp: totp.trim() })
      await load(gw) // reload first — "saved" only shows once the fresh values are on screen
      setMsg({ ok: true, text: t('pricing.saved') })
    } catch (e: any) { setMsg({ text: String(e?.message ?? e) }) }
    finally { setBusy(false); setTotp('') }
  }

  if (!isElectron) return <Card><div className="muted">{t('models.desktopOnly')}</div></Card>
  if (hosted === null) return <div className="muted">{t('common.busy')}</div>
  if (!hosted) return <Card><div className="muted">{t('pricing.notHosting')}</div></Card>

  // ---- operator session states ----
  if (!op) return <div className="muted">{msg ? msg.text : t('common.busy')}</div>
  if (op.phase === 'disabled') return <Card><div className="muted">{t('pricing.authOff')}</div></Card>
  if (op.phase === 'not-admin') return <Card><div className="muted">{t('pricing.notAdmin')}</div></Card>
  if (op.phase === 'need-2fa') {
    return (
      <Card style={{ maxWidth: 420 }}>
        <div className="label" style={{ marginBottom: 8 }}>{t('pricing.signin')}</div>
        <div className="small muted" style={{ marginBottom: 12 }}>{t('pricing.totp')}</div>
        <input inputMode="numeric" autoComplete="one-time-code" autoFocus placeholder="123456" value={loginCode}
          onChange={(e) => setLoginCode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') submit2fa() }} />
        {msg && <div className="small" style={{ color: 'var(--danger)', marginTop: 8 }}>{msg.text}</div>}
        <div style={{ height: 12 }} />
        <button className="btn block" disabled={busy || !loginCode} onClick={submit2fa}>{busy ? '…' : t('common.confirm')}</button>
      </Card>
    )
  }
  if (!view) return <div className="muted">{t('common.busy')}</div>

  const editable = view.isGenesis && !view.writeLocked
  const rowKeys = ['default', ...models.map((m) => m.id).filter((id) => id !== 'default'),
    ...Object.keys(view.perModel).filter((id) => id !== 'default' && !models.some((m) => m.id === id))]
  const currentOf = (key: string): ModelPricing => (key === 'default' ? view.default : view.perModel[key] ?? view.default)

  const numField = (key: string, field: keyof Draft, label: string) => (
    <label style={{ flex: 1, minWidth: 130 }}>
      <div className="small muted" style={{ marginBottom: 4 }}>{label}</div>
      <input className="mono" inputMode="decimal" disabled={!editable} value={drafts[key]?.[field] ?? ''}
        onChange={(e) => { setDrafts((d) => ({ ...d, [key]: { ...(d[key] ?? toDraft(currentOf(key))), [field]: e.target.value } })); setConfirmKey(null) }} />
    </label>
  )

  return (
    <div className="grid" style={{ alignItems: 'start' }}>
      <Card>
        <div className="spread">
          <div>
            <div className="label">{t('pricing.subtitle')}</div>
            <div className="small muted mono" style={{ marginTop: 4 }}>{gwUrl}</div>
          </div>
          <div style={{ textAlign: 'right' }}>
            <span className="chip">{view.symbol} · genesis {shorten(view.genesis, 4, 4)}</span>
            {view.updatedAt > 0 && (
              <div className="small muted" style={{ marginTop: 6 }}>
                {t('pricing.updated')}: {new Date(view.updatedAt * 1000).toLocaleString()}{view.updatedBy ? ` · ${shorten(view.updatedBy, 4, 4)}` : ''}
              </div>
            )}
          </div>
        </div>
        {view.writeLocked ? (
          <div className="small" style={{ color: 'var(--warn)', marginTop: 10 }}>{t('pricing.writeLocked')}</div>
        ) : !editable && <div className="small" style={{ color: 'var(--warn)', marginTop: 10 }}>{t('pricing.genesisOnly')}</div>}
        <div className="small muted" style={{ marginTop: 10 }}>
          {t('pricing.usdRef')}: 1 {view.symbol} = {usd(USD_PER_KVR)}
        </div>
      </Card>

      {editable && !op.twofa && gwUrl && <EnrollBlock url={gwUrl} onDone={() => signIn(gwUrl)} />}

      {rowKeys.map((key) => {
        const cur = currentOf(key)
        const draft = drafts[key] ?? toDraft(cur)
        const next = parseDraft(draft)
        const changed = next ? !samePricing(next, cur) : false
        const inherited = key !== 'default' && !view.perModel[key]
        return (
          <Card key={key}>
            <div className="spread" style={{ marginBottom: 10 }}>
              <div className="row" style={{ gap: 8 }}>
                <span style={{ fontWeight: 700 }}>{key === 'default' ? t('pricing.default') : key}</span>
                {inherited && <span className="pill" style={{ color: 'var(--info)', background: 'color-mix(in srgb, var(--info) 16%, transparent)' }}>{t('pricing.inherit')}</span>}
              </div>
              {editable && (
                <button className="btn secondary" disabled={busy || !next || !changed} onClick={() => { setConfirmKey(key); setTotp(''); setMsg(null) }}>
                  {t('pricing.save')}
                </button>
              )}
            </div>
            <div className="row" style={{ gap: 12, flexWrap: 'wrap', alignItems: 'flex-end' }}>
              {numField(key, 'basePrice', `${t('pricing.basePrice')} (${view.symbol})`)}
              {numField(key, 'perToken', `${t('pricing.perToken')} (${view.symbol})`)}
              {numField(key, 'estOut', t('pricing.estOut'))}
            </div>
            <div style={{ marginTop: 10 }}><Preview p={next} symbol={view.symbol} /></div>

            {confirmKey === key && next && (
              <div style={{ marginTop: 12, borderTop: '1px solid var(--stroke)', paddingTop: 12 }}>
                <div className="label" style={{ marginBottom: 8 }}>{t('pricing.confirm')}</div>
                <div className="small mono" style={{ marginBottom: 10 }}>
                  {(['basePrice', 'perToken', 'estOut'] as const).filter((f) => cur[f] !== next[f]).map((f) => (
                    <div key={f}>{f}: {String(cur[f])} → <b>{String(next[f])}</b></div>
                  ))}
                </div>
                <div className="small muted" style={{ marginBottom: 6 }}>{t('pricing.totpHint')}</div>
                <div className="row" style={{ gap: 8 }}>
                  <input className="mono" style={{ width: 120 }} inputMode="numeric" autoComplete="one-time-code" autoFocus
                    placeholder="123456" value={totp} onChange={(e) => setTotp(e.target.value)}
                    onKeyDown={(e) => { if (e.key === 'Enter') save(key) }} />
                  <button className="btn" disabled={busy || !totp} onClick={() => save(key)}>{busy ? '…' : t('common.confirm')}</button>
                  <button className="btn ghost" disabled={busy} onClick={() => { setConfirmKey(null); setTotp('') }}>{t('common.cancel')}</button>
                </div>
              </div>
            )}
          </Card>
        )
      })}
      {msg && <div className="small" style={{ color: msg.ok ? 'var(--good)' : 'var(--danger)' }}>{msg.text}</div>}

      {/* Relay management — only meaningful on the SOURCE gateway (the machine
          the genesis wallet drives locally); relays/followers never show it. */}
      {gw && view.isSource === true && (
        <RelaysCard gw={gw} relays={relays} editable={editable} onChanged={() => load(gw)} />
      )}

      {/* Economy simulator follows the default-row DRAFT live, so the genesis
          can see the network-wide effect of a rate before committing it. */}
      {gwUrl && (
        <PricingSim
          pricing={parseDraft(drafts.default ?? toDraft(view.default)) ?? view.default}
          symbol={view.symbol}
          gwUrl={gwUrl}
        />
      )}

      <Card>
        <div className="spread">
          <div className="label">{t('pricing.audit')}</div>
          <button className="btn ghost small" onClick={() => setShowAudit((v) => !v)}>{showAudit ? t('set.hide') : `${audit.length}`}</button>
        </div>
        {showAudit && (
          <div style={{ marginTop: 10 }}>
            {audit.length === 0 && <div className="small muted">—</div>}
            {audit.map((a, i) => (
              <div key={i} className="small" style={{ padding: '6px 0', borderTop: i ? '1px solid var(--stroke)' : 'none' }}>
                <span className="muted">{new Date(a.at * 1000).toLocaleString()}</span>
                {' · '}<span className="mono">{a.target}</span>
                {' · '}<span className="mono">{fmt(a.prev.basePrice)}/{fmt(a.prev.perToken)}/{a.prev.estOut}</span>
                {' → '}<b className="mono">{fmt(a.next.basePrice)}/{fmt(a.next.perToken)}/{a.next.estOut}</b>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  )
}
