# Kvasir — homepage

Marketing homepage for **Kvasir**, the decentralized AI-inference network built on the
**linkcpp** control plane. Single-page scroll, dark-themed, brand-continuous with the app.

Built with **Vite + React + TypeScript + Tailwind CSS v4**.

## Run

```bash
npm install
npm run dev      # dev server at http://localhost:5173
npm run build    # type-check + production build to dist/
npm run preview  # preview the production build
```

## Internationalization (9 languages)

The site ships in **en, ko, zh, es, ja, fr, de, nl, id** with a flag language switcher
in the top-right of the nav. All copy lives in per-language dictionaries; language-agnostic
data (URLs, code, formulas, tier values, device/node structure) stays in
[`src/content.ts`](src/content.ts).

- [`src/i18n/en.ts`](src/i18n/en.ts) is the **source dictionary**; `Dict = typeof en`
  ([`src/i18n/types.ts`](src/i18n/types.ts)) so every other language is compile-checked to
  match its exact shape (a missing/renamed key fails `tsc`).
- [`src/i18n/langs.ts`](src/i18n/langs.ts) lists the languages + emoji flags.
- [`src/i18n/provider.tsx`](src/i18n/provider.tsx) holds the context: `useT()` returns the
  active dictionary, `useLang()` the code + setter. Initial language = saved choice →
  browser language → English. The choice persists in `localStorage` and sets `<html lang>`.
- Components read strings via `useT()`; add a language by creating `src/i18n/<code>.ts`
  and registering it in [`src/i18n/dicts.ts`](src/i18n/dicts.ts) + `LANGS`.

> Note: emoji flags render on macOS/iOS/Android/Linux but show 2-letter codes on Windows.
> Swap for inline SVG flags if Windows parity is required.

## SEO & LLM crawlers

- **Meta** in [`index.html`](index.html): title/description/keywords, canonical, full
  Open Graph + Twitter `summary_large_image`, `og:locale:alternate` for all 9 languages,
  and **JSON-LD** (`Organization` + `WebSite` + `SoftwareApplication`).
- **Social image** `public/og.png` (1200×630) and app icons
  `public/{apple-touch-icon,icon-192,icon-512}.png` + `favicon.svg` + `site.webmanifest`.
- **`public/robots.txt`** explicitly allows AI/LLM crawlers (GPTBot, ChatGPT-User,
  OAI-SearchBot, ClaudeBot, anthropic-ai, Claude-Web, Google-Extended, Googlebot, Bingbot,
  PerplexityBot, Applebot, CCBot) and links the sitemap.
- **`public/llms.txt`** (concise, llmstxt.org-style) and **`public/llms-full.txt`** (the
  complete site content as plain text) so OpenAI/Anthropic/Gemini ingest accurate, honest
  grounding — devnet, utility token, non-custodial, no financial-return claims.
- **`public/sitemap.xml`** — single canonical URL.

Canonical domain is `https://kvasir-ai.net`; update the absolute URLs in `index.html`,
`robots.txt`, `sitemap.xml` and the llms files if it changes.

## Design system

Brand tokens live in [`src/index.css`](src/index.css) under Tailwind's `@theme`.

- **Point color:** pink (`--color-brand-500 #ff3d8b`), extended into a pink → violet glow
  (`--color-violet-500 #a855f7`) for the DePIN / network motif.
- **Surfaces:** near-black, carried from the app UI (`#08090c` / `#0e1015` / `#14161d`).
- **Semantic:** success green `#3cbf8e`, caution amber `#f5b74c`, danger `#e5675f`.
- **Type:** Inter Variable (UI) + JetBrains Mono Variable (code / data), bundled via
  `@fontsource-variable`, so the page is fully self-contained (no external CDNs).

Reusable primitives (`Button`, `Card`, `Pill`, `SectionHeading`, `Reveal`, …) are in
[`src/components/ui.tsx`](src/components/ui.tsx). Translatable copy lives per-language under
[`src/i18n/`](src/i18n/) (see Internationalization above); language-agnostic data (links,
code, formulas, tier values, device/node structure) is in [`src/content.ts`](src/content.ts).

## Structure

Routes: `/` (landing), `/run-node` (node-operator guide), `/careers`
([`src/components/CareersPage.tsx`](src/components/CareersPage.tsx) — fully i18n
marketing/growth job posting via `t.careers.*`; no "founding member" wording, and
the co-founder-track pitch is intentionally kept off the public site and lives in
direct recruiting channels), and `/technology` + `/technology/:slug`
([`src/components/TechnologyPage.tsx`](src/components/TechnologyPage.tsx) — the tech
blog: a category sidebar over the articles in
[`src/tech/articles.ts`](src/tech/articles.ts), engineering write-ups on the
expert-sharded swarm — design / core-tech / Phase 0–7 milestones / field demos /
settlement security), and `/wiki` + `/wiki/:slug`
([`src/components/WikiPage.tsx`](src/components/WikiPage.tsx) — the knowledge base:
category sidebar + prev/next pager over the entries in
[`src/wiki/entries.ts`](src/wiki/entries.ts)).

Both content routes are **fully localized**: `articles.ts` / `entries.ts` are the
English sources of truth (structure, dates, code blocks, per-entry infographics under
`public/blog/` and `public/wiki/`), and per-language bodies live in
`src/tech/tr-<lang>.ts` / `src/wiki/tr-<lang>.ts`, merged by the active language in
`src/{tech,wiki}/translations.ts` with English fallback. Page chrome and category
labels come from `t.techBlog.*` / `t.wiki.*`; block renderers shared by both live in
[`src/components/blocks.tsx`](src/components/blocks.tsx).

`src/App.tsx` composes the sections in order: Hero (with the animated **ring topology** in
[`src/components/Topology.tsx`](src/components/Topology.tsx)) → Why (thesis) → The name
(Kvasir origin) → How it works → Contributors → Developers → Token (KVR) → Network & rewards
(node roles) → Under the hood → Roadmap → Proof strip → CTA footer.

## Positioning

The page leads with the **anti-monopoly / decentralized-AI thesis** (the `#why` section):
open models served across a permissionless network of everyday devices — GPU, CPU, NPU, and
phone — owned and earned by contributors, not a handful of hyperscalers.

- **Ring topology.** The hero visualizes the `ring_proxy` runtime: one model split into
  contiguous layer windows around a ring of heterogeneous devices; each node passes only the
  hidden-state boundary to its neighbor and the last returns the token — **no central master**.
- **The name.** A `#name` section ties "Kvasir" (the Norse being of pooled, shared wisdom) to
  the decentralization thesis.
- **Node roles & rewards.** The `#network` section explains the three roles (compute node,
  gateway host, hub host) and how each earns KVR — performance tiers (S/A/B/C), the gateway
  bonus, and infra uptime — grounded in `solana/staking-service` on the `kvasir-net` branch.

## Content guardrails

Copy is grounded in `homepage-brief.md` and honors its compliance constraints: KVR is a
**devnet utility token** (never framed as an investment / tradable asset), rewards reflect
**real compute** (no ROI or income figures), and the **non-custodial** claim is kept true.
No invented metrics, partner logos, or named competitors.

> **Owner overrides / forward-presentation (2026-07-12):** per the product owner:
> - Phones (and NPUs) are presented as **performing inference today** via the ring runtime —
>   beyond the brief's §10-3 (which had mobile as roadmap-only).
> - The **ring topology** (`ring_proxy`) is presented as the network's architecture, though
>   it is a **preview** runtime in the repo (the stable/default data plane is still the
>   `llama_rpc` master-star). The proven 122B/4-GPU demo ran on the RPC path.
>
> If these stances change, revert the `DEVICES` statuses + roadmap "Now" item and the ring
> framing in [`src/content.ts`](src/content.ts) / [`src/components/Topology.tsx`](src/components/Topology.tsx).

## Links (confirmed)

Set in [`src/content.ts`](src/content.ts) → `LINKS`:

- **GitHub:** `https://github.com/louisevandan/kvasir-net`
- **Gateway / wallet:** `https://gate.kvasir-ai.net` (also the base URL in the API code snippet)
- **Hub:** `https://hub.kvasir-ai.net`

Official domain is **`kvasir-ai.net`** (Cloudflare Tunnel). The retired `*.prototypebench.org`
hosts are intentionally not referenced anywhere.

## Before shipping — confirm

- Wire the in-page CTAs (`Run a node`, `Get an API key`) to their real destination pages
  (currently anchor-scroll to the relevant sections).
