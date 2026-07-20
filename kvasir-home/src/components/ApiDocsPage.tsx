import { useEffect, useState } from "react";
import { Container, Card, Button, Pill } from "./ui";
import Nav from "./Nav";
import Footer from "./Footer";
import { GithubIcon, ArrowIcon, TerminalIcon, CheckIcon, CoinIcon } from "./icons";
import { LINKS } from "../content";
import { useT } from "../i18n/provider";
import type { Dict } from "../i18n/types";
import {
  SNIPPETS,
  NODE_SETUP_SNIPPETS,
  INFERENCE_API_SNIPPETS,
  SELF_ISSUE_SNIPPETS,
  INFERENCE_API_REF,
  KEY_ISSUE_SNIPPET,
  STREAM_CURL_SNIPPET,
  API_REFS,
  GATEWAY_BASE,
  WALLET_SNIPPET,
  ADAPTER_SNIPPET,
  type ApiRef,
  type Snippet,
} from "./apiDocsSnippets";

/* ==========================================================================
   Developer API docs page (/docs/api). "Call Kvasir inference, paid in KVR."
   Fully i18n via t.apiDocs.* (all 9 languages) — but the code examples and the
   raw JSON in API_REFS are shared constants (apiDocsSnippets.ts), never
   translated. Contract verified against gate.kvasir-ai.net and the shipping
   wallets (wallet/desktop/src/services.ts). Own slim header, like RunNodePage.
   ========================================================================== */

type Api = Dict["apiDocs"];

const fill = (s: string, v: string) => s.replace("{0}", v);

/* Devnet KVR faucet — POSTs a wallet address to the gateway's /api/faucet, which
   dispenses a fixed amount of test KVR (rate-limited per address). Lets a
   developer fund a wallet before running the pay-per-inference flow above. */
function FaucetWidget({ a }: { a: Api }) {
  const [addr, setAddr] = useState("");
  const [status, setStatus] = useState<"idle" | "sending" | "done" | "error">("idle");
  const [result, setResult] = useState<{ amount?: number; signature?: string; error?: string }>({});

  const submit = async () => {
    const address = addr.trim();
    if (!address || status === "sending") return;
    setStatus("sending");
    setResult({});
    try {
      const res = await fetch(`${GATEWAY_BASE}/api/faucet`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ address }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        setResult({ error: data.error || `HTTP ${res.status}` });
        setStatus("error");
        return;
      }
      setResult({ amount: data.amount, signature: data.signature });
      setStatus("done");
    } catch (e) {
      setResult({ error: String((e as Error).message || e) });
      setStatus("error");
    }
  };

  return (
    <Card className="p-5 sm:p-6">
      <div className="flex flex-col gap-3 sm:flex-row">
        <input
          value={addr}
          onChange={(e) => setAddr(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
          spellCheck={false}
          autoComplete="off"
          placeholder={a.faucetPlaceholder}
          className="min-w-0 flex-1 rounded-full bg-surface-2/70 px-4 py-2.5 font-mono text-sm text-ink ring-1 ring-line placeholder:text-ink-faint focus:outline-none focus:ring-2 focus:ring-brand-400"
        />
        <button
          onClick={submit}
          disabled={status === "sending" || !addr.trim()}
          className="group inline-flex h-11 shrink-0 items-center justify-center gap-2 rounded-full bg-brand-500 px-5 text-sm font-semibold text-white shadow-glow transition-all duration-200 hover:bg-brand-400 disabled:pointer-events-none disabled:opacity-50"
        >
          <CoinIcon width={18} height={18} />
          {status === "sending" ? a.faucetSending : a.faucetButton}
        </button>
      </div>
      {status === "done" && (
        <div className="mt-4 flex flex-wrap items-center gap-2 rounded-lg bg-positive/8 px-3 py-2 text-sm text-positive ring-1 ring-positive/20">
          <CheckIcon width={16} height={16} className="shrink-0" />
          <span>{fill(a.faucetSuccess, String(result.amount ?? ""))}</span>
          {result.signature && (
            <a
              href={`https://explorer.solana.com/tx/${result.signature}?cluster=devnet`}
              target="_blank"
              rel="noreferrer"
              className="underline hover:no-underline"
            >
              {a.faucetViewTx} ↗
            </a>
          )}
        </div>
      )}
      {status === "error" && (
        <div className="mt-4 rounded-lg bg-negative/8 px-3 py-2 text-sm text-negative ring-1 ring-negative/20">
          {a.faucetError}
          {result.error ? ` — ${result.error}` : ""}
        </div>
      )}
    </Card>
  );
}

function CodeCard({ code, copy, copied }: { code: string; copy: string; copied: string }) {
  const [done, setDone] = useState(false);
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setDone(true);
      window.setTimeout(() => setDone(false), 1600);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };
  return (
    <div className="overflow-hidden rounded-2xl bg-[#0b0c11] ring-1 ring-line">
      <div className="flex items-center justify-end border-b border-line px-3 py-2">
        <button
          onClick={onCopy}
          className="rounded-md px-2 py-1 font-mono text-xs text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
        >
          {done ? copied : copy}
        </button>
      </div>
      <pre className="max-w-full overflow-x-auto px-5 py-4 font-mono text-[12.5px] leading-relaxed text-ink-muted">
        <code>{code}</code>
      </pre>
    </div>
  );
}

function CodeTabs({ snippets, copy, copied }: { snippets: Snippet[]; copy: string; copied: string }) {
  const [active, setActive] = useState(snippets[0].id);
  const [done, setDone] = useState(false);
  const snip = snippets.find((s) => s.id === active) ?? snippets[0];
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(snip.code);
      setDone(true);
      window.setTimeout(() => setDone(false), 1600);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };
  return (
    <div className="overflow-hidden rounded-2xl bg-[#0b0c11] ring-1 ring-line">
      <div className="flex items-center justify-between gap-2 border-b border-line px-3 py-2">
        <div className="flex min-w-0 flex-1 gap-1 overflow-x-auto">
          {snippets.map((s) => (
            <button
              key={s.id}
              onClick={() => setActive(s.id)}
              className={`shrink-0 rounded-md px-2.5 py-1 font-mono text-xs transition-colors ${
                s.id === active
                  ? "bg-white/10 text-ink"
                  : "text-ink-faint hover:bg-white/5 hover:text-ink-muted"
              }`}
            >
              {s.label}
            </button>
          ))}
        </div>
        <button
          onClick={onCopy}
          className="shrink-0 rounded-md px-2 py-1 font-mono text-xs text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
        >
          {done ? copied : copy}
        </button>
      </div>
      <pre className="max-w-full overflow-x-auto px-5 py-4 font-mono text-[12.5px] leading-relaxed text-ink-muted">
        <code>{snip.code}</code>
      </pre>
    </div>
  );
}

function RefCard({ ref: r, prose, reqLabel, resLabel }: { ref: ApiRef; prose: string; reqLabel: string; resLabel: string }) {
  return (
    <Card className="p-5 sm:p-6">
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded-md bg-brand-500/12 px-2 py-0.5 font-mono text-xs font-semibold text-brand-300 ring-1 ring-brand-500/25">
          {r.method}
        </span>
        <span className="font-mono text-sm text-ink">{r.path}</span>
      </div>
      <p className="mt-3 text-sm leading-relaxed text-ink-muted">{prose}</p>
      {r.body && (
        <div className="mt-4">
          <div className="mb-1 text-xs font-semibold uppercase tracking-wider text-ink-faint">{reqLabel}</div>
          <pre className="max-w-full overflow-x-auto rounded-lg bg-[#0b0c11] px-4 py-3 font-mono text-[12px] leading-relaxed text-ink-muted ring-1 ring-line">
            <code>{r.body}</code>
          </pre>
        </div>
      )}
      <div className="mt-4">
        <div className="mb-1 text-xs font-semibold uppercase tracking-wider text-ink-faint">{resLabel}</div>
        <pre className="max-w-full overflow-x-auto rounded-lg bg-[#0b0c11] px-4 py-3 font-mono text-[12px] leading-relaxed text-ink-muted ring-1 ring-line">
          <code>{r.sample}</code>
        </pre>
      </div>
    </Card>
  );
}

/* Interactive API-key issuer (paste-signature flow, no wallet deps). Orchestrates
   the live credit endpoints: challenge → (self-register if 403) → apikey → balance.
   The wallet signs each challenge message externally (wallet app / CLI) and pastes
   the base64 signature. Agent-friendly; humans can also use the curl above. */
function ApiKeyIssuer({ a, copy, copied }: { a: Api; copy: string; copied: string }) {
  const [wallet, setWallet] = useState("");
  const [label, setLabel] = useState("");
  const [phase, setPhase] = useState<"idle" | "register" | "key" | "done" | "pending">("idle");
  const [challenge, setChallenge] = useState<{ nonce: string; message: string } | null>(null);
  const [sig, setSig] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [copiedKey, setCopiedKey] = useState(false);
  const [balance, setBalance] = useState<{ balance: number; spent: number; symbol: string } | null>(null);

  const post = async (path: string, body: unknown) => {
    const res = await fetch(GATEWAY_BASE + path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({} as any));
    return { ok: res.ok, status: res.status, data };
  };

  const start = async () => {
    const w = wallet.trim();
    if (!w || busy) return;
    setBusy(true);
    setErr("");
    setChallenge(null);
    try {
      const r = await post("/api/credits/challenge", { wallet: w });
      if (r.ok) {
        setChallenge(r.data);
        setPhase("key");
      } else if (r.status === 403) {
        const reg = await post("/api/credits/register/challenge", { wallet: w });
        if (reg.ok) {
          setChallenge(reg.data);
          setPhase("register");
        } else setErr(reg.data.error || `HTTP ${reg.status}`);
      } else setErr(r.data.error || `HTTP ${r.status}`);
    } catch (e) {
      setErr(String((e as Error).message || e));
    }
    setBusy(false);
  };

  const submitSig = async () => {
    const w = wallet.trim();
    const s = sig.trim();
    if (!w || !s || !challenge || busy) return;
    setBusy(true);
    setErr("");
    try {
      if (phase === "register") {
        const r = await post("/api/credits/register", { wallet: w, nonce: challenge.nonce, signature: s });
        if (r.ok) {
          setSig("");
          const c = await post("/api/credits/challenge", { wallet: w });
          if (c.ok) {
            setChallenge(c.data);
            setPhase("key");
          } else setErr(c.data.error || `HTTP ${c.status}`);
        } else if (r.status === 403) setPhase("pending");
        else setErr(r.data.error || `HTTP ${r.status}`);
      } else {
        const r = await post("/api/credits/apikey", {
          wallet: w,
          nonce: challenge.nonce,
          signature: s,
          label: label.trim() || "kvasir-web",
        });
        if (r.ok && r.data.apiKey) {
          setApiKey(r.data.apiKey);
          setSig("");
          setPhase("done");
          try {
            const b = await fetch(GATEWAY_BASE + "/api/credits/balance", {
              headers: { authorization: `Bearer ${r.data.apiKey}` },
            });
            if (b.ok) setBalance(await b.json());
          } catch {
            /* balance is best-effort */
          }
        } else setErr(r.data.error || `HTTP ${r.status}`);
      }
    } catch (e) {
      setErr(String((e as Error).message || e));
    }
    setBusy(false);
  };

  const copyKey = async () => {
    try {
      await navigator.clipboard.writeText(apiKey);
      setCopiedKey(true);
      window.setTimeout(() => setCopiedKey(false), 1600);
    } catch {
      /* clipboard unavailable */
    }
  };

  const inputCls =
    "min-w-0 flex-1 rounded-full bg-surface-2/70 px-4 py-2.5 font-mono text-sm text-ink ring-1 ring-line placeholder:text-ink-faint focus:outline-none focus:ring-2 focus:ring-brand-400";
  const btnCls =
    "inline-flex h-11 shrink-0 items-center justify-center gap-2 rounded-full bg-brand-500 px-5 text-sm font-semibold text-white shadow-glow transition-all duration-200 hover:bg-brand-400 disabled:pointer-events-none disabled:opacity-50";

  return (
    <Card className="p-5 sm:p-6">
      {phase === "idle" && (
        <div className="flex flex-col gap-3">
          <input
            className={inputCls}
            value={wallet}
            onChange={(e) => setWallet(e.target.value)}
            spellCheck={false}
            autoComplete="off"
            placeholder={a.keyIssueWalletPh}
          />
          <div className="flex flex-col gap-3 sm:flex-row">
            <input
              className={inputCls}
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              spellCheck={false}
              autoComplete="off"
              placeholder={a.keyIssueLabelPh}
            />
            <button className={btnCls} onClick={start} disabled={busy || !wallet.trim()}>
              {a.keyIssueStart}
            </button>
          </div>
        </div>
      )}

      {(phase === "register" || phase === "key") && challenge && (
        <div className="flex flex-col gap-3">
          {phase === "register" && <p className="text-xs text-ink-muted">{a.keyIssueRegisterNote}</p>}
          <p className="text-sm font-medium text-ink">{a.keyIssueSignPrompt}</p>
          <pre className="max-w-full overflow-x-auto rounded-lg bg-[#0b0c11] px-4 py-3 font-mono text-[12px] leading-relaxed text-ink-muted ring-1 ring-line">
            <code>{challenge.message}</code>
          </pre>
          <textarea
            className={`${inputCls} rounded-2xl`}
            rows={2}
            value={sig}
            onChange={(e) => setSig(e.target.value)}
            spellCheck={false}
            placeholder={a.keyIssueSigPh}
          />
          <button className={btnCls} onClick={submitSig} disabled={busy || !sig.trim()}>
            {a.keyIssueSubmit}
          </button>
        </div>
      )}

      {phase === "pending" && (
        <p className="rounded-lg bg-caution/8 px-3 py-2 text-sm leading-relaxed text-caution ring-1 ring-caution/20">
          {a.keyIssuePending}
        </p>
      )}

      {phase === "done" && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <span className="text-sm font-medium text-positive">{a.keyIssueKeyReady}</span>
            <button
              onClick={copyKey}
              className="rounded-md px-2 py-1 font-mono text-xs text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
            >
              {copiedKey ? copied : copy}
            </button>
          </div>
          <pre className="max-w-full overflow-x-auto rounded-lg bg-[#0b0c11] px-4 py-3 font-mono text-[12.5px] text-brand-300 ring-1 ring-line">
            <code>{apiKey}</code>
          </pre>
          {balance &&
            (balance.balance > 0 ? (
              <p className="text-sm text-ink-muted">
                <span className="text-ink-faint">{a.keyIssueBalanceLabel}: </span>
                <span className="font-mono text-ink">
                  {balance.balance} {balance.symbol}
                </span>
              </p>
            ) : (
              <p className="rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
                {a.keyIssueTopUp}
              </p>
            ))}
          <p className="mt-1 text-xs leading-relaxed text-ink-muted">{a.keyIssueUseNote}</p>
          <CodeCard
            code={`curl -N ${GATEWAY_BASE}/v1/chat/completions \\\n  -H "Authorization: Bearer ${apiKey}" -H 'content-type: application/json' \\\n  -d '{"model":"f793eb7b:ctrl-f11eb9","messages":[{"role":"user","content":"hi"}],"stream":true}'`}
            copy={copy}
            copied={copied}
          />
        </div>
      )}

      {err && (
        <p className="mt-3 rounded-lg bg-negative/8 px-3 py-2 text-sm text-negative ring-1 ring-negative/20">
          {a.keyIssueError} — {err}
        </p>
      )}
    </Card>
  );
}

/* GitHub star gate (soft, growth-oriented). Content stays public/prerendered
   for SEO; this is an interactive overlay only. Fails OPEN when the OAuth env
   vars aren't configured yet, or on any network error, so the page never
   breaks. z-40 keeps the header (z-50) usable so a visitor can navigate away. */
function StarGate({ a }: { a: Api }) {
  const [gate, setGate] = useState<"loading" | "open" | "signin" | "nostar">("loading");
  const [login, setLogin] = useState<string | null>(null);
  useEffect(() => {
    fetch("/api/gh/status", { credentials: "same-origin" })
      .then((r) => r.json())
      .then((d) => {
        if (!d.configured || d.starred) setGate("open");
        else if (d.authed) {
          setLogin(d.login);
          setGate("nostar");
        } else setGate("signin");
      })
      .catch(() => setGate("open"));
  }, []);

  if (gate === "open" || gate === "loading") return null;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-bg/85 p-6 backdrop-blur-md">
      <Card className="w-full max-w-md p-8 text-center">
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-brand-500/10 text-brand-400 ring-1 ring-brand-500/20">
          <GithubIcon width={24} height={24} />
        </div>
        <h2 className="mt-5 text-2xl font-semibold text-ink">{a.gateTitle}</h2>
        {gate === "signin" ? (
          <>
            <p className="mt-3 text-sm leading-relaxed text-ink-muted">{a.gateBody}</p>
            <Button href="/api/gh/login" variant="primary" size="lg" className="mt-6 w-full justify-center">
              <GithubIcon width={18} height={18} />
              {a.gateSignIn}
            </Button>
          </>
        ) : (
          <>
            <p className="mt-3 text-sm leading-relaxed text-ink-muted">{fill(a.gateStarBody, login || "")}</p>
            <a
              href="https://github.com/louisevandan/kvasir-net"
              target="_blank"
              rel="noreferrer"
              className="mt-5 inline-flex text-sm font-medium text-brand-300 hover:underline"
            >
              {a.gateStarLink}
            </a>
            <Button href="/api/gh/login" variant="primary" size="lg" className="mt-4 w-full justify-center">
              {a.gateRecheck}
            </Button>
          </>
        )}
      </Card>
    </div>
  );
}

export default function ApiDocsPage() {
  const t = useT();
  const a: Api = t.apiDocs;

  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  useEffect(() => {
    document.title = a.docTitle;
  }, [a.docTitle]);

  return (
    <div className="min-h-screen">
      <Nav />
      <StarGate a={a} />

      {/* ambient background, mirrors the hero */}
      <div aria-hidden className="pointer-events-none fixed inset-0 -z-10 bg-grid" />
      <div
        aria-hidden
        className="pointer-events-none absolute -top-40 left-1/2 -z-10 h-[520px] w-[820px] max-w-[100vw] -translate-x-1/2 blur-3xl"
        style={{
          background:
            "radial-gradient(50% 50% at 50% 30%, rgba(255,61,139,0.16), transparent 70%), radial-gradient(40% 40% at 70% 60%, rgba(168,85,247,0.12), transparent 70%)",
        }}
      />

      <Container className="pt-28 pb-20 sm:pt-36">
        {/* hero */}
        <div className="max-w-3xl">
          <Pill tone="brand">
            <TerminalIcon width={14} height={14} />
            {a.pill}
          </Pill>
          <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
            {a.title}
          </h1>
          <p className="mt-5 text-lg leading-relaxed text-ink-muted">{a.lede}</p>
          <div className="mt-6 flex flex-wrap items-center gap-3">
            <span className="text-xs font-semibold uppercase tracking-wider text-ink-faint">{a.baseLabel}</span>
            <code className="rounded-md bg-surface-2/70 px-2.5 py-1 font-mono text-sm text-brand-300 ring-1 ring-line">
              {GATEWAY_BASE}
            </code>
          </div>
          <p className="mt-5 rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
            {a.devnetNote}
          </p>
        </div>

        {/* docs body — left sidebar + sections */}
        <div className="mt-14 grid gap-10 lg:grid-cols-[13rem_1fr]">
          <aside className="hidden lg:block">
            <nav aria-label="Developer docs" className="sticky top-24 space-y-6 text-sm">
              {[
                { label: a.catRunNode, items: [{ id: "self-host", label: a.selfHostTitle }] },
                {
                  label: a.catUseApi,
                  items: [
                    { id: "wallet", label: a.walletTitle },
                    { id: "faucet", label: a.faucetTitle },
                    { id: "flow", label: a.flowTitle },
                    { id: "inference-api", label: a.inferenceApiTitle },
                    { id: "reference", label: a.refTitle },
                    { id: "example", label: a.codeTitle },
                    { id: "adapter", label: a.adapterTitle },
                    { id: "prereqs", label: a.prereqTitle },
                    { id: "security", label: a.securityTitle },
                  ],
                },
              ].map((g) => (
                <div key={g.label}>
                  <div className="text-xs font-semibold uppercase tracking-[0.16em] text-brand-400">{g.label}</div>
                  <ul className="mt-3 space-y-1 border-l border-line">
                    {g.items.map((it) => (
                      <li key={it.id}>
                        <a
                          href={`#${it.id}`}
                          className="-ml-px block border-l border-transparent py-1.5 pl-4 leading-snug text-ink-muted transition-colors hover:border-line hover:text-ink"
                        >
                          {it.label}
                        </a>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </nav>
          </aside>

          <div className="min-w-0">
            {/* run a node → free inference (new) */}
            <section id="self-host" className="scroll-mt-24">
              <Pill tone="brand">
                <CoinIcon width={14} height={14} />
                {a.catRunNode}
              </Pill>
              <h2 className="mt-4 text-2xl font-semibold text-ink sm:text-3xl">{a.selfHostTitle}</h2>
              <p className="mt-4 rounded-2xl bg-brand-500/8 p-5 text-lg font-medium leading-relaxed text-ink ring-1 ring-brand-500/25">
                {a.selfHostPitch}
              </p>
              <p className="mt-4 max-w-3xl text-ink-muted">{a.selfHostBody}</p>
              <div className="mt-6">
                <CodeTabs snippets={NODE_SETUP_SNIPPETS} copy={t.actions.copy} copied={t.actions.copied} />
              </div>
              <p className="mt-4 max-w-3xl rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
                {a.selfHostNote}
              </p>
            </section>

            {/* create a test wallet */}
            <section id="wallet" className="mt-16 max-w-3xl scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.walletTitle}</h2>
              <p className="mt-3 text-ink-muted">{a.walletLede}</p>
              <div className="mt-6">
                <CodeCard code={WALLET_SNIPPET} copy={t.actions.copy} copied={t.actions.copied} />
              </div>
              <p className="mt-4 rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
                {a.walletNote}
              </p>
            </section>

            {/* faucet */}
            <section id="faucet" className="mt-16 max-w-3xl scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.faucetTitle}</h2>
              <p className="mt-3 text-ink-muted">{a.faucetLede}</p>
              <div className="mt-6">
                <FaucetWidget a={a} />
              </div>
            </section>

            {/* flow */}
            <section id="flow" className="mt-16 scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.flowTitle}</h2>
              <div className="mt-8 grid gap-4 sm:grid-cols-2">
                {a.flowSteps.map((s) => (
                  <Card key={s.n} className="h-full p-5">
                    <span className="grid h-9 w-9 place-items-center rounded-full bg-brand-500/12 font-mono text-sm font-bold text-brand-300 ring-1 ring-brand-500/25">
                      {s.n}
                    </span>
                    <h3 className="mt-4 font-semibold text-ink">{s.title}</h3>
                    <p className="mt-2 text-sm leading-relaxed text-ink-muted">{s.body}</p>
                  </Card>
                ))}
              </div>
            </section>

            {/* Inference API — native OpenAI endpoint, prepaid credits */}
            <section id="inference-api" className="mt-16 scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.inferenceApiTitle}</h2>
              <p className="mt-3 max-w-3xl text-ink-muted">{a.inferenceApiLede}</p>
              <div className="mt-8 grid gap-4 sm:grid-cols-3">
                {a.inferenceApiSteps.map((s, i) => (
                  <Card key={s.title} className="h-full p-5">
                    <span className="grid h-9 w-9 place-items-center rounded-full bg-brand-500/12 font-mono text-sm font-bold text-brand-300 ring-1 ring-brand-500/25">
                      {i + 1}
                    </span>
                    <h3 className="mt-4 font-semibold text-ink">{s.title}</h3>
                    <p className="mt-2 text-sm leading-relaxed text-ink-muted">{s.body}</p>
                  </Card>
                ))}
              </div>

              <p className="mt-8 text-sm font-medium text-ink">{a.inferenceApiKeyLede}</p>
              <div className="mt-3">
                <CodeCard code={KEY_ISSUE_SNIPPET} copy={t.actions.copy} copied={t.actions.copied} />
              </div>

              <p className="mt-8 text-sm font-medium text-ink">{a.keyIssueTitle}</p>
              <p className="mt-1 max-w-3xl text-sm leading-relaxed text-ink-muted">{a.keyIssueLede}</p>
              <div className="mt-3">
                <ApiKeyIssuer a={a} copy={t.actions.copy} copied={t.actions.copied} />
              </div>

              <p className="mt-8 text-sm font-medium text-ink">{a.selfIssueTitle}</p>
              <p className="mt-1 max-w-3xl text-sm leading-relaxed text-ink-muted">{a.selfIssueLede}</p>
              <div className="mt-3">
                <CodeTabs snippets={SELF_ISSUE_SNIPPETS} copy={t.actions.copy} copied={t.actions.copied} />
              </div>

              <p className="mt-8 text-sm font-medium text-ink">{a.inferenceApiCallLede}</p>
              <div className="mt-3">
                <CodeTabs snippets={INFERENCE_API_SNIPPETS} copy={t.actions.copy} copied={t.actions.copied} />
              </div>
              <div className="mt-3">
                <CodeCard code={STREAM_CURL_SNIPPET} copy={t.actions.copy} copied={t.actions.copied} />
              </div>

              <h3 className="mt-10 text-lg font-semibold text-ink">{a.inferenceApiRefTitle}</h3>
              <div className="mt-4 overflow-x-auto rounded-2xl ring-1 ring-line">
                <table className="w-full min-w-[28rem] text-left text-sm">
                  <tbody>
                    {INFERENCE_API_REF.map((row) => (
                      <tr key={row.key} className="border-t border-line first:border-t-0">
                        <td className="whitespace-nowrap px-4 py-3 font-mono text-[0.68rem] font-semibold uppercase tracking-wider text-ink-faint">
                          {a.inferenceApiRef[row.key]}
                        </td>
                        <td className="px-4 py-3 font-mono text-[12.5px] text-ink-muted">{row.value}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <p className="mt-4 max-w-3xl rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
                {a.inferenceApiThinkNote}
              </p>
            </section>

            {/* API reference */}
            <section id="reference" className="mt-16 scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.refTitle}</h2>
              <div className="mt-8 grid gap-4">
                {API_REFS.map((r) => (
                  <RefCard
                    key={r.path + r.method}
                    ref={r}
                    prose={a[r.reqKey]}
                    reqLabel={a.requestLabel}
                    resLabel={a.responseLabel}
                  />
                ))}
              </div>
            </section>

            {/* end-to-end code */}
            <section id="example" className="mt-16 scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.codeTitle}</h2>
              <p className="mt-3 max-w-3xl text-ink-muted">{a.codeLede}</p>
              <div className="mt-8">
                <CodeTabs snippets={SNIPPETS} copy={t.actions.copy} copied={t.actions.copied} />
              </div>
            </section>

            {/* OpenAI-compatible adapter */}
            <section id="adapter" className="mt-16 scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.adapterTitle}</h2>
              <p className="mt-3 max-w-3xl text-ink-muted">{a.adapterLede}</p>
              <div className="mt-8">
                <CodeCard code={ADAPTER_SNIPPET} copy={t.actions.copy} copied={t.actions.copied} />
              </div>
              <p className="mt-4 max-w-3xl rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
                {a.adapterNote}
              </p>
            </section>

            {/* prerequisites */}
            <section id="prereqs" className="mt-16 max-w-3xl scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.prereqTitle}</h2>
              <ul className="mt-6 space-y-3">
                {a.prereqs.map((p) => (
                  <li key={p} className="flex items-start gap-3 text-sm leading-relaxed text-ink-muted">
                    <CheckIcon width={18} height={18} className="mt-0.5 shrink-0 text-brand-400" />
                    {p}
                  </li>
                ))}
              </ul>
            </section>

            {/* security */}
            <section id="security" className="mt-16 max-w-3xl scroll-mt-24">
              <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.securityTitle}</h2>
              <ul className="mt-6 space-y-3">
                {a.security.map((p) => (
                  <li key={p} className="flex items-start gap-3 text-sm leading-relaxed text-ink-muted">
                    <CheckIcon width={18} height={18} className="mt-0.5 shrink-0 text-brand-400" />
                    {p}
                  </li>
                ))}
              </ul>
            </section>
          </div>
        </div>

        {/* CTA */}
        <div className="mt-16 rounded-3xl bg-surface-2/50 p-8 ring-1 ring-line sm:p-12">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{a.ctaTitle}</h2>
          <p className="mt-3 max-w-2xl text-ink-muted">{a.ctaBody}</p>
          <div className="mt-7">
            <Button href={LINKS.github} variant="primary" size="lg" target="_blank" rel="noreferrer">
              <GithubIcon width={18} height={18} />
              {a.ctaButton}
              <ArrowIcon width={16} height={16} className="transition-transform group-hover:translate-x-0.5" />
            </Button>
          </div>
        </div>
      </Container>

      <Footer />
    </div>
  );
}
