import { Container, Button } from "./ui";
import { Wordmark } from "./Logo";
import { GithubIcon, BoltIcon, TerminalIcon, RssIcon } from "./icons";
import { LINKS, NAV_ITEMS } from "../content";
import { useT } from "../i18n/provider";

export default function Footer() {
  const t = useT();
  return (
    <footer className="relative overflow-hidden border-t border-line">
      <div
        aria-hidden
        className="pointer-events-none absolute -top-32 left-1/2 -z-10 h-[420px] w-[820px] -translate-x-1/2 blur-3xl"
        style={{
          background:
            "radial-gradient(50% 50% at 50% 40%, rgba(255,61,139,0.12), transparent 70%)",
        }}
      />

      {/* CTA band */}
      <Container>
        <div className="py-20 text-center sm:py-24">
          <h2 className="mx-auto max-w-2xl text-3xl font-semibold text-ink sm:text-4xl">
            {t.footer.ctaTitle}
          </h2>
          <p className="mx-auto mt-4 max-w-xl text-ink-muted">{t.footer.ctaBody}</p>
          <div className="mt-8 flex flex-col justify-center gap-3 sm:flex-row">
            <Button href={LINKS.runNode} variant="primary" size="lg">
              <BoltIcon width={18} height={18} />
              {t.actions.runNode}
            </Button>
            <Button href={LINKS.api} variant="secondary" size="lg">
              <TerminalIcon width={18} height={18} />
              {t.actions.getApiAccess}
            </Button>
            <Button
              href={LINKS.github}
              variant="ghost"
              size="lg"
              target="_blank"
              rel="noreferrer"
            >
              <GithubIcon width={18} height={18} />
              {t.actions.github}
            </Button>
          </div>
        </div>
      </Container>

      {/* meta */}
      <div className="border-t border-line">
        <Container>
          <div className="flex flex-col gap-8 py-10 md:flex-row md:items-start md:justify-between">
            <div className="max-w-sm">
              <Wordmark size={28} />
              <p className="mt-4 text-sm leading-relaxed text-ink-faint">
                {t.footer.tagline}
              </p>
            </div>

            <nav className="grid grid-cols-2 gap-x-12 gap-y-2 text-sm">
              {NAV_ITEMS.flatMap((item): { key: string; href: string }[] =>
                "children" in item
                  ? item.children.map((c) => ({ key: c.key, href: c.href }))
                  : [{ key: item.key, href: item.href }]
              ).map((item) => (
                <a
                  key={item.href}
                  href={item.href}
                  className="text-ink-muted transition-colors hover:text-ink"
                >
                  {t.nav[item.key as keyof typeof t.nav]}
                </a>
              ))}
              <a
                href="/careers"
                className="text-ink-muted transition-colors hover:text-ink"
              >
                {t.nav.careers}
              </a>
              <a
                href={LINKS.github}
                target="_blank"
                rel="noreferrer"
                className="text-ink-muted transition-colors hover:text-ink"
              >
                {t.actions.github}
              </a>
              <a
                href={LINKS.feed}
                className="inline-flex items-center gap-1.5 text-ink-muted transition-colors hover:text-ink"
                title="Subscribe to the blog feed (RSS)"
              >
                <RssIcon width={14} height={14} />
                RSS
              </a>
            </nav>
          </div>

          {/* compliance line */}
          <div className="border-t border-line py-6">
            <p className="text-xs leading-relaxed text-ink-faint">
              <strong className="font-semibold text-ink-muted">
                {t.footer.disclaimerStrong}
              </strong>{" "}
              {t.footer.disclaimer}
            </p>
            <p className="mt-4 text-xs text-ink-faint">{t.footer.rights}</p>
          </div>
        </Container>
      </div>
    </footer>
  );
}
