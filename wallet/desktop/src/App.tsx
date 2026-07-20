import { useEffect, useState } from 'react'
import { useWallet } from './state'
import { useI18n, LANGS, Lang } from './i18n'
import { NavCtx, Route } from './nav'
import { api, lockCapable, isElectron } from './api'
import { Onboarding } from './screens/onboarding'
import { UnlockScreen, UpgradeScreen } from './screens/lock'
import { Operator2FAGate } from './screens/operator'
import { Overview } from './screens/overview'
import { WalletScreen } from './screens/wallet'
import { NodesScreen } from './screens/nodes'
import { MapScreen } from './screens/map'
import { NodeSettingsScreen } from './screens/nodeSettings'
import { InferenceScreen } from './screens/inference'
import { ModelsScreen } from './screens/models'
import { PricingScreen } from './screens/pricing'
import { SettingsScreen } from './screens/settings'

const NAV: { route: Route; icon: string }[] = [
  { route: 'overview', icon: '◇' }, { route: 'wallet', icon: '💳' },
  { route: 'node', icon: '📊' }, { route: 'map', icon: '🌍' }, { route: 'nodeSettings', icon: '🖥' },
  { route: 'inference', icon: '✨' }, { route: 'models', icon: '🧠' }, { route: 'pricing', icon: '🏷' },
  { route: 'settings', icon: '⚙' },
]
const navKey: Record<Route, string> = {
  overview: 'nav.overview', wallet: 'nav.wallet', node: 'nav.node', map: 'nav.map',
  nodeSettings: 'nav.nodeSettings', inference: 'nav.inference', models: 'models.title',
  pricing: 'pricing.title', settings: 'nav.settings',
}

// Pricing governance is a desktop-only surface, and only when THIS machine
// hosts the gateway (writes must reach it directly, never via the public proxy).
function useHostsGateway(): boolean {
  const [hosted, setHosted] = useState(false)
  useEffect(() => {
    if (!isElectron) return
    let alive = true
    const check = () => api.gateway.status()
      .then((s) => { if (alive) setHosted(!!s.running) })
      .catch(() => { if (alive) setHosted(false) })
    check()
    const id = setInterval(check, 5000)
    return () => { alive = false; clearInterval(id) }
  }, [])
  return isElectron && hosted
}

export default function App() {
  const { ready, hasWallet, locked, needsUpgrade, network, address, loading, refresh, setNetwork, lock, logout, operator } = useWallet()
  const { t, lang, setLang } = useI18n()
  const [route, setRoute] = useState<Route>('overview')
  const [menuOpen, setMenuOpen] = useState(false)
  const hostsGateway = useHostsGateway()

  if (!ready) return <div style={{ display: 'grid', placeItems: 'center', height: '100vh' }} className="muted">…</div>
  if (!hasWallet) return <Onboarding />
  if (needsUpgrade) return <UpgradeScreen />
  if (locked) return <UnlockScreen />
  // Operator wallets with 2FA complete the second factor before entering the app.
  if (operator.phase === 'need-2fa') return <Operator2FAGate />

  const screen = { overview: <Overview />, wallet: <WalletScreen />, node: <NodesScreen />, map: <MapScreen />, nodeSettings: <NodeSettingsScreen />, inference: <InferenceScreen />, models: <ModelsScreen />, pricing: <PricingScreen />, settings: <SettingsScreen /> }[route]
  const nav = NAV.filter((n) => n.route !== 'pricing' || hostsGateway)

  return (
    <NavCtx.Provider value={{ route, go: setRoute }}>
      <div className="shell">
        <aside className="sidebar">
          <div className="brand"><div className="brand-mark">K</div><div className="brand-name">{isElectron ? 'Kvasir' : 'Kvasir Gateway'}</div></div>
          {nav.map((n) => (
            <button key={n.route} className={`nav-item ${route === n.route ? 'active' : ''}`} onClick={() => setRoute(n.route)}>
              <span className="ico">{n.icon}</span><span>{t(navKey[n.route])}</span>
            </button>
          ))}
          <div className="nav-spacer" />
          <div className="card" style={{ padding: 12 }}>
            <div className="small muted">{t('common.address')}</div>
            <div className="mono small" style={{ wordBreak: 'break-all', marginTop: 4 }}>{address?.slice(0, 16)}…</div>
          </div>
        </aside>

        <div className="main">
          <header className="topbar">
            <div>
              <h1>{t(navKey[route])}</h1>
              {route === 'overview' && <div className="sub">{t('ov.subtitle')}</div>}
            </div>
            <div className="nav-spacer" />
            <span className="row" style={{ gap: 6 }}>
              <span className="small muted">🌐</span>
              <select value={lang} onChange={(e) => setLang(e.target.value as Lang)} style={{ width: 'auto', padding: '7px 10px' }} title={t('set.language')}>
                {LANGS.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
              </select>
            </span>
            <span className="chip"><span className="badge-dot" style={{ background: network === 'mainnet' ? 'var(--good)' : 'var(--info)', marginRight: 6 }} />{t(`net.${network}`)}</span>
            <div className="menu-wrap">
              <button className="menu-btn" onClick={() => setMenuOpen((v) => !v)} title="menu">⋮</button>
              {menuOpen && (
                <>
                  <div style={{ position: 'fixed', inset: 0, zIndex: 40 }} onClick={() => setMenuOpen(false)} />
                  <div className="menu-pop">
                    <div className="menu-label">{t('set.network')}</div>
                    <button className="menu-item" onClick={() => { setNetwork('devnet'); setMenuOpen(false) }}>{network === 'devnet' ? '✓' : ''} {t('net.devnet')}</button>
                    <button className="menu-item" onClick={() => { setNetwork('mainnet'); setMenuOpen(false) }}>{network === 'mainnet' ? '✓' : ''} {t('net.mainnet')}</button>
                    <div className="menu-sep" />
                    <button className="menu-item" onClick={() => { refresh(); setMenuOpen(false) }}>↻ {t('common.refresh')}</button>
                    <button className="menu-item" onClick={() => { setRoute('settings'); setMenuOpen(false) }}>⚙ {t('nav.settings')}</button>
                    {lockCapable && <button className="menu-item" onClick={() => { lock(); setMenuOpen(false) }}>🔒 {t('lock.lockNow')}</button>}
                    <div className="menu-sep" />
                    <button className="menu-item" style={{ color: 'var(--danger)' }} onClick={() => { logout(); setMenuOpen(false) }}>🗑 {t('set.deleteWallet')}</button>
                  </div>
                </>
              )}
            </div>
          </header>
          <div className="content">{screen}</div>
        </div>
      </div>
    </NavCtx.Provider>
  )
}
