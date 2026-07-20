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
  NetworkIcon,
  TerminalIcon,
  GaugeIcon,
  LayersIcon,
  CheckIcon,
} from "./icons";
import { LINKS } from "../content";
import { useT } from "../i18n/provider";

/* ==========================================================================
   Careers page (/careers) — fully i18n via t.careers.* (all 9 languages).
   This public page carries the *marketing / growth* opening only (per owner
   decision 2026-07-14 — no "founding member" wording); the co-founder-track
   pitch lives in direct recruiting channels (Superteam Talent, Colosseum,
   YC CFM), not here.
   Grounded in docs/team-recruiting-strategy.md; honors homepage-brief.md
   compliance constraints (devnet utility token, no financial-return promises).
   ========================================================================== */

const APPLY_MAILTO =
  "mailto:louisevandan@gmail.com?subject=Kvasir%20%E2%80%94%20Marketing%20%26%20Growth";

const OWN_ICONS = [BoltIcon, NetworkIcon, TerminalIcon, GaugeIcon, LayersIcon];

/* ---- slim top bar (landing nav anchors don't apply here) ---- */
function CareersHeader({ home }: { home: string }) {
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
        <a
          href="/"
          className="rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-400"
        >
          <Wordmark size={30} />
        </a>
        <div className="flex items-center gap-2">
          <a
            href="/"
            className="rounded-full px-3 py-2 text-sm text-ink-muted transition-colors hover:text-ink"
          >
            {home}
          </a>
          <LanguageSwitcher />
          <KvrExplorer className="hidden sm:inline-flex" />
        </div>
      </Container>
    </header>
  );
}

function FactRow({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5 sm:flex-row sm:gap-4">
      <span className="w-40 shrink-0 text-xs font-semibold uppercase tracking-wider text-ink-faint">
        {k}
      </span>
      <span className="text-sm text-ink-muted">{v}</span>
    </div>
  );
}

export default function CareersPage() {
  const t = useT();
  const c = t.careers;

  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  useEffect(() => {
    document.title = c.docTitle;
  }, [c.docTitle]);

  return (
    <div className="min-h-screen">
      <CareersHeader home={t.guide.home} />

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
        <div className="max-w-3xl">
          <Pill tone="brand">
            <BoltIcon width={14} height={14} />
            {c.pill}
          </Pill>
          <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
            {c.headline1}
            <br />
            <span className="text-brand-gradient">{c.headline2}</span>
          </h1>
          <p className="mt-5 text-lg leading-relaxed text-ink-muted">{c.sub}</p>

          <div className="mt-8 space-y-3 rounded-2xl bg-surface-2/60 p-6 ring-1 ring-line">
            <FactRow k={c.factRole} v={c.factRoleV} />
            <FactRow k={c.factLocation} v={c.factLocationV} />
            <FactRow k={c.factComp} v={c.factCompV} />
            <FactRow
              k={c.factEngine}
              v={
                <a
                  href={LINKS.github}
                  target="_blank"
                  rel="noreferrer"
                  className="text-brand-300 hover:underline"
                >
                  github.com/louisevandan/kvasir-net
                </a>
              }
            />
          </div>
        </div>

        {/* what's live */}
        <div className="mt-16 max-w-3xl">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{c.liveTitle}</h2>
          <p className="mt-3 text-ink-muted">{c.liveLede}</p>
          <ul className="mt-5 space-y-3">
            {c.liveProof.map((p) => (
              <li
                key={p}
                className="flex items-start gap-3 text-sm leading-relaxed text-ink-muted"
              >
                <CheckIcon width={18} height={18} className="mt-0.5 shrink-0 text-brand-400" />
                {p}
              </li>
            ))}
          </ul>
          <p className="mt-5 rounded-lg bg-caution/8 px-3 py-2 text-xs leading-relaxed text-caution ring-1 ring-caution/20">
            {c.devnetNote}
          </p>
        </div>

        {/* what you'll own */}
        <div className="mt-16">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{c.ownsTitle}</h2>
          <p className="mt-3 max-w-3xl text-ink-muted">{c.ownsLede}</p>
          <div className="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {c.owns.map((o, i) => {
              const Icon = OWN_ICONS[i];
              return (
                <Card key={o.title} interactive className="h-full p-6">
                  <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-brand-500/10 text-brand-400 ring-1 ring-brand-500/20">
                    <Icon width={20} height={20} />
                  </div>
                  <h3 className="mt-4 font-semibold text-ink">{o.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-ink-muted">{o.body}</p>
                </Card>
              );
            })}
          </div>
        </div>

        {/* who we're looking for */}
        <div className="mt-16 max-w-3xl">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{c.profileTitle}</h2>
          <ul className="mt-6 space-y-3">
            {c.profile.map((p) => (
              <li
                key={p}
                className="flex items-start gap-3 text-sm leading-relaxed text-ink-muted"
              >
                <CheckIcon width={18} height={18} className="mt-0.5 shrink-0 text-brand-400" />
                {p}
              </li>
            ))}
          </ul>
        </div>

        {/* process */}
        <div className="mt-16 max-w-3xl">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{c.processTitle}</h2>
          <p className="mt-3 text-ink-muted">{c.processLede}</p>
          <div className="mt-8 space-y-0">
            {c.process.map((s, i) => (
              <div key={s.title} className="grid gap-5 sm:grid-cols-[auto_1fr] sm:gap-7">
                <div className="flex sm:flex-col sm:items-center">
                  <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full bg-brand-500/12 text-base font-bold text-brand-300 ring-1 ring-brand-500/25">
                    {i + 1}
                  </span>
                  {i < c.process.length - 1 && (
                    <span className="ml-4 mt-0 hidden w-px flex-1 bg-line sm:ml-0 sm:mt-2 sm:block" />
                  )}
                </div>
                <div className="pb-8">
                  <h3 className="text-lg font-semibold text-ink">{s.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-ink-muted">{s.body}</p>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* apply CTA */}
        <div className="mt-10 rounded-3xl bg-surface-2/50 p-8 ring-1 ring-line sm:p-12">
          <h2 className="text-2xl font-semibold text-ink sm:text-3xl">{c.applyTitle}</h2>
          <p className="mt-3 max-w-2xl text-ink-muted">{c.applyBody}</p>
          <div className="mt-7 flex flex-col gap-3 sm:flex-row">
            <Button href={APPLY_MAILTO} variant="primary" size="lg">
              {c.applyCta} — louisevandan@gmail.com
              <ArrowIcon
                width={16}
                height={16}
                className="transition-transform group-hover:translate-x-0.5"
              />
            </Button>
            <Button href={LINKS.github} variant="secondary" size="lg" target="_blank" rel="noreferrer">
              <GithubIcon width={18} height={18} />
              {c.readCode}
            </Button>
          </div>
        </div>
      </Container>

      <Footer />
    </div>
  );
}
