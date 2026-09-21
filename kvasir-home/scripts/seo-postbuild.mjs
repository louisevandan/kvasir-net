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
    `export { WIKI_ENTRIES } from "./src/wiki/entries.ts";\n` +
    `export { getTechArticles } from "./src/tech/translations.ts";\n` +
    `export { getWikiEntries } from "./src/wiki/translations.ts";\n` +
    `export { LANGS, DEFAULT_LANG } from "./src/i18n/langs.ts";\n` +
    `export { DICTS } from "./src/i18n/dicts.ts";\n`
);
await build({
  entryPoints: [tmp],
  bundle: true,
  format: "esm",
  outfile: resolve(ROOT, ".seo-data.bundle.mjs"),
  logLevel: "silent",
});
const { TECH_ARTICLES, WIKI_ENTRIES, getTechArticles, getWikiEntries, LANGS, DEFAULT_LANG, DICTS } = await import(
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

/** The homepage as text, from the dictionary of the requested language. */
function homeBody(lang) {
  const t = DICTS[lang] ?? DICTS[DEFAULT_LANG];
  const li = (items) => items.map((x) => `<li>${esc(x)}</li>`).join("");
  const section = (heading, lede, items) =>
    `<section class="mx-auto max-w-3xl px-6 py-10">` +
    `<h2 class="text-2xl font-semibold tracking-tight text-ink">${esc(heading)}</h2>` +
    (lede ? `<p class="mt-3 text-ink-muted">${esc(lede)}</p>` : "") +
    (items && items.length ? `<ul class="mt-4 list-disc space-y-2 pl-5 text-ink-muted">${li(items)}</ul>` : "") +
    `</section>`;

  const steps = (t.how?.steps ?? []).map((s) => `${s.title}: ${s.body}`);
  const points = (t.tech?.points ?? []).map((p) => `${p.title}: ${p.body}`);
  const roadmap = (t.roadmap?.items ?? []).map((i) => `${i.phase} — ${i.title}: ${i.body}`);
  const proof = [...(t.proof?.items ?? []), t.proof?.strip].filter(Boolean);

  return (
    `<section class="mx-auto max-w-3xl px-6 py-24">` +
    `<p class="text-sm uppercase tracking-wider text-ink-faint">${esc(t.hero?.eyebrow ?? "")}</p>` +
    `<h1 class="mt-3 text-4xl font-semibold tracking-tight text-ink">${esc(t.hero?.headline1 ?? "")} ${esc(t.hero?.headline2 ?? "")}</h1>` +
    `<p class="mt-4 text-lg text-ink-muted">${esc(t.hero?.sub ?? "")}</p>` +
    (t.hero?.badges?.length ? `<ul class="mt-4 flex flex-wrap gap-2 text-sm text-ink-muted">${li(t.hero.badges)}</ul>` : "") +
    `</section>` +
    section(t.thesis?.title ?? "", t.thesis?.lede ?? "", t.thesis?.kvasirPoints ?? []) +
    section(t.how?.title ?? "", t.how?.lede ?? "", steps) +
    section(t.tech?.title ?? "", t.tech?.lede ?? "", points) +
    section(t.proof?.title ?? "", t.proof?.pill ?? "", proof) +
    section(t.roadmap?.title ?? "", t.roadmap?.lede ?? "", roadmap)
  );
}

/**
 * The questions an assistant is actually asked about a project like this, with
 * the answers we would want quoted — from the dictionary, so every language
 * gets its own, and grounded in the same facts the page states.
 */
function faqFor(lang) {
  const t = DICTS[lang] ?? DICTS[DEFAULT_LANG];
  const qa = [
    [t.thesis?.title, t.hero?.sub],
    [t.how?.title, (t.how?.steps ?? []).map((s) => `${s.title}: ${s.body}`).join(" ")],
    [t.tech?.title, t.tech?.lede],
    [t.proof?.title, t.proof?.strip],
  ].filter(([q, a]) => q && a);
  return {
    "@type": "FAQPage",
    mainEntity: qa.map(([q, a]) => ({
      "@type": "Question",
      name: q,
      acceptedAnswer: { "@type": "Answer", text: a },
    })),
  };
}
/**
 * Routes, per language.
 *
 * The site is written in nine languages and used to publish one URL, so a
 * crawler indexed one of them and the other eight did not exist as far as
 * search or an LLM was concerned. Each language now gets its own path —
 * English bare, the rest under a prefix — and every page lists all nine as
 * alternates so they are understood as one document, not nine duplicates.
 */
const routesFor = (lang) => {
const TECH = lang === DEFAULT_LANG ? TECH_ARTICLES : getTechArticles(lang);
const WIKI = lang === DEFAULT_LANG ? WIKI_ENTRIES : getWikiEntries(lang);
const blogRoutes = TECH.map((a) => ({
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

const wikiRoutes = WIKI.map((e) => ({
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
    // The title is what a search result and a shared link show, so it carries
    // the same claim the page now leads with: a model too large for any one
    // machine, running across ordinary ones. The old title sold the supply
    // side, which rests on a network that is not open yet.
    title: "Kvasir — a 428B model on machines that cannot hold it",
    description:
      "Kvasir runs open models larger than any single machine serving them. The p4 engine cuts a model into layer windows across a peer-to-peer ring of GPUs, CPUs, NPUs and phones, each holding only its own. Step-3.7-Flash, 428B parameters, at 28–31 tokens a second. Solana devnet.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    // Built from the dictionary rather than written here: a crawler that does
    // not run JavaScript used to see a headline and one sentence, in English,
    // whichever language the URL asked for. It now reads the page's actual
    // claims — what is served, what was measured, how a node is paid — in the
    // language it requested.
    bodyHtml: homeBody(lang),
    jsonld: [faqFor(lang)],
  },
  {
    path: "/run-node",
    title: "Run a node — Kvasir",
    description:
      "Turn your machine into a Kvasir node. Contribute GPU, CPU, NPU or even a phone to the decentralized inference ring and earn KVR for the layers you serve. Self-custody wallet, Solana devnet.",
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
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Kvasir developer API</h1><p class="mt-4 text-lg text-ink-muted">Pay-per-inference in KVR over an OpenAI-compatible surface: discover a model, quote, pay on-chain from your own wallet, redeem. Solana devnet.</p></section>`,
    jsonld: [breadcrumb([["API", "/docs/api"]])],
  },
  {
    path: "/releases",
    title: "Release notes — Kvasir",
    description:
      "What shipped on the Kvasir network and what it was measured on: the p4 engine, the models being served, the gateway and the wallet.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">What shipped, and what it was measured on</h1><p class="mt-4 text-lg text-ink-muted">Release notes for the Kvasir network: the p4 engine, the models being served, the settlement gateway and the wallet. Each entry carries the evidence it was accepted on.</p></section>`,
    jsonld: [breadcrumb([["Release notes", "/releases"]])],
  },
  {
    path: "/install",
    title: "Installing the mobile builds — Kvasir",
    description:
      "How to install the Kvasir wallet on Android and iOS. Both are development builds, and both platforms ask the owner of the device to allow one on purpose.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Installing the mobile builds</h1><p class="mt-4 text-lg text-ink-muted">The Kvasir wallet is not in the App Store or on Google Play yet. The Android package is signed with a development key and installs once you permit your browser to install it. iOS will not run an application that is not signed for your specific device, so there is no file to tap — you build it with Xcode and a free Apple ID.</p></section>`,
    jsonld: [
      breadcrumb([["Installing the mobile builds", "/install"]]),
      howTo({
        name: "Install the Kvasir wallet on Android",
        description:
          "The Android package is a development build. Android asks the owner of the device to allow an install from outside a store, and this is how.",
        path: "/install",
        steps: [
          { name: "Download the APK on the phone", text: "Open the install page on the phone itself and tap Download the APK. Copying the file from a computer also works but is the longer road." },
          { name: "Allow your browser to install apps", text: "Android blocks installs from outside a store until you permit a specific app to ask. Follow the prompt to Settings, Install unknown apps, and turn it on for that browser only." },
          { name: "Install, and read the scanner's warning", text: "Play Protect will say the app was not scanned or comes from an unknown developer, which is what it should say about a build signed with a development key. Choose Install anyway only because you know where the file came from." },
          { name: "If it says App not installed", text: "Either an older Kvasir build signed with a different key is already installed — uninstall it first, exporting your recovery phrase beforehand — or the download was truncated." },
          { name: "Verify what you installed", text: "Compare the file's SHA-256 against the value published on the install page." },
        ],
      }),
      howTo({
        name: "Build and install the Kvasir wallet on iOS",
        description:
          "Apple will not run an application that is not signed for the specific device, so there is no file to tap. You build it with Xcode and a free Apple ID.",
        path: "/install",
        steps: [
          { name: "Get the source and its build tooling", text: "Clone github.com/louisevandan/kvasir-net and install xcodegen with Homebrew." },
          { name: "Build the native libraries the app links", text: "Initialise the submodules and run scripts/build-ios-ring.sh. The wallet runs inference on the device, so it links a compiled ring stage and expert worker." },
          { name: "Generate the Xcode project", text: "The project file is generated from wallet/ios/project.yml rather than committed. Run xcodegen generate in wallet/ios and open the project." },
          { name: "Sign it with your own Apple ID", text: "In Signing and Capabilities, enable automatic signing, choose your own team, and change the bundle identifier to one of your own — a free account cannot claim an identifier someone else registered." },
          { name: "Connect the phone and run", text: "Plug the phone in, unlock it, tap Trust, pick it as the run destination and press Run." },
          { name: "Trust the developer on the phone", text: "The first launch is refused with Untrusted Developer. In Settings, General, VPN and Device Management, tap your Apple ID and trust it." },
        ],
      }),
      softwareApp({
        name: "Kvasir Wallet for Android",
        os: "Android 8.0+",
        url: "/download/Kvasir-Wallet-android-arm64.apk",
        description: "Self-custody KVR wallet and inference node for Android. Development build.",
      }),
      softwareApp({
        name: "Kvasir Wallet for macOS",
        os: "macOS",
        url: "/download/Kvasir-Wallet-mac-universal.dmg",
        description: "Self-custody KVR wallet and node dashboard for macOS, supervising a p4 agent.",
      }),
      softwareApp({
        name: "Kvasir Wallet for Windows",
        os: "Windows",
        url: "/download/Kvasir-Wallet-win-x64.exe",
        description: "Self-custody KVR wallet and node dashboard for Windows, supervising a p4 agent.",
      }),
    ],
  },
  {
    path: "/legal",
    title: "Terms of use & privacy — Kvasir",
    description:
      "Terms of use and privacy notice for the Kvasir devnet preview: the website, gateway, hub and wallet apps, and the data they handle.",
    image: `${ORIGIN}/og.png`,
    kind: "website",
    bodyHtml:
      `<section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Terms of use &amp; privacy</h1><p class="mt-4 text-lg text-ink-muted">Kvasir is a devnet preview. KVR is a devnet utility token with no monetary value. Wallet keys stay on your device; on devnet, staked KVR and prepaid credits are held by the gateway treasury. Pay-per-call prompts and answers are stored by the gateway.</p></section>`,
    jsonld: [breadcrumb([["Terms & privacy", "/legal"]])],
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

/**
 * Step-by-step instructions, declared as such.
 *
 * /install is a how-to in the literal sense — a person follows it with a phone
 * in hand — and search engines and assistants surface that kind of page
 * differently when it says so. The steps here mirror the page; when the page
 * changes, change both, because a HowTo that disagrees with its own page is
 * worse than none.
 */
function howTo({ name, description, path, steps }) {
  return {
    "@type": "HowTo",
    name,
    description,
    url: ORIGIN + path,
    step: steps.map((step, i) => ({
      "@type": "HowToStep",
      position: i + 1,
      name: step.name,
      text: step.text,
      url: `${ORIGIN}${path}#step-${i + 1}`,
    })),
  };
}

/** The wallet, as software someone can actually download and run. */
function softwareApp({ name, os, url, description }) {
  return {
    "@type": "SoftwareApplication",
    name,
    applicationCategory: "FinanceApplication",
    operatingSystem: os,
    description,
    softwareVersion: "0.1.0",
    url: ORIGIN + "/run-node",
    downloadUrl: url.startsWith("http") ? url : ORIGIN + url,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    author: { "@type": "Organization", name: "Kvasir AI Network", url: ORIGIN },
  };
}

function breadcrumb(pairs) {
  return {
    "@type": "BreadcrumbList",
    itemListElement: [{ name: "Home", url: ORIGIN }, ...pairs.map(([n, p]) => ({ name: n, url: ORIGIN + p }))].map(
      (it, i) => ({ "@type": "ListItem", position: i + 1, name: it.name, item: it.url })
    ),
  };
}

return [...staticRoutes, ...blogRoutes, ...wikiRoutes];
};

/** Where a route lives for a language: English bare, everything else prefixed. */
const localePath = (lang, path) =>
  lang === DEFAULT_LANG ? path : `/${lang}${path === "/" ? "" : path}`;

const LANG_CODES = LANGS.map((l) => l.code);
const BY_LANG = new Map(LANG_CODES.map((lang) => [lang, routesFor(lang)]));
const ALL = BY_LANG.get(DEFAULT_LANG);

/* ---- 4. per-route HTML from the built dist/index.html template ---------- */
const template = readFileSync(resolve(DIST, "index.html"), "utf8");

function pageHtml(r, lang = DEFAULT_LANG) {
  let html = template;
  const here = localePath(lang, r.path);
  const url = ORIGIN + (here === "/" ? "/" : here);
  const type = r.kind === "article" ? "article" : "website";
  // The served document must declare the language it is actually written in,
  // or a translated page reads to a crawler as English prose it cannot parse.
  html = html.replace(/<html lang="[^"]*"/, `<html lang="${lang}"`);

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
  // Every language of this page, named to each other. x-default points at the
  // English one, which is what a crawler with no language preference gets.
  for (const code of LANG_CODES) {
    const alt = localePath(code, r.path);
    inject.push(
      `<link rel="alternate" hreflang="${code}" href="${ORIGIN}${alt === "/" ? "/" : alt}" />`
    );
  }
  inject.push(`<link rel="alternate" hreflang="x-default" href="${ORIGIN}${r.path === "/" ? "/" : r.path}" />`);
  inject.push(`<meta property="og:locale" content="${lang}" />`);
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
  // Function replacer: bodyHtml can contain "$" sequences (e.g. "~$100") that a
  // string replacement would treat as capture-group references.
  html = html.replace(
    /(<div id="root">)(<\/div>)/,
    (_m, open, close) => `${open}<div data-prerender>${r.bodyHtml}</div>${close}`
  );
  return html;
}

let count = 0;
for (const [lang, routes] of BY_LANG) {
for (const r of routes) {
  // Flat `<path>.html` files (not `<path>/index.html`) so Cloudflare Pages
  // serves them at the clean no-trailing-slash URL with 200 — no 308 redirect,
  // so the canonical/OG/sitemap URLs match the actually-served URL exactly.
  const here = localePath(lang, r.path);
  const outFile = here === "/" ? resolve(DIST, "index.html") : resolve(DIST, here.replace(/^\//, "") + ".html");
  mkdirSync(dirname(outFile), { recursive: true });
  writeFileSync(outFile, pageHtml(r, lang), "utf8");
  count++;
}
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
// Every language of every route, each entry listing its alternates so the set
// is understood as one document in nine languages rather than nine rivals.
const smUrls = ALL.flatMap((r) =>
  LANG_CODES.map((lang) => {
    const here = localePath(lang, r.path);
    const loc = ORIGIN + (here === "/" ? "/" : here);
    const lastmod = r.date || BUILD_DATE;
    const pri = r.path === "/" ? (lang === DEFAULT_LANG ? "1.0" : "0.9") : r.kind === "article" ? "0.7" : "0.8";
    const alts = LANG_CODES.map((code) => {
      const alt = localePath(code, r.path);
      return `<xhtml:link rel="alternate" hreflang="${code}" href="${ORIGIN}${alt === "/" ? "/" : alt}"/>`;
    }).join("");
    return `<url><loc>${loc}</loc>${alts}<lastmod>${lastmod}</lastmod><changefreq>weekly</changefreq><priority>${pri}</priority></url>`;
  })
).join("\n");
writeFileSync(
  resolve(DIST, "sitemap.xml"),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${smUrls}\n</urlset>\n`,
  "utf8"
);

/* ---- 7. 404 page -------------------------------------------------------- */
let notFound = template
  .replace(/<title>[\s\S]*?<\/title>/, `<title>Not found — Kvasir</title>`)
  .replace(/<meta name="robots"[^>]*\/>/, `<meta name="robots" content="noindex" />`)
  .replace(/(<div id="root">)(<\/div>)/, (_m, open, close) => `${open}<div data-prerender><section class="mx-auto max-w-3xl px-6 py-24"><h1 class="text-4xl font-semibold text-ink">Page not found</h1><p class="mt-4 text-ink-muted"><a class="text-brand-300 hover:underline" href="/">Return home</a></p></section></div>${close}`);
writeFileSync(resolve(DIST, "404.html"), notFound, "utf8");

console.log(`[seo] prerendered ${count} routes · feed.xml (${sorted.length} items) · sitemap.xml (${ALL.length} urls) · 404.html`);
