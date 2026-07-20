/* ==========================================================================
   Kvasir tech blog — article content for /technology.
   Grounded in the kvasir-net engineering artifacts (docs/design/
   moe-expert-sharding.md, cross-backend-numerical-equivalence.md and the
   milestone reports M0–M4 + real-device demo, 2026-07). All numbers are
   measured results quoted verbatim from those reports.

   This file is the ENGLISH source of truth. Localized bodies live in
   tr-<lang>.ts (same slugs; title/dek/blocks replaced per language) and are
   merged by getTechArticles() in translations.ts. Nav/category/page labels
   are translated via t.techBlog.* in src/i18n. Inline markup supported in
   `md` strings: **bold** and `code`.
   ========================================================================== */

export const TECH_CATEGORIES = ["overview", "core", "milestones", "demos"] as const;
export type TechCategory = (typeof TECH_CATEGORIES)[number];

export type TechBlock =
  | { t: "p"; md: string }
  | { t: "h2"; text: string; kick?: string }
  | { t: "stats"; items: { n: string; l: string }[] }
  | { t: "code"; code: string; caption?: string }
  | { t: "ul"; items: string[] }
  | { t: "table"; head: string[]; rows: string[][] }
  | { t: "callout"; md: string }
  | { t: "img"; src: string; alt: string };

export type TechArticle = {
  slug: string;
  category: TechCategory;
  title: string;
  dek: string;
  date: string; // ISO, display-only
  tags: string[];
  blocks: TechBlock[];
};

/* A per-language override of one article's translatable content. Structure
   (slug, category, date, tags) stays in TECH_ARTICLES; translations live in
   tr-<lang>.ts (same block sequence, code kept verbatim, img blocks repeated
   at the same positions) and are merged by getTechArticles() in
   translations.ts. */
export type TechTranslation = Pick<TechArticle, "title" | "dek" | "blocks">;

export const TECH_ARTICLES: TechArticle[] = [
  /* ------------------------------------------------------------------ */
  /* Overview                                                            */
  /* ------------------------------------------------------------------ */
  {
    slug: "expert-sharded-swarm-design",
    category: "overview",
    title: "Expert-Sharded Swarm Inference: The Design",
    dek: "86% of a 122B MoE is 12,544 independent 5.3 MB experts. Slice the model at that grain and a phone can carry a real share of frontier inference.",
    date: "2026-05-12",
    tags: ["design", "MoE", "kvasir-net"],
    blocks: [
      {
        t: "callout",
        md: "**The thesis:** 86% of Qwen3.5-122B's weight is 12,544 mutually independent 5.3 MB experts. Shard at the expert grain and a weak device carries \"8–64 experts (42–340 MB)\" instead of \"a 1.4 GB layer\" — exactly the unit a phone can actually hold. MoE is the natural substrate of a swarm.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      {
        t: "h2",
        kick: "Substrate · Qwen3.5-122B-A10B (Q4_K_M)",
        text: "The weights are already packaged in swarm-sized units",
      },
      {
        t: "stats",
        items: [
          { n: "49", l: "layers" },
          { n: "256", l: "experts / layer" },
          { n: "8", l: "active / token" },
          { n: "5.3 MB", l: "one expert (Q4)" },
          { n: "12,544", l: "experts total" },
          { n: "86%", l: "of weight in experts" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "expert dim = outermost" },
        ],
      },
      {
        t: "p",
        md: "The expert index is the **outermost dimension** of every MoE tensor, so each expert is a contiguous, quant-block-aligned slab. An expert-sliced mini-GGUF is a clean byte-range copy — no dequantization, no re-packing.",
      },
      { t: "h2", kick: "Two roles", text: "Backbone stage × expert worker" },
      {
        t: "ul",
        items: [
          "**Backbone stage (strong node):** attention + KV cache, all norms, the **router**, the shared expert, and the residual combine — the entire dense path. It also keeps all experts resident as a fallback replica (RAM offload), which gives the swarm churn tolerance.",
          "**Expert worker (a phone):** not a transformer. No attention, no KV, no sampler — a pure function `(hidden, local_ids) → out` made of three mat-muls, holding only its own expert slice. It fits any budget down to a 4 GB phone.",
        ],
      },
      {
        t: "code",
        caption: "The cut point: router runs once on the backbone, with authority.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Because the router runs **exactly once** on the backbone, each selected expert is computed exactly once by whichever node owns it. There is **no approximation** — the sharding only moves where the mat-muls happen.",
      },
      { t: "h2", kick: "Not a new subsystem", text: "The swarm is the proven reward market, at a finer grain" },
      {
        t: "p",
        md: "Kvasir already runs an autonomous scarcity market for **layer** shards, verified on real devices: a NAT-bound phone polls the demand map, self-enrolls into the **highest-reward** segment, partially downloads only that window, loads it on its Adreno GPU, and completes ring inference — earning contribution rewards. Expert sharding reuses all of it — coverage map, max-reward self-enroll, partial download, per-node rewards — changing only the coverage unit from *layer ranges* to *(layer, expert-range)*.",
      },
      { t: "h2", kick: "Two innovations, already device-verified", text: "Partial-weight participation + the 443 relay" },
      {
        t: "ul",
        items: [
          "**Reward-driven partial-weight download:** conventional RPC/TP/PP setups ship the full checkpoint to every rank and a scheduler dictates placement. In Kvasir a node downloads **only the slice it will compute**, and picks that slice **itself, by reward** — a 254 MB stage mini-GGUF versus the 77.6 GB full model. This is how a 4 GB phone joins a model far bigger than itself.",
          "**443 relay data plane:** Cloudflare's 80/443-only edge plus carrier NAT means no direct dial in either direction. A per-edge WebSocket bridge with a 1-byte role preamble lets **both sides dial outbound** (the phone opens zero inbound ports). Landing it meant fixing three real bugs — build-fingerprint agreement, node-token download auth, and a Kotlin `Int.ushr` frame-length bug that silently corrupted every frame ≥ 64 KiB (`ushr` uses only the low 5 bits of its shift; `len ushr 56` became `len ushr 24`) — fixed by moving to `Long` shifts.",
        ],
      },
      { t: "h2", kick: "The honest crux", text: "A throughput fabric, not a low-latency decoder" },
      {
        t: "p",
        md: "Decode is 49 serial layers, and a cross-internet round trip per layer costs 2.5–10 s per token. So the swarm's contest is **serving models nobody can host alone**, measured in aggregate throughput: batch dispatch amortizes RTT, the backbone keeps a hot-expert cache, and requests route to near replicas. The low-latency path stays with the pipeline ring.",
      },
      { t: "h2", kick: "Roadmap", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — backbone expert RAM offload: run 122B on one coordinator, no graph surgery.",
          "**M1** — single-host expert-parallel proof: expert-sliced mini-GGUF + worker runtime + dispatch, logits exactly matching monolithic.",
          "**M2** — LAN + NAT phone workers computing real 122B experts through the 443 relay.",
          "**M3** — expert-grain coverage market with replicas and churn fallback.",
          "**M4** — throughput: batched dispatch + hot-expert cache, tokens/s scaling with worker count.",
        ],
      },
    ],
  },
  {
    slug: "swarm-verified-and-keystone",
    category: "overview",
    title: "From Blueprint to Hardware: What's Verified, and the Keystone",
    dek: "A recap of the verification campaign — design through M2-core proved on a real 122B — and the one integration piece that unlocks the rest.",
    date: "2026-06-24",
    tags: ["progress", "milestones", "122B"],
    blocks: [
      {
        t: "p",
        md: "Over the past weeks, the hard, novel pieces of the expert-sharded swarm were proven one by one on a real **Qwen3.5-122B** — not simulated, not toy-sized. Here is the verification trail so far, and the single keystone that remained.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "The trail · all verified on the real 122B", text: "What has landed so far" },
      {
        t: "ul",
        items: [
          "**Design (5 revisions since the blueprint)** — EP architecture, partial-weight autonomous participation, the relay, worker kernel spec, and cross-backend numerical equivalence codified as core technology. Router authority written down as the coherence invariant.",
          "**M0 — backbone expert RAM offload (planner verified):** 122B is *feasible* on a single 64 GB coordinator — 10 expert layers offloaded to RAM, VRAM 62.6 GiB / RAM 14.2 GiB, wired via `--override-tensor`.",
          "**M1 — expert-slice data path:** per-expert mini-GGUF slicing (ne[2] slabs, byte copy without dequant) + the `/expert-shard` download endpoint.",
          "**M1 — numerical oracle:** dispatch + combine == monolithic with **max|Δ| = 3.6e-12** on real layer-0 experts — sharding is an exact regrouping of the same weighted sum.",
          "**M1 — C++ worker on hardware:** `linkcpp-expert-worker` (pure ggml/gguf) built and run on ROCm, **cosine 0.99995** vs the oracle; router → two C++ workers → combine matches monolithic at cosine 0.9997–0.9999.",
          "**M2 core — a phone computes real 122B experts:** Android cross-build, run on an SM-S938N, **cosine 0.99992** vs the oracle.",
          "**Numerics — 3-backend equivalence matrix:** the same 122B computation on ROCm × phone ARM CPU × numpy — ROCm↔phone cosine 0.99990, ROCm↔numpy 0.99996, phone↔numpy 0.99992. All equivalent, none bit-identical.",
        ],
      },
      { t: "h2", kick: "The keystone", text: "Backbone dispatch, integrated into live decode" },
      {
        t: "callout",
        md: "Every **component** — slices, workers, dispatch/combine logic, numerical equivalence, phone compute — was device-verified. What remained was wiring them **inside a real inference engine decode**: a `build_moe_ffn` hook that dispatches experts to their owner nodes mid-graph. It required modifying the pinned inference engine submodule and several build-verify cycles. Once this keystone stands, **M2 relay integration, the M3 expert coverage market, and M4 batched throughput** open in sequence — all of them depend on this dispatch.",
      },
      {
        t: "p",
        md: "The keystone has since landed: the follow-up posts on M2, M3, M4 and the live phone demo are the results of exactly this integration.",
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Core technology                                                     */
  /* ------------------------------------------------------------------ */
  {
    slug: "cross-backend-numerical-equivalence",
    category: "core",
    title: "Numerical Equivalence Across Heterogeneous Backends",
    dek: "CUDA, ROCm, Adreno and CPUs will never agree bit-for-bit. That the swarm still produces one coherent model is a designed property, not luck.",
    date: "2026-06-18",
    tags: ["core tech", "numerics", "router authority"],
    blocks: [
      {
        t: "h2",
        kick: "The key distinction",
        text: "Exact vs equivalent — two different properties",
      },
      {
        t: "ul",
        items: [
          "**Within one backend — exact (3.6e-12):** splitting experts across nodes and combining is the same weighted sum regrouped; the only difference is floating-point accumulation order. Oracle-verified.",
          "**Across backends — equivalent (1e-3…1e-6):** the same op on different hardware carries a per-op relative error around 1e-3–1e-6, and is never zero. **The swarm lives in this regime.**",
        ],
      },
      {
        t: "p",
        md: "\"Exact\" is what decomposition guarantees inside one device. \"Equivalent\" is what heterogeneous hardware gives you. The swarm's job is to keep equivalence from compounding into divergence.",
      },
      { t: "h2", kick: "Measured · real 122B, three backends", text: "Not a theory — measured on hardware" },
      {
        t: "p",
        md: "The same Qwen3.5-122B layer-0 expert FFN, computed by `linkcpp-expert-worker` on an MI250 (**ROCm**), a phone's **ARM CPU** (SM-S938N), and an x86 **numpy** reference — same inputs, same weights, different instruction sets and reduction orders:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Backend pair", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Phone ARM CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["ROCm GPU vs phone ARM CPU", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Three instruction sets, one computation — every pair equivalent (cosine ≈ 0.9999), no pair bit-identical (Δ ≈ 1e-6). The residuals are small **because router authority pinned the inputs and the expert selection**.",
      },
      {
        t: "p",
        md: "A later run on real **NVIDIA GB10 Grace Blackwell** hardware closed the matrix on the last backend: CUDA ↔ ROCm landed at **cosine 1.0000000000** (max abs 3.5e-10, effectively bit-identical, since both GPU backends share kernel sources), and CUDA ↔ Grace ARM CPU at cosine 0.99975 — the same GPU↔CPU pattern seen above.",
      },
      { t: "h2", kick: "Why backends differ", text: "Floating-point addition is not associative" },
      {
        t: "ul",
        items: [
          "**Matmul reduction order** — tensor cores, MFMA tiles, OpenCL workgroups and SIMD lanes each accumulate in different orders and tilings.",
          "**FMA fusion** — `a*b+c` rounded once (FMA) or twice, fused differently per backend.",
          "**Accumulation precision** — F16/BF16 storage with F32 vs F16 accumulators (the biggest lever on divergence).",
          "**Transcendental approximations** — polynomial/table variants of exp (softmax), silu/sigmoid (swiglu), rsqrt (norms).",
          "**Dequant + matmul path** — dequantize-then-matmul vs fused quantized kernels round intermediates differently.",
          "**Nondeterministic kernels** — atomic/split-K reductions can differ run to run on the same device.",
        ],
      },
      { t: "p", md: "None of this is a bug. It is the price each accelerator's fast path pays." },
      { t: "h2", kick: "Why it still works", text: "One authority for decisions, enough precision for accumulation" },
      {
        t: "callout",
        md: "**ROUTER AUTHORITY — the core invariant.** The only discrete decision inside the network is MoE routing (top-8 of 256). If every backend re-ran the router, borderline tokens would pick **different experts** and genuinely diverge. Kvasir runs the router **once, on the backbone**, and sends workers only the selected expert ids. A heterogeneous swarm may differ in the *magnitude* of each expert's output — it never differs in *which experts run*. This converts catastrophic discrete divergence into bounded continuous error, and is the coherence rule of heterogeneous expert sharding.",
      },
      {
        t: "ul",
        items: [
          "**Discrete argmax:** decoding is an argmax over logits. A 1e-3 wobble flips a token only when two candidates are within 1e-3 — at most positions the margin is far larger, so **tokens come out identical**; the rare flips are positions as ambiguous as a different seed.",
          "**Combine is addition:** partial results merge as a probability-weighted **sum**. Independent ~1e-4 errors add incoherently — they grow like √k, not k — and there is no cancellation of large values, so the residual stays well-conditioned.",
        ],
      },
      { t: "h2", kick: "Where it can break · and the rules that stop it", text: "Divergence modes and defenses" },
      {
        t: "table",
        head: ["Divergence mode", "Mechanism", "Rule"],
        rows: [
          ["Routing mismatch", "Backends pick different top-8 for borderline tokens", "Router authority — decided once on the backbone, ids dispatched"],
          ["Trajectory fork", "Per-token logit wobble eventually flips a token; the sequence forks like a new seed", "Decode/sampling pinned to one node"],
          ["Depth accumulation", "49 layers × ~1e-4 each → up to 1e-2 at the final logits", "F32 accumulation at boundaries and combine"],
          ["Self-nondeterminism", "Atomic/split-K kernels vary run-to-run", "Deterministic combine kernels; verification uses tolerances"],
          ["Precision mismatch", "One node accumulates F16, another F32", "Accumulation precision advertised as a capability; F32 nodes preferred for output ranks"],
        ],
      },
      { t: "h2", kick: "Equivalence is a number", text: "The measurement protocol" },
      {
        t: "ul",
        items: [
          "**Per-op delta** — same inputs, A vs B relative error on matmul, swiglu, softmax, norm.",
          "**Layer-boundary drift** — residual delta after one layer, stacked to see whether depth accumulates as √L or L.",
          "**End-to-end logit divergence** — L∞, L2 and **KL divergence** over the full forward.",
          "**Decision agreement** — top-1 token agreement plus top-8 routing agreement (validating why router authority is necessary).",
          "**Generation stability** — greedy N tokens; the first index where A and B diverge.",
          "**Task level** — perplexity and eval-score deltas: the only metric a user actually feels.",
        ],
      },
      {
        t: "p",
        md: "A pass is a **tolerance** — \"top-1 agreement ≥ 99.x%, KL ≤ ε\". A node outside tolerance is marked unfit for sensitive ranks, not rejected outright.",
      },
      { t: "h2", kick: "Why this is core swarm technology", text: "Bit-agreement is impossible — and unnecessary" },
      {
        t: "p",
        md: "A homogeneous cluster can assume bit-exactness; a swarm cannot — its premise is *whatever hardware shows up*. So Kvasir treats numerical equivalence exactly like protocol compatibility: a **first-class, measured contract**. Backends and accumulation precision are advertised as node capabilities, router authority is enforced as an invariant, and every verification uses tolerances instead of bit-equality. **Measured numerical equivalence + single-authority discrete decisions** — that is what lets one model run on every GPU on earth at once. That is the swarm.",
      },
    ],
  },

  {
    slug: "blackwell-joins-the-swarm",
    category: "core",
    title: "NVIDIA Blackwell Joined the Swarm",
    dek: "A GB10 Grace Blackwell computed real 122B expert-FFN slices in CUDA and matched AMD ROCm bit-for-bit (cosine 1.0000000000) and Grace ARM CPU within tolerance. The cross-backend matrix is complete.",
    date: "2026-07-16",
    tags: ["core tech", "numerics", "CUDA", "Blackwell"],
    blocks: [
      {
        t: "p",
        md: "A swarm's premise is *whatever hardware shows up*. Numerical equivalence — the proof that CUDA, ROCm, Adreno and CPU workers all emit the same token — was already measured on ROCm, phone ARM and numpy. NVIDIA is the **default and best-optimized** path in stock ggml/inference engine, yet it was the one backend the matrix hadn't been closed on. Running real Blackwell hardware closes it.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. On the same real Qwen3.5-122B layer-0 expert slice, the two GPU backends are effectively bit-identical.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Measured · real Qwen3.5-122B-A10B, layer-0 expert slice", text: "The cross-backend matrix" },
      {
        t: "table",
        head: ["Comparison", "Hardware", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "The two GPU backends (CUDA, ROCm) share kernel sources, so they land **effectively bit-identical** (10⁻¹⁰). GPU↔CPU carries ~10⁻³ per-op perturbation from a different accumulation order, but stays equivalent at **cosine 0.99975** — the same pattern as the earlier ROCm↔phone-ARM 0.99992. The router-authority principle holds again on NVIDIA: **the discrete decisions (argmax, expert selection) are invariant over this continuous perturbation.**",
      },
      { t: "h2", kick: "Setup", text: "What ran, on what" },
      {
        t: "ul",
        items: [
          "**Device** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124.5 GB unified memory.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; the pure-ggml/gguf expert-worker built with Blackwell kernels.",
          "**Model** — Qwen3.5-122B-A10B-Q4_K_M, layer-0 all experts (256 experts, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Method** — the 1.58 GB L0 slice streamed MI250 → GB10 (lossless compare); the same input (h/ids) run through CUDA, CPU and ROCm; float32 output vectors (36,864) compared by cosine, relative L2 and max-abs.",
        ],
      },
      {
        t: "callout",
        md: "**One real-hardware gotcha:** the GB10's integrated GPU classifies as ggml device type `ACCEL`, not `GPU` — so `init_by_type(GPU)` found nothing. Fixed by selecting the first non-CPU device instead of hard-coding the GPU type.",
      },
      { t: "h2", kick: "Why it matters", text: "The matrix is closed" },
      {
        t: "p",
        md: "For heterogeneous workers to serve one model, a CUDA box and a ROCm box must be **interchangeable**, and a GPU and a CPU must be **numerically equivalent**. With Blackwell measured, both hold across the full backend matrix: CUDA↔ROCm workers can stand in for each other, and GPU↔CPU workers agree within a bounded, well-conditioned tolerance. The most common accelerator on earth is now a verified swarm citizen.",
      },
    ],
  },
  {
    slug: "securing-the-kvr-money-path",
    category: "core",
    title: "Hardening the Money Path: Transaction Security in KVR Settlement",
    dek: "Three real vulnerability classes — payment-signature replay, unauthenticated reward minting, and race double-spends — found, exploited in tests, and closed in the gateway's settlement service.",
    date: "2026-07-15",
    tags: ["security", "settlement", "Solana"],
    blocks: [
      {
        t: "p",
        md: "In a DePIN, the money path is exactly as adversarial as the compute path: every endpoint that credits KVR will eventually be probed by someone who wants KVR without doing the work. A security pass over the gateway's settlement service — the process that verifies on-chain payments and credits stakes, node rewards and inference charges — found and closed **three real vulnerability classes**. Each one was demonstrated with an exploit-style test before the fix and re-verified after.",
      },
      { t: "h2", kick: "The trust model", text: "Verify on-chain facts, not client claims" },
      {
        t: "p",
        md: "Kvasir's custody model keeps keys with users: wallets sign transactions, Solana records them, and the settlement service's only job is to **verify what actually happened on chain** before touching a balance. Payments follow *quote → payment → inference*, with every consumed transaction signature recorded in a one-shot `usedSignatures` registry so it can never be presented twice. That makes the settlement service the chokepoint — and the rule it must never break is: credit only what the chain proves, never what the client asserts.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Fix #1 · sender binding", text: "Bind the payment to the payer" },
      {
        t: "p",
        md: "Solana signatures are **public**. The stake-verification path checked that the vault *received* the expected KVR — but never *who sent it*. An attacker could watch devnet for a victim's KVR→vault transfer, then submit `{owner: attacker, signature: victim's}`: the vault-received check passed, the principal was credited to the attacker, and one unstake later the funds were theirs. Direct theft, using nothing but a block explorer.",
      },
      {
        t: "code",
        caption: "The fix: the KVR must have been debited from token accounts owned by the credited owner.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Fix #2 · trusted reporter", text: "Rewards only from authenticated sources" },
      {
        t: "p",
        md: "The node-reward endpoints minted claimable KVR from **self-reported input**: `POST /api/node/contribution` credited whatever `units` the client claimed — `units: 1e9` and a claim call could drain the vault — and register/heartbeat honored self-declared hub/gateway roles (hourly infra rewards) and performance scores (reward multipliers). The fix gates every reward-affecting assertion behind a **trusted reporter**: only the M2M service token used by the hub's contribution poll, or an authenticated admin, may assert units, infra roles or perf tiers — enforced even in open LAN mode, because these mint KVR. The token comparison is constant-time, and wallet↔node linking still works freely; it just can't assert its own rewards anymore.",
      },
      { t: "h2", kick: "Fix #3 · settlement serialization", text: "One writer on every balance" },
      {
        t: "p",
        md: "Settlement state was a lock-free read-modify-write, and every money operation *awaits* an on-chain payout or verification in the middle — yielding the event loop with a stale balance in hand. Two concurrent claims could both read the same 100 KVR pending balance and both pay out. This wasn't theoretical: the exploit test showed **three concurrent claims paying 300 for a 100 balance**.",
      },
      {
        t: "code",
        caption: "Per-key async serialization: same-key money operations run strictly one after another.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Defense in depth", text: "Where each layer now stands" },
      {
        t: "table",
        head: ["Layer", "Mechanism"],
        rows: [
          ["Identity", "SIWS wallet-signature login over a server nonce + TOTP 2FA + single-use backup codes"],
          ["Transport", "Wallet-derived node tokens on shard downloads; build-fingerprint agreement on the relay"],
          ["Payment", "Sender binding on stake transfers; one-shot usedSignatures; private requestId on inference"],
          ["Settlement", "Per-key locks around every balance write; pay-first-then-debit; idempotent re-submit"],
          ["Reporting", "Reward-affecting facts only from the M2M service token or admin, constant-time compared"],
          ["Custody", "Non-custodial wallets — the service can move only what the vault holds, never user keys"],
        ],
      },
      { t: "h2", kick: "Measured, not assumed", text: "Each fix carries its own exploit test" },
      {
        t: "ul",
        items: [
          "Replaying a victim's transfer signature under an attacker's wallet is now rejected (\"not sent by owner\"); legitimate stakes, over-claims and inference payments behave unchanged.",
          "Three concurrent claims against one balance pay **exactly once**; re-submitting an already-paid inference idempotently returns the same result.",
          "Self-asserted `units`, hub/gateway roles and perf tiers from unauthenticated clients no longer move a single lamport of rewards.",
        ],
      },
      {
        t: "p",
        md: "The through-line of all three fixes is one principle applied three ways: **the chain is the source of truth, the service is a verifier, and every balance has exactly one writer**. The settlement service still runs on Solana devnet — which is exactly where you want to find, exploit and fix these classes before mainnet raises the stakes.",
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Milestones                                                          */
  /* ------------------------------------------------------------------ */
  {
    slug: "linkcpp-control-plane",
    category: "milestones",
    title: "Phase 0 — The Engine: linkcpp, a Control Plane for inference engine",
    dek: "inference engine ships a capable RPC data plane but no control plane. linkcpp adds the missing half — discovery, planning, launch and gateways — around stock binaries.",
    date: "2026-03-10",
    tags: ["linkcpp", "control plane", "BSL"],
    blocks: [
      {
        t: "p",
        md: "Everything Kvasir runs on starts here. **linkcpp** is a source-available control plane (Business Source License) around inference engine's RPC data plane: it runs large AI models across multiple GPUs and machines using *stock* `ggml-rpc-server` / `llama-server` binaries. The data plane stays unforked — everything linkcpp adds is orchestration.",
      },
      { t: "h2", kick: "The gap", text: "A data plane without a control plane" },
      {
        t: "p",
        md: "inference engine can already split a model across machines over RPC — but someone has to discover the GPUs, decide which layers go where, launch the right workers with the right budgets, check that every node speaks the same protocol, and expose an API developers can actually call. Doing that by hand for one cluster is a chore; doing it for an open network of strangers' devices is impossible. That coordination layer is linkcpp.",
      },
      { t: "h2", kick: "Architecture", text: "One hub, stock workers, standard gateways" },
      {
        t: "code",
        caption: "Request flow — the hub orchestrates, stock binaries compute.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock inference engine engines below" },
      {
        t: "ul",
        items: [
          "**Three ways a machine joins:** fixed **local node slots** with editable VRAM/RAM/CPU budgets; **remote units** — register another hub and import its nodes; and **managed node agents** — worker-only services that join over plain request/response HTTP, deliberately not a persistent stream, so they survive simple LAN/VPN routing.",
          "**Compatibility gating is first-class:** every unit, node and agent reports a protocol / runtime-pack identity plus backend details. Unit, runtime-pack, inference engine-revision and RPC-ABI mismatches are **hard-blocked before bind, plan, load or infer** — backend differences (CUDA/Metal/Vulkan/CPU) are tracked as capabilities, not rejections.",
          "**The planner** reads GGUF metadata and produces contiguous per-node layer placement, `--tensor-split`, KV-cache/layer/expert VRAM estimates, and optional expert-FFN offload to RAM.",
          "**Gateways:** every controller exposes OpenAI-compatible (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) and Anthropic-compatible (`/anthropic/v1/messages|models`) endpoints, backed by the same loaded model — existing clients work unchanged.",
        ],
      },
      {
        t: "p",
        md: "This deliberate split — an unmodified data plane under an open control plane — is what everything later builds on: the ring runtime, the layer market, and eventually the expert-sharded swarm are all control-plane evolutions over the same stock compute.",
      },
    ],
  },
  {
    slug: "ring-topology-pipeline-inference",
    category: "milestones",
    title: "Phase 1 — The Ring: Pipeline Inference Without a Master",
    dek: "Every device loads only its layer window and passes a small hidden-state boundary to its neighbor. No node holds the model; no central master exists.",
    date: "2026-04-14",
    tags: ["ring runtime", "topology", "NAT"],
    blocks: [
      { t: "h2", kick: "Why not a star", text: "The RPC master is a bottleneck and a gatekeeper" },
      {
        t: "p",
        md: "In the classic RPC topology one master opens the **entire GGUF** and dials out to every worker. That shape breaks in an open network three ways: the master must hold and serve the whole checkpoint; every worker must be dialable — phones behind carrier NAT are not; and the master is a single owner in a network that should have none.",
      },
      { t: "h2", kick: "The ring", text: "Layer windows + boundary passing" },
      {
        t: "ul",
        items: [
          "Every device stores the same model but **loads only its contiguous layer window**, then opens exactly two links: one to its predecessor, one to its successor.",
          "A request enters the ring; each node runs its layers and passes only the **hidden-state boundary** to its neighbor. The last rank samples the token and sends it back around — no central master, and no node holds the whole model.",
          "Placement comes from the planner's **rank manifest** — for Qwen3.5-122B, 49 layers split across whatever mix of GPU, CPU, NPU and phone shows up.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Making weak devices real members", text: "Partial shards, mobile GPUs, and the 443 relay" },
      {
        t: "ul",
        items: [
          "**Partial-shard download:** a ring stage doesn't need the checkpoint — it needs its window. A stage mini-GGUF carries just those tensors (**254 MB of 26 tensors** versus the 77.6 GB full model), so a phone pulls ~1.5 GB for a one-layer window instead of everything.",
          "**Mobile GPU path:** the RPC route to a phone GPU proved infeasible (Adreno's OpenCL buffer layout doesn't survive RPC serialization), but a **ring stage runs on the Adreno GPU directly** — the stage owns its backend locally, so nothing crosses the wire but boundaries.",
          "**NAT traversal:** phones can't accept inbound connections, so the data plane runs through a **443 relay** — a per-edge WebSocket bridge with a 1-byte role preamble that lets both ends dial outbound. The phone opens zero inbound ports.",
          "**Self-enrollment market:** stages are claimed, not assigned. A node polls the coverage/demand map, picks the **highest-reward** uncovered window, downloads that window, and joins — verified end-to-end with a NAT-bound phone completing ring inference and earning its contribution.",
        ],
      },
      { t: "h2", kick: "Where the ring fits", text: "The low-latency path" },
      {
        t: "p",
        md: "The ring is Kvasir's **latency** path: boundaries are small, hops are few, and decode flows around the loop without gathering anything centrally. Its limitation is granularity — the smallest unit a node can carry is a layer (~1.4 GB on the 122B). Removing that floor is what the expert-sharded swarm does; the ring remains the serving backbone it plugs into.",
      },
    ],
  },
  {
    slug: "inside-a-122b-moe",
    category: "milestones",
    title: "Phase 2 — Inside a 122B MoE: Why the Weights Want to Be Sharded",
    dek: "A tensor-level analysis of Qwen3.5-122B: 86% of the bytes are 12,544 independent expert slabs, each one a clean byte-range copy away from standing alone.",
    date: "2026-05-04",
    tags: ["MoE", "GGUF", "analysis"],
    blocks: [
      {
        t: "p",
        md: "Before designing anything, we took the 122B apart on disk. The question: if a swarm of weak devices is to carry this model, what is the natural unit of carrying? The answer fell out of the GGUF tensor layout itself.",
      },
      { t: "h2", kick: "Anatomy · Qwen3.5-122B-A10B (Q4_K_M)", text: "What a MoE layer is actually made of" },
      {
        t: "stats",
        items: [
          { n: "49", l: "layers" },
          { n: "256", l: "experts / layer" },
          { n: "8", l: "active / token" },
          { n: "12,544", l: "experts total" },
          { n: "5.3 MB", l: "one expert (Q4)" },
          { n: "86%", l: "of weight in experts" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "full checkpoint" },
        ],
      },
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Each layer splits into a **dense path** — attention + KV, the norms, the router (`ffn_gate_inp`), a shared expert — and an **expert bank**: 256 independent FFNs stored as three stacked tensors (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). The dense path is the minority of the bytes; the expert bank is 86% of the model.",
      },
      { t: "h2", kick: "The layout gift", text: "Experts are contiguous, block-aligned slabs" },
      {
        t: "ul",
        items: [
          "The expert index is the **outermost ggml dimension** (`ne[2]`) of every expert tensor — expert *e* occupies one contiguous, quant-block-aligned slab of raw quantized bytes.",
          "That makes per-expert extraction a **byte-range copy**: `data[a:b]`, no dequantization, no re-packing — an expert-sliced mini-GGUF is cheap to produce and bit-faithful.",
          "Per token only **8 of 256** experts fire per layer, chosen by the router — so at decode time a layer's expert traffic is a handful of small matrix multiplies over one hidden vector.",
        ],
      },
      { t: "h2", kick: "The implication", text: "The carrying unit drops from 1.4 GB to 5.3 MB" },
      {
        t: "p",
        md: "At layer grain, the least a node can hold is ~**1.4 GB** — out of reach for most phones once the app, KV and OS take their share. At expert grain, the unit is **5.3 MB**, and a realistic contribution is 8–64 experts (**42–340 MB**) — comfortably inside any modern device. The experts are mutually independent, so ownership can be scattered arbitrarily and re-balanced freely. This analysis is what made expert-level sharding the design bet: the weights were already packaged in swarm-sized units — the network just had to honor the packaging.",
      },
    ],
  },
  {
    slug: "m0-backbone-expert-ram-offload",
    category: "milestones",
    title: "Phase 3 — Backbone Expert RAM Offload (M0)",
    dek: "Stream MoE expert FFNs from CPU RAM instead of VRAM, and a single 64 GB coordinator holds a 122B — with no graph surgery.",
    date: "2026-05-26",
    tags: ["M0", "planner", "RAM offload"],
    blocks: [
      {
        t: "p",
        md: "Expert FFNs don't have to live in VRAM. Streaming them from CPU RAM lets one coordinator hold a model whose experts exceed its VRAM — the foundation that lets weak nodes join a large MoE at all.",
      },
      { t: "h2", kick: "Planner verified · real 122B GGUF", text: "A 122B fits a single 64 GB coordinator" },
      {
        t: "p",
        md: "Previously the ring placed weights VRAM-only, so the 122B (77.6 GB) was **infeasible** on a 64 GB GCD. With expert-offload rules the dry-run comes back **feasible**:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "122B ring plan" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (experts)" },
          { n: "10", l: "offloaded layers" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Planner output — inference engine -ot rule format.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # inference engine -ot format`,
      },
      { t: "h2", kick: "What was wired · pure Python, no C++ rebuild", text: "Carrying the planner's offload rules into a real load" },
      {
        t: "ul",
        items: [
          "**planner** — already emits `ot` (comma-joined `-ot` rules) in each placement.",
          "**protocol.py** — added the `StageStartRequest.ot` field.",
          "**runtime.py** — forwards the placement's `ot` into the stage request.",
          "**stage_service.py** — the coordinator launches with `--override-tensor`.",
          "`linkcpp-server` forwards unknown args to stock llama-server, so `-ot` applies untouched.",
        ],
      },
      {
        t: "code",
        code: `# stage_service.py — coordinator branch
if request.ot:
    command += ["--override-tensor", request.ot]`,
      },
      {
        t: "p",
        md: "Verified with an `ot` protocol round-trip test plus confirmation that the coordinator command emits `--override-tensor`; the hub redeployed cleanly with no regressions. Remaining at the time: the full 2-node 77 GB load (coordinator with offload + a phone holding a ~1.5 GB one-layer window), gated on server availability. The core of M0 — the backbone offload that lets weak nodes participate in a big MoE — was complete at the code and planner level.",
      },
    ],
  },
  {
    slug: "m1-expert-slice-data-path",
    category: "milestones",
    title: "Phase 4 — The Expert-Slice Data Path (M1)",
    dek: "A weak device downloads a few 6 MB experts, not a 1.4 GB layer — and sharded compute matches monolithic to 3.6e-12.",
    date: "2026-06-09",
    tags: ["M1", "oracle 3.6e-12", "ROCm"],
    blocks: [
      { t: "h2", kick: "Verified · real Qwen3.5-122B-A10B", text: "An expert slice is a byte copy — no dequant" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "expert-dim slice" },
          { n: "~6.1", l: "MB / expert (Q4+Q6)" },
          { n: "206 MB", l: "2 layers × 16 experts download" },
          { n: "200", l: "HTTP, valid GGUF" },
        ],
      },
      {
        t: "p",
        md: "MoE expert tensors stack all experts along the outermost ggml dimension, so the reader exposes `(n_expert, rows, row_bytes)` of raw quantized bytes. Expert *e* is a quant-block-aligned contiguous slab — the slice is literally `data[a:b]`, with no dequantization and no re-packing.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — the verified round trip.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "The numerical oracle", text: "dispatch + combine == monolithic, exactly" },
      {
        t: "p",
        md: "With real 122B layer-0 experts (dequantized reference), splitting the experts into 4 shards, computing each separately and combining **matches the monolithic MoE FFN**: sharding is an exact regrouping of the same weighted sum, not an approximation.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "relative error" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "experts touched" },
        ],
      },
      { t: "h2", kick: "C++ worker, hardware-verified", text: "linkcpp-expert-worker reproduces the oracle on ROCm" },
      {
        t: "ul",
        items: [
          "**Pure ggml/gguf** (no libllama): loads the slice into a GPU backend and runs `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`.",
          "**ROCm build + run** on an MI250: 122B layer-0, experts [0,8), 4 tokens.",
          "**Cosine 0.99995 vs the oracle**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — this residual is itself the first measured instance of cross-backend equivalence (ROCm vs numpy).",
          "The same code path covers CUDA/Metal/Vulkan/CPU (`mul_mat_id`/`swiglu` are stock ggml; CUDA has a dedicated MoE kernel).",
        ],
      },
      {
        t: "p",
        md: "The hardest, riskiest piece — the on-device worker kernel — was verified here. What remained was backbone↔worker orchestration; the worker is a proven pure function consuming these slices.",
      },
    ],
  },
  {
    slug: "m2-distributed-expert-dispatch",
    category: "milestones",
    title: "Phase 5 — Distributed Expert Dispatch (M2)",
    dek: "A live 122B decode hands one layer's expert compute to a separate worker process over TCP — and predicts exactly the same token.",
    date: "2026-06-30",
    tags: ["M2", "argmax MATCH", "TCP dispatch"],
    blocks: [
      { t: "h2", kick: "Verified · real 122B, two processes", text: "Backbone decode → TCP → worker → experts → same token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "logit cosine" },
          { n: "0", l: "transport loss (byte-identical)" },
          { n: "2", l: "processes (backbone + worker)" },
        ],
      },
      {
        t: "p",
        md: "The expert worker serves the layer-0 slice as a **separate process** (ROCm), and the 122B backbone's `build_moe_ffn` dispatch callback ships `(cur, sel)` over TCP and receives the expert outputs. The logit cosine is **exactly the in-process value** (0.99868775) — the transport is lossless. Expert-parallel swarm compute works across a process boundary.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "One long-lived TCP connection — the same stream the ring/443 relay can tunnel.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Done", text: "The distributed dispatch pipeline" },
      {
        t: "ul",
        items: [
          "`--serve` mode: load the slice, listen on TCP, answer `(n_used, n_tokens, cur, sel) → experts`.",
          "`--dispatch-port`: the backbone callback sends/receives over TCP to a separate worker, replacing in-process compute.",
          "Measured on a live 122B decode with layer-0 dispatched out-of-process → **argmax MATCH**, cosine 0.99869 (= in-process, lossless).",
          "M2 core (earlier): the phone's ARM computed real 122B experts at cosine 0.99992 (Android cross-build).",
        ],
      },
      {
        t: "p",
        md: "Next from here: tunneling the same TCP stream through the **443 relay** to workers on other machines and phones (the transport was already proven in the ring work), then the M3 coverage market and M4 batched throughput.",
      },
    ],
  },
  {
    slug: "m3-expert-coverage-market",
    category: "milestones",
    title: "Phase 6 — The Expert Coverage Market (M3)",
    dek: "Weak nodes see which (layer, expert-range) is scarcest and highest-reward, and fill it themselves — the proven layer market, regrained.",
    date: "2026-07-07",
    tags: ["M3", "market", "self-healing"],
    blocks: [
      {
        t: "p",
        md: "Kvasir's layer-shard market — demand map, max-reward self-enrollment, partial download, per-node rewards — was already device-verified. M3 re-parameterizes the same mechanism at the **(layer, expert-range)** grain, so coverage self-heals toward the most under-replicated, highest-reward expert ranges.",
      },
      { t: "h2", kick: "Verified · API", text: "Scarcity aggregation → max-reward range assignment" },
      {
        t: "p",
        md: "Three workers register on layer 0: A = [0,128), B = [128,256), C = [0,128) as a second replica, with `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["layer", "experts", "replicas", "scarcity"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (target met)"],
          ["0", "[128, 256)", "1", "0.5 (under target)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → clips the scarcest range to the node's budget.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Done · pure Python (hub)", text: "A supply/demand market at expert grain" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — workers heartbeat their (layer, expert-range) holdings.",
          "`GET /api/expert-demand` — per-expert replica aggregation → contiguous expert-range segments with scarcity scores.",
          "`POST /api/expert-volunteer` — assigns the scarcest range clipped to the node's budget.",
          "The existing layer market (self-enroll · partial download · rewards) re-parameterized to (layer, expert-range).",
        ],
      },
      {
        t: "p",
        md: "What follows in M4: batched dispatch for concurrent requests plus a hot-expert cache — tokens/s proportional to worker count — and replica routing (nearest/fastest worker) with churn fallback.",
      },
    ],
  },
  {
    slug: "m4-batched-dispatch-throughput",
    category: "milestones",
    title: "Phase 7 — Batched Dispatch Throughput (M4)",
    dek: "The swarm is a throughput fabric, not a latency play: batching dispatch calls amortizes per-request overhead 77× per token.",
    date: "2026-07-11",
    tags: ["M4", "throughput", "batching"],
    blocks: [
      { t: "h2", kick: "Measured · ROCm, expert FFN, n_used = 8", text: "Bigger batches, more tok/s per worker" },
      {
        t: "table",
        head: ["batch", "tok/s per worker"],
        rows: [
          ["1", "688"],
          ["16", "4,255"],
          ["64", "8,130"],
          ["256", "30,666"],
          ["512", "53,067"],
        ],
      },
      { t: "img", src: "/blog/m4-batched-dispatch-throughput.jpg", alt: "A conveyor packing tokens into growing batch crates feeding one GPU, output meter rocketing" },
      {
        t: "p",
        md: "From **1.45 ms/tok** at batch 1 to **0.019 ms/tok** at batch 512 — a 77× per-token improvement. The per-call time barely moves (1.45 → 9.6 ms) while the batch grows 512× — the GPU processes the batch nearly for free behind a fixed overhead. This is the **throughput-fabric property** that makes expert-parallel practical: batched dispatch amortizes per-request RTT and overhead.",
      },
      { t: "h2", kick: "Done", text: "Batched dispatch throughput" },
      {
        t: "ul",
        items: [
          "Worker `--bench`: compute_dispatch timings for batch 1…512 → tok/s.",
          "**53k tok/s per worker** at batch 512 (ROCm) — batching amortizes the overhead.",
          "Hot-expert caching and multi-worker aggregate scaling (replica routing) stack on top of this.",
        ],
      },
      {
        t: "callout",
        md: "With M4, the whole **M0 → M4 pipeline is demonstrated on a real 122B**: backbone offload · expert slices · verified workers · live decode dispatch (argmax MATCH) · distributed processes · coverage market · batched throughput.",
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Field demos                                                         */
  /* ------------------------------------------------------------------ */
  {
    slug: "phone-joins-122b-inference",
    category: "demos",
    title: "A Phone Joined 122B Inference",
    dek: "A Galaxy S25 autonomously downloaded its expert slice from the hub and computed one layer's experts every step of a live 122B decode. The output was correct.",
    date: "2026-07-14",
    tags: ["demo", "Galaxy S25", "122B live"],
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → generated (with the phone in the loop): **\" Paris.\"** — 8/8 tokens identical to the local run.",
      },
      { t: "h2", kick: "Measured · real 122B, phone computing layer-0", text: "Correctness + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens identical to local" },
          { n: "4.01", l: "TPS local (baseline)" },
          { n: "3.13", l: "TPS with phone" },
          { n: "1.58 GB", l: "autonomous download" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Even with the phone computing layer-0's experts for every token, the **generated tokens are exactly the local ones** — the correct \"Paris.\". TPS drops to 3.13 from 4.01 — the phone-dispatch round trip (MI250 → tunnel → phone, ~100 ms/token) costs 22%. Throughput comes back with batching and replicas (M4).",
      },
      { t: "h2", kick: "The autonomous participation flow", text: "Discover → reward-driven download → join the compute" },
      {
        t: "code",
        code: `1. Phone knows the hub (hub.kvasir-ai.net) — already holds its wallet node-token
2. GET /api/proxy/models/…/expert-shard?layers=0:1&experts=0:256
   # partially downloads its own expert slice (1.58 GB, WiFi)
3. linkcpp-expert-worker --serve
   # loads the slice (Adreno device, CPU backend) + waits for dispatch
4. MI250 backbone decodes the 122B → every token, layer-0 experts
   dispatch to the phone → experts return → combine → "Paris."
   (8/8 identical to local)`,
      },
      { t: "h2", kick: "Verified vs remaining", text: "The mechanism is complete; the in-app loop is productionization" },
      {
        t: "ul",
        items: [
          "Partial download (the expert-shard endpoint), worker serving, backbone dispatch, live 122B generation and TPS — all verified on the real device.",
          "Correctness: with the phone participating, 8/8 tokens equal the local run, with the correct answer.",
          "Remaining: the in-app autonomous loop (poll expert-demand → volunteer → download → serve → register) is Kotlin wiring — this demo drove the mechanism directly.",
          "Transport: this demo used an SSH tunnel; production uses the 443 relay (already verified in the ring work).",
        ],
      },
    ],
  },
  {
    slug: "kvasir-economy-virtuous-cycle",
    category: "overview",
    title: "The Kvasir Economy: A Virtuous Cycle of Cost and Reward",
    dek: "A decentralized inference network only works if the price consumers pay and the reward nodes earn reinforce each other. Here is the flywheel we're building toward, the spirals that kill it, and the three invariants that keep it turning.",
    date: "2026-07-16",
    tags: ["economics", "tokenomics", "KVR", "design"],
    blocks: [
      {
        t: "callout",
        md: "**Thesis:** Kvasir is a two-sided market settled in one token — consumers pay KVR to infer, nodes earn KVR to serve. The whole design succeeds or fails on one property: those two sides must form a **virtuous cycle**, where each turn makes the next turn easier. Get that wrong and any price policy eventually collapses; get it right and the network grows *cheaper* as it grows *larger*.",
      },
      {
        t: "p",
        md: "It's tempting to treat cost and reward as a tug-of-war — every dollar a consumer saves is a dollar a node doesn't earn. That framing is a trap. In a healthy network they are the **same flywheel** seen from two ends: payments become rewards, rewards become supply, supply becomes capacity and lower prices, lower prices become more usage, and more usage becomes more payments. The question isn't how to split a fixed pie; it's how to keep the wheel turning so the pie grows.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "The flywheel", text: "Why usage and supply grow together" },
      {
        t: "p",
        md: "The engine of the cycle is a single rule already true in Kvasir: **inference must be paid in KVR**. That makes every unit of usage a unit of real demand for the token — utility, not speculation. Token demand supports the value of the KVR nodes earn; attractive rewards pull in supply; supply expands capacity and, through competition and finer expert-sharding, drives the marginal cost of serving down; cheaper, faster, more capable service pulls in more usage. Kvasir tightens the loop with a property no centralized API can copy: a participant can be **consumer and supplier at once**. The demand side and the supply side often grow inside the *same people*, which damps the imbalances that wreck one-sided markets.",
      },
      { t: "h2", kick: "The failure modes", text: "Four spirals that run the wheel backward" },
      {
        t: "p",
        md: "A flywheel can spin down as easily as up. Naming the death spirals is how you design against them:",
      },
      {
        t: "table",
        head: ["Spiral", "How it starts", "Where it ends"],
        rows: [
          ["Reward dilution", "More nodes chase flat demand", "Per-node reward falls, nodes leave, capacity drops"],
          ["Price-too-low", "Cheap price, rewards below node cost", "Serving stops paying, supply and quality collapse"],
          ["Price-too-high", "Good rewards, but above market", "Users pick a cheaper API, revenue dries up"],
          ["Emission dependence", "Rewards paid by minting, not revenue", "Inflation erodes KVR until both sides give up"],
        ],
      },
      { t: "h2", kick: "The invariants", text: "Three rules that keep the cycle virtuous" },
      {
        t: "ul",
        items: [
          "**Rewards are funded by real revenue.** At steady state, what nodes earn comes from what consumers pay — not from open-ended token emission. Emission is a bootstrap subsidy that must *taper* as fee revenue grows. Kvasir already helps here by rewarding **real work** — KVR per tokens actually served × layer share, not mere presence — so subsidy can't leak to idle 'mercenary' nodes.",
          "**KVR is the mandatory medium.** Because you can't infer without paying KVR, usage is a permanent demand sink for the token. That anchors token value to real utility instead of speculation — the difference between a currency and a chip.",
          "**Price floats inside a band.** A floor kept above node marginal cost keeps serving worthwhile; a ceiling kept below centralized alternatives keeps Kvasir competitive. Between them, price moves — which is where the network's growth finally shows up as lower cost.",
        ],
      },
      { t: "h2", kick: "The thermostat", text: "Making \"more nodes → cheaper\" true in code" },
      {
        t: "p",
        md: "Today price is a governed constant — sensible for a devnet, but it means adding nodes raises *capacity*, not affordability. The design direction is a **utilization-driven price**: idle supply nudges the price down toward the floor, congestion nudges it up toward the ceiling. That single signal turns the intuition *\"the more people share compute, the cheaper it gets\"* into a rule the protocol enforces — while the floor keeps operators solvent so the supply that made it cheap doesn't evaporate. Because price is a sensitive economic parameter, it changes only under **genesis-wallet authority with wallet-signature + 2FA**, never a stray environment variable.",
      },
      {
        t: "callout",
        md: "**\"Free\" is the net, not the price.** You pay for what you infer and earn for what you serve; contribute roughly as much as you consume and your bill nets to zero. No subscription API — Claude Max, a Codex seat — can offer that, because you can never be their supply side. With Kvasir you can run models your own machine can't hold *and* be paid for helping others run theirs.",
      },
      {
        t: "p",
        md: "None of this requires exotic mechanism design. It requires discipline about three things: reward from revenue, value from usage, balance from a bounded floating price. Kvasir already ships the hard, honest parts — non-custodial settlement, work-proportional rewards, a token you must actually spend to use the network. The rest is the economic roadmap: the taper, the fee split that funds an insurance pool for failed inferences, and the thermostat. Built in that order, cost and reward stop fighting and start compounding.",
      },
    ],
  },
  {
    slug: "remote-gpu-joins-122b",
    category: "demos",
    title: "A GPU Across the Internet Joined 122B Inference",
    dek: "A Blackwell workstation in another city dialed one outbound 443 connection and computed experts for a live 122B decode — byte-identical to a local run, and paid in KVR for the work it did.",
    date: "2026-07-16",
    tags: ["swarm", "WAN", "expert-dispatch", "demo"],
    blocks: [
      {
        t: "callout",
        md: "**What happened:** a 122B decode running on an AMD backbone in one place sent its per-token expert work to an NVIDIA GB10 (Grace Blackwell) machine in another city — over a single outbound WebSocket on port 443 — and got back expert outputs that produced the **exact same tokens** as computing locally. No tunnel, no port-forwarding, no inbound firewall hole. The remote box earned KVR for the bytes it served.",
      },
      {
        t: "p",
        md: "Kvasir's premise is *whatever hardware shows up* — including hardware behind carrier NAT, on the public internet, in a different city. Qwen3.5-122B-A10B carries **86% of its weight in 12,544 independent experts** (48 layers × 256, top-8), each a 5.3 MB pure function. That grain is what lets a distant, unrelated machine hold a slice and contribute. The open question was never *can we split it* — it was *can a worker across the open internet actually participate in a live decode, correctly and accountably*. Now it has.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "One outbound dial", text: "No tunnel, no inbound ports" },
      {
        t: "p",
        md: "The remote worker opens **one** connection — outbound `wss://` to the public gateway on 443, the only port carrier NAT and CDN edges reliably pass. The gateway doesn't parse the stream; it **raw-splices** the WebSocket to the LAN-only hub, which bridges it to the backbone's expert-dispatch listener. Both ends dialed outward and met in the middle. The worker exposes zero inbound ports and needs no public address.",
      },
      {
        t: "code",
        caption: "Two outbound dials, spliced into one ordinary dispatch stream.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Byte-identical across the internet", text: "The router decides once; the math regroups exactly" },
      {
        t: "p",
        md: "The backbone runs the router **once** and authoritatively; the worker is a pure `(hidden, ids) → out` function. So moving that function across a continent changes *where* the multiply happens, not *what* it computes. On a live 122B decode with layer-0 experts served remotely: the greedy token stream was **8/8 identical** (\" Paris.\"), logit **cosine 0.99773**, argmax match. This is the same router-authority property that keeps CUDA↔ROCm↔CPU discrete decisions invariant — heterogeneous backends stay bounded by continuous error, never a catastrophic branch.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "greedy tokens identical" },
          { n: "0.99773", l: "logit cosine, remote vs local" },
          { n: "1.2%", l: "TPS overhead, direct (13 ms RTT)" },
          { n: "0.00895", l: "KVR to the worker, first remote session" },
        ],
      },
      { t: "h2", kick: "The honest cost is RTT", text: "Why the swarm is a throughput fabric, not a low-latency decoder" },
      {
        t: "p",
        md: "Serial per-token dispatch pays a round trip per step. Measured: with a direct link (13 ms RTT) the throughput overhead was **1.2%** (4.220 → 4.169 tok/s); routed through a CDN edge on 443 it was **~28%**. We publish that honestly, because it points at the design truth — a WAN swarm is **RTT-bound**, so its strength isn't the latency of one stream but **aggregate capacity**. Batching amortizes the round trip: batched expert dispatch reaches **77× the per-token throughput** at batch 512. Bytes are headroom; round-trips are the thing to hide — which is the subject of the companion roadmap post.",
      },
      { t: "h2", kick: "Paid for exactly the work", text: "Metered bytes become KVR" },
      {
        t: "p",
        md: "Participation is worthless if it isn't accountable. The relay **meters the bytes bridged per session** into the hub's contribution ledger; the gateway polls that ledger and delta-credits KVR to the worker's **own** wallet — non-custodial, like everything else. The first cross-internet session actually accrued: **1.28 MB of work → 1.277952 units → 0.00895 KVR** in pending rewards. Small, and that's the point — it's real, per-work settlement, not a participation trophy.",
      },
      {
        t: "p",
        md: "The same outbound-443 path is exactly how a **phone** joins: a Galaxy S25 has already computed 122B experts over it (8/8 identical tokens, cosine 0.99992). A frontier-scale model, served by a backbone in one place, a datacenter GPU in another city, and a phone in someone's pocket — all producing the same tokens, each paid for its share. What's next is making the WAN round trip cheap; that roadmap is grounded in others' production numbers and our own measurements.",
      },
    ],
  },
  {
    slug: "wan-dispatch-comm-roadmap",
    category: "core",
    title: "Making WAN Dispatch Cheap: A Grounded Roadmap",
    dek: "Remote expert dispatch works and is byte-identical — but a WAN decode is round-trip-bound. Here's the plan to cut the cost, grounded in production numbers from DeepSeek, Petals and others (roadmap, not shipped).",
    date: "2026-07-17",
    tags: ["roadmap", "WAN", "MoE", "communication"],
    blocks: [
      {
        t: "callout",
        md: "**Framing:** the numbers *we measured* are stated as measured; everything described as a plan is a **roadmap**, not a shipped result. The goal is to take remote expert dispatch — already correct and paid (see the companion post) — and make the WAN round trip cheap enough that a distant GPU or a phone is a first-class swarm member, not a slow one.",
      },
      {
        t: "p",
        md: "Our own measurement, published plainly: dispatch costs about **110 KB per token per layer** — 12.3 KB out (dispatch) plus 98.3 KB back (combine). The 8× asymmetry is because each selected expert returns its full output *before* the weighted sum. Directly linked, that's a **1.2%** throughput overhead; through a CDN relay, **~28%**. Those are the facts. The rest of this post is how we intend to close the gap — and why bytes are the easy part.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "The dominating law", text: "A WAN decode is round-trip-bound" },
      {
        t: "p",
        md: "The single most important published result here isn't ours — it's Petals': as RTT goes from <5 ms to 100 ms, decode drops from **1.24 to 0.57 steps/s**, while a **10× bandwidth cut changes it by ~0**. Latency dominates; bandwidth is slack. That reframes the whole problem: shaving bytes is headroom, but **cutting round trips is the substance**. Every item below is ranked by how much round-trip cost it removes.",
      },
      { t: "h2", kick: "Cheaper on the wire", text: "Accuracy-first byte reduction" },
      {
        t: "ul",
        items: [
          "**Return weighted partial sums, not raw expert outputs.** By linearity the backbone's combine is exact either way, but the worker returns one summed vector instead of 8 — that's the ~8× combine reduction, and it's precisely what DeepSeek-V3 / DeepEP do in production.",
          "**Parallel star, not serial chain** across multiple workers: ΣRTT collapses to max RTT.",
          "**F16 on the wire** — we already accept a cross-backend cosine of ~0.998, so F16 transport is inside existing tolerance; **blockwise INT8/FP8 later**, after our own argmax/cosine gate clears it against Q4_K_M weights (Petals showed INT8 over the real internet with no quality loss).",
          "Together these target **~110 KB → 9–12 KB per token (~12×)** — real, but remember it's the *headroom*, not the bottleneck.",
        ],
      },
      { t: "h2", kick: "Amortizing the round trip", text: "The substance: fewer trips, hidden trips" },
      {
        t: "ul",
        items: [
          "**Speculative decoding** turns many tokens into one round trip. At a measured 80 ms WAN, break-even is only **~1.15–1.2 accepted tokens/step** — so even a weak n-gram guess wins (vanilla Jacobi can backfire; technique choice matters). Our dispatch protocol already carries `n_tokens > 1`, so no wire change is needed.",
          "**Continuous batching at the gateway** folds concurrent requests into one trip; **slot-affinity prefix caching** keeps a session on the same replicas.",
          "**Latency hiding**: the shared expert is an independent additive term, so the backbone computes it *locally* during the remote round trip (ScMoE reports 1.82× over PCIe, no retraining). Keep **hot experts local**, send only cold ones remote (EPLB replicates the ~32 hottest for a 2.54× decode speedup in production).",
        ],
      },
      { t: "h2", kick: "Policy & the fat-pipe future", text: "Route by peer, and what 200 Gb/s changes" },
      {
        t: "p",
        md: "Path policy: publicly-routable peers take the **direct** path (the 1.2% route); the relay is for NAT-bound devices only. And when wide 200 Gb/s links arrive, the 110 KB serializes in **~4.4 µs** — the bandwidth term vanishes even before the reductions above, and a 794 MB slice ships in ~32 ms. But **RTT is physics; it doesn't shrink** — so speculative decoding and overlap remain the real levers even at 200 G. Where the fat pipe genuinely matters is multi-backbone federation (several backbones sharing one expert pool) and bandwidth-bound work: long-prompt prefill and large-batch throughput.",
      },
      {
        t: "callout",
        md: "**One caveat, stated honestly:** the transport layer itself (WebSocket vs QUIC, masking overhead, NAT hole-punching) has **no external result we can cite** — that's engineering we'll measure ourselves before claiming anything. Everything above rests on published production numbers (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) plus our own measurements; when a roadmap item ships, its numbers and tense get updated here.",
      },
      { t: "h2", kick: "What's next", text: "Onboarding targets" },
      {
        t: "p",
        md: "Our dispatch hook sits on `build_moe_ffn` — **a single function 43 MoE architectures share** in the inference engine. Three invariants are model-independent: the MoE math (routed = Σ wᵢ·Eᵢ(x), linear), the shared code path, and GGUF's standard stacked expert tensors (outermost `ne[2]` → block-aligned slicing). So onboarding a new model isn't a redesign — it's one pass through a per-model argmax/cosine verification gate.",
      },
      {
        t: "table",
        head: ["model", "experts · routing", "per-expert (Q4≈)", "shared", "status"],
        rows: [
          ["Qwen3.5-122B (serving today)", "256 · top-8", "5.3 MB (measured)", "yes", "in production"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "yes", "ready — first candidate"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "yes", "ready (hook verified)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "no", "ready (hook verified)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "yes", "ready (deepseek2 graph)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "yes", "ready (deepseek-family)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "no", "ready"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "no", "ready"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "yes", "ready (MoE every other layer)"],
          ["MiniMax M3 428B", "128 · top-4", "TBD (GGUF)", "yes", "waiting on upstream engine"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "no", "works — GPU workers only"],
        ],
      },
      {
        t: "p",
        md: "The industry is converging on fine-grained MoE — smaller experts, more of them, higher sparsity (DeepSeek, Qwen, Kimi, GLM, gpt-oss all moved this way). Every step in that direction makes the swarm's unit of participation smaller and the scarcity market's grain finer. The models above aren't a wish list; each already flows through the same dispatch hook we run in production — onboarding is a verification gate, not an engineering project.",
      },
    ],
  },
  {
    slug: "what-200g-buys-a-swarm",
    category: "core",
    title: "The 200G Question",
    dek: "Our swarm hubs can already link at 200 Gb/s with parts on the shelf — one is built into the GB10. Here is what a fat pipe buys a distributed MoE, and the one thing it can't.",
    date: "2026-07-17",
    tags: ["networking", "200GbE", "architecture"],
    blocks: [
      {
        t: "callout",
        md: "**The premise:** WAN decode is RTT-bound, not bandwidth-bound — our comm roadmap showed bytes are the easy part. So what actually changes when hubs get 200 Gb/s links? Almost everything about *capacity*, and almost nothing about *latency*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Already in the box · ConnectX-7", text: "The hardware is not futuristic — one ships inside our GB10 worker" },
      {
        t: "p",
        md: "The GB10 Grace Blackwell that computes our 122B experts carries an **NVIDIA ConnectX-7 with two 200 GbE QSFP ports** on board. Two of these machines direct-connect with a single ~$100 QSFP56 DAC cable — a 200G two-hub cluster with zero switches. ARM is a first-class citizen here: the same `mlx5` driver stack that runs these NICs in x86 datacenters runs them on aarch64, which is exactly what the GB10 is.",
      },
      {
        t: "callout",
        md: "**The fine print:** the GB10 feeds its ConnectX-7 through two PCIe Gen5 x4 links in multi-host mode. Measured full speed (~185–190 Gb/s) requires **RoCE (RDMA) and a correctly mapped topology** — naive TCP over a mis-mapped path lands at ~95 Gb/s or worse. Fat pipes are bought with configuration, not just cables.",
      },
      { t: "h2", kick: "The distance ladder", text: "200G is a catalog item at every reach" },
      {
        t: "table",
        head: ["reach", "part", "form factor"],
        rows: [
          ["rack (0.5–3 m)", "QSFP56 DAC copper", "cable, ~$100"],
          ["room (~30 m)", "AOC active optical", "cable"],
          ["campus (2–10 km)", "200G FR4 / LR4 optics", "QSFP56 module"],
          ["metro (~40 km)", "200G ER4 optics", "QSFP56 module"],
          ["region (~120 km)", "400G ZR+ coherent, run at 200G line rate", "QSFP-DD module"],
          ["long-haul (100s of km)", "carrier 200G wavelength / DWDM line system", "leased service"],
        ],
      },
      {
        t: "p",
        md: "In the WAN, the *cable* is just standard single-mode fiber — speed-neutral glass that already spans every city. The speed lives in the pluggable optics at each end, and **OpenZR+ made 200G-over-120 km a module you plug into a switch**, not a telecom project. Beyond that, you lease a wavelength.",
      },
      { t: "h2", kick: "What it buys", text: "Every bandwidth term in the swarm vanishes" },
      {
        t: "ul",
        items: [
          "A dispatch payload (~110 KB/token/layer today, ~10 KB after the wire roadmap) serializes in **microseconds** — payload size stops being a design constraint at all.",
          "An **expert slice ships in ~32 ms** (794 MB, theoretical) and a whole 122B model syncs in **~3 s** — coverage-market rebalancing and new-hub onboarding become near-instant.",
          "Long-context prefill — the one genuinely bandwidth-heavy phase — moves at wire speed, so first-token time on 100K-token prompts becomes backbone-compute-bound.",
          "**Batched dispatch scales without a wire ceiling**: expert-pool traffic aggregated across many user streams is exactly the bandwidth-heavy, latency-tolerant load a fat pipe absorbs. This is what makes a multi-backbone federation — several hubs, each holding KV for its own users, sharing one expert pool — practical.",
        ],
      },
      { t: "h2", kick: "What it can't buy", text: "Light does not hurry" },
      {
        t: "p",
        md: "Fiber carries light at ~5 µs/km, and no amount of bandwidth changes that. A 13 ms round trip is 13 ms at 200 Gb/s. Autoregressive decode pays that round trip per sharded layer, per token — which is why **speculative decoding (k tokens per round trip) and shared-expert overlap (compute while the dispatch is in flight) stay essential** even between hubs joined by the fattest pipe on the market. Bandwidth buys throughput; only round-trip discipline buys latency.",
      },
      {
        t: "p",
        md: "So the architecture settles into two tiers. A **hub tier** — backbones and hot experts joined by 200G-class links, where capacity is effectively unbounded — and an **edge tier** — phones and small devices on the 443 relay, holding the long tail of experts the scarcity market assigns them. The fat pipe makes the first tier feel like one machine; the relay keeps the second tier open to anyone. Neither replaces the other: that split *is* the design.",
      },
    ],
  },
  {
    slug: "the-swarm-that-grows-under-load",
    category: "core",
    title: "The swarm that grows under load",
    dek: "A giant model that borrows help only when it needs it — the MoE swarm now scales itself to traffic: tight and fast when quiet, wide and parallel when busy.",
    date: "2026-07-17",
    tags: ["core tech", "elastic scaling", "MoE"],
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir serves models far larger than any single machine holds — a 122B-parameter Mixture-of-Experts model runs across a coordinator plus a swarm of workers: GPUs on the LAN, GPUs across a 200 Gb/s link, even phones that dial in over the internet. Because an MoE model routes each token to only a handful of its experts, most of the weights sit idle at any moment, and those idle experts can live **off** the main node — on whatever hardware volunteered to hold them.",
      },
      {
        t: "callout",
        md: "The new part is that the swarm now **scales itself to the load**.",
      },
      { t: "h2", kick: "How it behaves", text: "Tight when quiet, wide when busy" },
      {
        t: "p",
        md: "When traffic is light, the coordinator serves everything on its own GPU — the fastest path per token, no network hops. When requests start piling up and its inference slots saturate, two things happen automatically:",
      },
      {
        t: "ul",
        items: [
          "**It re-engages the workers it already has.** The coordinator watches its own queue. Under saturation it keeps streaming routed-expert work out to **proven** workers — ones that have actually served before — trading a little per-token latency for a lot more total throughput. A worker that merely connected but never computed is never trusted with load; a brand-new worker still gets a fair first try.",
          "**The hub recruits new ones.** The control hub notices the same saturation and raises the \"demand\" for that model's experts. Idle nodes — a phone in someone's pocket, a spare GPU across town — are already polling that demand market. The moment demand rises, they're offered a slice of experts to serve, download it, dial in, and join. When the surge passes, demand falls back and the extra workers quietly drop away.",
        ],
      },
      {
        t: "p",
        md: "No one schedules this. No node is pushed. The swarm breathes with the load: tight and fast when quiet, wide and parallel when busy — and it works even for nodes behind home routers, because everything is pull-based.",
      },
      {
        t: "p",
        md: "That's the shape of a network that can serve trillion-parameter models on hardware no one person owns: idle capacity is invited in exactly when it's worth inviting, and only then.",
      },
    ],
  },
];

export function techArticleBySlug(slug: string): TechArticle | undefined {
  return TECH_ARTICLES.find((a) => a.slug === slug);
}
