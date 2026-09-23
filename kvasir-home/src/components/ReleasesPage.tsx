import { useEffect } from "react";
import { Container, Pill } from "./ui";
import Nav from "./Nav";
import Footer from "./Footer";
import { LINKS } from "../content";
import { useLang } from "../i18n/provider";
import data from "../releases.json";

/* ==========================================================================
   Release notes (/releases). English only in every locale, for the same reason
   as /legal: one authoritative text, no translation drift in a record people
   will quote back at us.

   The content comes from src/releases.json, which the daily monitoring job
   appends to when a version actually ships (tools/kvasir-watch). Every entry
   carries the evidence it was accepted on — a release note that only says what
   changed asks to be believed; one that says what was measured can be checked.
   ========================================================================== */

type Release = {
  id: string;
  component: string;
  version: string;
  date: string;
  title: string;
  body: string;
  evidence?: string[];
};

const COMPONENT_LABEL: Record<string, string> = {
  engine: "Engine",
  network: "Network",
  gateway: "Gateway",
  wallet: "Wallet",
  site: "Site",
};

/* Each component gets its own colour so a reader scanning the left rail can see
   which part of the system moved, without reading a word. */
const COMPONENT_TONE: Record<string, string> = {
  engine: "text-brand-300 ring-brand-300/30 bg-brand-300/10",
  network: "text-emerald-300 ring-emerald-300/30 bg-emerald-300/10",
  gateway: "text-amber-300 ring-amber-300/30 bg-amber-300/10",
  wallet: "text-sky-300 ring-sky-300/30 bg-sky-300/10",
  site: "text-ink-muted ring-line bg-surface-2",
};

const longDate = (iso: string) =>
  new Date(`${iso}T00:00:00Z`).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });

function Entry({ release }: { release: Release }) {
  const tone = COMPONENT_TONE[release.component] ?? COMPONENT_TONE.site;
  return (
    <article className="relative border-t border-line pt-8 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-center gap-3">
        <span className={`rounded-full px-2.5 py-1 text-[11px] font-medium uppercase tracking-wider ring-1 ${tone}`}>
          {COMPONENT_LABEL[release.component] ?? release.component}
        </span>
        <span className="font-mono text-sm text-ink">{release.version}</span>
        <time className="text-sm text-ink-faint" dateTime={release.date}>
          {longDate(release.date)}
        </time>
      </div>

      <h2 className="mt-4 text-2xl font-semibold tracking-tight text-ink">{release.title}</h2>
      <p className="mt-3 text-[15px] leading-relaxed text-ink-muted">{release.body}</p>

      {release.evidence?.length ? (
        <div className="mt-5 rounded-xl border border-line bg-surface-2/50 px-5 py-4">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Evidence</h3>
          <ul className="mt-3 space-y-2">
            {release.evidence.map((item, i) => (
              <li key={i} className="flex gap-3 text-[14px] leading-relaxed text-ink-muted">
                <span aria-hidden className="mt-2 h-1 w-1 shrink-0 rounded-full bg-brand-400" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </article>
  );
}

export default function ReleasesPage() {
  const { lang } = useLang();
  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  useEffect(() => {
    document.title = "Release notes — Kvasir";
  }, []);

  const releases = [...(data.releases as Release[])].sort((a, b) => b.date.localeCompare(a.date));

  return (
    <div className="min-h-screen">
      <Nav />
      <Container className="pt-28 pb-20 sm:pt-36">
        <div className="max-w-3xl">
          <Pill tone="brand">Release notes</Pill>
          <h1 className="mt-5 text-4xl font-semibold leading-tight tracking-tight text-ink sm:text-5xl">
            What shipped, and what it was measured on
          </h1>
          <p className="mt-5 text-lg leading-relaxed text-ink-muted">{data.note}</p>
          <p className="mt-3 text-sm text-ink-faint">
            Last updated {data.updated} ·{" "}
            <a className="text-brand-300 hover:underline" href={LINKS.github} target="_blank" rel="noreferrer">
              source on GitHub
            </a>
          </p>
          {lang !== "en" && (
            <p className="mt-3 text-sm text-ink-faint">This page is provided in English only.</p>
          )}
        </div>

        <div className="mt-14 max-w-3xl space-y-10">
          {releases.map((release) => (
            <Entry key={release.id} release={release} />
          ))}
        </div>

        <p className="mt-14 max-w-3xl text-sm leading-relaxed text-ink-faint">
          Work in progress is not a release. Three things are being built right now — hardening the ring, porting the
          settlement gateway to p4, and turning the desktop client into a node that runs a p4 agent — and each will
          appear here when it ships, not before.
        </p>
      </Container>
      <Footer />
    </div>
  );
}
