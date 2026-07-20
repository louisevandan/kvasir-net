import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Container, Card, Button, Pill } from "./ui";
import { Wordmark } from "./Logo";
import LanguageSwitcher from "./LanguageSwitcher";
import KvrExplorer from "./KvrExplorer";
import Footer from "./Footer";
import {
  BoltIcon,
  GithubIcon,
  ArrowIcon,
  WalletIcon,
  GpuIcon,
  CoinIcon,
} from "./icons";
import { LINKS, DOWNLOADS } from "../content";
import { useT, useLang } from "../i18n/provider";
import type { Dict } from "../i18n/types";

/* ==========================================================================
   Node-operator guide page (/run-node). Two platforms: Desktop / Mobile app.
   Fully i18n via t.guide.* (all 9 languages). Screenshots are per-language under
   /guide/<lang>/{desktop,mobile}/… (the app UI is captured in that language).
   Download + store links come from content.ts → DOWNLOADS (empty = "coming soon").
   ========================================================================== */

type Guide = Dict["guide"];
type Platform = "desktop" | "mobile";

const fill = (s: string, v: string) => s.replace("{0}", v);

/* ---- slim top bar (own header — landing nav anchors don't apply here) ---- */
function GuideHeader({ g }: { g: Guide }) {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 12);
    on();
    window.addEventListener("scroll", on, { passive: true });
    return () => window.removeEventListener("scroll", on);
  }, []);
  return (
    <header
      className={`fixed inset-x-0 top-0 z-50 transition-colors duration-300 ${
        scrolled ? "glass border-b border-line" : "border-b border-transparent"
      }`}
    >
      <Container className="flex h-16 items-center justify-between gap-3">
        <a href="/" className="rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-400">
          <Wordmark size={30} />
        </a>
        <div className="flex items-center gap-2">
          <a href="/" className="rounded-full px-3 py-2 text-sm text-ink-muted transition-colors hover:text-ink">
            {g.home}
          </a>
          <LanguageSwitcher />
          <KvrExplorer className="hidden sm:inline-flex" />
        </div>
      </Container>
    </header>
  );
}

/* ---- store / download button (renders a disabled "coming soon" if href empty) ---- */
function StoreButton({ href, top, main, sub, soon }: { href: string; top: string; main: string; sub?: string; soon: string }) {
  const ready = !!href;
  const inner = (
    <span className="flex items-center gap-3">
      <span className="text-2xl leading-none">{sub}</span>
      <span className="flex flex-col text-left leading-tight">
        <span className="text-[0.7rem] uppercase tracking-wide text-ink-faint">{top}</span>
        <span className="text-[0.95rem] font-semibold text-ink">{main}</span>
      </span>
    </span>
  );
  if (!ready) {
    return (
      <span className="inline-flex items-center gap-3 rounded-xl bg-surface-2/60 px-5 py-3 ring-1 ring-line opacity-70">
        {inner}
        <span className="ml-1 rounded-full bg-caution/10 px-2 py-0.5 text-[0.65rem] font-medium text-caution ring-1 ring-caution/25">
          {soon}
        </span>
      </span>
    );
  }
  return (
    <a href={href} target="_blank" rel="noreferrer" className="inline-flex items-center gap-3 rounded-xl bg-surface-2 px-5 py-3 ring-1 ring-line transition-all hover:-translate-y-0.5 hover:ring-brand-500/40">
      {inner}
    </a>
  );
}

/* ---- a numbered step with optional media ---- */
function Step({ n, title, children, media }: { n: number; title: string; children: ReactNode; media?: ReactNode }) {
  return (
    <div className="grid gap-6 sm:grid-cols-[auto_1fr] sm:gap-8">
      <div className="flex sm:flex-col sm:items-center">
        <span className="grid h-11 w-11 shrink-0 place-items-center rounded-full bg-brand-500/12 text-lg font-bold text-brand-300 ring-1 ring-brand-500/25">
          {n}
        </span>
        <span className="ml-4 mt-0 hidden w-px flex-1 bg-line sm:ml-0 sm:mt-2 sm:block" />
      </div>
      <div className="pb-10">
        <h3 className="text-xl font-semibold text-ink">{title}</h3>
        <div className="mt-3 space-y-3 text-[0.97rem] leading-relaxed text-ink-muted">{children}</div>
        {media && <div className="mt-5">{media}</div>}
      </div>
    </div>
  );
}

/* ---- a desktop screenshot (rounded, framed) ---- */
function Shot({ src, alt, w = "max-w-sm" }: { src: string; alt: string; w?: string }) {
  return (
    <figure className={`${w}`}>
      <img src={src} alt={alt} loading="lazy" className="w-full rounded-xl ring-1 ring-line" />
      <figcaption className="mt-2 text-xs text-ink-faint">{alt}</figcaption>
    </figure>
  );
}

/* ---- a phone screenshot (shared by iOS + Android — the apps share one UI) ---- */
function Phone({ src, alt }: { src: string; alt: string }) {
  return (
    <figure className="w-[230px]">
      <img src={src} alt={alt} loading="lazy" className="w-full rounded-[1.4rem] ring-1 ring-line" />
      <figcaption className="mt-2 text-center text-xs text-ink-faint">{alt}</figcaption>
    </figure>
  );
}

/* ---- highlighted callout (brand / caution tones use literal classes for JIT) ---- */
function Callout({ tone = "brand", title, children }: { tone?: "brand" | "caution"; title?: string; children: ReactNode }) {
  const cls = tone === "caution" ? "bg-caution/8 ring-caution/25" : "bg-brand-500/8 ring-brand-500/25";
  return (
    <div className={`rounded-xl ${cls} p-4 ring-1`}>
      {title && <div className="mb-1.5 text-sm font-semibold text-ink">{title}</div>}
      <div className="space-y-1.5 text-sm leading-relaxed text-ink-muted [&_a]:text-brand-300 [&_a:hover]:underline [&_code]:rounded [&_code]:bg-surface-2 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:text-[0.82em] [&_code]:text-ink">
        {children}
      </div>
    </div>
  );
}

/* devnet SOL faucet guidance — reused on desktop + mobile funding steps. */
function FaucetNote({ g }: { g: Guide }) {
  return (
    <Callout title={g.faucetTitle}>
      <p>{g.faucetIntro}</p>
      <ul className="ml-4 list-disc space-y-1">
        <li><a href="https://faucet.solana.com" target="_blank" rel="noreferrer">{g.faucetWeb}</a></li>
        <li><code>{g.faucetCli}</code></li>
        <li>{g.faucetAlt} (<a href="https://faucet.quicknode.com/solana/devnet" target="_blank" rel="noreferrer">QuickNode</a> · <a href="https://solfaucet.com" target="_blank" rel="noreferrer">SolFaucet</a>)</li>
      </ul>
      <p className="text-ink-faint">{g.faucetKvr}</p>
    </Callout>
  );
}

const CLI = `# linkcpp checkout · backend = auto|cuda|rocm|metal|cpu
bash scripts/build-node-runtime.sh auto

LINKCPP_HUB_URL=https://hub.kvasir-ai.net \\
LINKCPP_NODE_OWNER=<your wallet address> \\
  bash scripts/run-node-agent.sh auto`;

/* ============================ platform panels ============================ */

function DesktopGuide({ g, base }: { g: Guide; base: string }) {
  const d = `${base}/desktop`;
  const media: (ReactNode | null)[] = [
    null,
    null,
    <div className="flex flex-wrap gap-4">
      <Shot src={`${d}/01-welcome.png`} alt={g.capWelcome} w="max-w-xs" />
      <Shot src={`${d}/02-recovery.png`} alt={g.capRecovery} w="max-w-xs" />
      <Shot src={`${d}/03-passphrase.png`} alt={g.capPassphrase} w="max-w-xs" />
    </div>,
    <div className="flex flex-wrap gap-4">
      <Shot src={`${d}/04-receive.png`} alt={g.capReceive} w="max-w-[15rem]" />
      <Shot src={`${d}/05-balances.png`} alt={g.capBalances} w="max-w-[15rem]" />
      <Shot src={`${d}/06-staking.png`} alt={g.capStaking} w="max-w-md" />
    </div>,
    <div className="flex flex-wrap gap-4">
      <Shot src={`${d}/07-backend.png`} alt={g.capBackend} w="max-w-sm" />
      <Shot src={`${d}/08-mode.png`} alt={g.capMode} w="max-w-sm" />
    </div>,
    <div className="space-y-4">
      <Shot src={`${d}/09-runlive.png`} alt={g.capRunlive} w="max-w-sm" />
      <pre className="overflow-x-auto rounded-xl bg-surface-2/70 p-4 text-[0.8rem] leading-relaxed text-ink ring-1 ring-line">
        <code>{CLI}</code>
      </pre>
    </div>,
    <div className="flex flex-wrap gap-4">
      <Shot src={`${d}/10-nodes.png`} alt={g.capNodes} w="max-w-md" />
      <Shot src={`${d}/11-claim.png`} alt={g.capClaim} w="max-w-md" />
    </div>,
  ];
  return (
    <div className="mt-10">
      <Card className="p-6 sm:p-8">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <div className="text-sm font-semibold text-ink">{g.desktopTitle}</div>
            <div className="mt-1 text-sm text-ink-muted">{g.desktopSub}</div>
          </div>
          <div className="flex flex-wrap gap-3">
            <StoreButton href={DOWNLOADS.desktopMac} top={g.download} main="macOS" sub="" soon={g.soon} />
            <StoreButton href={DOWNLOADS.desktopWin} top={g.download} main="Windows" sub="⊞" soon={g.soon} />
            <StoreButton href={DOWNLOADS.desktopLinux} top={g.download} main="Linux" sub="🐧" soon={g.soon} />
          </div>
        </div>
      </Card>

      <div className="mt-12">
        {g.desktop.map((step, i) => (
          <Step key={i} n={i + 1} title={step.title} media={media[i]}>
            <p>{step.body}</p>
            {step.body2 && <p>{step.body2}</p>}
            {i === 3 && <FaucetNote g={g} />}
          </Step>
        ))}
      </div>
    </div>
  );
}

function MobileGuide({ g, base }: { g: Guide; base: string }) {
  const m = `${base}/mobile`;
  const store = "App Store / Google Play";
  const media: (ReactNode | null)[] = [
    null,
    <Phone src={`${m}/wallet.png`} alt={g.capWallet} />,
    <Phone src={`${m}/nodeset.png`} alt={g.capNodeset} />,
    <Phone src={`${m}/staking.png`} alt={g.capStakingM} />,
  ];
  return (
    <div className="mt-10">
      <Card className="p-6 sm:p-8">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <div className="text-sm font-semibold text-ink">{fill(g.mobileTitle, "Mobile")}</div>
            <div className="mt-1 text-sm text-ink-muted">{g.mobileSub}</div>
          </div>
          <div className="flex flex-wrap gap-3">
            <StoreButton href={DOWNLOADS.appStore} top="Download on the" main="App Store" sub="" soon={g.soon} />
            <StoreButton href={DOWNLOADS.googlePlay} top="GET IT ON" main="Google Play" sub="▶" soon={g.soon} />
          </div>
        </div>
      </Card>

      <div className="mt-12">
        {g.mobile.map((step, i) => (
          <Step key={i} n={i + 1} title={step.title} media={media[i]}>
            <p>{fill(step.body, store)}</p>
            {i === 3 && <FaucetNote g={g} />}
            {step.note && <p className="text-sm text-ink-faint">※ {step.note}</p>}
          </Step>
        ))}
      </div>
    </div>
  );
}

/* ================================ page ================================ */

export default function RunNodePage() {
  const g = useT().guide;
  const { lang } = useLang();
  const base = `/guide/${lang}`;
  const [platform, setPlatform] = useState<Platform>("desktop");

  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  useEffect(() => {
    document.title = `${g.eyebrow} — Kvasir`;
  }, [g.eyebrow]);

  const tabs: { id: Platform; label: string; icon: string }[] = [
    { id: "desktop", label: g.tabDesktop, icon: "🖥" },
    { id: "mobile", label: g.tabMobile, icon: "📱" },
  ];

  return (
    <div className="min-h-screen">
      <GuideHeader g={g} />

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
        {/* intro */}
        <div className="max-w-2xl">
          <Pill tone="brand">
            <BoltIcon width={14} height={14} />
            {g.eyebrow}
          </Pill>
          <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
            {g.headline1}
            <br />
            <span className="text-brand-gradient">{g.headline2}</span>
          </h1>
          <p className="mt-5 text-lg leading-relaxed text-ink-muted">{g.sub}</p>
          <div className="mt-5 flex flex-wrap items-center gap-3 text-sm text-ink-faint">
            <span className="inline-flex items-center gap-1.5"><WalletIcon width={15} height={15} className="text-positive" /> {g.badgeCustody}</span>
            <span className="inline-flex items-center gap-1.5"><GpuIcon width={15} height={15} className="text-positive" /> {g.badgeDevices}</span>
            <span className="inline-flex items-center gap-1.5"><CoinIcon width={15} height={15} className="text-positive" /> {g.badgeToken}</span>
          </div>
          <p className="mt-4 rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
            {g.devnetNote}
          </p>
          <div className="mt-4">
            <Callout tone="caution" title={g.reqTitle}>
              <p>{g.reqBody}</p>
            </Callout>
          </div>
        </div>

        {/* platform tabs */}
        <div className="mt-12 inline-flex flex-wrap gap-2 rounded-2xl bg-surface-2/60 p-1.5 ring-1 ring-line">
          {tabs.map((p) => (
            <button
              key={p.id}
              onClick={() => setPlatform(p.id)}
              className={`inline-flex items-center gap-2 rounded-xl px-5 py-2.5 text-sm font-semibold transition-all ${
                platform === p.id ? "bg-brand-500 text-white shadow-glow" : "text-ink-muted hover:text-ink"
              }`}
            >
              <span className="text-base leading-none">{p.icon}</span>
              {p.label}
            </button>
          ))}
        </div>

        {platform === "desktop" && <DesktopGuide g={g} base={base} />}
        {platform === "mobile" && <MobileGuide g={g} base={base} />}

        {/* bottom CTA */}
        <div className="mt-16 flex flex-wrap gap-3">
          <Button href={LINKS.github} variant="secondary" size="lg" target="_blank" rel="noreferrer">
            <GithubIcon width={18} height={18} /> {g.viewGithub}
          </Button>
          <Button href="/" variant="ghost" size="lg">
            {g.home} <ArrowIcon width={16} height={16} />
          </Button>
        </div>
      </Container>

      <Footer />
    </div>
  );
}
