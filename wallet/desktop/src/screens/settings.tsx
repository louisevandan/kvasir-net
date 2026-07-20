import { useState, useEffect } from 'react'
import { useWallet } from '../state'
import { useI18n, LANGS, Lang } from '../i18n'
import { api, lockCapable, fmt, Network, GatewayStatus } from '../api'
import { Card, Field, CopyButton } from '../components'
import { passphraseError } from './lock'
import { OperatorPanel } from './operator'

// A gateway-start failure caused by macOS blocking the app from the folder/volume
// (e.g. a repo on an external drive) — offer to open Full Disk Access settings.
const isPermissionError = (e: string) => /EPERM|EACCES|operation not permitted|must be readable|uv_cwd|not permitted|Operation not permitted/i.test(e || '')

function GatewayCard() {
  const { t } = useI18n()
  const [gw, setGw] = useState<GatewayStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const refresh = () => api.gateway.status().then(setGw).catch(() => {})
  useEffect(() => { refresh(); const id = setInterval(refresh, 3000); return () => clearInterval(id) }, [])
  const running = !!gw?.running
  const toggle = async () => {
    setBusy(true)
    try { setGw(running ? await api.gateway.stop() : await api.gateway.start()) } catch (e: any) { setGw((g) => g && { ...g, lastError: String(e?.message ?? e) }) } finally { setBusy(false) }
  }
  const pickKey = async () => { setBusy(true); try { setGw(await api.gateway.pickKey()) } catch { /* ignore */ } finally { setBusy(false) } }
  const clearKey = async () => { setBusy(true); try { setGw(await api.gateway.setKeyFile('')) } catch { /* ignore */ } finally { setBusy(false) } }
  return (
    <Card>
      <div className="spread" style={{ marginBottom: 8 }}>
        <div className="label">{t('set.gateway')}</div>
        <span className="row" style={{ gap: 6 }}>
          <span className="badge-dot" style={{ background: running ? 'var(--good)' : 'var(--text-3)' }} />
          <span className="small">{running ? t('set.gatewayOn') : t('set.gatewayOff')}</span>
        </span>
      </div>
      <div className="small muted" style={{ marginBottom: 10 }}>{t('set.gatewayDesc')}</div>
      <button className={`btn ${running ? 'ghost' : 'primary'} block`} disabled={busy} onClick={toggle}>
        {busy ? '…' : running ? t('set.gatewayStop') : t('set.gatewayStart')}
      </button>

      {/* Setup: the gateway CODE is bundled in the app; the operator only points at
          their settlement key FILE. The key is never bundled or copied — the main
          process reads it at start and passes it inline. Shown under Start. */}
      {!running && (
        <div style={{ marginTop: 12, borderTop: '1px solid var(--stroke)', paddingTop: 12 }}>
          <div className="small" style={{ fontWeight: 600, marginBottom: 4 }}>{t('set.gatewaySetup')}</div>
          <div className="small muted" style={{ marginBottom: 8 }}>{t('set.gatewaySetupDesc')}</div>
          <div className="row" style={{ gap: 8, alignItems: 'center' }}>
            <span className="mono" style={{ flex: 1, wordBreak: 'break-all', color: gw?.keyExists ? 'var(--text)' : 'var(--text-3)' }}>
              {gw?.keyPath || t('set.gatewayKeyNone')}
            </span>
            <button className="btn ghost" disabled={busy} onClick={pickKey}>{t('set.gatewayKeyPick')}</button>
          </div>
          {gw?.keyPath && (
            <div className="row" style={{ gap: 10, marginTop: 6, alignItems: 'center' }}>
              {gw.keyExists
                ? <span className="small" style={{ color: 'var(--good)' }}>✓ {t('set.gatewayKeyFound')}</span>
                : <span className="small" style={{ color: 'var(--warn)' }}>⚠ {t('set.gatewayKeyMissing')}</span>}
              {gw.keyExists && <button className="btn ghost" onClick={() => api.revealPath(gw.keyPath!)}>{t('set.gatewayKeyReveal')}</button>}
              <button className="btn ghost" onClick={clearKey}>{t('set.gatewayKeyClear')}</button>
            </div>
          )}
          <div className="small muted" style={{ marginTop: 6 }}>{t('set.gatewayKeyWarn')}</div>
          {gw?.lastError && (
            <div style={{ marginTop: 8 }}>
              <div className="small" style={{ color: 'var(--danger)', whiteSpace: 'pre-wrap' }}>{gw.lastError}</div>
              {isPermissionError(gw.lastError) && (
                <>
                  <div className="small muted" style={{ marginTop: 6 }}>{t('set.gatewayPermHint')}</div>
                  <button className="btn ghost" style={{ marginTop: 6 }}
                    onClick={() => api.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles')}>
                    {t('set.gatewayOpenSettings')}
                  </button>
                </>
              )}
            </div>
          )}
        </div>
      )}
      {running && gw && (
        <div style={{ marginTop: 12 }}>
          <div className="small muted" style={{ marginBottom: 6 }}>{t('set.gatewayShare')}</div>
          <div className="row" style={{ gap: 8 }}>
            <input className="mono" readOnly value={gw.hostUrl} style={{ flex: 1 }} />
            <CopyButton text={gw.hostUrl} label={t('set.copyUrl')} />
          </div>
          <div className="small muted mono" style={{ marginTop: 6 }}>{gw.ipUrl}</div>
        </div>
      )}
      <div className="small" style={{ color: 'var(--warn)', marginTop: 10 }}>{t('set.gatewayBonus')}</div>
    </Card>
  )
}

// Genesis gateway (network coordination node) info for the active settlement URL.
function GenesisCard() {
  const { t } = useI18n()
  const { genesis, stakingUrl } = useWallet()
  const reachable = !!genesis
  const advertised = genesis?.publicUrl && genesis.publicUrl !== stakingUrl ? genesis.publicUrl : null
  const bonusPct = genesis?.gatewayBonus ? Math.round((genesis.gatewayBonus - 1) * 100) : null
  const fact = (label: string, value: string) => (
    <div className="spread" style={{ padding: '4px 0' }}>
      <span className="small muted">{label}</span>
      <span className="small mono" style={{ fontWeight: 600 }}>{value}</span>
    </div>
  )
  return (
    <Card>
      <div className="spread" style={{ marginBottom: 6 }}>
        <div className="label">{t('genesis.title')}</div>
        <span className="row" style={{ gap: 6 }}>
          <span className="badge-dot" style={{ background: reachable ? 'var(--good)' : 'var(--danger)' }} />
          <span className="small">{reachable ? t('genesis.reachable') : t('genesis.unreachable')}</span>
        </span>
      </div>
      <div className="small muted" style={{ marginBottom: 10 }}>{t('genesis.desc')}</div>
      <div className="small muted" style={{ marginBottom: 2 }}>{t('genesis.active')}</div>
      <div className="mono small" style={{ wordBreak: 'break-all', marginBottom: 8 }}>{stakingUrl || '—'}</div>
      {advertised && (
        <>
          <div className="small muted" style={{ marginBottom: 2 }}>{t('genesis.advertised')}</div>
          <div className="mono small" style={{ wordBreak: 'break-all', marginBottom: 8, color: 'var(--warn)' }}>{advertised}</div>
        </>
      )}
      {reachable && genesis && (
        <div style={{ marginTop: 4 }}>
          {fact(t('set.network'), genesis.cluster)}
          {genesis.symbol ? fact(t('genesis.token'), genesis.symbol) : null}
          {typeof genesis.aprPercent === 'number' ? fact('APR', `${fmt(genesis.aprPercent)}%`) : null}
          {typeof genesis.rewardPerUnit === 'number' ? fact(t('genesis.rewardPerUnit'), `${fmt(genesis.rewardPerUnit)} ${genesis.symbol}/unit`) : null}
          {bonusPct != null ? fact(t('genesis.gatewayBonus'), `+${bonusPct}%`) : null}
        </div>
      )}
    </Card>
  )
}

function SecurityCard() {
  const { t } = useI18n()
  const { lock } = useWallet()
  const [open, setOpen] = useState(false)
  const [cur, setCur] = useState('')
  const [nu, setNu] = useState('')
  const [conf, setConf] = useState('')
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<{ ok?: boolean; text: string } | null>(null)

  const change = async () => {
    const ve = passphraseError(nu, conf, t); if (ve) { setMsg({ text: ve }); return }
    setBusy(true); setMsg(null)
    try {
      await api.wallet.changePassphrase(cur, nu)
      setMsg({ ok: true, text: t('set.passChanged') }); setCur(''); setNu(''); setConf(''); setOpen(false)
    } catch { setMsg({ text: t('lock.wrong') }) } finally { setBusy(false) }
  }

  return (
    <Card>
      <div className="label" style={{ marginBottom: 10 }}>{t('set.security')}</div>
      <button className="btn ghost block" onClick={() => lock()}>🔒 {t('set.lockNow')}</button>
      <div style={{ height: 10 }} />
      {!open ? (
        <button className="btn ghost block" onClick={() => { setOpen(true); setMsg(null) }}>{t('set.changePass')}</button>
      ) : (
        <div className="grid" style={{ gap: 10 }}>
          <input type="password" placeholder={t('set.currentPass')} value={cur} onChange={(e) => setCur(e.target.value)} />
          <input type="password" placeholder={t('set.newPass')} value={nu} onChange={(e) => setNu(e.target.value)} />
          <input type="password" placeholder={t('lock.confirm')} value={conf} onChange={(e) => setConf(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') change() }} />
          <div className="row" style={{ gap: 8 }}>
            <button className="btn" disabled={busy || !cur || !nu} onClick={change}>{busy ? t('common.busy') : t('set.changePass')}</button>
            <button className="btn ghost" onClick={() => { setOpen(false); setMsg(null) }}>{t('common.cancel')}</button>
          </div>
        </div>
      )}
      {msg && <div className="small" style={{ color: msg.ok ? 'var(--good)' : 'var(--danger)', marginTop: 8 }}>{msg.text}</div>}
    </Card>
  )
}

export function SettingsScreen() {
  const { t, lang, setLang } = useI18n()
  const { network, setNetwork, stakingUrl, setStakingUrl, logout } = useWallet()
  const [theme, setThemeState] = useState<string>(document.documentElement.dataset.theme || 'dark')
  const [url, setUrl] = useState(stakingUrl)
  const [phrase, setPhrase] = useState<string | null>(null)

  const reveal = async () => { setPhrase(await api.wallet.mnemonic()) }

  const setTheme = (th: string) => { document.documentElement.dataset.theme = th; localStorage.setItem('theme', th); setThemeState(th) }

  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', alignItems: 'start' }}>
      <div className="grid">
        <Card>
          <Field label={t('set.language')}>
            <select value={lang} onChange={(e) => setLang(e.target.value as Lang)}>
              {LANGS.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
            </select>
          </Field>
          <div style={{ height: 14 }} />
          <Field label={t('set.theme')}>
            <div className="row" style={{ gap: 8 }}>
              <button className={`chip ${theme === 'dark' ? 'on' : ''}`} onClick={() => setTheme('dark')}>{t('set.themeDark')}</button>
              <button className={`chip ${theme === 'light' ? 'on' : ''}`} onClick={() => setTheme('light')}>{t('set.themeLight')}</button>
            </div>
          </Field>
        </Card>
        <Card>
          <Field label={t('set.network')}>
            <div className="row" style={{ gap: 8 }}>
              {(['devnet', 'mainnet'] as Network[]).map((n) => (
                <button key={n} className={`chip ${network === n ? 'on' : ''}`} onClick={() => setNetwork(n)}>{t(`net.${n}`)}</button>
              ))}
            </div>
          </Field>
        </Card>
        <GatewayCard />
      </div>

      <div className="grid">
        <Card>
          <Field label={t('set.stakingUrl')}>
            <input className="mono" value={url} onChange={(e) => setUrl(e.target.value)} onBlur={() => setStakingUrl(url)} />
          </Field>
        </Card>
        <GenesisCard />
        <Card>
          <div className="label" style={{ marginBottom: 6 }}>{t('set.export')}</div>
          <div className="small muted" style={{ marginBottom: 12 }}>{t('set.exportDesc')}</div>
          {phrase == null ? (
            <button className="btn ghost block" onClick={reveal}>{t('set.reveal')}</button>
          ) : (
            <>
              <div className="small" style={{ color: 'var(--warn)', marginBottom: 10 }}>{t('set.exportWarn')}</div>
              {phrase.startsWith('raw:') ? (
                // Imported raw secret key (no mnemonic exists for it) — e.g. the genesis wallet.
                <div className="mono small" style={{ background: 'var(--surface-2)', borderRadius: 10, padding: 10, wordBreak: 'break-all' }}>
                  {phrase.slice(4)}
                </div>
              ) : (
                <div className="grid cols-3" style={{ gap: 8 }}>
                  {phrase.split(' ').map((w, i) => (
                    <div key={i} className="row" style={{ background: 'var(--surface-2)', borderRadius: 10, padding: '8px 10px' }}>
                      <span className="grad-text" style={{ fontWeight: 700, fontSize: 12, width: 18 }}>{i + 1}</span>
                      <span className="mono" style={{ fontWeight: 600 }}>{w}</span>
                    </div>
                  ))}
                </div>
              )}
              <div className="row" style={{ gap: 10, marginTop: 12 }}>
                <CopyButton text={phrase.startsWith('raw:') ? phrase.slice(4) : phrase} label={t('set.copyPhrase')} />
                <button className="btn ghost" onClick={() => setPhrase(null)}>{t('set.hide')}</button>
              </div>
            </>
          )}
        </Card>

        {lockCapable && <SecurityCard />}
        <OperatorPanel />

        <Card>
          <div className="label" style={{ marginBottom: 10 }}>{t('set.about')}</div>
          <div className="small muted">Kvasir Wallet · 0.1.0 · {api.isElectron ? 'Electron' : 'Web'}</div>
          <hr className="divider" />
          <button className="btn ghost block" style={{ color: 'var(--danger)' }} onClick={logout}>{t('set.deleteWallet')}</button>
        </Card>
      </div>
    </div>
  )
}
