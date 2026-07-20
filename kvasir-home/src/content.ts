/* ==========================================================================
   Language-AGNOSTIC data for the Kvasir homepage.
   All translatable copy lives in src/i18n/<lang>.ts. This file holds only the
   things that never translate: URLs, code, numeric facts, tier values, and the
   device / node structure. Grounded in homepage-brief.md; compliance constraints
   (devnet, utility token, non-custodial) are honored across every language.
   ========================================================================== */

export const LINKS = {
  github: "https://github.com/louisevandan/kvasir-net",
  hub: "https://hub.kvasir-ai.net",
  gateway: "https://gate.kvasir-ai.net",
  runNode: "/run-node",
  technology: "/technology",
  wiki: "/wiki",
  api: "/docs/api",
  feed: "/feed.xml",
  // KVR SPL token on the Solana explorer — on-chain transaction activity.
  explorerDevnet:
    "https://explorer.solana.com/address/6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ?cluster=devnet",
  explorerMainnet:
    "https://explorer.solana.com/address/6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ?cluster=mainnet-beta",
};

/* Download destinations for the node-operator guide. Empty string => the guide
   renders a "준비 중 (coming soon)" state instead of a dead link. Fill these in
   as the desktop installers ship and the mobile apps are published. */
// Desktop installers live in the Cloudflare R2 bucket `kvasir-downloads`
// (public r2.dev URL). Version-less object keys keep these links stable across
// releases — re-upload the same key to publish a new build.
const R2_DOWNLOADS = "https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev";
export const DOWNLOADS = {
  desktopMac: `${R2_DOWNLOADS}/Kvasir-Wallet-mac-universal.dmg`,
  desktopWin: `${R2_DOWNLOADS}/Kvasir-Wallet-win-x64.exe`,
  desktopLinux: `${R2_DOWNLOADS}/Kvasir-Wallet-linux-x64.tar.gz`,
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
] as const;

/* Example request — code, never translated. */
export const CODE_SNIPPET = `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "Qwen3.5-122B-A10B",
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

/* linkcpp's own one-liner — a verbatim English quote, shown as a callout. */
export const LINKCPP_TAGLINE =
  "Run large AI models across multiple GPUs and machines with stock inference engine binaries.";

/* Device types (name + status are universal; the detail line is translated in
   t.contributors.deviceDetails, aligned by index). */
export const DEVICE_META = [
  { name: "GPU", status: "live" as const },
  { name: "CPU", status: "live" as const },
  { name: "NPU", status: "live" as const },
  { name: "Mobile", status: "live" as const },
];

/* Proof stats — the numbers are universal; labels are translated in t.proof.items. */
export const PROOF_STATS = ["122B", "4", "2", "4"];

/* Roadmap tone per item (positive = live, caution = coming). */
export const ROADMAP_TONE = ["positive", "caution", "caution"] as const;

/* A 49-layer model split across a heterogeneous set of devices — one of each,
   to show that any device can be a node. Layers sum to 49 (Qwen3.5-122B). */
export const NODE_SPLIT = [
  { id: "GPU", layers: 15, tier: "S" },
  { id: "CPU", layers: 12, tier: "B" },
  { id: "NPU", layers: 12, tier: "B" },
  { id: "Phone", layers: 10, tier: "C" },
];
