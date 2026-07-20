# Kvasir / linkcpp — Homepage Build Brief

> **Purpose:** a complete, self-contained brief for a separate session to build the
> marketing homepage. Everything here is grounded in the actual codebase and what was
> verified running in the 2026‑07‑12 session. Where something is **roadmap / not yet
> live**, it is labeled — do not present roadmap items as shipped.
>
> **How to use:** treat "LIVE / PROVEN" as safe to claim, "ROADMAP" as future tense,
> and "CONSTRAINTS" as hard rules (honesty/compliance). Draft copy in each section is a
> starting point, not final wording. Confirm the open questions at the end before building.

---

## 1. Brand system (use these names precisely)

| Name | What it is | Use it for |
| --- | --- | --- |
| **linkcpp** | The open-source **tech / control plane**: a Dockerized control hub around llama.cpp's RPC data plane. Discovers GPUs, plans layer placement across GPUs/machines, launches workers, exposes OpenAI/Anthropic-compatible gateways. MIT-licensed. Data plane is *stock* llama.cpp. | Developer/tech audience, GitHub, "the engine" |
| **Kvasir** | The **product / network brand**: the non-custodial wallet + settlement gateway + node-reward network built on top of linkcpp. UI surfaces are "Kvasir Hub", "Kvasir Gateway", "Kvasir Wallet". | Consumer/contributor audience, the network, rewards |
| **KVR** | The **token**. On-chain name "Kvasir", symbol **KVR**, 6 decimals, Solana. Used to pay for inference and to reward contributors. (`LKC` also appears in older `solana/` tooling — treat KVR as the current token.) | Token/rewards sections |

**Relationship in one line:** *linkcpp is the engine; Kvasir is the network that turns it
into a rewarded, decentralized AI inference marketplace paid in KVR.*

---

## 2. Positioning & taglines

**Category:** DePIN (Decentralized Physical Infrastructure Network) for AI inference —
a distributed network that runs large language models across contributed GPU (and,
later, edge) hardware, paying contributors in tokens for the compute they provide.

**One-liner options (pick/refine one):**
- "Run frontier-scale models on a network of shared GPUs — and get paid for the compute you contribute."
- "Decentralized AI inference. Bring a GPU, earn KVR."
- "The control plane for distributed llama.cpp — turned into a rewarded inference network."

**Elevator (2 sentences):** Kvasir splits large open models across many GPUs and machines
using stock llama.cpp, so no single node needs to hold the whole model. Everyone whose
hardware runs part of an inference earns KVR proportional to the work their node did.

---

## 3. Target audiences (design the page for these three)

1. **Contributors / node operators** — people with GPUs (or spare compute) who want to
   earn KVR by running a node. Primary CTA: "Run a node / Connect your GPU."
2. **Developers / API users** — want a cheap, OpenAI/Anthropic-compatible endpoint backed
   by the network. CTA: "Get an API key / Try the gateway."
3. **Investors / partners** — want the thesis, traction, and roadmap. CTA: "Read the deck /
   Contact us." (Lighter footprint on the homepage; depth in the business plan.)

---

## 4. Core value propositions

- **Run models too big for one GPU** — contiguous layer placement + tensor-split spreads a
  model (e.g. a 122B) across multiple GPUs/machines; the master is GPU-less.
- **Get paid for real compute** — rewards are **proportional to the layers your node runs**
  (layer-share of the tokens produced), not vague participation. Settled on Solana.
- **Non-custodial by design** — keys live in the user's wallet (browser/desktop/mobile);
  the operator login is a wallet signature (Sign-In With Solana) + optional 2FA.
- **Drop-in for developers** — OpenAI- and Anthropic-compatible endpoints; pay-per-inference
  in KVR.
- **Open engine** — the data plane is stock llama.cpp (MIT); linkcpp adds only orchestration.

---

## 5. Proof points — LIVE / PROVEN this session (safe to feature)

- **Real distributed inference:** `Qwen3.5-122B-A10B-Q4_K_M` served split across **4 AMD
  MI250 GPUs** (layers 13/12/12/12, ~15–20 GB per GPU), answering prompts in Korean and
  English. Also runs single-GPU.
- **Contribution crediting, end-to-end:** each participating node accrues KVR **weighted by
  its layer share** of every inference's output tokens (1 unit ≈ 1k tokens × layer share),
  credited to that node's **own owner wallet**. Verified with 4 distinct owner wallets, each
  earning its share; performance tiers (S/A/B/C by tok/s) apply a multiplier.
- **Secured operator access:** Sign-In With Solana wallet login + **Google Authenticator
  TOTP 2FA + single-use backup codes**, on both the hub and the gateway. Live over HTTPS
  (Cloudflare Tunnel) on the project's own domain: **`gate.kvasir-ai.net`** (wallet +
  gateway) and **`hub.kvasir-ai.net`** (hub) — verified serving 2026‑07‑12.
- **Pay-per-inference gateway:** OpenAI-compatible; quote → KVR payment → inference; a live
  model catalog aggregated from reachable hubs.
- **Wallets shipped:** non-custodial **web + desktop (Electron/React)** wallet and **iOS
  (Swift) + Android (Kotlin)** apps, with a node monitor showing contribution, tier, and
  rewards; staking (off-chain devnet MVP).
- **Cross-platform nodes:** native managed node agents on Linux/macOS/Windows; AMD ROCm,
  NVIDIA CUDA, Apple Metal, Vulkan, CPU backends.

## 6. Roadmap — NOT yet live (present as future / "coming")

- **Mobile & edge devices as inference nodes.** Today, contributing nodes are GPU RPC
  workers (desktop/server). Phones currently register for visibility/rewards but do **not**
  perform inference compute. True mobile participation is planned via a peer-to-peer **ring
  runtime** (each device holds a few layers locally, passes only small boundary state to its
  neighbor). *Do not claim mobile mining/earning is live.*
- **Mainnet.** Everything today is **Solana devnet**. KVR is a devnet token. Do not imply a
  tradable mainnet asset or financial return.
- **Larger/open contributor network.** Current demos ran on one operator's hardware.

---

## 7. Recommended homepage structure (sections + draft copy)

Single-page scroll, three-audience aware. Order:

1. **Hero**
   - Headline: the chosen one-liner (§2).
   - Sub: "Kvasir splits large open models across shared GPUs with stock llama.cpp. Contribute compute, earn KVR."
   - Primary CTA: **Run a node**. Secondary: **Use the API**.
   - Visual: an animated topology — one model, layers distributed across several GPU nodes, tokens flowing; small "earning KVR" counters per node. (Mirror the real 4-GPU split.)

2. **How it works (3 steps)**
   - *Split* — a big model is divided into contiguous layer windows across nodes; no node holds it all.
   - *Serve* — a request runs through the nodes; the network returns an OpenAI/Anthropic-compatible response.
   - *Reward* — each node earns KVR weighted by the layers it ran, settled on Solana to its own wallet.

3. **For contributors** — "Turn your GPU into income."
   - Bullets: bring any supported GPU (CUDA/ROCm/Metal/Vulkan); layer-share rewards; performance tiers (S/A/B/C); uptime rewards for infra roles; non-custodial (your keys).
   - CTA: run-a-node guide.

4. **For developers** — "One endpoint, backed by many GPUs."
   - OpenAI + Anthropic compatible; pay-per-inference in KVR; live model catalog.
   - Small code snippet (curl to `/v1/chat/completions`).

5. **Token & rewards (KVR)**
   - What KVR is (pay for inference + reward compute); how rewards are computed (layer-share × perf tier); non-custodial wallet.
   - CONSTRAINT banner: devnet, utility token, not investment advice (§ Constraints).

6. **Under the hood (tech / trust)**
   - Stock llama.cpp data plane (MIT), linkcpp control plane; distributed layer placement; SIWS + 2FA security; runtime compatibility gating.
   - Link to GitHub.

7. **Roadmap**
   - Mobile/edge nodes (ring runtime), mainnet, growing network — clearly future.

8. **Proof / traction strip**
   - "122B served across 4 GPUs • OpenAI-compatible • wallets on web/desktop/iOS/Android • live on public domains." (only the LIVE items from §5.)

9. **CTA footer** — Run a node · Get API access · GitHub · Contact.

---

## 8. Technical fact sheet (accurate details to pull from)

- Data plane: **stock llama.cpp** RPC (`ggml-rpc-server` workers + GPU-less `llama-server` master). Control plane: **linkcpp** (Python FastAPI hub), single Docker image.
- Distribution: planner reads GGUF metadata → **contiguous per-node layer windows + `--tensor-split`**, optional MoE expert-FFN offload to CPU RAM.
- Model demoed: **Qwen3.5-122B-A10B** (MoE, ~10B active), Q4_K_M, 49 layers, 4096 ctx.
- Rewards math: per node, `units += (output_tokens / 1000) × (node_layers / total_layers)`;
  `effective = units × perf_multiplier × gateway_bonus`; performance tiers S(×1.5)/A(×1.25)/B(×1.0)/C(×0.7) by measured tok/s; infra roles earn hourly **uptime** rewards too.
- Settlement: **Solana** (devnet); token **KVR** (6 decimals). Off-chain staking/settlement service today (custodial devnet MVP), on-chain program later.
- Gateways per model: `/v1/chat/completions`, `/v1/responses`, `/v1/models`, `/anthropic/v1/messages|models`.
- Security: Sign-In With Solana (ed25519 signature over a server nonce) + TOTP 2FA + backup codes; unauthenticated LAN/VPN mode for local use.
- Nodes: fixed local GPU slots, imported remote units, and managed native agents (Linux/macOS/Windows).
- License: **MIT**.

---

## 9. Design direction (suggestion — confirm)

- **Tone:** technical-credible but accessible; "infrastructure you can trust," not hype.
- **Motif:** distribution / mesh / rings of nodes; tokens (KVR) flowing along the layer path.
- **Palette:** the apps use a dark UI (deep near-black `#0b0c0f`/`#15171c`, accent blue
  `#4c8bf5`, success green `#3cbf8e`, danger `#e5675f`). Match for brand continuity; consider
  a light variant for the marketing page. Theme-aware (light/dark) recommended.
- **Imagery:** prefer real screenshots (hub topology view, wallet node monitor with tier &
  rewards, the distributed-load view) over stock art. Diagrams for "how it works."
- **Must be self-contained if built as an Artifact** (inline CSS/JS, no external CDNs,
  embed assets as data URIs), responsive, and theme-aware.

---

## 10. CONSTRAINTS (hard rules — honesty / compliance)

1. **Devnet only.** State or don't contradict that the token/rewards run on Solana **devnet**;
   never imply a live tradable asset, price, or financial return.
2. **KVR is a utility/contribution token**, not an investment. No "earn X%," no ROI promises,
   no guaranteed income figures. "Earn KVR for compute you contribute" is fine; "make money"
   framing is not.
3. **Mobile earning is roadmap.** Do not show phones actively mining/earning as a shipped
   feature. Phones = "coming soon" via the ring runtime.
4. **No impersonation / fake metrics.** Don't invent user counts, TVL, partner logos, or
   testimonials. Use only the real proof points in §5.
5. **Non-custodial claim must stay true** to the code (keys in the user's wallet).
6. Keep infra endpoints (hub/gateway ports) out of any "connect" instructions that imply they
   are public/unauthenticated services beyond the demo domains.

---

## 11. Assets & links available

- GitHub (tech): the linkcpp repo (MIT). (Two remotes exist internally; public-facing link
  TBD — confirm which repo is the public one before linking.)
- **Official domain: `kvasir-ai.net`** (Cloudflare). Live production endpoints:
  - `https://gate.kvasir-ai.net` — wallet web app + settlement gateway (serves `Kvasir Wallet`).
  - `https://hub.kvasir-ai.net` — the linkcpp hub.
  Both are live over HTTPS via Cloudflare Tunnel and are safe to feature/link. (The former
  `*.prototypebench.org` hosts still resolve during migration but are being retired — do not
  feature them.) Use `kvasir-ai.net` for any brand email, canonical URLs, and social handles.
- Screenshots to capture for the page: hub topology/distributed-load view, wallet node monitor
  (tier + contribution + rewards), gateway model list, 2FA enrollment (QR).
- Brand colors + UI: see `controller/web/hub.css` and `wallet/desktop/src` for the existing look.

---

## 12. Open questions to confirm before building

1. **Language:** Korean, English, or bilingual? (Apps are multilingual; audience likely KR + global.)
2. **Public repo / demo links:** which GitHub repo and which domains are OK to expose publicly?
3. **Primary CTA priority:** contributors (run a node) vs developers (API) vs investors — which leads?
4. **Static marketing page vs app-integrated** — standalone landing, or a section of the existing web app?
5. **Brand lock:** lead with "Kvasir" (network) and reference "linkcpp" (engine) — confirm.
6. **Logo / wordmark:** exists? (UI uses a "K" mark.) Provide or design.

---

*Source of truth for the facts above: the linkcpp/Kvasir codebase and the verified state as of
2026‑07‑12. See project memory: `multinode-contribution`, `gateway-db-and-vram-fixes`,
`kvasir-2fa-auth`, `marketing-deliverables-todo`.*
