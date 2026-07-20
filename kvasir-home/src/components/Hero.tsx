import { Container, Button, Pill } from "./ui";
import { ArrowIcon, BoltIcon, CheckIcon, TerminalIcon } from "./icons";
import Topology from "./Topology";
import { LINKS } from "../content";
import { useT } from "../i18n/provider";

export default function Hero() {
  const t = useT();
  return (
    <section id="top" className="relative overflow-hidden pt-28 pb-16 sm:pt-36 sm:pb-24">
      {/* ambient background */}
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-grid" />
      <div
        aria-hidden
        className="pointer-events-none absolute -top-40 left-1/2 -z-10 h-[560px] w-[900px] -translate-x-1/2 blur-3xl"
        style={{
          background:
            "radial-gradient(50% 50% at 50% 30%, rgba(255,61,139,0.18), transparent 70%), radial-gradient(40% 40% at 70% 60%, rgba(168,85,247,0.14), transparent 70%)",
        }}
      />

      <Container>
        <div className="grid items-center gap-12 lg:grid-cols-[1.05fr_1fr]">
          {/* copy */}
          <div>
            <Pill tone="brand">
              <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse-glow" />
              {t.hero.eyebrow}
            </Pill>

            <h1 className="mt-6 text-[2.7rem] font-semibold leading-[1.02] tracking-tight text-ink sm:text-[3.375rem] lg:text-[4.05rem]">
              {t.hero.headline1}
              <br />
              <span className="text-brand-gradient">{t.hero.headline2}</span>
            </h1>

            <p className="mt-6 max-w-xl text-lg leading-relaxed text-ink-muted">
              {t.hero.sub}
            </p>

            <div className="mt-8 flex flex-col gap-3 sm:flex-row">
              <Button href={LINKS.runNode} variant="primary" size="lg">
                <BoltIcon width={18} height={18} />
                {t.actions.runNode}
                <ArrowIcon
                  width={16}
                  height={16}
                  className="transition-transform group-hover:translate-x-0.5"
                />
              </Button>
              <Button href={LINKS.api} variant="secondary" size="lg">
                <TerminalIcon width={18} height={18} />
                {t.actions.useApi}
              </Button>
            </div>

            {/* trust badges */}
            <ul className="mt-9 flex flex-wrap gap-x-5 gap-y-2.5">
              {t.hero.badges.map((b) => (
                <li key={b} className="flex items-center gap-1.5 text-sm text-ink-muted">
                  <CheckIcon width={15} height={15} className="text-positive" />
                  {b}
                </li>
              ))}
            </ul>
          </div>

          {/* visual */}
          <div className="lg:pl-4">
            <Topology />
          </div>
        </div>
      </Container>
    </section>
  );
}
