import { Fragment, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Container, Pill } from "./ui";
import { Wordmark } from "./Logo";
import LanguageSwitcher from "./LanguageSwitcher";
import KvrExplorer from "./KvrExplorer";
import Footer from "./Footer";
import { BlockView } from "./blocks";
import { LayersIcon, ArrowIcon } from "./icons";
import { useLang, useT } from "../i18n/provider";
import { WIKI_CATEGORIES, type WikiEntry, type WikiCategory } from "../wiki/entries";
import { getWikiEntries } from "../wiki/translations";

/* ==========================================================================
   Wiki page (/wiki, /wiki/:slug) — the knowledge base.
   Sidebar = categories → entries (content in src/wiki/entries.ts, English);
   page chrome / category labels are i18n via t.wiki.*.
   Mirrors TechnologyPage's layout; block renderers shared via blocks.tsx.
   ========================================================================== */

/* ---- slim top bar, same pattern as CareersPage / TechnologyPage ---- */
function WikiHeader({ home }: { home: string }) {
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

/* ---- sidebar: categories → entries ---- */
function Sidebar({
  allEntries,
  active,
  categoryLabels,
  allLabel,
}: {
  allEntries: WikiEntry[];
  active?: string;
  categoryLabels: Record<WikiCategory, string>;
  allLabel: string;
}) {
  return (
    <nav aria-label="Wiki entries" className="space-y-7">
      <Link
        to="/wiki"
        className={`block text-sm font-semibold transition-colors ${
          active ? "text-ink-muted hover:text-ink" : "text-brand-300"
        }`}
      >
        {allLabel}
      </Link>
      {WIKI_CATEGORIES.map((cat) => {
        const entries = allEntries.filter((e) => e.category === cat);
        if (entries.length === 0) return null;
        return (
          <div key={cat}>
            <div className="text-xs font-semibold uppercase tracking-[0.18em] text-ink-faint">
              {categoryLabels[cat]}
            </div>
            <ul className="mt-3 space-y-1 border-l border-line">
              {entries.map((e) => {
                const isActive = e.slug === active;
                return (
                  <li key={e.slug}>
                    <Link
                      to={`/wiki/${e.slug}`}
                      aria-current={isActive ? "page" : undefined}
                      className={`-ml-px block border-l py-1.5 pl-4 pr-2 text-sm leading-snug transition-colors ${
                        isActive
                          ? "border-brand-400 text-ink"
                          : "border-transparent text-ink-muted hover:border-line hover:text-ink"
                      }`}
                    >
                      {e.title}
                    </Link>
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

/* ---- index (no slug): intro + grouped entry list ---- */
function IndexView({
  allEntries,
  categoryLabels,
}: {
  allEntries: WikiEntry[];
  categoryLabels: Record<WikiCategory, string>;
}) {
  const t = useT();
  const w = t.wiki;
  return (
    <div>
      <Pill tone="brand">
        <LayersIcon width={14} height={14} />
        {w.pill}
      </Pill>
      <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
        {w.title}
      </h1>
      <p className="mt-5 max-w-2xl text-lg leading-relaxed text-ink-muted">{w.lede}</p>
      {w.langNote && <p className="mt-3 text-xs text-ink-faint">{w.langNote}</p>}

      {WIKI_CATEGORIES.map((cat) => {
        const entries = allEntries.filter((e) => e.category === cat);
        if (entries.length === 0) return null;
        return (
          <div key={cat} className="mt-12">
            <h2 className="text-xs font-semibold uppercase tracking-[0.18em] text-brand-400">
              {categoryLabels[cat]}
            </h2>
            <div className="mt-4 divide-y divide-line rounded-2xl bg-surface-2/50 ring-1 ring-line">
              {entries.map((e) => (
                <Link
                  key={e.slug}
                  to={`/wiki/${e.slug}`}
                  className="group flex items-start justify-between gap-4 px-5 py-4 transition-colors first:rounded-t-2xl last:rounded-b-2xl hover:bg-surface-2"
                >
                  <div>
                    <h3 className="font-semibold text-ink">{e.title}</h3>
                    <p className="mt-1 text-sm leading-relaxed text-ink-muted">{e.summary}</p>
                  </div>
                  <ArrowIcon
                    width={16}
                    height={16}
                    className="mt-1.5 shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5 group-hover:text-brand-300"
                  />
                </Link>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/* ---- single entry ---- */
/* ---- prev/next pager at the bottom of an entry ---- */
function EntryPager({ entries, current }: { entries: WikiEntry[]; current: WikiEntry }) {
  const idx = entries.findIndex((e) => e.slug === current.slug);
  const prev = idx > 0 ? entries[idx - 1] : undefined;
  const next = idx >= 0 && idx < entries.length - 1 ? entries[idx + 1] : undefined;
  if (!prev && !next) return null;
  return (
    <nav aria-label="Adjacent entries" className="mt-14 grid min-w-0 grid-cols-1 gap-3 border-t border-line pt-6 sm:grid-cols-2">
      {prev ? (
        <Link
          to={`/wiki/${prev.slug}`}
          className="group flex min-w-0 items-center gap-3 overflow-hidden rounded-2xl bg-surface-2/60 p-4 ring-1 ring-line transition-all hover:bg-surface-2 hover:ring-brand-500/40"
        >
          <ArrowIcon
            width={16}
            height={16}
            className="shrink-0 rotate-180 text-ink-faint transition-transform group-hover:-translate-x-0.5 group-hover:text-brand-300"
          />
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-ink">{prev.title}</span>
            <span className="block truncate text-xs text-ink-faint">{prev.summary}</span>
          </span>
        </Link>
      ) : (
        <span aria-hidden className="hidden sm:block" />
      )}
      {next && (
        <Link
          to={`/wiki/${next.slug}`}
          className="group flex min-w-0 items-center justify-end gap-3 overflow-hidden rounded-2xl bg-surface-2/60 p-4 text-right ring-1 ring-line transition-all hover:bg-surface-2 hover:ring-brand-500/40"
        >
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-ink">{next.title}</span>
            <span className="block truncate text-xs text-ink-faint">{next.summary}</span>
          </span>
          <ArrowIcon
            width={16}
            height={16}
            className="shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5 group-hover:text-brand-300"
          />
        </Link>
      )}
    </nav>
  );
}

function EntryFigure({ image }: { image: NonNullable<WikiEntry["image"]> }) {
  return (
    <div className="mt-6 overflow-hidden rounded-2xl ring-1 ring-line">
      <img src={image.src} alt={image.alt} loading="lazy" className="block w-full" />
    </div>
  );
}

function EntryView({
  entry,
  categoryLabels,
}: {
  entry: WikiEntry;
  categoryLabels: Record<WikiCategory, string>;
}) {
  const imagePos = entry.image ? entry.imagePos : undefined;
  return (
    <article>
      <Pill tone="brand">{categoryLabels[entry.category]}</Pill>
      <h1 className="mt-4 text-3xl font-semibold leading-tight tracking-tight text-ink sm:text-4xl">
        {entry.title}
      </h1>
      <p className="mt-4 max-w-3xl text-lg leading-relaxed text-ink-muted">{entry.summary}</p>
      {entry.image && imagePos === undefined && <EntryFigure image={entry.image} />}
      <div className="mt-2">
        {entry.blocks.map((blk, i) => (
          <Fragment key={i}>
            <BlockView b={blk} />
            {entry.image && imagePos === i && <EntryFigure image={entry.image} />}
          </Fragment>
        ))}
      </div>
    </article>
  );
}

export default function WikiPage() {
  const t = useT();
  const w = t.wiki;
  const { lang } = useLang();
  const { slug } = useParams<{ slug: string }>();
  const allEntries = getWikiEntries(lang);
  const entry = slug ? allEntries.find((e) => e.slug === slug) : undefined;

  const categoryLabels: Record<WikiCategory, string> = w.categories;

  useEffect(() => {
    window.scrollTo(0, 0);
  }, [slug]);
  useEffect(() => {
    document.title = entry ? `${entry.title} · ${w.docTitle}` : w.docTitle;
  }, [entry, w.docTitle]);

  return (
    <div className="min-h-screen">
      <WikiHeader home={t.guide.home} />

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

      <Container className="pt-28 pb-20 sm:pt-32">
        <div className="grid gap-10 lg:grid-cols-[16.5rem_minmax(0,1fr)] lg:gap-14">
          {/* sidebar — collapsible on mobile, sticky on desktop */}
          <aside>
            <details className="group rounded-2xl bg-surface-2/60 p-4 ring-1 ring-line lg:hidden">
              <summary className="cursor-pointer list-none text-sm font-semibold text-ink">
                {w.sidebarTitle}
              </summary>
              <div className="mt-4">
                <Sidebar
                  allEntries={allEntries}
                  active={entry?.slug}
                  categoryLabels={categoryLabels}
                  allLabel={w.allEntries}
                />
              </div>
            </details>
            <div className="hidden lg:sticky lg:top-24 lg:block">
              <Sidebar
                allEntries={allEntries}
                active={entry?.slug}
                categoryLabels={categoryLabels}
                allLabel={w.allEntries}
              />
            </div>
          </aside>

          {/* content */}
          <div className="min-w-0">
            {slug && !entry ? (
              <div>
                <h1 className="text-2xl font-semibold text-ink">404</h1>
                <p className="mt-3 text-ink-muted">{w.notFound}</p>
                <Link
                  to="/wiki"
                  className="mt-5 inline-flex items-center gap-1.5 text-sm font-medium text-brand-300 hover:underline"
                >
                  {w.allEntries}
                  <ArrowIcon width={14} height={14} />
                </Link>
              </div>
            ) : entry ? (
              <>
                <EntryView entry={entry} categoryLabels={categoryLabels} />
                <EntryPager entries={allEntries} current={entry} />
              </>
            ) : (
              <IndexView allEntries={allEntries} categoryLabels={categoryLabels} />
            )}
          </div>
        </div>
      </Container>

      <Footer />
    </div>
  );
}
