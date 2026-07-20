import { useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { api, fmt } from '../api'
import { Gateway, Credit, PayModel } from '../services'

interface Msg {
  role: 'user' | 'assistant'
  content: string
  thinking?: boolean
  cost?: { total: number; token?: number }
  error?: boolean
  model?: string
}

// Inference conversation is persisted locally so it survives reloads/relaunches.
const CHAT_KEY = 'kvasir.inference.history'
const CHAT_MAX = 100
function loadChat(): Msg[] {
  try {
    const raw = localStorage.getItem(CHAT_KEY)
    if (raw) return (JSON.parse(raw) as Msg[]).filter((m) => !m.thinking)
  } catch { /* ignore corrupt/absent */ }
  return []
}

export function InferenceScreen() {
  const { t } = useI18n()
  const { stakingUrl, network, refresh } = useWallet()
  const gw = new Gateway(stakingUrl)
  const credit = new Credit(stakingUrl)
  const [models, setModels] = useState<PayModel[]>([])
  const [localModels, setLocalModels] = useState<{ name: string; sizeBytes: number }[]>([])
  const [model, setModel] = useState('')
  const [input, setInput] = useState('')
  const [msgs, setMsgs] = useState<Msg[]>(loadChat)
  const [busy, setBusy] = useState(false)
  // Prepaid-credit state: networked inference streams over /v1/chat/completions
  // (credit-debited) so slow models stream past Cloudflare's 100s timeout.
  const [creditBalance, setCreditBalance] = useState<number | null>(null)
  const [depositRecipient, setDepositRecipient] = useState('')
  const [showTopUp, setShowTopUp] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    // On-device models first (free, no network), then gateway models.
    window.linkcpp?.models?.list().then((list) => {
      setLocalModels(list)
      if (!model && list[0]) setModel(`local:${list[0].name}`)
    }).catch(() => {})
    gw.models().then((m) => { setModels(m.models); setDepositRecipient(m.recipient); setModel((cur) => cur || m.models[0]?.id || '') }).catch(() => {})
    refreshBalance()
  }, [stakingUrl])
  useEffect(() => { scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' }) }, [msgs])
  // Persist on every change, dropping the transient "thinking" placeholder.
  useEffect(() => {
    try { localStorage.setItem(CHAT_KEY, JSON.stringify(msgs.filter((m) => !m.thinking).slice(-CHAT_MAX))) } catch { /* ignore */ }
  }, [msgs])

  const clearChat = () => { setMsgs([]); try { localStorage.removeItem(CHAT_KEY) } catch { /* ignore */ } }

  const isLocal = model.startsWith('local:')

  // Prepaid-credit onboarding + streaming. The wallet self-registers and mints an
  // API key on first use (both sign a gateway nonce), stored per-wallet.
  const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u))
  const apiKeyKey = (addr: string) => `kvasir.credit.apikey.${addr}`

  async function ensureApiKey(): Promise<string> {
    const addr = await api.wallet.address()
    if (!addr || !api.wallet.signMessage) throw new Error(t('inf.walletLocked'))
    const stored = localStorage.getItem(apiKeyKey(addr))
    if (stored) return stored
    const rc = await credit.registerChallenge(addr)                        // self-whitelist (idempotent)
    await credit.register(addr, rc.nonce, b64(await api.wallet.signMessage(new TextEncoder().encode(rc.message))))
    const ac = await credit.apikeyChallenge(addr)
    const { apiKey } = await credit.apikey(addr, ac.nonce, b64(await api.wallet.signMessage(new TextEncoder().encode(ac.message))), 'Kvasir Desktop')
    localStorage.setItem(apiKeyKey(addr), apiKey)
    return apiKey
  }

  async function refreshBalance() {
    const addr = await api.wallet.address()
    if (!addr) return
    const key = localStorage.getItem(apiKeyKey(addr))
    if (!key) { setCreditBalance(null); return }
    try { setCreditBalance((await credit.balance(key)).balance) } catch { /* ignore */ }
  }

  async function topUp(amount: number): Promise<string> {
    const addr = await api.wallet.address()
    if (!addr) return t('inf.walletLocked')
    if (!depositRecipient) return t('inf.topUpNoVault')
    try {
      await ensureApiKey()                                                 // must be whitelisted before deposit
      const sig = await api.solana.sendToken({ to: depositRecipient, amount, network })
      const r = await credit.deposit(addr, amount, sig)
      setCreditBalance(r.balance)
      await refresh()
      return t('inf.topUpOk')
    } catch (e: any) { return `${t('inf.topUpFailed')}: ${e?.message ?? e}` }
  }

  const send = async () => {
    const prompt = input.trim()
    if (!prompt || busy || !model) return
    if (isLocal) { await sendLocal(prompt); return }
    const modelName = models.find((mm) => mm.id === model)?.name ?? model
    // Prior turns + this prompt (drop placeholders/errors, cap the tail).
    const convo = [
      ...msgs.filter((m) => !m.thinking && !m.error && m.content).slice(-20).map((m) => ({ role: m.role, content: m.content })),
      { role: 'user', content: prompt },
    ]
    setInput(''); setBusy(true)
    setMsgs((m) => [...m, { role: 'user', content: prompt }, { role: 'assistant', content: '', thinking: true, model: modelName }])
    try {
      const key = await ensureApiKey()
      let acc = ''
      await credit.streamChat(key, model, convo, (e) => {
        if (e.type === 'token') { acc += e.text; setMsgs((m) => replaceLast(m, { role: 'assistant', content: acc, model: modelName })) }
        else setMsgs((m) => replaceLast(m, { role: 'assistant', content: acc, model: modelName, cost: { total: e.usage.totalTokens, token: e.usage.costToken } }))
      })
      await refreshBalance()
    } catch (e: any) {
      setMsgs((m) => replaceLast(m, { role: 'assistant', content: String(e?.message ?? e), error: true }))
    } finally { setBusy(false) }
  }

  // On-device inference over a downloaded GGUF: spawns a local llama-server, no
  // payment. First run pays the model-load cost, then answers stream back.
  const sendLocal = async (prompt: string) => {
    const fileName = model.slice('local:'.length)
    const label = `${fileName.replace(/\.gguf$/i, '')} · ${t('models.onDevice')}`
    setInput(''); setBusy(true)
    setMsgs((m) => [...m, { role: 'user', content: prompt }, { role: 'assistant', content: '', thinking: true, model: label }])
    try {
      const r = await window.linkcpp!.models!.generate(fileName, prompt, 512)
      if (r.ok) setMsgs((m) => replaceLast(m, { role: 'assistant', content: r.text || '', model: label }))
      else setMsgs((m) => replaceLast(m, { role: 'assistant', content: r.error || t('models.loadFailed'), error: true }))
    } catch (e: any) {
      setMsgs((m) => replaceLast(m, { role: 'assistant', content: String(e?.message ?? e), error: true }))
    } finally { setBusy(false) }
  }

  return (
    <div className="chat-wrap">
      <div className="chat-scroll" ref={scrollRef}>
        {msgs.length === 0 ? (
          <div className="chat-empty">
            <div style={{ fontSize: 40 }}>✨</div>
            <div style={{ fontWeight: 700, marginTop: 8, color: 'var(--text-2)' }}>{t('inf.title')}</div>
            <div className="small" style={{ marginTop: 4 }}>{t('inf.actualNote')}</div>
          </div>
        ) : msgs.map((m, i) => (
          <div key={i} className={`msg ${m.role === 'user' ? 'user' : 'ai'}`}>
            <div className="avatar">{m.role === 'user' ? '🧑' : '✨'}</div>
            <div style={{ minWidth: 0 }}>
              {m.role === 'assistant' && !m.error && m.model && (
                <div className="small" style={{ color: 'var(--pink)', fontWeight: 600, marginBottom: 4 }}>{m.model}</div>
              )}
              <div className="bubble" style={m.error ? { borderColor: 'var(--danger)', color: 'var(--danger)' } : undefined}>
                {m.thinking ? (
                  <span className="thinking">{t('inf.thinking')}<span className="dots"><i /><i /><i /></span></span>
                ) : m.role === 'assistant' ? (
                  <div className="md"><ReactMarkdown remarkPlugins={[remarkGfm]}>{m.content}</ReactMarkdown></div>
                ) : m.content}
              </div>
              {m.cost && <div className="cost">{t('inf.actualTokens')}: {m.cost.total} tok{m.cost.token != null ? ` · ${fmt(m.cost.token)} KVR` : ''}</div>}
            </div>
          </div>
        ))}
      </div>

      <div className="composer">
        <div className="row" style={{ gap: 8, marginBottom: 10 }}>
          <select className="model-select" value={model} onChange={(e) => setModel(e.target.value)} disabled={!models.length && !localModels.length}>
            {!models.length && !localModels.length && <option value="">—</option>}
            {localModels.length > 0 && (
              <optgroup label={t('models.onDevice')}>
                {localModels.map((lm) => <option key={lm.name} value={`local:${lm.name}`}>{lm.name.replace(/\.gguf$/i, '')}</option>)}
              </optgroup>
            )}
            {models.length > 0 && (
              <optgroup label={t('inf.gatewayModels')}>
                {models.map((mm) => <option key={mm.id} value={mm.id}>{mm.name}</option>)}
              </optgroup>
            )}
          </select>
          <button className="btn ghost" onClick={() => { setShowTopUp(true); refreshBalance() }} title={t('inf.topUpTitle')}>
            💳 {creditBalance != null ? fmt(creditBalance) : t('inf.credits')}
          </button>
          {msgs.length > 0 && <button className="btn ghost" onClick={clearChat} title={t('inf.clear')}>🗑</button>}
        </div>
        <div className="box">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); send() } }}
            placeholder={t('inf.prompt')}
            rows={2}
          />
          <button className="btn" style={{ height: 52, minWidth: 96 }} disabled={busy || !input.trim() || !model} onClick={send}>
            {busy ? '…' : isLocal ? t('models.run') : t('inf.payRun')}
          </button>
        </div>
      </div>

      {showTopUp && (
        <TopUpModal balance={creditBalance} recipient={depositRecipient} onClose={() => setShowTopUp(false)} onTopUp={topUp} />
      )}
    </div>
  )
}

// Credit top-up: transfer KVR on-chain to the gateway vault and credit it to the
// prepaid balance that networked (streaming) inference debits.
function TopUpModal({ balance, recipient, onClose, onTopUp }: {
  balance: number | null; recipient: string; onClose: () => void; onTopUp: (amount: number) => Promise<string>
}) {
  const { t } = useI18n()
  const [amount, setAmount] = useState('10')
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)
  const submit = async () => {
    const amt = Number(amount)
    if (!Number.isFinite(amt) || amt <= 0) { setMsg(t('inf.topUpInvalid')); return }
    setBusy(true); const r = await onTopUp(amt); setBusy(false); setMsg(r)
    if (r === t('inf.topUpOk')) setTimeout(onClose, 700)
  }
  return (
    <div onClick={onClose} style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,.5)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 }}>
      <div className="card" onClick={(e) => e.stopPropagation()} style={{ width: 380, maxWidth: '90vw', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div style={{ fontWeight: 700, fontSize: 16 }}>{t('inf.topUpTitle')}</div>
        <div className="small">{t('inf.currentBalance')}: <b>{balance != null ? `${fmt(balance)} KVR` : '—'}</b></div>
        <div className="small" style={{ color: 'var(--text-2)' }}>{t('inf.topUpNote')}</div>
        <input className="input" type="number" min="0" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder={t('inf.topUpAmount')} />
        {msg && <div className="small" style={{ color: 'var(--text-2)' }}>{msg}</div>}
        <div className="row" style={{ gap: 8, justifyContent: 'flex-end' }}>
          <button className="btn ghost" onClick={onClose}>{t('common.close')}</button>
          <button className="btn" disabled={busy || !recipient} onClick={submit}>{busy ? '…' : t('inf.topUpConfirm')}</button>
        </div>
      </div>
    </div>
  )
}

function replaceLast(list: Msg[], msg: Msg): Msg[] {
  const copy = list.slice()
  copy[copy.length - 1] = msg
  return copy
}
