import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Container, Card, Pill } from "./ui";
import { Wordmark } from "./Logo";
import LanguageSwitcher from "./LanguageSwitcher";
import KvrExplorer from "./KvrExplorer";
import Footer from "./Footer";
import { BlockView } from "./blocks";
import { BoltIcon, ArrowIcon, RssIcon } from "./icons";
import { LINKS } from "../content";
import { useLang, useT } from "../i18n/provider";
import { TECH_CATEGORIES, type TechArticle, type TechCategory } from "../tech/articles";
import { getTechArticles } from "../tech/translations";

/* ==========================================================================
   Technology page (/technology, /technology/:slug) — the tech blog.
   Sidebar = categories → articles (content in src/tech/articles.ts, English);
   page chrome / category labels are i18n via t.techBlog.*.
   ========================================================================== */

/* ---- slim top bar, same pattern as CareersPage ---- */
function TechHeader({ home }: { home: string }) {
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

/* ---- sidebar: categories → articles ---- */
function Sidebar({
  allArticles,
  active,
  categoryLabels,
  allLabel,
}: {
  allArticles: TechArticle[];
  active?: string;
  categoryLabels: Record<TechCategory, string>;
  allLabel: string;
}) {
  return (
    <nav aria-label="Technology articles" className="space-y-7">
      <Link
        to="/technology"
        className={`block text-sm font-semibold transition-colors ${
          active ? "text-ink-muted hover:text-ink" : "text-brand-300"
        }`}
      >
        {allLabel}
      </Link>
      {TECH_CATEGORIES.map((cat) => {
        const posts = allArticles.filter((a) => a.category === cat);
        if (posts.length === 0) return null;
        return (
          <div key={cat}>
            <div className="text-xs font-semibold uppercase tracking-[0.18em] text-ink-faint">
              {categoryLabels[cat]}
            </div>
            <ul className="mt-3 space-y-1 border-l border-line">
              {posts.map((a) => {
                const isActive = a.slug === active;
                return (
                  <li key={a.slug}>
                    <Link
                      to={`/technology/${a.slug}`}
                      aria-current={isActive ? "page" : undefined}
                      className={`-ml-px block border-l py-1.5 pl-4 pr-2 text-sm leading-snug transition-colors ${
                        isActive
                          ? "border-brand-400 text-ink"
                          : "border-transparent text-ink-muted hover:border-line hover:text-ink"
                      }`}
                    >
                      {a.title}
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

/* ---- index (no slug): intro + grouped article cards ---- */
function IndexView({
  allArticles,
  categoryLabels,
}: {
  allArticles: TechArticle[];
  categoryLabels: Record<TechCategory, string>;
}) {
  const t = useT();
  const b = t.techBlog;
  return (
    <div>
      <Pill tone="brand">
        <BoltIcon width={14} height={14} />
        {b.pill}
      </Pill>
      <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
        {b.title}
      </h1>
      <p className="mt-5 max-w-2xl text-lg leading-relaxed text-ink-muted">{b.lede}</p>
      <a
        href={LINKS.feed}
        className="mt-5 inline-flex items-center gap-1.5 rounded-full border border-line px-3 py-1.5 text-sm font-medium text-ink-muted transition-colors hover:border-brand-400 hover:text-ink"
        title="Subscribe to the blog feed (RSS)"
      >
        <RssIcon width={15} height={15} />
        RSS
      </a>
      {b.langNote && <p className="mt-3 text-xs text-ink-faint">{b.langNote}</p>}

      {TECH_CATEGORIES.map((cat) => {
        const posts = allArticles.filter((a) => a.category === cat);
        if (posts.length === 0) return null;
        return (
          <div key={cat} className="mt-12">
            <h2 className="text-xs font-semibold uppercase tracking-[0.18em] text-brand-400">
              {categoryLabels[cat]}
            </h2>
            <div className="mt-4 grid gap-4 sm:grid-cols-2">
              {posts.map((a) => (
                <Link key={a.slug} to={`/technology/${a.slug}`} className="group">
                  <Card interactive className="h-full p-6">
                    <div className="font-mono text-[0.7rem] text-ink-faint">{a.date}</div>
                    <h3 className="mt-2 font-semibold leading-snug text-ink">{a.title}</h3>
                    <p className="mt-2 text-sm leading-relaxed text-ink-muted">{a.dek}</p>
                    <span className="mt-4 inline-flex items-center gap-1.5 text-sm font-medium text-brand-300">
                      {b.read}
                      <ArrowIcon
                        width={14}
                        height={14}
                        className="transition-transform group-hover:translate-x-0.5"
                      />
                    </span>
                  </Card>
                </Link>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/* ---- prev/next pager at the bottom of an article ---- */
function ArticlePager({ articles, current }: { articles: TechArticle[]; current: TechArticle }) {
  const idx = articles.findIndex((a) => a.slug === current.slug);
  const prev = idx > 0 ? articles[idx - 1] : undefined;
  const next = idx >= 0 && idx < articles.length - 1 ? articles[idx + 1] : undefined;
  if (!prev && !next) return null;
  return (
    <nav aria-label="Adjacent articles" className="mt-14 grid min-w-0 grid-cols-1 gap-3 border-t border-line pt-6 sm:grid-cols-2">
      {prev ? (
        <Link
          to={`/technology/${prev.slug}`}
          className="group flex min-w-0 items-center gap-3 overflow-hidden rounded-2xl bg-surface-2/60 p-4 ring-1 ring-line transition-all hover:bg-surface-2 hover:ring-brand-500/40"
        >
          <ArrowIcon
            width={16}
            height={16}
            className="shrink-0 rotate-180 text-ink-faint transition-transform group-hover:-translate-x-0.5 group-hover:text-brand-300"
          />
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-ink">{prev.title}</span>
            <span className="block truncate text-xs text-ink-faint">{prev.dek}</span>
          </span>
        </Link>
      ) : (
        <span aria-hidden className="hidden sm:block" />
      )}
      {next && (
        <Link
          to={`/technology/${next.slug}`}
          className="group flex min-w-0 items-center justify-end gap-3 overflow-hidden rounded-2xl bg-surface-2/60 p-4 text-right ring-1 ring-line transition-all hover:bg-surface-2 hover:ring-brand-500/40"
        >
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-ink">{next.title}</span>
            <span className="block truncate text-xs text-ink-faint">{next.dek}</span>
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

/* ---- single article ---- */
function ArticleView({
  article,
  categoryLabels,
}: {
  article: TechArticle;
  categoryLabels: Record<TechCategory, string>;
}) {
  return (
    <article>
      <div className="flex flex-wrap items-center gap-2">
        <Pill tone="brand">{categoryLabels[article.category]}</Pill>
        <span className="font-mono text-xs text-ink-faint">{article.date}</span>
      </div>
      <h1 className="mt-4 text-3xl font-semibold leading-tight tracking-tight text-ink sm:text-4xl">
        {article.title}
      </h1>
      <p className="mt-4 max-w-3xl text-lg leading-relaxed text-ink-muted">{article.dek}</p>
      <div className="mt-4 flex flex-wrap gap-2">
        {article.tags.map((tag) => (
          <span
            key={tag}
            className="rounded-full bg-surface-2 px-2.5 py-1 font-mono text-[0.68rem] text-ink-faint ring-1 ring-line"
          >
            {tag}
          </span>
        ))}
      </div>
      <div className="mt-2">
        {article.blocks.map((blk, i) => (
          <BlockView key={i} b={blk} />
        ))}
      </div>
    </article>
  );
}

export default function TechnologyPage() {
  const t = useT();
  const b = t.techBlog;
  const { lang } = useLang();
  const { slug } = useParams<{ slug: string }>();
  const allArticles = getTechArticles(lang);
  const article = slug ? allArticles.find((a) => a.slug === slug) : undefined;

  const categoryLabels: Record<TechCategory, string> = b.categories;

  useEffect(() => {
    window.scrollTo(0, 0);
  }, [slug]);
  useEffect(() => {
    document.title = article ? `${article.title} · ${b.docTitle}` : b.docTitle;
  }, [article, b.docTitle]);

  return (
    <div className="min-h-screen">
      <TechHeader home={t.guide.home} />

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
                {b.sidebarTitle}
              </summary>
              <div className="mt-4">
                <Sidebar
                  allArticles={allArticles}
                  active={article?.slug}
                  categoryLabels={categoryLabels}
                  allLabel={b.allArticles}
                />
              </div>
            </details>
            <div className="hidden lg:sticky lg:top-24 lg:block">
              <Sidebar
                allArticles={allArticles}
                active={article?.slug}
                categoryLabels={categoryLabels}
                allLabel={b.allArticles}
              />
            </div>
          </aside>

          {/* content */}
          <div className="min-w-0">
            {slug && !article ? (
              <div>
                <h1 className="text-2xl font-semibold text-ink">404</h1>
                <p className="mt-3 text-ink-muted">{b.notFound}</p>
                <Link
                  to="/technology"
                  className="mt-5 inline-flex items-center gap-1.5 text-sm font-medium text-brand-300 hover:underline"
                >
                  {b.allArticles}
                  <ArrowIcon width={14} height={14} />
                </Link>
              </div>
            ) : article ? (
              <>
                <ArticleView article={article} categoryLabels={categoryLabels} />
                <ArticlePager articles={allArticles} current={article} />
              </>
            ) : (
              <IndexView allArticles={allArticles} categoryLabels={categoryLabels} />
            )}
          </div>
        </div>
      </Container>

      <Footer />
    </div>
  );
}
