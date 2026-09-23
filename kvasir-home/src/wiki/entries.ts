/* ==========================================================================
   Kvasir wiki — knowledge-base entries for /wiki.
   Reference entries on every concept in the network, grounded in the actual
   code (the p4 engine and its staged llama.cpp adapter, p4bridge, the
   settlement gateway), homepage-brief.md and the tech-blog posts.

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
  {
    slug: "node-relay",
    category: "network",
    title: "Node relay",
    summary: "A public address held on behalf of a machine that has none, so a node behind NAT can be reached without opening a single port.",
    blocks: [
      {
        t: "p",
        md: "A **node relay** gives a contributor's machine an address the network can dial. p4 delivers work by opening a connection *to* a node, and a home machine behind network address translation has no address to open a connection to. The relay holds one publicly, the node keeps a single outbound connection to the relay, and work dialled at the public address travels down the connection the node already has.",
      },
      {
        t: "p",
        md: "Neither end of p4 learns the relay is there. The caller sees an ordinary address; the node's agent still binds `127.0.0.1` and listens on nothing else.",
      },
      { t: "h2", text: "Why a tunnel and not a forwarded port" },
      {
        t: "p",
        md: "p4 carries no authentication — any host that can reach an agent's port may send `NODE_LOAD`, `NODE_UNLOAD` or `INSPECT`. Forwarding a port on a home router into that would expose the machine to anyone who finds it. Behind a relay the node listens on nothing and proves an operator wallet before its connection carries anything, using the same signature scheme the settlement gateway uses, so reachability and authentication are solved by the same mechanism.",
      },
      { t: "h2", text: "What it does not do" },
      {
        t: "ul",
        items: [
          "**It does not read the traffic.** Payloads pass through byte for byte and are never parsed, so the relay cannot distinguish one command from another — and must not, since understanding the traffic would make it capable of changing it.",
          "**It does not schedule.** Placement stays with the operator's plan; to the relay a node is an address and nothing more.",
          "**It is not evidence of work.** Bytes carried through a relay say nothing about inference performed, and are never counted toward contribution.",
        ],
      },
      { t: "h2", text: "Related" },
      {
        t: "p",
        md: "See also **p4 agent**, the process a contributor's machine runs, and **node operator**, the wallet a relay authenticates before granting an address.",
      },
    ],
  },
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
        md: "**Kvasir** is a decentralized AI-inference network: large open models are split across shared hardware with the **p4** engine, so no single node has to hold the whole model. Anyone can contribute a GPU, CPU, NPU — even a phone — and earn **KVR** for the layers or experts their device actually serves. Developers reach the network through OpenAI/Anthropic-compatible gateways and pay per inference.",
      },
      {
        t: "ul",
        items: [
          "**Source-available engine** — p4 is licensed under the Business Source License 1.1 (non-monetized internal use is permitted; hosted or revenue-generating use requires a commercial license); the llama.cpp data plane underneath stays close to upstream and inspectable.",
          "**Self-custody wallet** — keys never leave the user's device, and rewards are paid to each node owner's own Solana wallet. On devnet, staked KVR and prepaid credits are held by the gateway's treasury and tracked in its ledger until an on-chain staking program ships.",
          "**Proven on real hardware** — a 122B model has run end to end across 3 physical machines on our test fleet, with per-node contribution credited end-to-end.",
          "**Named after the Norse myth** — Kvasir, the wisest being, born from the pooled essence of every god and owned by none.",
        ],
      },
      { t: "h2", kick: "One request, many devices", text: "How an inference flows" },
      {
        t: "code",
        caption: "Every hop is ordinary HTTP/TCP; the model itself is what's distributed.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Roles **stack**: one machine can be a compute node, a gateway host and a bridge host at once, and its rewards sum. The network's job is to make the aggregate look like one machine — one endpoint in front, thousands of imperfect devices behind.",
      },
      {
        t: "p",
        md: "Today the network runs on **Solana devnet**; KVR is a utility / contribution token, not a tradable asset or investment, and nothing on this page is financial advice.",
      },
    ],
  },
  {
    slug: "architecture",
    category: "network",
    title: "Kvasir architecture",
    summary: "One map of the whole system: wallets, the gateway that takes payment, the bridge that fronts the engine, and the p4 network that runs the model.",
    image: { src: "/wiki/kvasir-architecture.svg", alt: "Kvasir end-to-end architecture: wallets, Cloudflare Tunnel, gateway, p4 bridge, p4 agents and stage servers, the relay, and P4 Studio" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "Kvasir is four layers with one seam each. **Wallets** hold the keys. The **gateway** takes the payment and keeps the ledger. The **bridge** puts an HTTP face on the inference engine. The **p4 network** actually runs the model. Everything below is a consequence of where those seams fall — and the diagram marks what runs today against what is still a design.",
      },
      { t: "h2", kick: "Wallets", text: "Keys never leave the device" },
      {
        t: "p",
        md: "iOS (Swift), Android (Kotlin) and desktop (React + Electron) are separate builds of the same wallet, and the desktop build is also what the gateway serves at `/` as a browser wallet — a full wallet with in-page signing, not a read-only console. Rewards are paid to each owner's own Solana address; the gateway never holds a user key.",
      },
      { t: "h2", kick: "Gateway", text: "One process, two surfaces" },
      {
        t: "p",
        md: "`solana/staking-service` is both the **API gateway** (an OpenAI-compatible `/v1/chat/completions`, plus the pay-per-request flow at `/api/pay/quote` → `/api/inference`) and the **settlement gateway** (staking, the node registry, credit accounts, contribution credit). They are one process because they share one ledger: a request is only served after its KVR transfer is verified on-chain, and the same ledger credits the nodes that served it.",
      },
      {
        t: "callout",
        md: "**The payment settles before the inference runs.** If the bridge then fails, the gateway refunds the payer from the treasury and returns a 502 rather than charging for nothing. There is no mock model and no placeholder catalogue behind it: a model the app offers is one a bridge is serving, or the list is empty.",
      },
      { t: "h2", kick: "Bridge", text: "The HTTP face of the engine" },
      {
        t: "p",
        md: "The bridge (`p4bridge`) is an **OUTER** in p4 terms: it installs a session across the stages, submits to the head, and gathers the token stream. To the gateway it is a small, fixed contract — which models are loaded, who contributed how much, and completions.",
      },
      {
        t: "table",
        head: ["Route", "What it answers"],
        rows: [
          ["`/api/controllers`", "which models are loaded, and every stage's state"],
          ["`/api/runtime`", "the operator wallet and the machines behind it"],
          ["`/api/contributions`", "per-node rows, units, requests, throughput"],
          ["`/c/<model>/v1/chat/completions`", "inference"],
        ],
      },
      {
        t: "p",
        md: "Two jobs p4 deliberately leaves to the bridge: **the chat template** (p4 hands the stage server an opaque prompt and applies none, so an instruct model would continue your text instead of answering it) and **the reasoning block** (returned as `reasoning_content`, separate from `content`, so a thinking pass cannot silently eat the token budget and bill the payer for a blank reply).",
      },
      {
        t: "callout",
        md: "**The bridge is never published.** Its only authentication is a shared service token, and anything that reaches it can run the ring. It binds loopback; the tunnel is the door.",
      },
      { t: "h2", kick: "p4 network", text: "Agents own nodes, stage servers hold layers" },
      {
        t: "p",
        md: "An **agent** owns a host's nodes; a **stage server** is one process holding a slice of the model's layers. A stage hands its result to the next by asking its own agent to dial that stage's agent **at the address that agent advertises** — so the advertised address has to be reachable from the other hosts, and should be the fastest network they share. On the MI250 rack that is the InfiniBand link, not the office LAN and never loopback.",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` and `p4_staged_server` are one release.** An agent built from a newer tree fails at READY with a missing HELLO capability — after loading the whole model.",
          "**Placement is an operator artifact.** Which layers sit on which GPU, at which load generation, comes from a placement plan; the bridge answers `409` to anyone asking it to serve, and the gateway's watchdog says so once and stops asking.",
          "**A pipeline needs at least two stages.** The session command refuses a one-stage pipeline.",
        ],
      },
      { t: "h2", kick: "Relay", text: "A dialable address for a laptop" },
      {
        t: "p",
        md: "Edge nodes — a desktop app, a phone — have no address anyone can dial. The **relay** gives them one: the node connects out, proves the wallet keypair over an ed25519 challenge, and is then reachable through the relay. The relay is the authentication boundary and never parses payloads. The desktop installer ships the p4 agent alongside the app, so joining is not a second install.",
      },
      { t: "h2", kick: "Settlement", text: "Credit follows participation" },
      {
        t: "p",
        md: "Each stage reports the token rows it ran. The bridge accumulates them per node, and the gateway polls `/api/contributions` every 30 seconds and credits the wallet the bridge names, as `rows / 1000` units scaled by the node's performance tier. **In a pipeline every stage sees the same rows**, so a four-stage ring pays its four stages equally no matter how many layers each holds — credit follows participation, not weight share. Expert sharding, where nodes hold different fractions of a layer, is the case that will need this revisited.",
      },
      { t: "h2", kick: "P4 Studio", text: "What the diagram marks as proposed" },
      {
        t: "p",
        md: "**P4 Studio** is p4's own operator console. The per-request observability feed it wants from the agents is a proposal upstream, not something running here — the diagram draws it dashed for that reason, alongside expert shards served from edge nodes, which is designed and not yet running.",
      },
    ],
  },
  {
    slug: "bridge",
    category: "network",
    title: "Bridge",
    summary: "The HTTP face of the inference engine: what is loaded, who contributed, completions — and the market a contributing device joins.",
    image: { src: "/wiki/bridge.jpg", alt: "A bridge fronting a ring of stage servers" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "The **bridge** is the one thing the settlement gateway talks to for inference. It is an **OUTER** in p4 terms: it installs a session across the model's stages, submits a request to the head stage, gathers the token stream, and reports what each node contributed. It owns no placement, no scheduling and no state beyond a catalog of what is loaded — deliberately small, because everything it does not decide is something that cannot drift.",
      },
      { t: "h2", kick: "The contract", text: "Four routes, one token" },
      {
        t: "table",
        head: ["Route", "What it answers"],
        rows: [
          ["`/api/controllers`", "which models are loaded, and every stage's state"],
          ["`/api/runtime`", "the operator wallet and the machines behind it"],
          ["`/api/contributions`", "per-node rows, units, requests, throughput"],
          ["`/c/<model>/v1/chat/completions`", "inference"],
        ],
      },
      {
        t: "p",
        md: "Every route except `/api/health` requires a shared service token, sent as `X-Kvasir-Service-Token`. That token is the **only** thing standing between the open internet and free use of the ring, which is why the bridge binds loopback and is reached through a tunnel rather than published.",
      },
      { t: "h2", kick: "What p4 leaves to it", text: "Two jobs the engine will not do" },
      {
        t: "ul",
        items: [
          "**The chat template.** p4 hands the stage server an opaque prompt and applies no turn format of its own. The bridge renders the model's — read from the GGUF and named in the catalog as `prompt_format`. Skip it and an instruct model continues your text instead of answering it, never emits its end-of-turn token, and runs to the token limit every time.",
          "**The reasoning block.** A reasoning model opens its reply by thinking. The bridge returns that as `reasoning_content`, separate from `content`, and honours `enable_thinking: false` by closing the block in the prompt — otherwise a long thinking pass can consume the whole budget and hand the caller an empty answer it has already paid for.",
        ],
      },
      { t: "h2", kick: "Participation", text: "The half that was missing" },
      {
        t: "p",
        md: "A device that wants to contribute does not have a service token and should not be given one. It proves a **wallet** instead: the bridge issues a single-use nonce, the device signs it, and an ed25519 check returns a bearer token scoped to participation and nothing else. From there it asks what is under-covered, claims a window, and opens a relay — because a phone behind carrier NAT cannot be dialled, so both ends dial the bridge and it splices them.",
      },
      {
        t: "table",
        head: ["Route", "What it answers"],
        rows: [
          ["`/api/auth/challenge` → `/api/auth/node-token`", "a wallet signature becomes a 30-day participation token"],
          ["`/api/expert-demand`", "which expert windows are short of replicas"],
          ["`/api/expert-volunteer`", "the scarcest window this device should take"],
          ["`/api/expert-coverage`", "what it has taken — and this is what wires its relay session"],
          ["`ws /api/expert-relay`, `ws /api/ring-relay`", "a WebSocket spliced to a TCP endpoint"],
        ],
      },
      {
        t: "callout",
        md: "**Order matters, and silently.** The coverage POST is what creates the relay target the device then dials. Reverse them and the socket still connects and still carries bytes — it simply credits nobody. Two more traps worth naming: the ring's one-byte role preamble (`P` predecessor, `N` successor) is payload to the relay and must cross untouched; and participation eligibility is **not** operator eligibility — the retired control plane gated this on holding a minimum balance, which refused every phone that ever asked.",
      },
      { t: "h2", kick: "Placement is not its job", text: "Why it answers 409" },
      {
        t: "p",
        md: "Asking the bridge to serve a model returns **409**. Which layers sit on which GPU, at which load generation, comes from a placement plan an operator wrote and loaded; there is no remote reload to perform. The gateway's ring watchdog learns this once and stops asking rather than retrying something that cannot work.",
      },
      {
        t: "callout",
        md: "**Contribution counters live in memory.** A bridge restart loses whatever the gateway had not yet polled — it polls every 30 seconds — and the gateway rebaselines rather than double-counting when a counter goes backwards. A node whose owner the bridge does not know is skipped **silently**, so an unset operator wallet reads as \"these machines earned nothing\".",
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
        md: "The **gateway** is where developers meet the network. Every controller exposes OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/models`) and Anthropic-compatible ones (`/anthropic/v1/messages`, `/anthropic/v1/models`), all backed by the same loaded model — an existing client works by changing only the base URL and key.",
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
        md: "Usage settles in KVR through a three-step flow — **quote → payment → inference** — so a request is priced before it runs and the nodes that served it are credited after. The gateway also aggregates a **live model catalog** from every reachable bridge, so `/v1/models` reflects what the network can actually serve right now.",
      },
      {
        t: "ul",
        items: [
          "Gateway hosts earn an **hourly uptime reward** for keeping the entry point online, plus a **×1.5 bonus** on every inference they help serve.",
          "The gateway role is **assigned by the network, not claimed**: a node cannot set its own gateway or bridge flag, and uptime is credited only while the gateway can see it answering.",
          "Public deployments protect operator access with **SIWS + 2FA**; a bare bridge is designed for trusted host / LAN / VPN only.",
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
          "Nodes register under their owner's wallet; rewards are paid to that wallet. Four separate owner wallets, each earning its layer share, have been verified end to end on a single operator's test fleet.",
          "Capability data (backend, accumulation precision, resource budgets) decides what the planner may place on a node — and, in the swarm, which ranks it may serve.",
          "A node that can't provide resource monitoring is excluded from adaptive loading rather than trusted blindly.",
        ],
      },
    ],
  },

  /* ------------------------------------------------------------------ */
  /* Inference & engine                                                  */
  /* ------------------------------------------------------------------ */
  {
    slug: "p4",
    category: "inference",
    title: "p4",
    summary: "The engine behind Kvasir: an event-addressed protocol where agents own nodes, stage servers hold layers, and placement is something an operator states rather than something the network guesses.",
    image: { src: "/wiki/p4.jpg", alt: "Agents and stage servers carrying one model between them" },
    imagePos: 2,
    blocks: [
      {
        t: "p",
        md: "**p4** runs one model across several machines by cutting it into **stages** — contiguous slices of its layers — and giving each stage its own process. An **agent** owns the nodes on a host: it spawns stage servers, routes events between them, and answers for their lifecycle. There is no scheduler deciding where things go; an operator writes a placement plan, loads it, and the network then serves exactly that.",
      },
      {
        t: "code",
        caption: "The request path through a p4 deployment.",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "Addressing", text: "A stage dials the next stage's agent" },
      {
        t: "p",
        md: "When a stage finishes its layers it hands the result to the next stage by asking its own agent to open a connection to **that stage's agent, at the address that agent advertises**. The advertised address is therefore not cosmetic: it has to be reachable from every other host in the ring, and it should name the fastest network they share. Advertise loopback and a two-host ring quietly dials itself.",
      },
      { t: "h2", kick: "Lifecycle", text: "One number ties a load together" },
      {
        t: "ul",
        items: [
          "**The load generation is chosen by whoever loads** and is compared for exact equality on every session, inference, settlement and unload. It is recorded nowhere on the machines, so the loader writes it to disk *before* the first command leaves — without it a loaded model cannot even be taken down.",
          "**A node's generation and the load generation are the same number.** The adapter checks a release receipt's source generation against the load it belongs to and stops the node when they differ, so a ring loaded with two different numbers serves one request and then loses its head.",
          "**An operational journal is required** before a model will load at all: it is the admission record that makes a load replay-safe, not a debugging aid.",
        ],
      },
      { t: "h2", kick: "What it does not do", text: "Deliberate omissions" },
      {
        t: "p",
        md: "p4 applies **no chat template** — it forwards an opaque prompt and expects the caller to have rendered the model's turn format. It makes **no placement decisions**. And it carries no notion of who should be paid: stages report the token rows they ran, and settlement is somebody else's contract. Each of these is a seam Kvasir fills in the [bridge](/wiki/bridge), which keeps the engine narrow enough to track upstream.",
      },
      {
        t: "callout",
        md: "**The agent and the native stage server are one release.** An agent built from a newer tree fails at READY with a missing capability in the stage server's HELLO — after loading the entire model. Build both from the same checkout.",
      },
      { t: "h2", kick: "Operating it", text: "Two costs that do not announce themselves" },
      {
        t: "ul",
        items: [
          "**A ring gets slower the longer it runs.** Every journal record asks whether it fits, and the answer is computed by listing the journal directory and stat-ing every entry; nothing is ever deleted from it, so each write pays for every record the agent has written since it started. Measured on two stages: 22.90 tok/s falling to 12.58 over four consecutive runs. Agent-to-agent hops write records a single-agent ring never writes, so a ring split across agents decays several times faster.",
          "**The stage servers spin while the GPU works.** All the arithmetic runs on the accelerator, but each stage starts a CPU thread pool the size of the machine's cores and the OpenMP runtime spins those threads while idle — two stages peg every logical core, and the agent's per-event work, which paces the ring, runs on what is left. `OMP_WAIT_POLICY=PASSIVE` took agent CPU per token from 116 ms to 8 ms.",
          "**One agent per host, where the shape allows it.** A hop inside an agent is a function call; a hop between two agents is a journalled, durable event. Four stages under one agent served 7.37 tok/s; the same four split across two agents on the same machine served 2.80.",
        ],
      },
    ],
  },
  {
    slug: "in-flight-ring",
    category: "inference",
    title: "In-flight ring",
    summary: "A pipeline that never drains: several requests occupy different stages at the same time, so no stage waits for the one in front to finish.",
    image: { src: "/wiki/in-flight-ring.jpg", alt: "Stages of one model with several requests moving through them at once" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "Kvasir serves a model as a **pipeline of stages**, each holding a contiguous slice of its layers. A stage runs its layers and passes the boundary — a hidden state, not weights — to the next. No stage holds the whole model, and nothing sits in the middle of the data path: the bridge submits to the head and reads from the tail, while the stages hand results to each other through their own agents.",
      },
      { t: "h2", kick: "The in-flight part", text: "Why a drained pipeline wastes most of the machine" },
      {
        t: "p",
        md: "If a pipeline finishes one request before admitting the next, every stage but one is idle at any moment — a four-stage ring runs at a quarter of its hardware. The **in-flight** design keeps several requests moving at once: while stage 3 decodes one request, stage 0 is already prefilling another. Stages report how long they held a batch and how long they had nothing to submit, so a ring that is starved looks different from a ring that is saturated.",
      },
      {
        t: "code",
        caption: "Four stages, three requests, one instant in time.",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "Membership", text: "What a batch is, exactly" },
      {
        t: "p",
        md: "Rows from different requests are packed into one physical batch, and that exact membership is forwarded to every downstream stage rather than re-decided per hop. It is what lets a prefill and several decodes share a single pass, and it is why the size of a batch is a property of the load: the plan states the row and micro-batch widths up front, and those widths set the largest result a stage can ever return.",
      },
      {
        t: "callout",
        md: "**A pipeline needs at least two stages.** A one-stage pipeline is refused outright — the head and the tail are different roles, and a single node collapsing both is a different engine, not a smaller ring.",
      },
      {
        t: "p",
        md: "The ring is the **latency** path, and its granularity is a layer. Expert sharding removes that floor by cutting inside a layer, and plugs into the same serving fabric.",
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
        md: "A **Mixture-of-Experts** model replaces each layer's single FFN with a bank of independent expert FFNs plus a **router** that picks a few per token. Step-3.7-Flash, the 428B MoE serving today, has 288 experts per layer with top-8 routing. Qwen3.5-122B-A10B is the worked example below, because it is the one whose numbers were measured end to end:",
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
      {
        t: "callout",
        md: "**Engine status, 2026-09-21.** Expert-grain sharding was built and demonstrated on Kvasir's previous engine, and the results below are from that work. Carrying it onto [p4](/wiki/p4) is partly done. Live: a device proves a wallet, is told which expert window is scarcest, claims it, opens a relay, and is credited for the bytes it carries. Not yet: the endpoint that hands it the weights for that window, and — more fundamentally — any path in the p4 stage adapter for dispatching expert work to a remote device. The market and the transport run; the data path and the engine hook do not. Where a detail below names a tool or a route, it is the one that ran on the previous engine.",
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
        t: "callout",
        md: "**Engine status, 2026-09-21.** Expert-grain sharding was built and demonstrated on Kvasir's previous engine, and the results below are from that work. Carrying it onto [p4](/wiki/p4) is partly done. Live: a device proves a wallet, is told which expert window is scarcest, claims it, opens a relay, and is credited for the bytes it carries. Not yet: the endpoint that hands it the weights for that window, and — more fundamentally — any path in the p4 stage adapter for dispatching expert work to a remote device. The market and the transport run; the data path and the engine hook do not. Where a detail below names a tool or a route, it is the one that ran on the previous engine.",
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
        md: "**GGUF** is the single-file model format of the inference engine ecosystem: metadata (architecture, layer count, dimensions, quantization) plus the tensors as raw quantized bytes (e.g. Q4_K_M). a placement plan is written against that metadata — layer ranges, device assignment and size estimates; the serving side slices the tensor bytes to produce downloads.",
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
          "**Earn side** — contribution units × layer share × performance tier for compute; hourly uptime for bridge/gateway roles.",
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
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "Roles **stack** — one machine can be compute + gateway + bridge, and its streams sum.",
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
    summary: "Locking KVR in the vault. It no longer gates operator roles, and nothing requires it.",
    image: { src: "/wiki/staking.jpg", alt: "Locked tokens unlocking operator roles" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "Staking locks KVR in the vault from the wallet's staking panel. It used to be the gate on operator roles — a bridge or gateway required a stake of 100,000 KVR — and **that requirement is gone**. No stake is needed to run any node, and a wallet holding no KVR at all can register one and earn. Those two infra roles are assigned by the network instead, which is a stronger control than a price: the old check read a wallet balance once at registration, never locked it and never looked again, so the same 100,000 KVR could register any number of nodes and then be moved away.",
      },
      {
        t: "ul",
        items: [
          "Staking happens in the wallet's dashboard staking panel: enter an amount, **Stake**, and the position is held in the vault until you unstake it.",
          "It is not a requirement for anything. Node rewards come from the work a node actually does, plus verified uptime for infra roles — never from holding a balance.",
          "The devnet staking rate is currently **0%**, so a position earns nothing on its own. Treat the panel as a mechanism that exists, not as a way to earn.",
          "On devnet, staked KVR is held in the staking vault; the staked amount and node rewards are visible in the staking panel.",
          "Devnet KVR for staking comes from the distribution faucet; devnet SOL for fees comes from the public faucet.",
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
        md: "The Kvasir Wallet is **non-custodial by design**: the 12-word recovery phrase and keys are stored only on the user's own device, never with an operator. Rewards settle on Solana directly to each node's owner wallet — verified on a test fleet across four distinct owner wallets, each earning its own layer share. Staking works differently on devnet: staked KVR is held in the gateway's treasury and tracked in its ledger until an on-chain staking program ships.",
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
        md: "For public deployments, operator access to the gateway is authenticated by **Sign-In With Solana**: the operator's wallet signs a server-issued nonce, proving ownership without any password or custodied credential. On top of that, **TOTP 2FA** and single-use backup codes protect the session.",
      },
      {
        t: "ul",
        items: [
          "**No passwords anywhere** — the wallet key is the identity, and the nonce prevents replay; there is nothing server-side to phish or leak.",
          "**Per-wallet TOTP enrollment** is persisted in the gateway ledger, so 2FA survives a restart.",
          "**Backup codes are single-use** — each one is consumed on login, for recovery when the authenticator device is unavailable.",
          "**Scope honestly stated** — the bridge and the engine ports assume a trusted host / LAN / VPN; SIWS + 2FA is the layer that makes *public* domains safe to expose.",
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
        md: "Because inference **must** be paid in KVR, the token is tied to real usage — utility, not speculation. Usage funds the KVR that nodes earn, which keeps contributing attractive, which grows capacity, which lowers price and latency, which attracts more usage. Kvasir's sharpest advantage turns the loop tighter still: a participant can be **consumer and supplier at once** (a *prosumer*), so the two sides often grow inside the same people.",
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
        md: "Kvasir already rewards **real work** (KVR per tokens served × layer share, not mere presence) and pays each node's own wallet, which is the hard part of making revenue-funded rewards honest. The rest — a utilization-driven price and an emission→revenue taper — is the economic roadmap that turns \"more nodes → cheaper\" from an intuition into a rule the protocol enforces. The **Inference pricing** entry covers the price side; **Contribution units** covers how work becomes reward.",
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
        md: "Access to the network is **pay-per-inference**: the gateway quotes a KVR price for your request, your wallet pays it on-chain, and only then does the ring run the model. Pricing is a small, transparent formula — a per-request floor plus a per-token rate — quoted up front and settled on **actual** token usage after generation.",
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
      {
        t: "callout",
        md: "**Engine status, 2026-09-21.** Expert-grain sharding was built and demonstrated on Kvasir's previous engine, and the results below are from that work. Carrying it onto [p4](/wiki/p4) is partly done. Live: a device proves a wallet, is told which expert window is scarcest, claims it, opens a relay, and is credited for the bytes it carries. Not yet: the endpoint that hands it the weights for that window, and — more fundamentally — any path in the p4 stage adapter for dispatching expert work to a remote device. The market and the transport run; the data path and the engine hook do not. Where a detail below names a tool or a route, it is the one that ran on the previous engine.",
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
          "**Reward is per work.** Bridged work accrues to the bridge's contribution ledger; the gateway delta-credits KVR to your **own** wallet. You need a wallet address to be paid.",
        ],
      },
      {
        t: "callout",
        md: "The worker speaks the same dispatch protocol as an in-datacenter GPU — `(n_used, n_tokens, cur, sel) → experts` over one long-lived stream. A partial-shard worker just sets `n_used = 1`. That uniformity is why a phone, a CPU box and a Blackwell card are interchangeable members of the same swarm.",
      },
    ],
  },
  {
    slug: "bridge-operations",
    category: "network",
    title: "Operating a bridge",
    summary: "Operator notes for running the bridge and the ring behind it: load a plan, survive a restart, keep contribution flowing, and keep the engine off the internet.",
    image: { src: "/wiki/bridge-operations.jpg", alt: "An operator console watching over a bridge, a gateway and the public boundary" },
    imagePos: 0,
    blocks: [
      {
        t: "p",
        md: "The **gateway** (public entry point) and the **bridge** (engine face) are the two long-lived services an operator keeps healthy, with the p4 agents and their stage servers behind them. The engine speaks no authentication at all — it assumes the machines that can reach each other are meant to — so everything public converges on the gateway, and the bridge is reached through a tunnel rather than published.",
      },
      { t: "h2", kick: "Loading", text: "A plan, and the number it is loaded under" },
      {
        t: "ul",
        items: [
          "**Placement is a plan you write**, not a request you make: the bridge answers `409` to anyone asking it to serve. Generate the plan, dry-run it, then load with `--confirm`.",
          "**The load generation is written to disk before the first command leaves.** It is chosen by the loader, checked for exact equality on every session and on unload, and recorded nowhere on the machines — lose it and a loaded model cannot even be taken down.",
          "**The node generation is that same number.** Load a ring with two different ones and it serves exactly one request before the head stops; the next session hangs part-loaded. Use a fresh value per load, or a registration left behind by a failed attempt collides with it.",
          "**The agent refuses to load without its operational journal.** That is the admission record a replay-safe load needs, not a debug flag.",
        ],
      },
      { t: "h2", kick: "Survive a restart", text: "What comes back and what does not" },
      {
        t: "ul",
        items: [
          "**An agent restart drops its nodes.** Stage servers are runtime-only; the model has to be loaded again from the plan. That is the recovery procedure, not a failure of one.",
          "**The gateway will not reload it for you.** Its ring watchdog notices a model that stopped serving, learns from the bridge's `409` that placement is external, says so once, and stops asking.",
          "**Contribution counters live in the bridge's memory.** The gateway polls every 30 s and delta-credits; a restart loses only what had not been polled, and the gateway re-baselines rather than double-paying when a counter goes backwards.",
        ],
      },
      { t: "h2", kick: "Lock it down", text: "The engine is not internet-facing" },
      {
        t: "ul",
        items: [
          "Bind the bridge to loopback and give it a service token. Without the token it authenticates nobody, and anything that reaches it can run the ring for free — it says so at startup rather than letting you find out later.",
          "Agents advertise the address other agents dial. Use the fastest network the hosts share, never loopback across hosts, and keep that network off the public internet.",
          "If something must sit on a public IP, remember Docker's published ports are **DNAT'd before the INPUT chain**, so a rule on `dport` will not match. Filter in the `DOCKER-USER` chain on conntrack's original destination port (`--ctorigdstport`), and persist with a systemd oneshot ordered `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Footguns", text: "Two that cost real time" },
      {
        t: "ul",
        items: [
          "**An unset operator wallet reads as zero earnings.** The gateway skips any contribution row without an owner and logs nothing. The nodes look idle while they are serving.",
          "**`pkill` matches its own command line.** `ssh host 'pkill -f server.js; ...'` kills the shell running it. Put the pattern in a script file rather than the remote command, use a character class (`server[.]js`), and remember a process started as a bare `node server.js` carries no path to match on — find it by its listening port instead.",
        ],
      },
    ],
  },
  {
    slug: "wan-interconnect",
    category: "network",
    title: "WAN interconnect (200G optics)",
    summary: "How compute sites link at 200 Gb/s across a room, a campus, or a city: which optic at which distance, what plugs where, and what it takes to actually hit line rate.",
    image: { src: "/wiki/wan-interconnect.jpg", alt: "Two sites joined by fiber, with the optic module doing the work at each end" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "When two sites both have public routes, the expert-dispatch data plane should be a **direct link** — the relay is for edges with no address of their own. This entry is the concrete recipe for making that direct link 200 Gb/s-class with catalog parts. One rule organizes everything: **the fiber is speed-neutral glass; the speed lives in the pluggable at each end.**",
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
          "**NIC side** — ConnectX-6/7-class cards expose QSFP56 cages; DAC/AOC/FR4/LR4/ER4 all seat directly in the NIC. A GB10-class host already has two 200 GbE QSFP ports on board, so a two-site link needs exactly one cable and zero new hardware.",
          "**Switch side** — coherent ZR+ optics are QSFP-DD form factor and belong in a switch or router; the site's NIC then joins that switch at 200G over a short DAC. Use this tier when the far site is tens of kilometers away.",
          "**The fiber itself** — standard single-mode (G.652) duplex LC pairs, leased as dark fiber per strand. The same glass carries 100G today and 400G later; upgrades are a module swap, never civil works.",
          "**Beyond ~120 km** — you stop buying parts and start leasing a wavelength from a carrier; the demarcation is an Ethernet handoff on your switch.",
        ],
      },
      {
        t: "code",
        caption: "Three reference builds, cheapest first.",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "Step 3 · actually hitting 200G", text: "Line rate is a configuration, not a purchase" },
      {
        t: "ul",
        items: [
          "Use **RDMA (RoCE)** for the dispatch stream where available — GB10-class hosts feed the NIC through split PCIe links, and measured full speed (~185–190 Gb/s) shows up under RoCE with a correctly mapped topology; a mis-mapped path caps near half rate and untuned plain TCP lands far lower.",
          "Enable **jumbo frames (MTU 9000)** end-to-end and keep `TCP_NODELAY` on the dispatch sockets (the bridge already sets it).",
          "Expect to *verify*, not assume: run a perftest between sites after every physical change — the difference between 95 and 190 Gb/s is invisible until measured.",
          "Keep the **443 relay as the fallback path** — the dial policy is direct-first for public peers, relay for NAT. The relay's job is reach, the direct link's job is speed.",
        ],
      },
      {
        t: "p",
        md: "Why this matters to the architecture: decode latency is bounded by round-trip time (~5 µs/km in fiber — physics, unaffected by bandwidth), so a fat pipe buys **prefill speed, batched-dispatch throughput, and near-instant expert-slice distribution**, not lower per-token latency. That is exactly the site-tier role in the two-tier design: capacity in the fat-pipe tier, reach in the relay tier.",
      },
    ],
  },
  {
    slug: "load-adaptive-scaling",
    category: "network",
    title: "Load-adaptive scaling",
    summary: "Kvasir's MoE serving path grows and shrinks with traffic: the coordinator re-engages proven workers under saturation, and the bridge recruits idle nodes by raising expert demand — all pull-based, so NAT'd devices join too.",
    image: { src: "/wiki/load-adaptive-scaling.jpg", alt: "A coordinator and bridge cooperating to grow a worker pool as load rises" },
    imagePos: 1,
    blocks: [
      {
        t: "p",
        md: "Kvasir's MoE serving path scales elastically with load, in two cooperating layers. When it's quiet the coordinator serves everything locally for the fastest path per token; when it saturates, the two layers below grow the swarm — and shrink it again when the surge passes.",
      },
      {
        t: "callout",
        md: "**Engine status, 2026-09-21.** Expert-grain sharding was built and demonstrated on Kvasir's previous engine, and the results below are from that work. Carrying it onto [p4](/wiki/p4) is partly done. Live: a device proves a wallet, is told which expert window is scarcest, claims it, opens a relay, and is credited for the bytes it carries. Not yet: the endpoint that hands it the weights for that window, and — more fundamentally — any path in the p4 stage adapter for dispatching expert work to a remote device. The market and the transport run; the data path and the engine hook do not. Where a detail below names a tool or a route, it is the one that ran on the previous engine.",
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
      { t: "h2", kick: "Layer 2", text: "Control-plane side: load-adaptive recruitment" },
      {
        t: "p",
        md: "The control plane watches every MoE coordinator and grows the worker pool when needed:",
      },
      {
        t: "ul",
        items: [
          "A background loop polls each coordinator's slots and records saturation per model.",
          "While a model is saturated, its **effective expert-replica target** is raised (base + boost). The coverage market then reads already-covered experts as scarce again, and a model with **no** live workers is seeded from its GGUF metadata (expert count) so demand is visible even from zero.",
          "Idle nodes poll the demand market (`/api/expert-volunteer`) and are handed a `(layer, expert-range)` slice to serve. They download the slice, dial the relay, and register coverage; the control plane auto-wires them to the coordinator's dispatch map.",
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
