/* ==========================================================================
   Language-AGNOSTIC data for the Kvasir homepage.
   All translatable copy lives in src/i18n/<lang>.ts. This file holds only the
   things that never translate: URLs, code, numeric facts, tier values, and the
   device / node structure. Grounded in homepage-brief.md; compliance constraints
   (devnet, utility token, non-custodial) are honored across every language.
   ========================================================================== */

export const LINKS = {
  github: "https://github.com/louisevandan/kvasir-net",
  // There is no hub link. The control plane that answered at hub.kvasir-ai.net
  // was retired, the bridge took over its role, and the DNS record was removed
  // on 2026-09-21 — the host does not resolve, so a link to it cannot even fail
  // informatively.
  gateway: "https://gate.kvasir-ai.net",
  runNode: "/run-node",
  technology: "/technology",
  wiki: "/wiki",
  api: "/docs/api",
  feed: "/feed.xml",
  // KVR SPL token on the Solana explorer — on-chain transaction activity.
  explorerDevnet:
    "https://explorer.solana.com/address/6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ?cluster=devnet",
};

/* Download destinations for the node-operator guide. Empty string => the guide
   renders a "준비 중 (coming soon)" state instead of a dead link. Fill these in
   as the desktop installers ship and the mobile apps are published. */
// Desktop installers are published under kvasir-ai.net/download/... — see
// public/_redirects, which forwards each key to the Cloudflare R2 bucket
// `kvasir-downloads`. Keys carry no version, so re-uploading the same key ships
// a new build without invalidating a single published link.
const DOWNLOAD_BASE = "/download";
export const DOWNLOADS = {
  desktopMac: `${DOWNLOAD_BASE}/Kvasir-Wallet-mac-universal.dmg`,
  desktopWin: `${DOWNLOAD_BASE}/Kvasir-Wallet-win-x64.exe`,
  desktopLinux: `${DOWNLOAD_BASE}/Kvasir-Wallet-linux-x64.tar.gz`,
  // A development build, signed with a development key. /install says what that
  // means before anyone taps it — Android refuses the install until the owner
  // permits it, and a download with no explanation just reads as broken.
  android: `${DOWNLOAD_BASE}/Kvasir-Wallet-android-arm64.apk`,
  // iOS has no equivalent file to host: Apple will not run an application that
  // is not signed for the specific device, so /install explains building it.
  installGuide: "/install",
  appStore: "",
  googlePlay: "",
};

/* Nav section anchors — labels come from the active dictionary (t.nav[key]). */
export const NAV_ITEMS = [
  { key: "why", href: "/#why" },
  { key: "how", href: "/#how" },
  { key: "contributors", href: "/#contributors" },
  { key: "developers", href: "/docs/api" },
  { key: "token", href: "/#token" },
  { key: "rewards", href: "/#network" },
  {
    key: "technology",
    children: [
      { key: "blog", href: "/technology" },
      { key: "wiki", href: "/wiki" },
    ],
  },
  // Top level rather than tucked into a dropdown: at seed stage the question
  // "who is building this" is asked early and answered nowhere else, and the
  // footer link was invisible on mobile, where the nav is a hamburger.
  { key: "team", href: "/team" },
] as const;

/* Example request — code, never translated. */
export const CODE_SNIPPET = `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "step-3.7-flash",
    "messages": [
      { "role": "user", "content": "Explain layer-split inference." }
    ]
  }'`;

/* Reward formula expressions — code-like, kept identical across languages
   (only the labels in t.network.formula[i].label are translated). */
export const REWARD_EXPR = [
  "units += (tokens / 1k) × (node_layers / total_layers)",
  "effective = units × perf_tier × gateway_bonus",
  "hub_uptime/hr  >  gateway_uptime/hr   (summed on top)",
];

/* Performance tiers — tier letters, thresholds and multipliers are universal. */
export const PERF_TIERS = [
  { tier: "S", tps: "≥ 90 tok/s", mult: "×1.5" },
  { tier: "A", tps: "≥ 60 tok/s", mult: "×1.25" },
  { tier: "B", tps: "≥ 30 tok/s", mult: "×1.0" },
  { tier: "C", tps: "< 30 tok/s", mult: "×0.7" },
];

/* The engine's own one-liner — a verbatim English quote, shown as a callout. */
export const ENGINE_TAGLINE =
  "Run large AI models across multiple GPUs and machines on an inference engine kept close to upstream.";

/* Device types (name + status are universal; the detail line is translated in
   t.contributors.deviceDetails, aligned by index). */
export const DEVICE_META = [
  { name: "GPU", status: "live" as const },
  { name: "CPU", status: "live" as const },
  { name: "NPU", status: "coming" as "live" | "coming" },
  { name: "Mobile", status: "live" as const },
];

/* Proof stats — the numbers are universal; labels are translated in t.proof.items.
   Each one is something an operator can check: the model serving right now, how
   many stages it is placed as, the cross-backend agreement we measured, and the
   platforms the wallet ships on. */
export const PROOF_STATS = ["428B", "16", "0.9999", "4"];

/* Roadmap tone per item (positive = live, caution = coming). */
export const ROADMAP_TONE = ["positive", "caution", "caution", "caution"] as const;

/* A 45-layer model split across a heterogeneous set of devices — one of each, to
   show that any device can be a node. Layers sum to 45 (Step-3.7-Flash, the 428B
   MoE serving today). Illustrative: production places it as 16 stages on two
   machines, not one device per kind. */
export const NODE_SPLIT = [
  { id: "GPU", layers: 14, tier: "S" },
  { id: "CPU", layers: 11, tier: "B" },
  { id: "NPU", layers: 11, tier: "B" },
  { id: "Phone", layers: 9, tier: "C" },
];
