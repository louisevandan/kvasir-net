import { useState, useEffect } from "react";
import Nav from "./components/Nav";
import Hero from "./components/Hero";
import Footer from "./components/Footer";
import { LogoMark } from "./components/Logo";
import {
  Container,
  Section,
  SectionHeading,
  Card,
  Pill,
  Button,
  Reveal,
} from "./components/ui";
import {
  SplitIcon,
  NetworkIcon,
  CoinIcon,
  GpuIcon,
  GaugeIcon,
  ShieldIcon,
  WalletIcon,
  CpuIcon,
  LayersIcon,
  BoltIcon,
  TerminalIcon,
  CheckIcon,
  ArrowIcon,
  GithubIcon,
  RingIcon,
} from "./components/icons";
import {
  LINKS,
  CODE_SNIPPET,
  REWARD_EXPR,
  PERF_TIERS,
  LINKCPP_TAGLINE,
  DEVICE_META,
  PROOF_STATS,
  ROADMAP_TONE,
} from "./content";
import { useT } from "./i18n/provider";

const STEP_ICONS = [SplitIcon, NetworkIcon, CoinIcon];
const CONTRIB_ICONS = [CpuIcon, LayersIcon, GaugeIcon, ShieldIcon];
const TECH_ICONS = [RingIcon, CpuIcon, LayersIcon, ShieldIcon];
const ROLE_ICONS = [CpuIcon, TerminalIcon, RingIcon];
const DEVICE_ICONS: Record<string, typeof GpuIcon> = {
  GPU: GpuIcon,
  CPU: CpuIcon,
  NPU: BoltIcon,
  Mobile: NetworkIcon,
};

/* -------------------------------------------------------------------------- */
/* Thesis — decentralized AI, beyond the monopoly                              */
/* -------------------------------------------------------------------------- */
function XMark() {
  return (
    <svg
      width={18}
      height={18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      className="mt-0.5 shrink-0 text-ink-faint"
    >
      <path d="M7 7l10 10M17 7L7 17" />
    </svg>
  );
}

function Thesis() {
  const t = useT();
  return (
    <Section id="why" className="relative overflow-hidden border-t border-line">
      <div
        aria-hidden
        className="pointer-events-none absolute -top-24 left-1/2 -z-10 h-[420px] w-[820px] -translate-x-1/2 blur-3xl"
        style={{
          background:
            "radial-gradient(50% 50% at 50% 40%, rgba(255,61,139,0.10), transparent 70%)",
        }}
      />
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.thesis.eyebrow}
            title={t.thesis.title}
            lede={t.thesis.lede}
            align="center"
          />
        </Reveal>

        <div className="mx-auto mt-14 grid max-w-4xl gap-4 lg:grid-cols-2">
          {/* Centralized AI */}
          <Reveal>
            <Card className="h-full p-7 sm:p-8">
              <div className="text-sm font-semibold uppercase tracking-wider text-ink-faint">
                {t.thesis.centralizedLabel}
              </div>
              <ul className="mt-5 space-y-3.5">
                {t.thesis.centralizedPoints.map((p) => (
                  <li
                    key={p}
                    className="flex items-start gap-3 text-sm text-ink-muted"
                  >
                    <XMark />
                    {p}
                  </li>
                ))}
              </ul>
            </Card>
          </Reveal>

          {/* Kvasir */}
          <Reveal delay={90}>
            <div className="relative h-full">
              <div
                aria-hidden
                className="pointer-events-none absolute inset-0 -z-10 rounded-2xl blur-2xl"
                style={{ background: "rgba(255,61,139,0.10)" }}
              />
              <div className="h-full rounded-2xl bg-brand-500/[0.05] p-7 shadow-glow ring-1 ring-brand-500/30 sm:p-8">
                <div className="flex items-center gap-2 text-sm font-semibold uppercase tracking-wider text-brand-300">
                  <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse-glow" />
                  {t.thesis.kvasirLabel}
                </div>
                <ul className="mt-5 space-y-3.5">
                  {t.thesis.kvasirPoints.map((p) => (
                    <li key={p} className="flex items-start gap-3 text-sm text-ink">
                      <CheckIcon
                        width={18}
                        height={18}
                        className="mt-0.5 shrink-0 text-brand-400"
                      />
                      {p}
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </Reveal>
        </div>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* The name — Kvasir origin                                                    */
/* -------------------------------------------------------------------------- */
function Origin() {
  const t = useT();
  return (
    <Section id="name" className="relative overflow-hidden border-t border-line">
      <div
        aria-hidden
        className="pointer-events-none absolute -top-20 right-1/4 -z-10 h-[380px] w-[560px] blur-3xl"
        style={{
          background:
            "radial-gradient(50% 50% at 50% 40%, rgba(168,85,247,0.10), transparent 70%)",
        }}
      />
      <Container>
        <Reveal>
          <SectionHeading eyebrow={t.origin.eyebrow} title={t.origin.title} align="center" />
        </Reveal>

        <div className="mt-14 grid gap-4 lg:grid-cols-2 lg:items-stretch">
          {/* the myth */}
          <Reveal>
            <div className="relative h-full overflow-hidden rounded-2xl bg-surface-2/60 p-8 ring-1 ring-line">
              <span
                aria-hidden
                className="pointer-events-none absolute -left-2 -top-6 select-none font-serif text-[9rem] leading-none text-brand-500/15"
              >
                &ldquo;
              </span>
              <div className="relative">
                <div className="mb-4 flex items-center gap-2">
                  <LogoMark size={26} />
                  <span className="text-xs font-semibold uppercase tracking-[0.2em] text-brand-400">
                    {t.origin.mythLabel}
                  </span>
                </div>
                <p className="text-lg leading-relaxed text-ink">{t.origin.myth}</p>
              </div>
            </div>
          </Reveal>

          {/* why we chose it */}
          <Reveal delay={90}>
            <div className="flex h-full flex-col gap-3">
              <div className="mb-1 text-xs font-semibold uppercase tracking-[0.2em] text-brand-400">
                {t.origin.whyLabel}
              </div>
              {t.origin.mappings.map((m) => (
                <div
                  key={m.from}
                  className="rounded-2xl bg-surface-2/50 p-5 ring-1 ring-line"
                >
                  <div className="text-sm font-medium text-brand-300">{m.from}</div>
                  <div className="mt-2 flex items-start gap-2 text-sm leading-relaxed text-ink-muted">
                    <ArrowIcon
                      width={16}
                      height={16}
                      className="mt-0.5 shrink-0 text-ink-faint"
                    />
                    {m.to}
                  </div>
                </div>
              ))}
            </div>
          </Reveal>
        </div>

        <Reveal>
          <p className="mt-8 text-center font-mono text-xs text-ink-faint">
            {t.origin.footnote}
          </p>
        </Reveal>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* How it works                                                                */
/* -------------------------------------------------------------------------- */
function HowItWorks() {
  const t = useT();
  return (
    <Section id="how" className="relative">
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.how.eyebrow}
            title={t.how.title}
            lede={t.how.lede}
          />
        </Reveal>

        <div className="mt-14 grid gap-5 md:grid-cols-3">
          {t.how.steps.map((step, i) => {
            const Icon = STEP_ICONS[i];
            return (
              <Reveal key={step.title} delay={i * 90}>
                <Card interactive className="h-full p-7">
                  <div className="flex items-center justify-between">
                    <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-brand-500/10 text-brand-400 ring-1 ring-brand-500/20">
                      <Icon width={22} height={22} />
                    </div>
                    <span className="font-mono text-sm text-ink-faint">0{i + 1}</span>
                  </div>
                  <h3 className="mt-5 text-xl font-semibold text-ink">{step.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-ink-muted">
                    {step.body}
                  </p>
                  <div className="mt-5 rounded-lg bg-surface-3/60 px-3 py-2 font-mono text-[11px] leading-relaxed text-brand-300 ring-1 ring-line">
                    {step.note}
                  </div>
                </Card>
              </Reveal>
            );
          })}
        </div>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Contributors                                                                */
/* -------------------------------------------------------------------------- */
function Contributors() {
  const t = useT();
  return (
    <Section id="contributors" className="relative border-t border-line">
      <Container>
        <div className="grid gap-14 lg:grid-cols-[0.9fr_1.1fr] lg:items-start">
          <Reveal>
            <Pill tone="brand">{t.contributors.pill}</Pill>
            <h2 className="mt-5 text-3xl font-semibold text-ink sm:text-4xl">
              {t.contributors.title}
            </h2>
            <p className="mt-4 text-lg leading-relaxed text-ink-muted">
              {t.contributors.lede}
            </p>
            <div className="mt-7 flex flex-col gap-3 sm:flex-row">
              <Button href={LINKS.runNode} variant="primary" size="lg">
                <BoltIcon width={18} height={18} />
                {t.actions.runNodeGuide}
              </Button>
              <Button
                href={LINKS.github}
                variant="secondary"
                size="lg"
                target="_blank"
                rel="noreferrer"
              >
                <GithubIcon width={18} height={18} />
                {t.actions.readDocs}
              </Button>
            </div>

            <div className="mt-8 flex items-center gap-2.5 rounded-xl bg-surface-2/60 px-4 py-3 text-sm text-ink-muted ring-1 ring-line">
              <ShieldIcon width={18} height={18} className="text-positive shrink-0" />
              {t.contributors.nonCustodial}
            </div>
          </Reveal>

          <div className="grid gap-4 sm:grid-cols-2">
            {t.contributors.points.map((p, i) => {
              const Icon = CONTRIB_ICONS[i];
              return (
                <Reveal key={p.title} delay={i * 80}>
                  <Card interactive className="h-full p-6">
                    <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-surface-3 text-brand-400 ring-1 ring-line">
                      <Icon width={20} height={20} />
                    </div>
                    <h3 className="mt-4 font-semibold text-ink">{p.title}</h3>
                    <p className="mt-2 text-sm leading-relaxed text-ink-muted">
                      {p.body}
                    </p>
                  </Card>
                </Reveal>
              );
            })}
          </div>
        </div>

        {/* supported devices */}
        <Reveal>
          <div className="mt-14">
            <div className="mb-4 flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.2em] text-brand-400">
              {t.contributors.devicesLabel}
            </div>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {DEVICE_META.map((d, i) => {
                const Icon = DEVICE_ICONS[d.name] ?? CpuIcon;
                const live = d.status === "live";
                return (
                  <div
                    key={d.name}
                    className="flex items-start gap-3 rounded-xl bg-surface-2/60 p-4 ring-1 ring-line"
                  >
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-surface-3 text-brand-400 ring-1 ring-line">
                      <Icon width={20} height={20} />
                    </div>
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-semibold text-ink">{d.name}</span>
                        <span
                          className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide ring-1 ${
                            live
                              ? "text-positive ring-positive/25 bg-positive/10"
                              : "text-caution ring-caution/25 bg-caution/10"
                          }`}
                        >
                          <span
                            className={`h-1.5 w-1.5 rounded-full ${
                              live ? "bg-positive" : "bg-caution"
                            }`}
                          />
                          {live ? t.contributors.statusLive : t.contributors.statusComing}
                        </span>
                      </div>
                      <div className="mt-0.5 font-mono text-xs text-ink-muted">
                        {t.contributors.deviceDetails[i]}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </Reveal>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Developers                                                                  */
/* -------------------------------------------------------------------------- */
function CodeBlock() {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(CODE_SNIPPET);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };

  return (
    <div className="overflow-hidden rounded-2xl bg-[#0b0c11] ring-1 ring-line">
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <div className="flex items-center gap-2">
          <span className="h-3 w-3 rounded-full bg-negative/70" />
          <span className="h-3 w-3 rounded-full bg-caution/70" />
          <span className="h-3 w-3 rounded-full bg-positive/70" />
          <span className="ml-2 font-mono text-xs text-ink-faint">
            {t.developers.codeHeader}
          </span>
        </div>
        <button
          onClick={copy}
          className="rounded-md px-2 py-1 font-mono text-xs text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
        >
          {copied ? t.actions.copied : t.actions.copy}
        </button>
      </div>
      <pre className="overflow-x-auto px-5 py-4 font-mono text-[12.5px] leading-relaxed text-ink-muted">
        <code>{CODE_SNIPPET}</code>
      </pre>
    </div>
  );
}

function Developers() {
  const t = useT();
  return (
    <Section id="developers" className="relative border-t border-line">
      <Container>
        <div className="grid gap-14 lg:grid-cols-[1fr_1fr] lg:items-center">
          <Reveal>
            <Pill tone="brand">{t.developers.pill}</Pill>
            <h2 className="mt-5 text-3xl font-semibold text-ink sm:text-4xl">
              {t.developers.title}
            </h2>
            <p className="mt-4 text-lg leading-relaxed text-ink-muted">
              {t.developers.lede}
            </p>
            <ul className="mt-7 space-y-3">
              {t.developers.points.map((p) => (
                <li key={p} className="flex items-start gap-3 text-sm text-ink-muted">
                  <CheckIcon
                    width={18}
                    height={18}
                    className="mt-0.5 shrink-0 text-brand-400"
                  />
                  <span className="font-mono text-[13px]">{p}</span>
                </li>
              ))}
            </ul>
            <div className="mt-8">
              <Button href={LINKS.api} variant="primary" size="lg">
                <TerminalIcon width={18} height={18} />
                {t.actions.getApiKey}
                <ArrowIcon
                  width={16}
                  height={16}
                  className="transition-transform group-hover:translate-x-0.5"
                />
              </Button>
            </div>
          </Reveal>

          <Reveal delay={100}>
            <CodeBlock />
          </Reveal>
        </div>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Token & rewards                                                             */
/* -------------------------------------------------------------------------- */
function Token() {
  const t = useT();
  return (
    <Section id="token" className="relative border-t border-line">
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.token.eyebrow}
            title={t.token.title}
            lede={t.token.lede}
            titleClassName="lg:whitespace-nowrap"
          />
        </Reveal>

        {/* facts */}
        <div className="mt-12 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {t.token.facts.map((f, i) => (
            <Reveal key={i} delay={i * 70}>
              <Card className="h-full p-6">
                <div className="text-xs font-medium uppercase tracking-wider text-ink-faint">
                  {f.k}
                </div>
                <div className="mt-2 text-2xl font-semibold text-ink">{f.v}</div>
                <div className="mt-1 text-xs text-ink-muted">{f.note}</div>
              </Card>
            </Reveal>
          ))}
        </div>

        {/* what it's for + custody */}
        <div className="mt-6 grid gap-4 lg:grid-cols-2">
          <Reveal>
            <Card className="h-full p-7">
              <div className="flex items-center gap-2 text-brand-400">
                <CoinIcon width={20} height={20} />
                <h3 className="font-semibold text-ink">{t.token.whatForTitle}</h3>
              </div>
              <p className="mt-4 text-sm leading-relaxed text-ink-muted">
                {t.token.whatForBody}
              </p>
              <div className="mt-5 flex flex-wrap gap-2">
                {t.token.whatForChips.map((p) => (
                  <span
                    key={p}
                    className="rounded-md bg-surface-3/70 px-2.5 py-1 font-mono text-xs text-ink-muted ring-1 ring-line"
                  >
                    {p}
                  </span>
                ))}
              </div>
            </Card>
          </Reveal>

          <Reveal delay={90}>
            <Card className="h-full p-7">
              <div className="flex items-center gap-2 text-brand-400">
                <WalletIcon width={20} height={20} />
                <h3 className="font-semibold text-ink">{t.token.custodyTitle}</h3>
              </div>
              <p className="mt-4 text-sm leading-relaxed text-ink-muted">
                {t.token.custodyBody}
              </p>
              <div className="mt-5 flex flex-wrap gap-2">
                {t.token.custodyChips.map((p) => (
                  <span
                    key={p}
                    className="rounded-md bg-surface-3/70 px-2.5 py-1 font-mono text-xs text-ink-muted ring-1 ring-line"
                  >
                    {p}
                  </span>
                ))}
              </div>
            </Card>
          </Reveal>
        </div>

        {/* devnet constraint banner */}
        <Reveal>
          <div className="mt-6 flex items-start gap-3 rounded-2xl bg-caution/[0.07] px-5 py-4 ring-1 ring-caution/25">
            <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-caution/15 text-caution">
              <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round">
                <path d="M12 8v5M12 16.5v.5" />
                <path d="M10.3 3.9 2.5 18a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" strokeWidth={1.5} />
              </svg>
            </span>
            <p className="text-sm leading-relaxed text-ink-muted">
              <strong className="font-semibold text-caution">
                {t.token.devnetStrong}
              </strong>{" "}
              {t.token.devnetBody}
            </p>
          </div>
        </Reveal>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Network roles & rewards                                                     */
/* -------------------------------------------------------------------------- */
function NetworkRoles() {
  const t = useT();
  return (
    <Section id="network" className="relative border-t border-line">
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.network.eyebrow}
            title={t.network.title}
            lede={t.network.lede}
          />
        </Reveal>

        {/* roles */}
        <div className="mt-12 grid gap-4 md:grid-cols-3">
          {t.network.roles.map((r, i) => {
            const Icon = ROLE_ICONS[i];
            return (
              <Reveal key={r.role} delay={i * 80}>
                <Card interactive className="flex h-full flex-col p-6">
                  <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-brand-500/10 text-brand-400 ring-1 ring-brand-500/20">
                    <Icon width={22} height={22} />
                  </div>
                  <h3 className="mt-4 font-semibold text-ink">{r.role}</h3>
                  <div className="text-xs font-medium text-brand-300">{r.tagline}</div>
                  <p className="mt-2 flex-1 text-sm leading-relaxed text-ink-muted">
                    {r.body}
                  </p>
                  <div className="mt-4 flex items-center gap-2 rounded-lg bg-surface-3/60 px-3 py-2 font-mono text-[11px] text-positive ring-1 ring-line">
                    <CoinIcon width={14} height={14} className="shrink-0" />
                    earns · {r.earns}
                  </div>
                </Card>
              </Reveal>
            );
          })}
        </div>

        {/* roles stack note */}
        <Reveal>
          <div className="mt-4 flex items-start gap-2.5 rounded-xl bg-surface-2/60 px-4 py-3 text-sm text-ink-muted ring-1 ring-line">
            <LayersIcon width={18} height={18} className="mt-0.5 shrink-0 text-brand-400" />
            {t.network.rolesNote}
          </div>
        </Reveal>

        {/* reward mechanics: formula + tiers */}
        <div className="mt-6 grid gap-4 lg:grid-cols-2">
          <Reveal>
            <Card className="h-full p-7">
              <div className="flex items-center gap-2 text-brand-400">
                <CoinIcon width={20} height={20} />
                <h3 className="font-semibold text-ink">{t.network.formulaTitle}</h3>
              </div>
              <div className="mt-5 space-y-3">
                {REWARD_EXPR.map((expr, i) => (
                  <div key={i}>
                    <div className="mb-1 text-xs font-medium uppercase tracking-wider text-ink-faint">
                      {t.network.formulaLabels[i]}
                    </div>
                    <div className="overflow-x-auto rounded-lg bg-surface-3/60 px-3 py-2.5 font-mono text-[12.5px] text-brand-300 ring-1 ring-line">
                      {expr}
                    </div>
                  </div>
                ))}
              </div>
            </Card>
          </Reveal>

          <Reveal delay={90}>
            <Card className="h-full p-7">
              <div className="flex items-center gap-2 text-brand-400">
                <GaugeIcon width={20} height={20} />
                <h3 className="font-semibold text-ink">{t.network.tiersTitle}</h3>
              </div>
              <p className="mt-3 text-sm leading-relaxed text-ink-muted">
                {t.network.tiersBody}
              </p>
              <div className="mt-4 space-y-2">
                {PERF_TIERS.map((tier) => (
                  <div
                    key={tier.tier}
                    className="flex items-center justify-between rounded-lg bg-surface-3/60 px-3 py-2 ring-1 ring-line"
                  >
                    <div className="flex items-center gap-3">
                      <span className="flex h-6 w-6 items-center justify-center rounded-md bg-brand-500/10 font-mono text-xs font-semibold text-brand-300 ring-1 ring-brand-500/25">
                        {tier.tier}
                      </span>
                      <span className="font-mono text-xs text-ink-muted">{tier.tps}</span>
                    </div>
                    <span className="font-mono text-sm font-semibold text-brand-300">
                      {tier.mult}
                    </span>
                  </div>
                ))}
              </div>
            </Card>
          </Reveal>
        </div>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Under the hood                                                              */
/* -------------------------------------------------------------------------- */
function Tech() {
  const t = useT();
  return (
    <Section id="tech" className="relative border-t border-line">
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.tech.eyebrow}
            title={t.tech.title}
            lede={t.tech.lede}
          />
        </Reveal>

        {/* linkcpp's own tagline, verbatim */}
        <Reveal>
          <figure className="mt-8 rounded-2xl bg-surface-2/50 p-6 ring-1 ring-line sm:p-7">
            <blockquote className="text-lg font-medium leading-relaxed text-ink sm:text-xl">
              <span className="mr-1 text-brand-400">&ldquo;</span>
              {LINKCPP_TAGLINE}
              <span className="ml-0.5 text-brand-400">&rdquo;</span>
            </blockquote>
            <figcaption className="mt-3 font-mono text-xs text-ink-faint">
              {t.tech.taglineCaption}
            </figcaption>
          </figure>
        </Reveal>

        <div className="mt-6 grid gap-4 sm:grid-cols-2">
          {t.tech.points.map((tp, i) => {
            const Icon = TECH_ICONS[i];
            return (
              <Reveal key={tp.title} delay={i * 70}>
                <Card interactive className="h-full p-6">
                  <div className="flex items-start gap-4">
                    <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-brand-500/10 text-brand-400 ring-1 ring-brand-500/20">
                      <Icon width={22} height={22} />
                    </div>
                    <div>
                      <h3 className="font-semibold text-ink">{tp.title}</h3>
                      <p className="mt-1.5 text-sm leading-relaxed text-ink-muted">
                        {tp.body}
                      </p>
                    </div>
                  </div>
                </Card>
              </Reveal>
            );
          })}
        </div>

        <Reveal>
          <div className="mt-6 flex flex-col items-start justify-between gap-4 rounded-2xl bg-surface-2/60 px-6 py-5 ring-1 ring-line sm:flex-row sm:items-center">
            <p className="text-sm text-ink-muted">{t.tech.openText}</p>
            <Button
              href={LINKS.github}
              variant="secondary"
              size="md"
              target="_blank"
              rel="noreferrer"
              className="shrink-0"
            >
              <GithubIcon width={18} height={18} />
              {t.actions.viewGithub}
            </Button>
          </div>
        </Reveal>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Roadmap                                                                     */
/* -------------------------------------------------------------------------- */
function Roadmap() {
  const t = useT();
  return (
    <Section id="roadmap" className="relative border-t border-line">
      <Container>
        <Reveal>
          <SectionHeading
            eyebrow={t.roadmap.eyebrow}
            title={t.roadmap.title}
            lede={t.roadmap.lede}
          />
        </Reveal>

        <div className="mt-12 space-y-4">
          {t.roadmap.items.map((r, i) => {
            const tone = ROADMAP_TONE[i];
            return (
              <Reveal key={r.title} delay={i * 70}>
                <div className="flex flex-col gap-4 rounded-2xl bg-surface-2/60 p-6 ring-1 ring-line sm:flex-row sm:items-center">
                  <div className="sm:w-32 shrink-0">
                    <Pill tone={tone}>
                      {tone === "positive" && <CheckIcon width={13} height={13} />}
                      {r.phase}
                    </Pill>
                  </div>
                  <div className="hidden h-10 w-px bg-line sm:block" />
                  <div>
                    <h3 className="font-semibold text-ink">{r.title}</h3>
                    <p className="mt-1.5 text-sm leading-relaxed text-ink-muted">
                      {r.body}
                    </p>
                  </div>
                </div>
              </Reveal>
            );
          })}
        </div>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* Proof / traction strip                                                      */
/* -------------------------------------------------------------------------- */
function Proof() {
  const t = useT();
  return (
    <Section className="relative border-t border-line">
      <Container>
        <Reveal>
          <div className="relative overflow-hidden rounded-3xl bg-surface-2/50 p-8 ring-1 ring-line sm:p-12">
            <div
              aria-hidden
              className="pointer-events-none absolute inset-0 -z-10 bg-grid opacity-60"
            />
            <div className="text-center">
              <Pill tone="positive">
                <CheckIcon width={13} height={13} /> {t.proof.pill}
              </Pill>
              <h2 className="mx-auto mt-5 max-w-2xl text-2xl font-semibold text-ink sm:text-3xl">
                {t.proof.title}
              </h2>
            </div>

            <div className="mt-10 grid grid-cols-2 gap-6 lg:grid-cols-4">
              {t.proof.items.map((label, i) => (
                <Reveal key={label} delay={i * 80} className="text-center">
                  <div className="text-4xl font-semibold text-brand-gradient sm:text-5xl">
                    {PROOF_STATS[i]}
                  </div>
                  <div className="mx-auto mt-2 max-w-[16ch] text-sm text-ink-muted">
                    {label}
                  </div>
                </Reveal>
              ))}
            </div>

            <p className="mt-10 text-center font-mono text-xs text-ink-faint">
              {t.proof.strip}
            </p>
          </div>
        </Reveal>
      </Container>
    </Section>
  );
}

/* -------------------------------------------------------------------------- */
/* App                                                                         */
/* -------------------------------------------------------------------------- */
export default function App() {
  // Cross-page nav lands here as /#section (e.g. from /docs/api). The target
  // element mounts after this render, so scroll to the hash once on mount.
  useEffect(() => {
    const id = window.location.hash.slice(1);
    if (!id) return;
    const el = document.getElementById(id);
    if (el) requestAnimationFrame(() => el.scrollIntoView({ behavior: "smooth", block: "start" }));
  }, []);

  return (
    <>
      <Nav />
      <main>
        <Hero />
        <Thesis />
        <Origin />
        <HowItWorks />
        <Contributors />
        <Developers />
        <Token />
        <NetworkRoles />
        <Tech />
        <Roadmap />
        <Proof />
      </main>
      <Footer />
    </>
  );
}
