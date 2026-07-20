import type { ReactNode } from "react";
import { Card } from "./ui";
import type { TechBlock } from "../tech/articles";

/* ==========================================================================
   Shared article-block renderers for the tech blog (/technology) and the
   wiki (/wiki). Content lives in src/tech/articles.ts / src/wiki/entries.ts
   as typed blocks; inline markup supported in `md` strings: **bold**, `code`.
   ========================================================================== */

/* ---- tiny inline renderer: **bold** and `code` ---- */
export function inlineMd(md: string): ReactNode[] {
  return md.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**")) {
      return (
        <strong key={i} className="font-semibold text-ink">
          {part.slice(2, -2)}
        </strong>
      );
    }
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code
          key={i}
          className="rounded bg-surface-3 px-1.5 py-0.5 font-mono text-[0.85em] text-brand-300"
        >
          {part.slice(1, -1)}
        </code>
      );
    }
    return part;
  });
}

/* ---- block renderers ---- */
export function BlockView({ b }: { b: TechBlock }) {
  switch (b.t) {
    case "h2":
      return (
        <div className="mt-12">
          {b.kick && (
            <div className="text-xs font-semibold uppercase tracking-[0.18em] text-brand-400">
              {b.kick}
            </div>
          )}
          <h2 className="mt-2 text-2xl font-semibold text-ink">{b.text}</h2>
        </div>
      );
    case "p":
      return (
        <p className="mt-5 max-w-3xl leading-relaxed text-ink-muted">{inlineMd(b.md)}</p>
      );
    case "callout":
      return (
        <div className="mt-6 rounded-2xl bg-brand-500/8 p-6 ring-1 ring-brand-500/25">
          <p className="leading-relaxed text-ink-muted">{inlineMd(b.md)}</p>
        </div>
      );
    case "stats":
      return (
        <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          {b.items.map((s) => (
            <Card key={s.l} className="p-4">
              <div className="font-mono text-xl font-bold tracking-tight text-brand-gradient">
                {s.n}
              </div>
              <div className="mt-1 font-mono text-[0.7rem] text-ink-faint">{s.l}</div>
            </Card>
          ))}
        </div>
      );
    case "ul":
      return (
        <ul className="mt-5 max-w-3xl space-y-3">
          {b.items.map((it, i) => (
            <li key={i} className="flex items-start gap-3 text-sm leading-relaxed text-ink-muted">
              <span className="mt-2 h-1.5 w-1.5 shrink-0 rounded-sm bg-brand-400" />
              <span>{inlineMd(it)}</span>
            </li>
          ))}
        </ul>
      );
    case "code":
      return (
        <div className="mt-6">
          {b.caption && <p className="mb-2 text-xs text-ink-faint">{inlineMd(b.caption)}</p>}
          <div className="overflow-x-auto rounded-2xl bg-surface p-5 ring-1 ring-line">
            <pre className="font-mono text-[0.8rem] leading-relaxed text-ink-muted">
              {b.code}
            </pre>
          </div>
        </div>
      );
    case "img":
      return (
        <div className="mt-6 overflow-hidden rounded-2xl ring-1 ring-line">
          <img src={b.src} alt={b.alt} loading="lazy" className="block w-full" />
        </div>
      );
    case "table":
      return (
        <div className="mt-6 overflow-x-auto rounded-2xl ring-1 ring-line">
          <table className="w-full min-w-[28rem] text-left text-sm">
            <thead>
              <tr className="bg-surface-2/70">
                {b.head.map((h) => (
                  <th
                    key={h}
                    className="px-4 py-3 font-mono text-[0.68rem] font-semibold uppercase tracking-wider text-ink-faint"
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.map((row, ri) => (
                <tr key={ri} className="border-t border-line">
                  {row.map((cell, ci) => (
                    <td
                      key={ci}
                      className={`px-4 py-3 ${
                        ci === 0 ? "font-mono text-ink" : "text-ink-muted"
                      }`}
                    >
                      {cell}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}
