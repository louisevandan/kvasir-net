/* ==========================================================================
   SEO/GEO post-build: prerender every content route to static HTML so
   JS-less crawlers (GPTBot, ClaudeBot, PerplexityBot, Googlebot…) see the full
   body, plus per-route <title>/description/canonical/OG/Twitter, JSON-LD,
   RSS feed, sitemap and a real 404. Runs after `vite build`; operates on dist/.

   Approach (F1 option b): content is pure data (articles.ts / entries.ts block
   structs), so we render blocks → semantic HTML directly — no SSR of the React
   tree. The prerendered body is placed inside #root; the SPA's createRoot()
   replaces it on mount, so JS users keep the full client app (nav, i18n).
   ========================================================================== */
import { build } from "esbuild";
import { readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");
const DIST = resolve(ROOT, "dist");
const ORIGIN = "https://kvasir-ai.net";
const BUILD_DATE = readFileSync(resolve(DIST, "index.html"), "utf8") && new Date().toISOString().slice(0, 10);

/* ---- 1. load English content data (TS → ESM via esbuild) ---------------- */
const tmp = resolve(ROOT, ".seo-data.mjs");
writeFileSync(
  tmp,
  `export { TECH_ARTICLES } from "./src/tech/articles.ts";\n` +
    `export { WIKI_ENTRIES } from "./src/wiki/entries.ts";\n`
);
await build({
  entryPoints: [tmp],
  bundle: true,
  format: "esm",
  outfile: resolve(ROOT, ".seo-data.bundle.mjs"),
  logLevel: "silent",
});
const { TECH_ARTICLES, WIKI_ENTRIES } = await import(
  resolve(ROOT, ".seo-data.bundle.mjs") + "?t=" + Date.now()
);
rmSync(tmp, { force: true });
rmSync(resolve(ROOT, ".seo-data.bundle.mjs"), { force: true });

/* ---- 2. helpers --------------------------------------------------------- */
const esc = (s = "") =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const escAttr = (s = "") => esc(s).replace(/'/g, "&#39;");

/* inline **bold** / `code` → HTML (mirrors blocks.tsx inlineMd) */
function inlineMd(md = "") {
  return md
    .split(/(\*\*[^*]+\*\*|`[^`]+`)/g)
    .map((part) => {
      if (part.startsWith("**") && part.endsWith("**"))
        return `<strong class="font-semibold text-ink">${esc(part.slice(2, -2))}</strong>`;
      if (part.startsWith("`") && part.endsWith("`"))
        return `<code class="rounded bg-surface-3 px-1.5 py-0.5 font-mono text-[0.85em] text-brand-300">${esc(
          part.slice(1, -1)
        )}</code>`;
      return esc(part);
    })
    .join("");
}

/* block → HTML (mirrors blocks.tsx BlockView classes) */
function blockHtml(b) {
  switch (b.t) {
    case "h2":
      return `<div class="mt-12">${
        b.kick ? `<div class="text-xs font-semibold uppercase tracking-[0.18em] text-brand-400">${esc(b.kick)}</div>` : ""
      }<h2 class="mt-2 text-2xl font-semibold text-ink">${esc(b.text)}</h2></div>`;
    case "p":
      return `<p class="mt-5 max-w-3xl leading-relaxed text-ink-muted">${inlineMd(b.md)}</p>`;
    case "callout":
      return `<div class="mt-6 rounded-2xl bg-brand-500/8 p-6 ring-1 ring-brand-500/25"><p class="leading-relaxed text-ink-muted">${inlineMd(
        b.md
      )}</p></div>`;
    case "stats":
      return `<div class="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">${b.items
        .map(
          (s) =>
            `<div class="rounded-2xl bg-surface-2 p-4 ring-1 ring-line"><div class="font-mono text-xl font-bold tracking-tight text-brand-gradient">${esc(
              s.n
            )}</div><div class="mt-1 font-mono text-[0.7rem] text-ink-faint">${esc(s.l)}</div></div>`
        )
        .join("")}</div>`;
    case "ul":
      return `<ul class="mt-5 max-w-3xl space-y-3">${b.items
        .map(
          (it) =>
            `<li class="flex items-start gap-3 text-sm leading-relaxed text-ink-muted"><span class="mt-2 h-1.5 w-1.5 shrink-0 rounded-sm bg-brand-400"></span><span>${inlineMd(
              it
            )}</span></li>`
        )
        .join("")}</ul>`;
    case "code":
      return `<div class="mt-6">${
        b.caption ? `<p class="mb-2 text-xs text-ink-faint">${inlineMd(b.caption)}</p>` : ""
      }<div class="overflow-x-auto rounded-2xl bg-surface p-5 ring-1 ring-line"><pre class="font-mono text-[0.8rem] leading-relaxed text-ink-muted">${esc(
        b.code
      )}</pre></div></div>`;
    case "img":
      return `<div class="mt-6 overflow-hidden rounded-2xl ring-1 ring-line"><img src="${escAttr(
        b.src
      )}" alt="${escAttr(b.alt)}" loading="lazy" class="block w-full"></div>`;
    case "table":
      return `<div class="mt-6 overflow-x-auto rounded-2xl ring-1 ring-line"><table class="w-full min-w-[28rem] text-left text-sm"><thead><tr class="bg-surface-2/70">${b.head
        .map(
          (h) =>
            `<th class="px-4 py-3 font-mono text-[0.68rem] font-semibold uppercase tracking-wider text-ink-faint">${esc(
              h
            )}</th>`
        )
        .join("")}</tr></thead><tbody>${b.rows
        .map(
          (row) =>
            `<tr class="border-t border-line">${row
              .map(
                (cell, ci) =>
                  `<td class="px-4 py-3 ${ci === 0 ? "font-mono text-ink" : "text-ink-muted"}">${inlineMd(
                    cell
                  )}</td>`
              )
              .join("")}</tr>`
        )
        .join("")}</tbody></table></div>`;
    default:
      return "";
  }
}

/* plain text of a block (for RSS description fallback / meta) */
function blocksToHtml(blocks) {
  return blocks.map(blockHtml).join("\n");
}

/* ---- 3. route model ----------------------------------------------------- */
const blogRoutes = TECH_ARTICLES.map((a) => ({
  path: `/technology/${a.slug}`,
  title: `${a.title} — Kvasir`,
  description: a.dek,
  image: `${ORIGIN}/blog/${a.slug}.jpg`,
  date: a.date,
  kind: "article",
  tags: a.tags,
  bodyHtml:
    `<article class="mx-auto max-w-3xl px-6 py-24">` +
    `<h1 class="text-3xl font-semibold tracking-tight text-ink sm:text-4xl">${esc(a.title)}</h1>` +
    `<p class="mt-4 text-lg leading-relaxed text-ink-muted">${esc(a.dek)}</p>` +
    blocksToHtml(a.blocks) +
    `</article>`,
  jsonld: [
    {
      "@type": "TechArticle",
      headline: a.title,
      description: a.dek,
      datePublished: a.date,
      dateModified: a.date,
      image: `${ORIGIN}/blog/${a.slug}.jpg`,
      keywords: (a.tags || []).join(", "),
      author: { "@type": "Organization", name: "Kvasir", url: ORIGIN },
      publisher: { "@type": "Organization", name: "Kvasir", url: ORIGIN },
      mainEntityOfPage: `${ORIGIN}/technology/${a.slug}`,
    },
    breadcrumb([["Blog", "/technology"], [a.title, `/technology/${a.slug}`]]),
  ],
}));

const wikiRoutes = WIKI_ENTRIES.map((e) => ({
  path: `/wiki/${e.slug}`,
  title: `${e.title} — Kvasir Wiki`,
  description: e.summary,
  image: `${ORIGIN}/wiki/${e.slug}.jpg`,
  date: null,
  kind: "article",
  tags: [],
  bodyHtml:
    `<article class="mx-auto max-w-3xl px-6 py-24">` +
    `<h1 class="text-3xl font-semibold tracking-tight text-ink sm:text-4xl">${esc(e.title)}</h1>` +
    `<p class="mt-4 text-lg leading-relaxed text-ink-muted">${esc(e.summary)}</p>` +
    blocksToHtml(e.blocks) +
    `</article>`,
  jsonld: [
    {
      "@type": "DefinedTerm",
      name: e.title,
      description: e.summary,
      inDefinedTermSet: { "@type": "DefinedTermSet", name: "Kvasir Wiki", url: `${ORIGIN}/wiki` },
      url: `${ORIGIN}/wiki/${e.slug}`,
    },
    breadcrumb([["Wiki", "/wiki"], [e.title, `/wiki/${e.slug}`]]),
  ],
}));

function linkList(items, base) {
  return (
    `<div class="mx-auto max-w-3xl px-6 py-24"><ul class="space-y-2">` +
    items
      .map(
        (it) =>
          `<li><a class="text-brand-300 hover:underline" href="${base}/${it.slug}">${esc(it.title)}</a>${
            it.summary || it.dek ? ` — <span class="text-ink-muted">${esc(it.summary || it.dek)}</span>` : ""
          }</li>`
      )
      .join("") +
    `</ul></div>`
  );
}

const staticRoutes = [
  {
    path: "/",
    title: "Kvasir — Decentralized AI Inference. Bring compute, earn KVR.",
    description:
      "Kvasir is a decentralized AI inference network. It splits large open models across a peer-to-peer ring of shared GPUs, CPUs, NPUs and phones with linkcpp — contribute compute, earn KVR. Solana devnet.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold tracking-tight text-ink">Decentralized AI inference. Bring compute, earn KVR.</h1>` +
      `<p class="mt-4 text-lg text-ink-muted">Run frontier-scale open models on a peer-to-peer ring of shared GPUs, CPUs, NPUs and phones — and earn KVR for the compute you contribute. Source-available (BSL) engine, Solana devnet.</p></section>`,
    jsonld: [],
  },
  {
    path: "/run-node",
    title: "Run a node — Kvasir",
    description:
      "Turn your machine into a Kvasir node. Contribute GPU, CPU, NPU or even a phone to the decentralized inference ring and earn KVR for the layers you serve. Non-custodial, Solana devnet.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Run a Kvasir node</h1><p class="mt-4 text-lg text-ink-muted">Join the network with the desktop or mobile app, serve model layers, and earn KVR to your own wallet.</p></section>`,
    jsonld: [breadcrumb([["Run a node", "/run-node"]])],
  },
  {
    path: "/careers",
    title: "Marketing & Growth — Kvasir",
    description:
      "Join Kvasir's marketing & growth team — help a decentralized AI inference network reach the builders who will run it.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Marketing &amp; Growth at Kvasir</h1><p class="mt-4 text-lg text-ink-muted">Grow a decentralized AI inference network. Devnet utility token; no financial-return promises.</p></section>`,
    jsonld: [breadcrumb([["Careers", "/careers"]])],
  },
  {
    path: "/docs/api",
    title: "Kvasir API — pay-per-inference with KVR",
    description:
      "Call Kvasir inference paid in KVR. Discover live models, quote, pay on-chain, redeem — with an OpenAI-compatible adapter and code in seven languages. Solana devnet.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Kvasir developer API</h1><p class="mt-4 text-lg text-ink-muted">Pay-per-inference in KVR over an OpenAI-compatible surface: discover a model, quote, pay on-chain from your own wallet, redeem. Non-custodial, Solana devnet.</p></section>`,
    jsonld: [breadcrumb([["API", "/docs/api"]])],
  },
  {
    path: "/technology",
    title: "Blog — Kvasir",
    description: "Engineering notes from Kvasir: swarm inference, expert sharding, the ring runtime, settlement security and token economics.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml: `<section><h1 class="mx-auto max-w-3xl px-6 pt-24 text-4xl font-semibold text-ink">Kvasir blog</h1>${linkList(
      TECH_ARTICLES,
      "/technology"
    )}</section>`,
    jsonld: [breadcrumb([["Blog", "/technology"]])],
  },
  {
    path: "/wiki",
    title: "Wiki — Kvasir",
    description: "The Kvasir knowledge base — concise, precise entries on every concept in the decentralized inference network, from the ring runtime to KVR rewards.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml: `<section><h1 class="mx-auto max-w-3xl px-6 pt-24 text-4xl font-semibold text-ink">Kvasir wiki</h1>${linkList(
      WIKI_ENTRIES,
      "/wiki"
    )}</section>`,
    jsonld: [breadcrumb([["Wiki", "/wiki"]])],
  },
];

function breadcrumb(pairs) {
  return {
    "@type": "BreadcrumbList",
    itemListElement: [{ name: "Home", url: ORIGIN }, ...pairs.map(([n, p]) => ({ name: n, url: ORIGIN + p }))].map(
      (it, i) => ({ "@type": "ListItem", position: i + 1, name: it.name, item: it.url })
    ),
  };
}

const ALL = [...staticRoutes, ...blogRoutes, ...wikiRoutes];

/* ---- 4. per-route HTML from the built dist/index.html template ---------- */
const template = readFileSync(resolve(DIST, "index.html"), "utf8");

function pageHtml(r) {
  let html = template;
  const url = ORIGIN + (r.path === "/" ? "/" : r.path);
  const type = r.kind === "article" ? "article" : "website";

  // <title>
  html = html.replace(/<title>[\s\S]*?<\/title>/, `<title>${esc(r.title)}</title>`);
  // description (multi-line meta)
  html = html.replace(
    /<meta\s+name="description"[\s\S]*?\/>/,
    `<meta name="description" content="${escAttr(r.description)}" />`
  );
  // canonical
  html = html.replace(/<link rel="canonical"[^>]*\/>/, `<link rel="canonical" href="${url}" />`);
  // OG
  html = html
    .replace(/<meta property="og:type"[^>]*\/>/, `<meta property="og:type" content="${type}" />`)
    .replace(/<meta property="og:title"[^>]*\/>/, `<meta property="og:title" content="${escAttr(r.title)}" />`)
    .replace(
      /<meta\s+property="og:description"[\s\S]*?\/>/,
      `<meta property="og:description" content="${escAttr(r.description)}" />`
    )
    .replace(/<meta property="og:url"[^>]*\/>/, `<meta property="og:url" content="${url}" />`)
    .replace(/<meta property="og:image"[^>]*\/>/, `<meta property="og:image" content="${escAttr(r.image)}" />`);
  // Twitter
  html = html
    .replace(/<meta name="twitter:title"[^>]*\/>/, `<meta name="twitter:title" content="${escAttr(r.title)}" />`)
    .replace(
      /<meta\s+name="twitter:description"[\s\S]*?\/>/,
      `<meta name="twitter:description" content="${escAttr(r.description)}" />`
    )
    .replace(/<meta name="twitter:image"[^>]*\/>/, `<meta name="twitter:image" content="${escAttr(r.image)}" />`);

  // head injections: article:published_time + rss alternate + per-route JSON-LD
  const inject = [];
  if (r.date) inject.push(`<meta property="article:published_time" content="${escAttr(r.date)}" />`);
  inject.push(`<link rel="alternate" type="application/rss+xml" title="Kvasir blog" href="${ORIGIN}/feed.xml" />`);
  if (r.jsonld && r.jsonld.length) {
    const graph = r.jsonld.map((n) => ({ "@context": "https://schema.org", ...n }));
    inject.push(
      `<script type="application/ld+json">${JSON.stringify(graph.length === 1 ? graph[0] : graph)}</script>`
    );
  }
  html = html.replace("</head>", `    ${inject.join("\n    ")}\n  </head>`);

  // prerendered body into #root (SPA createRoot replaces it on mount)
  html = html.replace(
    /(<div id="root">)(<\/div>)/,
    `$1<div data-prerender>${r.bodyHtml}</div>$2`
  );
  return html;
}

let count = 0;
for (const r of ALL) {
  // Flat `<path>.html` files (not `<path>/index.html`) so Cloudflare Pages
  // serves them at the clean no-trailing-slash URL with 200 — no 308 redirect,
  // so the canonical/OG/sitemap URLs match the actually-served URL exactly.
  const outFile = r.path === "/" ? resolve(DIST, "index.html") : resolve(DIST, r.path.replace(/^\//, "") + ".html");
  mkdirSync(dirname(outFile), { recursive: true });
  writeFileSync(outFile, pageHtml(r), "utf8");
  count++;
}

/* ---- 5. RSS 2.0 feed (full content:encoded) ----------------------------- */
const sorted = [...TECH_ARTICLES].sort((a, b) => (a.date < b.date ? 1 : -1));
const rssItems = sorted
  .map((a) => {
    const link = `${ORIGIN}/technology/${a.slug}`;
    const content = blocksToHtml(a.blocks);
    return (
      `<item><title>${esc(a.title)}</title><link>${link}</link><guid isPermaLink="true">${link}</guid>` +
      `<pubDate>${new Date(a.date + "T00:00:00Z").toUTCString()}</pubDate>` +
      `<description>${esc(a.dek)}</description>` +
      (a.tags || []).map((t) => `<category>${esc(t)}</category>`).join("") +
      `<content:encoded><![CDATA[${content}]]></content:encoded></item>`
    );
  })
  .join("");
const rss =
  `<?xml version="1.0" encoding="UTF-8"?>\n` +
  `<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:atom="http://www.w3.org/2005/Atom">\n` +
  `<channel><title>Kvasir Blog</title><link>${ORIGIN}/technology</link>` +
  `<atom:link href="${ORIGIN}/feed.xml" rel="self" type="application/rss+xml" />` +
  `<description>Engineering notes from the Kvasir decentralized AI inference network.</description>` +
  `<language>en</language><lastBuildDate>${new Date().toUTCString()}</lastBuildDate>` +
  rssItems +
  `</channel></rss>\n`;
writeFileSync(resolve(DIST, "feed.xml"), rss, "utf8");

/* ---- 6. sitemap.xml (all routes) ---------------------------------------- */
const smUrls = ALL.map((r) => {
  const loc = ORIGIN + (r.path === "/" ? "/" : r.path);
  const lastmod = r.date || BUILD_DATE;
  const pri = r.path === "/" ? "1.0" : r.kind === "article" ? "0.7" : "0.8";
  return `<url><loc>${loc}</loc><lastmod>${lastmod}</lastmod><changefreq>weekly</changefreq><priority>${pri}</priority></url>`;
}).join("\n");
writeFileSync(
  resolve(DIST, "sitemap.xml"),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${smUrls}\n</urlset>\n`,
  "utf8"
);

/* ---- 7. 404 page -------------------------------------------------------- */
let notFound = template
  .replace(/<title>[\s\S]*?<\/title>/, `<title>Not found — Kvasir</title>`)
  .replace(/<meta name="robots"[^>]*\/>/, `<meta name="robots" content="noindex" />`)
  .replace(/(<div id="root">)(<\/div>)/, `$1<div data-prerender><section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Page not found</h1><p class="mt-4 text-ink-muted"><a class="text-brand-300 hover:underline" href="/">Return home</a></p></section></div>$2`);
writeFileSync(resolve(DIST, "404.html"), notFound, "utf8");

console.log(`[seo] prerendered ${count} routes · feed.xml (${sorted.length} items) · sitemap.xml (${ALL.length} urls) · 404.html`);
