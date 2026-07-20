import { useEffect, useState } from "react";
import { Container, Button } from "./ui";
import { Wordmark } from "./Logo";
import KvrExplorer from "./KvrExplorer";
import LanguageSwitcher from "./LanguageSwitcher";
import { NAV_ITEMS, LINKS } from "../content";
import { useT } from "../i18n/provider";
import type { Dict } from "../i18n/types";

type NavKey = keyof Dict["nav"];

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width={14}
      height={14}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`}
    >
      <path d="M6 9l6 6 6-6" />
    </svg>
  );
}

export default function Nav() {
  const t = useT();
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false); // mobile panel
  const [menu, setMenu] = useState<string | null>(null); // desktop dropdown
  const [group, setGroup] = useState<string | null>(null); // mobile expanded group

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

        <nav className="hidden items-center gap-0.5 xl:flex">
          {NAV_ITEMS.map((item) =>
            "children" in item ? (
              <div key={item.key} className="relative">
                <button
                  onClick={() => setMenu((v) => (v === item.key ? null : item.key))}
                  className="inline-flex items-center gap-1 rounded-full px-3 py-2 text-sm text-ink-muted transition-colors hover:text-ink"
                  aria-expanded={menu === item.key}
                >
                  {t.nav[item.key as NavKey]}
                  <Chevron open={menu === item.key} />
                </button>
                {menu === item.key && (
                  <div className="absolute left-0 top-full z-50 mt-1 min-w-[10rem] overflow-hidden rounded-xl border border-line bg-surface-2/95 py-1 shadow-glow backdrop-blur">
                    {item.children.map((c) => (
                      <a
                        key={c.href}
                        href={c.href}
                        onClick={() => setMenu(null)}
                        className="block px-4 py-2 text-sm text-ink-muted transition-colors hover:bg-white/5 hover:text-ink"
                      >
                        {t.nav[c.key as NavKey]}
                      </a>
                    ))}
                  </div>
                )}
              </div>
            ) : (
              <a
                key={item.key}
                href={item.href}
                className="rounded-full px-3 py-2 text-sm text-ink-muted transition-colors hover:text-ink"
              >
                {t.nav[item.key as NavKey]}
              </a>
            )
          )}
        </nav>

        <div className="flex items-center gap-2">
          <LanguageSwitcher />
          <KvrExplorer className="hidden sm:inline-flex" />
          <Button
            href={LINKS.runNode}
            variant="primary"
            size="md"
            className="hidden sm:inline-flex"
          >
            {t.actions.joinNode}
          </Button>

          {/* hamburger — shown whenever the inline nav is hidden (below xl) */}
          <button
            className="inline-flex h-10 w-10 items-center justify-center rounded-lg text-ink-muted ring-1 ring-line xl:hidden"
            aria-label={t.actions.menu}
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            <svg width={20} height={20} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round">
              {open ? <path d="M6 6l12 12M18 6L6 18" /> : <path d="M4 7h16M4 12h16M4 17h16" />}
            </svg>
          </button>
        </div>
      </Container>

      {/* click-away backdrop for the desktop dropdown */}
      {menu && <button aria-hidden tabIndex={-1} className="fixed inset-0 z-40 hidden cursor-default xl:block" onClick={() => setMenu(null)} />}

      {/* mobile panel */}
      {open && (
        <div className="glass border-b border-line xl:hidden">
          <Container className="flex flex-col gap-1 py-4">
            {NAV_ITEMS.map((item) =>
              "children" in item ? (
                <div key={item.key}>
                  <button
                    onClick={() => setGroup((v) => (v === item.key ? null : item.key))}
                    className="flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-sm text-ink-muted hover:bg-white/5 hover:text-ink"
                    aria-expanded={group === item.key}
                  >
                    {t.nav[item.key as NavKey]}
                    <Chevron open={group === item.key} />
                  </button>
                  {group === item.key && (
                    <div className="ml-3 flex flex-col gap-1 border-l border-line pl-3">
                      {item.children.map((c) => (
                        <a
                          key={c.href}
                          href={c.href}
                          onClick={() => setOpen(false)}
                          className="rounded-lg px-3 py-2.5 text-sm text-ink-muted hover:bg-white/5 hover:text-ink"
                        >
                          {t.nav[c.key as NavKey]}
                        </a>
                      ))}
                    </div>
                  )}
                </div>
              ) : (
                <a
                  key={item.key}
                  href={item.href}
                  onClick={() => setOpen(false)}
                  className="rounded-lg px-3 py-2.5 text-sm text-ink-muted hover:bg-white/5 hover:text-ink"
                >
                  {t.nav[item.key as NavKey]}
                </a>
              )
            )}
            <div className="mt-2 flex gap-2">
              <KvrExplorer className="flex-1" />
              <Button href={LINKS.runNode} variant="primary" size="md" className="flex-1" onClick={() => setOpen(false)}>
                {t.actions.joinNode}
              </Button>
            </div>
          </Container>
        </div>
      )}
    </header>
  );
}
