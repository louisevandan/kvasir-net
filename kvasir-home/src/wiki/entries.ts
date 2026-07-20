/* ==========================================================================
   Kvasir wiki — knowledge-base entries for /wiki.
   Reference entries on every concept in the network, grounded in the actual
   linkcpp code (controller/hub.py, planner.py, nodeagent.py, versioning.py,
   the expert-worker C++ tools), homepage-brief.md and the tech-blog posts.

   This file is the ENGLISH source of truth for structure and content.
   Localized bodies live in tr-<lang>.ts (same slugs; title/summary/blocks
   replaced per language) and are merged by getWikiEntries() in
   translations.ts. `image` is language-neutral and shared by all locales.

   Reuses the TechBlock content model + shared renderers in
   src/components/blocks.tsx. Inline markup: **bold**, `code`.
   ========================================================================== */

import type { TechBlock } from "../tech/articles";

export const WIKI_CATEGORIES = ["network", "inference", "token"] as const;
export type WikiCategory = (typeof WIKI_CATEGORIES)[number];

export type WikiEntry = {
  slug: string;
  category: WikiCategory;
  title: string;
  summary: string;
  /* Language-neutral infographic (public/wiki/). Rendered after the block at
     index `imagePos` — block sequences are identical across locales, so one
     position works for every language. Undefined → right under the summary. */
  image?: { src: string; alt: string };
  imagePos?: number;
  blocks: TechBlock[];
};

/* A per-language override of one entry's translatable content. Structure
   (slug, category, image) stays here; translations live in tr-<lang>.ts
   and are merged by getWikiEntries() in translations.ts. */
export type WikiTranslation = Pick<WikiEntry, "title" | "summary" | "blocks">;

export const WIKI_ENTRIES: WikiEntry[] = [
  /* ------------------------------------------------------------------ */
  /* Network & roles                                                     */
  /* ------------------------------------------------------------------ */
  {
    slug: "kvasir-network",
    category: "network",
    title: "Kvasir network",
    summary: "A decentralized AI-inference network (DePIN) where everyday devices serve open models and earn KVR.",
    image: { src: "/wiki/kvasir-network.jpg", alt: "Devices around the globe serving one model together" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "**Kvasir** is a decentralized AI-inference network: large open models are split across shared hardware with the **linkcpp** engine, so no single node holds the whole model. Anyone can contribute a GPU, CPU, NPU — even a phone — and earn **KVR** for the layers or experts their device actually serves. Developers reach the network through OpenAI/Anthropic-compatible gateways and pay per inference.",
      },
      {
        t: "ul",
        items: [
          "**Source-available engine** — linkcpp is licensed under the BSL (free for development and testing, production use requires a license); the inference engine data plane underneath stays stock and inspectable.",
          "**Non-custodial** — rewards settle to each node owner's own Solana wallet; keys never leave the user.",
          "**Proven on real hardware** — a 122B model has run split across 4 AMD MI250 GPUs, across a heterogeneous fleet of GPU/CPU/NPU/mobile nodes, with per-node contribution credited end-to-end.",
          "**Named after the Norse myth** — Kvasir, the wisest being, born from the pooled essence of every god and owned by none.",
        ],
      },
      { t: "h2", kick: "One request, many devices", text: "How an inference flows" },
      {
        t: "code",
        caption: "Every hop is ordinary HTTP/TCP; the model itself is what's distributed.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Roles **stack**: one machine can be a compute node, a gateway host and a hub host at once, and its rewards sum. The network's job is to make the aggregate look like one machine — one endpoint in front, thousands of imperfect devices behind.",
      },
      {
        t: "p",
        md: "Today the network runs on **Solana devnet**; KVR is a utility / contribution token, not a tradable asset or investment, and nothing on this page is financial advice.",
      },
    ],
  },
  {
    slug: "hub",
    category: "network",
    title: "Hub",
    summary: "The control plane: discovers devices, plans layer placement, launches workers, orchestrates the ring.",
    image: { src: "/wiki/hub.jpg", alt: "A hub orchestrating node slots, remote units and agents" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "The **hub** is the network's control plane, served by linkcpp as a single Docker image (`controller.hub:app`, a FastAPI service on port **19000**). It discovers devices, checks runtime compatibility, plans placement with the planner, launches stock inference engine workers, and exposes the per-controller gateways. It is deliberately boring infrastructure: request/response HTTP, restart-safe state, no exotic transport.",
      },
      { t: "h2", kick: "Three doors in", text: "How machines join a hub" },
      {
        t: "ul",
        items: [
          "**Local node slots** — five fixed slots per hub, mapped to RPC ports **50052–50056**. Slots always exist; you edit a slot's GPU + VRAM/RAM/CPU budgets rather than creating arbitrary nodes, and resources are editable **only while a slot is unbound**, which protects the capacity contract under a running controller.",
          "**Remote units** — register another running linkcpp hub and import its visible nodes. The data-plane endpoint is always derived from the registered *unit* URL plus the unit-exposed worker port — never from a node host the remote system advertises.",
          "**Managed node agents** — worker-only services (`nodeagent.py`) that join over plain request/response HTTP (`/control/join|status|download|load|unload`) and report via `POST /api/node-reports`. Deliberately **not** a persistent stream, so they survive simple LAN/VPN routing.",
        ],
      },
      { t: "h2", kick: "Nothing loads unverified", text: "Compatibility gating" },
      {
        t: "p",
        md: "Every unit, node and agent reports a protocol / runtime-pack identity plus backend details. Unit, runtime-pack, inference engine-revision and RPC-ABI mismatches are **hard-blocked before bind, plan, load or infer**; backend differences (CUDA/Metal/Vulkan/CPU) are tracked as node capabilities, not rejections. Adaptive loading is also blocked when a node can't provide the resource monitoring a safe plan needs.",
      },
      {
        t: "code",
        caption: "What survives a restart, and what doesn't.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Because the hub is the most critical role, hub hosts earn the **highest hourly uptime reward**. Operating a public hub requires staking **100,000 KVR**.",
      },
    ],
  },
  {
    slug: "gateway",
    category: "network",
    title: "Gateway",
    summary: "The public entry point: OpenAI/Anthropic-compatible APIs and KVR pay-per-inference settlement.",
    image: { src: "/wiki/gateway.jpg", alt: "One API door in front of many serving devices" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "The **gateway** is where developers meet the network. Every controller exposes OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) and Anthropic-compatible ones (`/anthropic/v1/messages`, `/anthropic/v1/models`), all backed by the same loaded model — an existing client works by changing only the base URL and key.",
      },
      {
        t: "code",
        caption: "A stock OpenAI-style call against the Kvasir gateway.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Metering", text: "Pay-per-inference in KVR" },
      {
        t: "p",
        md: "Usage settles in KVR through a three-step flow — **quote → payment → inference** — so a request is priced before it runs and the nodes that served it are credited after. The gateway also aggregates a **live model catalog** from every reachable hub, so `/v1/models` reflects what the network can actually serve right now.",
      },
      {
        t: "ul",
        items: [
          "Gateway hosts earn an **hourly uptime reward** for keeping the entry point online, plus a **×1.5 bonus** on every inference they help serve.",
          "Operating a public gateway requires staking **100,000 KVR** (same as a hub).",
          "Public deployments protect operator access with **SIWS + 2FA**; plain hubs are designed for trusted host / LAN / VPN only.",
        ],
      },
    ],
  },
  {
    slug: "node",
    category: "network",
    title: "Node",
    summary: "Any device serving a share of a model — GPU, CPU, NPU or phone — earning KVR for the work it does.",
    image: { src: "/wiki/node.jpg", alt: "Heterogeneous devices each holding a small share of a model" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "A **node** is any device that serves part of a model: a GPU box, a CPU machine, an NPU device, or a phone. A node holds only its share — a layer window on the ring, or an expert slice in the swarm — and earns KVR weighted by exactly the work it performed. The live fleet has mixed AMD MI250s, NVIDIA GB10s and RTX Pro 6000s, a MacBook, x86 Windows CPU machines and mobile nodes in one network.",
      },
      { t: "h2", kick: "From download to payout", text: "A node's lifecycle" },
      {
        t: "code",
        code: `join      → slot bind / unit import / agent /control/join
report    → capabilities: backend · VRAM/RAM/CPU budgets · monitoring
plan      → planner assigns a layer window (or expert range)
download  → partial shard: only the tensors that window needs
serve     → run its share; pass boundaries / answer dispatch
earn      → units × layer_share × perf_tier → owner wallet`,
      },
      {
        t: "ul",
        items: [
          "**Compute nodes** earn per contribution unit, weighted by layer share and scaled by performance tier — no stake required.",
          "Nodes register under their owner's wallet; rewards settle to that wallet, non-custodially. Four distinct owner wallets each earning their layer share has been verified end-to-end.",
          "Capability data (backend, accumulation precision, resource budgets) decides what the planner may place on a node — and, in the swarm, which ranks it may serve.",
          "A node that can't provide resource monitoring is excluded from adaptive loading rather than trusted blindly.",
        ],
      },
    ],
  },
  {
    slug: "relay-443",
    category: "network",
    title: "443 relay",
    summary: "The data plane for NAT-bound devices: both ends dial outbound through a WebSocket bridge on port 443.",
    image: { src: "/wiki/relay-443.jpg", alt: "Phone and backbone both dialing outbound into a relay" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "Phones behind carrier NAT can't accept inbound connections, and edges like Cloudflare only pass ports 80/443. The **443 relay** solves both: a per-edge WebSocket bridge with a **1-byte role preamble** lets both sides dial **outbound**, so a phone participates in the data plane while opening **zero inbound ports**.",
      },
      {
        t: "code",
        caption: "Two outbound dials meet in the middle; the preamble says who is who.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "Hardened in production", text: "Three real bugs, three fixes" },
      {
        t: "ul",
        items: [
          "**Build-fingerprint agreement** — both ends must prove they run the same runtime pack before any tensor bytes flow.",
          "**Node-token download auth** — partial-shard downloads are authenticated with the same wallet-derived node token the app already holds.",
          "**The `Int.ushr` frame stall** — Kotlin's `ushr` uses only the low 5 bits of its shift, so `len ushr 56` became `len ushr 24` and silently corrupted every frame ≥ 64 KiB (a 593 KB `result_output` was the first casualty). Fixed by moving length packing to `Long` shifts — and load-bearing for batched expert dispatch, which routinely exceeds 64 KiB.",
        ],
      },
      {
        t: "p",
        md: "The relay carries whatever the topology needs — ring layer boundaries or expert dispatch streams — and the same mechanism verified for the ring is what production phone workers use in the swarm.",
      },
      {
        t: "p",
        md: "Both `/api/expert-relay` and `/api/ring-relay` upgrades are **raw-spliced**: the gateway forwards WebSocket frames byte-for-byte without parsing them, so the relay stays a thin, model-agnostic pipe. It still **meters the bytes it bridges per session**, and that measured work flows into the hub's contribution ledger and settles to the worker's own wallet in **KVR** — relaying for a NAT'd phone earns exactly like a direct-connected node.",
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Inference & engine                                                  */
  /* ------------------------------------------------------------------ */
  {
    slug: "linkcpp",
    category: "inference",
    title: "linkcpp",
    summary: "The source-available (BSL) control plane that turns everyday hardware into a distributed inference engine.",
    image: { src: "/wiki/linkcpp.jpg", alt: "A control plane orchestrating stock inference engine workers" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "**linkcpp** is the engine behind Kvasir: a control plane around inference engine's RPC data plane that runs large AI models across multiple GPUs and machines using *stock* `ggml-rpc-server` / `llama-server` binaries. Everything it adds is orchestration — GPU discovery, node slots, layer-placement planning, worker launch, and the OpenAI/Anthropic gateways.",
      },
      { t: "h2", kick: "Architecture", text: "One hub, stock workers" },
      {
        t: "code",
        caption: "The request path through a linkcpp deployment.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Source-available under the BSL** — free to read, run and build on in development and testing; production use requires a license.",
          "The inference engine data plane stays **unforked** (one pinned mobile GPU-over-RPC patch aside), so upstream performance work keeps flowing in.",
          "Ships as a **single Docker image**: the FastAPI hub plus the two inference engine binaries baked in; native worker nodes build outside Docker for CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "The planner", text: "GGUF metadata in, placement out" },
      {
        t: "p",
        md: "The planner reads GGUF metadata and produces contiguous per-node layer windows, the matching `--tensor-split`, and KV-cache / layer / expert VRAM estimates per node — plus optional MoE expert-FFN offload to node RAM, emitted as inference engine `-ot` rules (e.g. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) and carried to launch via `--override-tensor`. A plan that doesn't fit is reported **infeasible** before anything loads, not discovered as an OOM at runtime.",
      },
      {
        t: "p",
        md: "Runtime compatibility is a first-class concept: protocol, runtime-pack, inference engine revision and RPC ABI are verified and mismatches hard-blocked before any bind, plan, load or inference.",
      },
    ],
  },
  {
    slug: "ring-runtime",
    category: "inference",
    title: "Ring runtime",
    summary: "Pipeline inference without a master: each device runs its layer window and passes only boundaries to its neighbor.",
    image: { src: "/wiki/ring-runtime.jpg", alt: "Devices in a ring passing hidden-state boundaries" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "The **ring runtime** is Kvasir's low-latency serving topology. Every device loads only its contiguous **layer window**, then opens exactly two links — predecessor and successor. Hidden-state boundaries circulate around the ring; the last rank samples the token and returns it. **No central master, and no node holds the whole model.**",
      },
      { t: "h2", kick: "Why not a star", text: "The RPC master problem" },
      {
        t: "p",
        md: "In the classic RPC topology one master opens the **entire GGUF** and dials out to every worker. That breaks in an open network three ways: the master must hold and serve the whole checkpoint; every worker must be dialable — phones behind carrier NAT are not; and the master is a single owner in a network that should have none. The ring removes all three: each stage owns its window, connections are neighbor-to-neighbor, and the relay makes NAT devices reachable.",
      },
      {
        t: "code",
        caption: "One decode step around a 4-stage ring.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "Placement comes from the planner's **rank manifest** — e.g. Qwen3.5-122B's 49 layers split across a GPU, CPU, NPU and phone.",
          "Boundaries are small (a hidden-state vector per token), so hops are cheap even for weak links.",
          "Mobile GPUs run ring stages **directly** (Adreno via OpenCL) — the RPC path to a phone GPU was infeasible because Adreno's buffer layout doesn't survive RPC serialization, but a local stage owns its backend, so only boundaries cross the wire.",
        ],
      },
      {
        t: "p",
        md: "The ring is the **latency** path; its floor is layer granularity (~1.4 GB on the 122B). The expert-sharded swarm removes that floor and plugs into the same serving fabric.",
      },
    ],
  },
  {
    slug: "layer-window",
    category: "inference",
    title: "Layer window & partial shards",
    summary: "A node's contiguous slice of the model — downloadable as a mini-GGUF instead of the full checkpoint.",
    image: { src: "/wiki/layer-window.jpg", alt: "A small window cut out of a tall stack of layers" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "A **layer window** is the contiguous range of transformer layers a ring node serves. A node doesn't need the full checkpoint to serve one — a **stage mini-GGUF** carries only the window's tensors: for the 122B, **254 MB of 26 tensors** (of 338 total) versus the 77.6 GB full model, or ~1.5 GB for a one-layer window on a phone.",
      },
      {
        t: "code",
        caption: "A rank manifest row: who serves what, within which budget.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Claimed, not assigned", text: "Self-enrollment" },
      {
        t: "ul",
        items: [
          "A node polls the **coverage/demand map** to see which windows are under-served and what each pays.",
          "It picks the **highest-reward** uncovered window that fits its budget, downloads exactly that, and joins.",
          "Coverage self-heals: when a node churns away, its window becomes scarce — and therefore lucrative — again.",
          "Verified end-to-end with a NAT-bound phone: poll → self-enroll → partial download → Adreno GPU load → ring inference completed, contribution credited.",
        ],
      },
      {
        t: "p",
        md: "The expert swarm reuses this exact market at the finer **(layer, expert-range)** grain — same map, same self-enrollment, same rewards, smaller units.",
      },
    ],
  },
  {
    slug: "moe",
    category: "inference",
    title: "Mixture of Experts (MoE)",
    summary: "A model whose FFNs are hundreds of independent experts, only a few of which fire per token.",
    image: { src: "/wiki/moe.jpg", alt: "A router lighting up 8 of 256 expert blocks" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "A **Mixture-of-Experts** model replaces each layer's single FFN with a bank of independent expert FFNs plus a **router** that picks a few per token. Qwen3.5-122B-A10B is the network's flagship example:",
      },
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
      {
        t: "p",
        md: "Each layer splits into a **dense path** — attention + KV, norms, the router (`ffn_gate_inp`), a shared expert — and an **expert bank** stored as three stacked tensors (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). The dense path is the minority of the bytes; the expert bank is 86% of the model.",
      },
      {
        t: "ul",
        items: [
          "The expert index is the **outermost GGUF dimension**, so each expert is a contiguous, quant-block-aligned slab — extraction is a byte-range copy, no dequantization.",
          "Per token only **8 of 256** experts fire per layer, so a layer's decode-time expert traffic is a handful of small matrix multiplies over one hidden vector (~6 KB of dispatch).",
          "Experts are mutually independent — ownership can be scattered across devices and re-balanced freely.",
        ],
      },
      {
        t: "p",
        md: "This is why MoE is the swarm's natural substrate: the weights come pre-packaged in device-sized, independently ownable units.",
      },
    ],
  },
  {
    slug: "expert-sharding",
    category: "inference",
    title: "Expert sharding",
    summary: "Splitting a MoE at the expert grain, so a phone carries 42–340 MB of experts instead of a 1.4 GB layer.",
    image: { src: "/wiki/expert-sharding.jpg", alt: "Experts scattered across many small devices" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "**Expert sharding** drops the swarm's carrying unit from a layer (~1.4 GB on the 122B) to an expert (**5.3 MB**). A weak device downloads a slice of 8–64 experts (**42–340 MB**), loads it as a pure-function worker — no attention, no KV, no sampler — and computes its experts whenever the backbone's router selects them.",
      },
      { t: "h2", kick: "Two roles", text: "Backbone × worker" },
      {
        t: "code",
        caption: "The cut point inside one MoE layer (router runs once, on the backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "The **backbone** keeps the dense path (attention, norms, router, shared expert, combine) and holds all experts as a RAM-offloaded fallback replica for churn tolerance.",
          "**Workers** (`linkcpp-expert-worker --serve`) answer `(n_used, n_tokens, cur, sel) → experts` over one long-lived TCP stream — the same stream the 443 relay tunnels for phones.",
          "Coverage self-heals through the **expert coverage market**: `POST /api/expert-coverage` heartbeats holdings, `GET /api/expert-demand` aggregates scarcity, `POST /api/expert-volunteer` assigns the scarcest range clipped to the node's budget.",
        ],
      },
      { t: "h2", kick: "Measured, not promised", text: "Verified on real hardware" },
      {
        t: "ul",
        items: [
          "Sharded compute == monolithic to **max|Δ| = 3.6e-12** (an exact regrouping, not an approximation).",
          "Cross-process dispatch on a live 122B decode: **argmax MATCH**, logit cosine 0.99869 — byte-identical to in-process.",
          "A Galaxy S25 autonomously downloaded its 1.58 GB slice and computed layer-0 experts every token: **8/8 tokens identical** to the local run.",
          "A remote GPU over the public internet — one WAN round trip per token — stayed **greedy 8/8 identical** (cosine 0.99773): **1.2% throughput overhead** on a direct link, ~28% through a CDN edge. The honest cost of serial per-token dispatch, and why the fabric's lever is batching, not lower latency.",
          "Batched dispatch reaches **53k tok/s per worker** at batch 512 (ROCm) — the throughput-fabric property that makes the swarm practical.",
        ],
      },
    ],
  },
  {
    slug: "router-authority",
    category: "inference",
    title: "Router authority",
    summary: "The swarm's coherence invariant: routing is decided once, on the backbone — workers only receive expert ids.",
    image: { src: "/wiki/router-authority.jpg", alt: "One router broadcasting expert ids to many workers" },
    imagePos: 1,
    blocks: [
      {
        t: "callout",
        md: "**The invariant:** the only discrete decision inside the network is MoE routing (top-8 of 256). Kvasir runs the router **exactly once, on the backbone**, and dispatches only the selected expert ids to workers. A heterogeneous swarm may differ slightly in each expert's output *magnitude* — it never differs in *which experts run*.",
      },
      {
        t: "p",
        md: "Without this rule, each backend would re-run the router and pick **different experts** for borderline tokens — genuine, catastrophic divergence, because from that token on the computation forks like a different random seed. With it, hardware differences reduce to bounded continuous error that the probability-weighted combine absorbs.",
      },
      { t: "h2", kick: "What it prevents", text: "Divergence modes closed by one decision point" },
      {
        t: "table",
        head: ["Divergence mode", "Without authority", "With authority"],
        rows: [
          ["Routing mismatch", "Backends pick different top-8 at boundaries", "Ids decided once, dispatched to owners"],
          ["Trajectory fork", "One flipped token forks the whole sequence", "Decode/sampling pinned to one node"],
          ["Verification", "Bit-compare across backends (impossible)", "Tolerance checks on well-defined residuals"],
        ],
      },
      {
        t: "p",
        md: "The cost is negligible: the backbone was already computing `ffn_norm` and the router logits; what crosses the wire is just the hidden rows plus selected ids — about **6 KB per decode step**.",
      },
    ],
  },
  {
    slug: "numerical-equivalence",
    category: "inference",
    title: "Numerical equivalence",
    summary: "Different backends never agree bit-for-bit; the swarm treats measured tolerance as a first-class contract.",
    image: { src: "/wiki/numerical-equivalence.jpg", alt: "Three different chips producing overlapping, near-equal outputs" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno and CPUs compute the same op with different reduction orders, FMA fusion, accumulators and transcendental approximations — so results differ by ~1e-6…1e-3 per op, **by design, never bit-identically**. A swarm made of whatever hardware shows up can't demand bit-exactness, so Kvasir measures equivalence instead.",
      },
      {
        t: "table",
        head: ["Backend pair (real 122B, layer-0 experts)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Phone ARM CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "The full backend matrix is closed: the two GPU backends (CUDA, ROCm) share kernel sources and land **effectively bit-identical** (cosine 1.0000000000), while GPU↔CPU pairs stay equivalent at ~0.9997. A CUDA worker and a ROCm worker are interchangeable; a GPU worker and a CPU worker are numerically equivalent.",
      },
      { t: "h2", kick: "Why they differ", text: "Floating-point addition is not associative" },
      {
        t: "ul",
        items: [
          "**Matmul reduction order** — tensor cores, MFMA tiles, OpenCL workgroups and SIMD lanes accumulate in different orders.",
          "**Accumulation precision** — F16/BF16 storage with F32 vs F16 accumulators is the biggest lever on divergence.",
          "**Transcendental approximations** — exp (softmax), silu (swiglu) and rsqrt (norms) use different polynomial/table variants per backend.",
        ],
      },
      { t: "h2", kick: "The contract", text: "Tolerances, capabilities, single authority" },
      {
        t: "ul",
        items: [
          "Verification is a **tolerance** — \"top-1 agreement ≥ 99.x%, KL ≤ ε\" — never bit-equality.",
          "Backends and accumulation precision are advertised as node **capabilities**; F32-accumulating nodes are preferred for output-sensitive ranks.",
          "Out-of-tolerance nodes are marked unfit for sensitive ranks, not rejected outright.",
          "Discrete decisions (routing, sampling) are pinned to single authorities so continuous error can never become discrete divergence.",
        ],
      },
    ],
  },
  {
    slug: "gguf",
    category: "inference",
    title: "GGUF",
    summary: "The quantized model file format inference engine uses — and the layout that makes partial and expert slicing cheap.",
    image: { src: "/wiki/gguf.jpg", alt: "A model file with clean byte-range slices being cut out" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "**GGUF** is the single-file model format of the inference engine ecosystem: metadata (architecture, layer count, dimensions, quantization) plus the tensors as raw quantized bytes (e.g. Q4_K_M). linkcpp's planner reads the metadata to compute placements and size estimates; the serving side slices the tensor bytes to produce downloads.",
      },
      {
        t: "ul",
        items: [
          "**Stage mini-GGUFs** carry one layer window's tensors — 254 MB instead of 77.6 GB for a 122B ring stage.",
          "**Expert-shard GGUFs** carry one (layer, expert-range) slice, served by `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` with node-token auth.",
          "Both are **valid GGUF files**: the reader on the node loads them with stock tooling, no custom format.",
        ],
      },
      {
        t: "code",
        caption: "Why expert slicing is a byte copy: the expert index is the outermost dimension.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "The router (`ffn_gate_inp`) and the shared expert are **excluded** from expert shards — they belong to the backbone, which is exactly what router authority requires.",
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Token & rewards                                                     */
  /* ------------------------------------------------------------------ */
  {
    slug: "kvr",
    category: "token",
    title: "KVR",
    summary: "The network's utility token: developers spend it on inference, contributors earn it for compute.",
    image: { src: "/wiki/kvr.jpg", alt: "One token flowing both directions between developers and contributors" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "**KVR** (on-chain name \"Kvasir\", 6 decimals, Solana) is one token flowing both directions: developers **spend** KVR to run inference through the gateway, contributors **earn** KVR for the compute their nodes provide. Rewards are computed from real work — layers and experts actually served — not from participation.",
      },
      {
        t: "ul",
        items: [
          "**Spend side** — pay-per-inference through the gateway: quote → payment → inference.",
          "**Earn side** — contribution units × layer share × performance tier for compute; hourly uptime for hub/gateway roles.",
          "**Settlement** — on Solana, to each node owner's own wallet; the settlement service credits every node that touched a request.",
          "**Named for the myth** — the Mead of Poetry, brewed from Kvasir, granting wisdom to everyone who drinks: open access, and rewards to everyone who pours in.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, utility token.** KVR currently runs on Solana devnet and is a utility / contribution token — not a tradable asset, price, or investment. Nothing here is financial advice or a promise of return.",
      },
    ],
  },
  {
    slug: "contribution-units",
    category: "token",
    title: "Contribution units",
    summary: "The reward formula: units follow tokens served weighted by layer share, then scale by performance tier.",
    image: { src: "/wiki/contribution-units.jpg", alt: "Tokens flowing through layers into a growing reward meter" },
    imagePos: 1,
    blocks: [
      {
        t: "code",
        caption: "How compute rewards are computed.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "One **unit** ≈ 1k tokens served, weighted by the node's **layer share** of each inference — a node running 12 of 49 layers earns 12/49 of each inference's units. The tier multiplier then rewards measured speed, and infra roles accrue hourly uptime on top.",
      },
      { t: "h2", kick: "Worked example", text: "One inference, four nodes" },
      {
        t: "table",
        head: ["Node", "Layers", "Share", "Tier", "Effective units / 1k tokens"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Phone", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Rewards follow **real work**: a node that served nothing earns nothing, regardless of uptime (compute roles).",
          "Roles **stack** — one machine can be compute + gateway + hub, and its streams sum.",
          "Everything settles in KVR to the node's own owner wallet; the dashboard shows raw × tier = effective and a claimable balance.",
        ],
      },
    ],
  },
  {
    slug: "performance-tiers",
    category: "token",
    title: "Performance tiers",
    summary: "Measured throughput sets a multiplier: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    image: { src: "/wiki/performance-tiers.jpg", alt: "Speed gauge dividing devices into S, A, B, C tiers" },
    imagePos: 1,
    blocks: [
      {
        t: "table",
        head: ["Tier", "Measured throughput", "Multiplier"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "A node's measured decode speed sets its tier, and the tier multiplies its earned units — faster hardware earns proportionally more for the same work. The wallet's node status view shows each node's tier next to its contribution.",
      },
      {
        t: "ul",
        items: [
          "Tiers are **measured, not self-declared** — throughput comes from the node's actual serving performance and is re-measured over time.",
          "A C-tier phone still earns — ×0.7 of its layer share — which is the point: the floor is participation-wide, not datacenter-only.",
          "Tier multiplies *effective* units, so it composes with layer share and the gateway bonus rather than replacing them.",
        ],
      },
    ],
  },
  {
    slug: "staking",
    category: "token",
    title: "Staking",
    summary: "Stake KVR to earn APR interest; 100,000 KVR staked qualifies a wallet to operate hub or gateway nodes.",
    image: { src: "/wiki/staking.jpg", alt: "Locked tokens growing interest and unlocking operator roles" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "Staking locks KVR in your own wallet to earn **APR interest** and to qualify for node rewards. Running a **hub** or **gateway** node requires a stake of **100,000 KVR**; regular compute nodes join without any stake and earn for the layers they run.",
      },
      {
        t: "ul",
        items: [
          "Staking happens in the wallet's dashboard staking panel: enter an amount, **Stake**, and the position starts accruing APR plus node-reward eligibility.",
          "The 100k requirement is a **skin-in-the-game filter** for the two roles that other people's traffic depends on — entry points and the control plane.",
          "Staking is non-custodial like everything else: the position lives in your own wallet, and principal, accrued interest and node rewards are all visible in the staking panel.",
          "Devnet KVR for staking comes via distribution or swap (SOL/ETH ↔ KVR swap: coming soon); devnet SOL for fees comes from the public faucet.",
        ],
      },
    ],
  },
  {
    slug: "non-custodial-wallet",
    category: "token",
    title: "Non-custodial wallet",
    summary: "Keys live only on the user's device — web, desktop, iOS and Android — and rewards settle straight to it.",
    image: { src: "/wiki/non-custodial-wallet.jpg", alt: "A key that never leaves the owner's device" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "The Kvasir Wallet is **non-custodial by design**: the 12-word recovery phrase and keys are stored only on the user's own device, never with an operator. Rewards settle on Solana directly to each node's owner wallet — verified across four distinct owner wallets, each earning its own layer share.",
      },
      {
        t: "ul",
        items: [
          "**Platforms** — web, desktop (macOS/Windows/Linux, wallet + node in one Electron app), iOS and Android.",
          "**One phrase, every device** — the same 12-word recovery phrase restores the same account on desktop, phone and web; a local passphrase unlocks each installation.",
          "**Wallet = node identity** — the wallet signs the node's network identity, so \"who earns for this device\" is cryptographic, not an account row on someone's server.",
          "**Lose the phrase, lose the account** — non-custody cuts both ways; there is no operator who can reset it.",
        ],
      },
      {
        t: "p",
        md: "The mobile apps double as node apps: the same wallet that holds your KVR configures the phone's compute backend and node mode, stakes, and claims rewards.",
      },
    ],
  },
  {
    slug: "siws-2fa",
    category: "token",
    title: "SIWS + 2FA",
    summary: "Operator login is a wallet signature (Sign-In With Solana) over a server nonce, plus optional TOTP 2FA.",
    image: { src: "/wiki/siws-2fa.jpg", alt: "A wallet signature and a one-time code guarding a door" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "For public deployments, operator access to the hub and gateway is authenticated by **Sign-In With Solana**: the operator's wallet signs a server-issued nonce, proving ownership without any password or custodied credential. On top of that, **TOTP 2FA** and single-use backup codes protect the session — on both hub and gateway.",
      },
      {
        t: "ul",
        items: [
          "**No passwords anywhere** — the wallet key is the identity, and the nonce prevents replay; there is nothing server-side to phish or leak.",
          "**Per-wallet TOTP enrollment** is persisted in hub state, so 2FA survives restarts along with slots and bindings.",
          "**Backup codes are single-use** — each one is consumed on login, for recovery when the authenticator device is unavailable.",
          "**Scope honestly stated** — bare hub and RPC ports are designed for trusted host / LAN / VPN; SIWS + 2FA is the layer that makes *public* domains safe to expose.",
        ],
      },
    ],
  },
  {
    slug: "token-economy",
    category: "token",
    title: "The KVR economy",
    summary: "How consumer cost and node reward form one self-reinforcing loop — the virtuous cycle that lets the network grow cheaper as it grows larger.",
    image: { src: "/wiki/token-economy.jpg", alt: "A flywheel linking usage, token demand, rewards and supply into one turning loop" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "Kvasir is a **two-sided market** settled in one token. Consumers pay **KVR** per inference into the treasury; nodes earn **KVR** for the exact work they served, paid back out to their own wallets. The design goal is that these two sides don't compete — they **compound**: more supply makes the network cheaper and better, which pulls in more demand, whose payments fund richer rewards, which pulls in more supply.",
      },
      { t: "h2", kick: "The flywheel", text: "Usage and supply grow together" },
      {
        t: "p",
        md: "Because inference **must** be paid in KVR, every unit of usage is real demand for the token — utility, not speculation. That demand supports the value of the KVR that nodes earn, which keeps contributing attractive, which grows capacity, which lowers price and latency, which attracts more usage. Kvasir's sharpest advantage turns the loop tighter still: a participant can be **consumer and supplier at once** (a *prosumer*), so the two sides often grow inside the same people.",
      },
      {
        t: "callout",
        md: "**\"Free when you contribute\" is net-free, not zero-cost.** You pay for what you infer and earn for what you serve; contribute roughly as much as you consume and the two cancel. The network isn't free — *your* bill is.",
      },
      { t: "h2", kick: "Keeping it virtuous", text: "Three invariants, and the spirals they prevent" },
      {
        t: "table",
        head: ["Invariant", "Spiral it prevents"],
        rows: [
          ["Rewards funded by real revenue (emission only to bootstrap, then taper)", "Inflation erodes KVR until both sides collapse"],
          ["KVR is the mandatory medium for inference", "Token value decouples from usage into pure speculation"],
          ["Price floats between a cost floor and a below-market ceiling", "Too-low starves nodes; too-high loses users to centralized APIs"],
        ],
      },
      {
        t: "p",
        md: "Kvasir already rewards **real work** (KVR per tokens served × layer share, not mere presence) and settles non-custodially, which is the hard part of making revenue-funded rewards honest. The rest — a utilization-driven price and an emission→revenue taper — is the economic roadmap that turns \"more nodes → cheaper\" from an intuition into a rule the protocol enforces. The **Inference pricing** entry covers the price side; **Contribution units** covers how work becomes reward.",
      },
    ],
  },
  {
    slug: "inference-pricing",
    category: "token",
    title: "Inference pricing",
    summary: "What an inference costs in KVR today, why a decentralized network is structurally cheaper, and how price is meant to fall as supply grows.",
    image: { src: "/wiki/inference-pricing.jpg", alt: "A price dial bounded between a cost floor and a market ceiling, moved by network utilization" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "Access to the network is **pay-per-inference**: the gateway quotes a KVR price for your request, your wallet pays it on-chain, and only then does the hub run the model. Pricing is a small, transparent formula — a per-request floor plus a per-token rate — quoted up front and settled on **actual** token usage after generation.",
      },
      {
        t: "code",
        caption: "The settlement formula — quoted before, charged on real usage after.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Why it can be cheaper", text: "No central margin to pay for" },
      {
        t: "p",
        md: "A centralized API prices at cost **plus** a large margin and capital recovery. A decentralized network prices near its contributors' **marginal cost** — electricity and hardware amortization — plus a thin protocol fee. That structural gap exists regardless of size. Growth widens it: **expert-sharding** means more nodes each hold a smaller slice, so cheaper devices can serve, lowering the marginal cost of participation and deepening supply.",
      },
      {
        t: "callout",
        md: "**Price is governed, not a free-for-all.** Rates are a sensitive economic parameter, changed only by the genesis wallet under wallet-signature + 2FA — never by an environment variable. This keeps the token economy stable and auditable.",
      },
      { t: "h2", kick: "Where it's heading", text: "Utilization-driven price" },
      {
        t: "p",
        md: "The design direction is a price that **floats with network utilization** between a floor (kept above node marginal cost, so serving stays worthwhile) and a ceiling (kept below centralized alternatives, so it stays competitive). Idle supply nudges the price down; congestion nudges it up. That is the mechanism that finally makes **\"more shared nodes → lower price\"** true in code — the natural thermostat of the **KVR economy**.",
      },
    ],
  },
  {
    slug: "run-expert-worker",
    category: "network",
    title: "Run an expert worker",
    summary: "Turn a spare GPU, CPU or phone into an expert worker: build it, volunteer for the scarcest slice, download it, serve, and dial out over 443 to earn KVR.",
    image: { src: "/wiki/run-expert-worker.jpg", alt: "A small device downloading an expert slice and dialing into the swarm" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "An **expert worker** is a pure `(hidden, ids) → out` function — no attention, no KV cache, no sampler — that computes a slice of a MoE model's experts whenever the backbone's router selects them. You don't choose what to serve; the **coverage market** hands you the scarcest, highest-reward range clipped to your budget, so a 4 GB phone and a datacenter GPU both find a slot.",
      },
      { t: "h2", kick: "Seven steps", text: "Build → volunteer → serve → dial → earn" },
      {
        t: "code",
        caption: "The whole path — the dial script waits for the backbone with a retry loop.",
        code: `# 1. build the worker for your backend (cuda | rocm | cpu)
bash scripts/build-node-runtime.sh <backend>   # -> linkcpp-expert-worker

# 2. volunteer — the market assigns the scarcest range within your budget
POST /api/expert-volunteer {model, max_experts}     -> {layer, experts:[a,b]}

# 3. download only that slice (node-token auth)
GET  /api/proxy/models/<model>/expert-shard?layers=L:L+1&experts=a:b   # a mini-GGUF

# 4. serve it
linkcpp-expert-worker --model <slice.gguf> --serve <port> --layer L --n-embd <E>

# 5. dial out over 443 (no inbound ports)
scripts/expert-relay-dial.py --mode worker \\
  --hub wss://gate.kvasir-ai.net/api/expert-relay --session <S> --local 127.0.0.1:<port>

# 6. heartbeat so the demand map and rewards can see you
POST /api/expert-coverage`,
      },
      {
        t: "ul",
        items: [
          "**The slice is tiny.** A layer-0, 128-expert slice is **794 MB** against the 72 GB full model — the grain that lets weak devices participate. You only download the range the market assigned.",
          "**Dial-out, never dial-in.** Step 5 opens one outbound WebSocket on 443, so carrier NAT and CDN edges pass it and you expose zero inbound ports — the same path a phone uses.",
          "**The heartbeat is load-bearing.** Without `POST /api/expert-coverage` you serve nothing the demand map knows about, and nothing you do is credited.",
          "**Reward is per work.** Bridged work accrues to the hub's contribution ledger; the gateway delta-credits KVR to your **own** wallet (non-custodial). You need a wallet address to be paid.",
        ],
      },
      {
        t: "callout",
        md: "The worker speaks the same dispatch protocol as an in-datacenter GPU — `(n_used, n_tokens, cur, sel) → experts` over one long-lived stream. A partial-shard worker just sets `n_used = 1`. That uniformity is why a phone, a CPU box and a Blackwell card are interchangeable members of the same swarm.",
      },
    ],
  },
  {
    slug: "hub-operations",
    category: "network",
    title: "Operating a hub",
    summary: "Operator notes for running a hub and gateway: hot-patch without rebuilds, survive restarts, keep the catalog registered, and lock the surface down to 443.",
    image: { src: "/wiki/hub-operations.jpg", alt: "An operator console watching over a hub, a gateway and the 443 boundary" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "The hub (control plane) and gateway (public entry point) are the two long-lived services an operator keeps healthy. The hub and its RPC ports are **unauthenticated by design** — trusted host / LAN / VPN only — and all public traffic converges on the gateway's single 443 surface. These are the operating notes that keep that arrangement stable across code changes, restarts and reboots.",
      },
      { t: "h2", kick: "Deploy & hot-patch", text: "Change code without rebuilding" },
      {
        t: "ul",
        items: [
          "**Fast path:** update hub/gateway code with `docker cp <file> <container>:/app/...` + `docker restart` — no image rebuild. But **adding an environment variable can't be done this way** (it needs a container recreate); prefer a runtime-config API that persists to hub-state instead.",
          "**Compose drift:** a long-running container can diverge from its compose file (network mode, entrypoint, env). Always `docker inspect` the real config before a `docker compose up -d` recreate — if it has drifted, recreation wipes the production settings. Use cp + restart.",
          "**Diff before you patch:** `docker cp` the in-container file out and diff it against repo HEAD before replacing it, so an earlier session's hot-patch isn't silently lost.",
        ],
      },
      { t: "h2", kick: "Survive a restart", text: "State persists; loaded models don't" },
      {
        t: "ul",
        items: [
          "A hub restart **stops serving.** Slots, controllers and bindings restore from `hub-state.json`, but a loaded model is runtime-only. After a restart, read each controller's `last_load` and re-fire `POST /api/controllers/{cid}/serve` — even a large model comes back in ~1 minute thanks to the page cache.",
          "**Gateway watchdog:** probe every served model with a 1-token request every 30 s and auto-reload from `last_load` on failure (with a cooldown). **Probe *all* models, not `catalog[0]`** — the moment a healthy model from another hub sorts to the front, a first-only probe misses a big model going down (a real bug, since fixed).",
          "**Catalog TTL:** `POST /api/pay/hub/register` has a 90 s TTL, so keep registration alive with a ~60 s heartbeat loop, made durable across reboots with an `@reboot` cron or a systemd unit.",
        ],
      },
      { t: "h2", kick: "Lock it down", text: "Everything public goes through 443" },
      {
        t: "ul",
        items: [
          "The hub (:19000) and RPC ports assume a trusted network; the only thing that should face the internet is the gateway on 443 (including its WebSocket relay passthrough).",
          "If a hub must sit on a public IP, firewall it to trusted IPs — but Docker's published ports are **DNAT'd before the INPUT chain**, so a rule on `dport` won't match. Filter in the `DOCKER-USER` chain using conntrack's original destination port (`--ctorigdstport`) instead, and persist the rules with a systemd oneshot ordered `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Settlement & footguns", text: "Pull, not push — and one shell trap" },
      {
        t: "ul",
        items: [
          "**Settlement is pull, not push:** the hub accumulates contributions; the gateway polls `GET /api/contributions` and delta-credits KVR. If a hub restart resets its counters, the gateway re-baselines so nothing is double-paid. The expert-work rate is set by `LINKCPP_EXPERT_UNITS_PER_MB`.",
          "**The `pkill` footgun:** `ssh host 'pkill -f X; ...'` matches its *own* command line and kills itself. Use a character class in the pattern (`X[x]`), and never put the spawn and the pkill in the same remote command.",
        ],
      },
    ],
  },
  {
    slug: "hub-wan-interconnect",
    category: "network",
    title: "Hub WAN interconnect (200G optics)",
    summary: "How hubs link at 200 Gb/s across a room, a campus, or a city: which optic at which distance, what plugs where, and what it takes to actually hit line rate.",
    image: { src: "/wiki/hub-wan-interconnect.jpg", alt: "Two hubs joined by fiber, with the optic module doing the work at each end" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "When two hubs both have public routes, the expert-dispatch data plane should be a **direct link** — the 443 relay is for NAT'd edges. This entry is the concrete recipe for making that direct link 200 Gb/s-class with catalog parts. One rule organizes everything: **the fiber is speed-neutral glass; the speed lives in the pluggable at each end.**",
      },
      { t: "h2", kick: "Step 1 · pick by distance", text: "The reach ladder" },
      {
        t: "table",
        head: ["distance", "part", "plugs into"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC, no switch"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / switch"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "NIC or switch QSFP56 cage"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "NIC or switch QSFP56 cage"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "switch/router QSFP-DD cage (not the NIC)"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "your switch hands off to the carrier"],
        ],
      },
      { t: "h2", kick: "Step 2 · what plugs where", text: "NIC side vs switch side" },
      {
        t: "ul",
        items: [
          "**NIC side** — ConnectX-6/7-class cards expose QSFP56 cages; DAC/AOC/FR4/LR4/ER4 all seat directly in the NIC. A GB10-class hub already has two 200 GbE QSFP ports on board, so a two-hub link needs exactly one cable and zero new hardware.",
          "**Switch side** — coherent ZR+ optics are QSFP-DD form factor and belong in a switch or router; the hub's NIC then joins that switch at 200G over a short DAC. Use this tier when the far hub is tens of kilometers away.",
          "**The fiber itself** — standard single-mode (G.652) duplex LC pairs, leased as dark fiber per strand. The same glass carries 100G today and 400G later; upgrades are a module swap, never civil works.",
          "**Beyond ~120 km** — you stop buying parts and start leasing a wavelength from a carrier; the demarcation is an Ethernet handoff on your switch.",
        ],
      },
      {
        t: "code",
        caption: "Three reference builds, cheapest first.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Step 3 · actually hitting 200G", text: "Line rate is a configuration, not a purchase" },
      {
        t: "ul",
        items: [
          "Use **RDMA (RoCE)** for the dispatch stream where available — GB10-class hosts feed the NIC through split PCIe links, and measured full speed (~185–190 Gb/s) shows up under RoCE with a correctly mapped topology; a mis-mapped path caps near half rate and untuned plain TCP lands far lower.",
          "Enable **jumbo frames (MTU 9000)** end-to-end and keep `TCP_NODELAY` on the dispatch sockets (the hub already sets it).",
          "Expect to *verify*, not assume: run a perftest between hubs after every physical change — the difference between 95 and 190 Gb/s is invisible until measured.",
          "Keep the **443 relay as the fallback path** — the dial policy is direct-first for public peers, relay for NAT. The relay's job is reach, the direct link's job is speed.",
        ],
      },
      {
        t: "p",
        md: "Why this matters to the architecture: decode latency is bounded by round-trip time (~5 µs/km in fiber — physics, unaffected by bandwidth), so a fat pipe buys **prefill speed, batched-dispatch throughput, and near-instant expert-slice distribution**, not lower per-token latency. That is exactly the hub-tier role in the two-tier design: capacity in the fat-pipe tier, reach in the relay tier.",
      },
    ],
  },
  {
    slug: "load-adaptive-scaling",
    category: "network",
    title: "Load-adaptive scaling",
    summary: "Kvasir's MoE serving path grows and shrinks with traffic: the coordinator re-engages proven workers under saturation, and the hub recruits idle nodes by raising expert demand — all pull-based, so NAT'd devices join too.",
    image: { src: "/wiki/load-adaptive-scaling.jpg", alt: "A coordinator and hub cooperating to grow a worker pool as load rises" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "Kvasir's MoE serving path scales elastically with load, in two cooperating layers. When it's quiet the coordinator serves everything locally for the fastest path per token; when it saturates, the two layers below grow the swarm — and shrink it again when the surge passes.",
      },
      { t: "h2", kick: "Layer 1", text: "Coordinator-side: load-adaptive dispatch" },
      {
        t: "p",
        md: "The backbone coordinator (a `linkcpp-server` running the full model) serves routed experts either on its own GPU (fast, local) or by dispatching them to remote workers. A background thread decides which, every few seconds:",
      },
      {
        t: "ul",
        items: [
          "It polls its **own** inference slots. When `busy >= saturation threshold` (default 2) the coordinator is under load.",
          "Under load, if a **proven** worker is live — one whose `last_serve_ms > 0`, i.e. it has actually computed experts before — the coordinator keeps dispatching to it past the normal idle timeout, favouring aggregate throughput over per-token latency.",
          "A worker that connected but never served (a phone that dialed the relay yet never computed) is **not** recruited under load, because dispatching to it would replace the fast local path with a slow fallback. New workers still get a first attempt through a short grace window.",
          "The self-query is time-bounded so a stalled poll can never block dispatch.",
        ],
      },
      { t: "h2", kick: "Layer 2", text: "Hub-side: load-adaptive recruitment" },
      {
        t: "p",
        md: "The control hub watches every MoE coordinator and grows the worker pool when needed:",
      },
      {
        t: "ul",
        items: [
          "A background loop polls each coordinator's slots and records saturation per model.",
          "While a model is saturated, its **effective expert-replica target** is raised (base + boost). The coverage market then reads already-covered experts as scarce again, and a model with **no** live workers is seeded from its GGUF metadata (expert count) so demand is visible even from zero.",
          "Idle nodes poll the demand market (`/api/expert-volunteer`) and are handed a `(layer, expert-range)` slice to serve. They download the slice, dial the relay, and register coverage; the hub auto-wires them to the coordinator's dispatch map.",
          "When load drains, the target falls back and demand disappears, so the extra workers are no longer dispatched and age out.",
        ],
      },
      {
        t: "callout",
        md: "The design is **pull-based**: nodes ask for work rather than being pushed, so a worker behind NAT participates with no inbound connectivity. A node recruited in Layer 2 that begins serving becomes a **proven** worker that Layer 1 then keeps engaged under load — the two layers compose into one elastic loop.",
      },
      {
        t: "p",
        md: "**Observability:** `GET /api/moe/recruitment` reports per-model busy/saturation and the base vs effective target; `/api/expert-demand` carries a `recruiting` flag.",
      },
    ],
  },
];

export function wikiEntryBySlug(slug: string): WikiEntry | undefined {
  return WIKI_ENTRIES.find((e) => e.slug === slug);
}
