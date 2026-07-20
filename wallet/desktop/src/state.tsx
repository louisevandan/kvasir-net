import { createContext, useContext, useState, useCallback, useEffect, ReactNode } from 'react'
import { api, isElectron, lockCapable, Balances, Network, TxRef } from './api'
import { Staking, StakingConfig } from './services'
import { OperatorState, operatorSignIn, operator2faLogin, operatorLogout } from './operatorAuth'

const AUTO_LOCK_MS = 10 * 60 * 1000

interface WalletState {
  ready: boolean
  hasWallet: boolean
  locked: boolean
  needsUpgrade: boolean
  address: string | null
  network: Network
  balances: Balances | null
  txs: TxRef[]
  stakingUrl: string
  treasuryOwner: string
  os: string
  arch: string
  // Genesis gateway info (/api/config of the active settlement URL); null while
  // loading or when the gateway is unreachable.
  genesis: StakingConfig | null
  loading: boolean
  error: string | null
  // Operator (gateway admin) auth layered on the wallet login. `phase` is
  // 'disabled' when the gateway has no admin allowlist, 'not-admin' for a normal
  // wallet, 'need-2fa' while the app must collect a TOTP code, 'signed-in' once done.
  operator: OperatorState
  setNetwork: (n: Network) => void
  setStakingUrl: (u: string) => void
  refresh: () => Promise<void>
  reload: () => Promise<void>
  unlock: (passphrase: string) => Promise<void>
  lock: () => Promise<void>
  upgrade: (passphrase: string) => Promise<void>
  logout: () => Promise<void>
  operatorRefresh: () => Promise<void>
  operatorSubmit2fa: (code: string) => Promise<void>
  operatorSignOut: () => Promise<void>
}

const Ctx = createContext<WalletState>(null as any)

export function WalletProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false)
  const [hasWallet, setHasWallet] = useState(false)
  const [locked, setLocked] = useState(false)
  const [needsUpgrade, setNeedsUpgrade] = useState(false)
  const [address, setAddress] = useState<string | null>(null)
  const [network, setNetworkState] = useState<Network>('devnet')
  const [balances, setBalances] = useState<Balances | null>(null)
  const [txs, setTxs] = useState<TxRef[]>([])
  const [stakingUrl, setStakingUrlState] = useState('')
  const [treasuryOwner, setTreasuryOwner] = useState('')
  const [os, setOs] = useState('unknown')
  const [arch, setArch] = useState('x64')
  const [genesis, setGenesis] = useState<StakingConfig | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [operator, setOperator] = useState<OperatorState>({ enabled: false, phase: 'disabled', twofa: false })

  const refresh = useCallback(async () => {
    const st = await api.wallet.state()
    if (!st.exists || st.locked || !st.encrypted) return // nothing to load until unlocked
    setLoading(true); setError(null)
    try {
      const [b, h] = await Promise.all([api.solana.balances(network), api.solana.history(network)])
      setBalances(b); setTxs(h)
    } catch (e: any) { setError(String(e?.message ?? e)) }
    finally { setLoading(false) }
  }, [network])

  const applyState = useCallback(async () => {
    const st = await api.wallet.state()
    // Gate on providers with a real persistent encrypted key store (desktop +
    // browser wallet). The mock has no real lock, so it never gates.
    const lockable = lockCapable
    setHasWallet(st.exists)
    setNeedsUpgrade(lockable && st.exists && !st.encrypted)
    setLocked(lockable && st.exists && st.encrypted && st.locked)
    setAddress(st.exists ? await api.wallet.address() : null)
    return st
  }, [])

  const reload = useCallback(async () => {
    const st = await applyState()
    if (st.exists && st.encrypted && !st.locked) await refresh()
  }, [applyState, refresh])

  useEffect(() => {
    (async () => {
      const [cfg, meta] = await Promise.all([api.config.get(), api.meta()])
      const initial = cfg.stakingUrl || meta.defaultStakingUrl
      setNetworkState(cfg.network); setStakingUrlState(initial)
      setTreasuryOwner(meta.treasuryOwner); setOs(meta.os); setArch(meta.arch)
      // Genesis discovery: on the native app, adopt the gateway's advertised
      // public URL unless the user set a custom one. Served web apps keep their
      // own origin (more reliable for whoever reached them).
      if (isElectron && (!cfg.stakingUrl || cfg.stakingUrl === meta.defaultStakingUrl)) {
        try {
          const gwc = await new Staking(initial).config()
          // Only adopt an advertised public URL that is actually reachable — a gateway
          // that advertises a not-yet-live domain (DNS/ports pending) must not strand us.
          if (gwc?.publicUrl && gwc.publicUrl !== initial) {
            await new Staking(gwc.publicUrl).config()
            setStakingUrlState(gwc.publicUrl); api.config.set({ stakingUrl: gwc.publicUrl })
          }
        } catch { /* gateway (or its advertised URL) unreachable; keep the shipped default */ }
      }
      await applyState()
      setReady(true)
    })()
  }, [applyState])

  useEffect(() => { if (ready && hasWallet && !locked && !needsUpgrade) refresh() }, [ready, hasWallet, locked, needsUpgrade, network, refresh])

  // Genesis gateway info: fetch /api/config of the active settlement URL for the
  // settings display. Re-runs whenever the URL changes (incl. after auto-adopt).
  useEffect(() => {
    if (!stakingUrl) { setGenesis(null); return }
    let alive = true
    new Staking(stakingUrl).config()
      .then((c) => { if (alive) setGenesis(c) })
      .catch(() => { if (alive) setGenesis(null) })
    return () => { alive = false }
  }, [stakingUrl])

  // Operator auth: after the wallet is unlocked, try to establish a gateway admin
  // session (SIWS). No-op for non-admin wallets / gateways without an allowlist.
  // On Electron this global flow stays off — the desktop signs in per-screen
  // against the LOCAL gateway it hosts (pricing governance), not the public URL.
  const operatorRefresh = useCallback(async () => {
    try {
      if (isElectron) { setOperator({ enabled: false, phase: 'disabled', twofa: false }); return }
      const addr = await api.wallet.address()
      const url = (await api.config.get()).stakingUrl || stakingUrl
      if (!addr || !url) { setOperator({ enabled: false, phase: 'disabled', twofa: false }); return }
      setOperator(await operatorSignIn(url, addr))
    } catch { setOperator({ enabled: false, phase: 'disabled', twofa: false }) }
  }, [stakingUrl])
  const operatorSubmit2fa = useCallback(async (code: string) => {
    const url = (await api.config.get()).stakingUrl || stakingUrl
    await operator2faLogin(url, operator.preAuth || '', code) // throws on bad code; caller shows it
    await operatorRefresh()
  }, [stakingUrl, operator.preAuth, operatorRefresh])
  const operatorSignOut = useCallback(async () => {
    const url = (await api.config.get()).stakingUrl || stakingUrl
    try { await operatorLogout(url) } catch { /* ignore */ }
    setOperator({ enabled: false, phase: 'disabled', twofa: false })
  }, [stakingUrl])

  // Run operator sign-in once the wallet is usable (and re-run if the URL changes).
  useEffect(() => {
    if (ready && hasWallet && !locked && !needsUpgrade) operatorRefresh()
    else if (locked) setOperator({ enabled: false, phase: 'disabled', twofa: false })
  }, [ready, hasWallet, locked, needsUpgrade, stakingUrl, operatorRefresh])

  const setNetwork = useCallback((n: Network) => { setNetworkState(n); api.config.set({ network: n }) }, [])
  const setStakingUrl = useCallback((u: string) => { setStakingUrlState(u); api.config.set({ stakingUrl: u }) }, [])

  const unlock = useCallback(async (passphrase: string) => {
    await api.wallet.unlock(passphrase) // throws on wrong passphrase; caller shows the error
    setLocked(false); setNeedsUpgrade(false)
    setAddress(await api.wallet.address())
  }, [])
  const lock = useCallback(async () => {
    if (!lockCapable) return // mock has no real lock — see applyState
    await api.wallet.lock()
    setLocked(true); setBalances(null); setTxs([]); setError(null)
  }, [])
  const upgrade = useCallback(async (passphrase: string) => {
    await api.wallet.upgrade(passphrase)
    setNeedsUpgrade(false); setLocked(false)
    setAddress(await api.wallet.address())
  }, [])
  const logout = useCallback(async () => {
    await api.wallet.clear()
    setHasWallet(false); setLocked(false); setNeedsUpgrade(false)
    setAddress(null); setBalances(null); setTxs([])
    setOperator({ enabled: false, phase: 'disabled', twofa: false })
  }, [])

  // Auto-lock (desktop only): after AUTO_LOCK_MS of no interaction. We deliberately
  // do NOT lock on window-hidden — switching apps to read docs was locking too
  // eagerly; the idle timer keeps running while hidden, so walking away still locks.
  useEffect(() => {
    if (!ready || !hasWallet || locked || needsUpgrade || !lockCapable) return
    let timer: ReturnType<typeof setTimeout>
    const arm = () => { clearTimeout(timer); timer = setTimeout(() => { lock() }, AUTO_LOCK_MS) }
    const acts: (keyof WindowEventMap)[] = ['mousemove', 'mousedown', 'keydown', 'wheel', 'touchstart']
    acts.forEach((e) => window.addEventListener(e, arm, { passive: true }))
    arm()
    return () => { clearTimeout(timer); acts.forEach((e) => window.removeEventListener(e, arm)) }
  }, [ready, hasWallet, locked, needsUpgrade, lock])

  return (
    <Ctx.Provider value={{ ready, hasWallet, locked, needsUpgrade, address, network, balances, txs, stakingUrl, treasuryOwner, os, arch, genesis, loading, error, operator, setNetwork, setStakingUrl, refresh, reload, unlock, lock, upgrade, logout, operatorRefresh, operatorSubmit2fa, operatorSignOut }}>
      {children}
    </Ctx.Provider>
  )
}
export const useWallet = () => useContext(Ctx)
