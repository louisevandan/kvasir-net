/* Nederlands — vertaling van de wiki-items. Structuur (slug, categorie,
   blokvolgorde, code) spiegelt exact entries.ts (Engelse bron); technische
   termen en identifiers (KVR, linkcpp, inferentie-engine, GGUF, MoE, ring runtime,
   tok/s enz.) blijven letterlijk. De compliance-framing (devnet,
   utility-token, non-custodial) blijft intact. */
import type { WikiTranslation } from "./entries";

export const nlWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Kvasir-netwerk",
    summary: "Een gedecentraliseerd AI-inferentienetwerk (DePIN) waar alledaagse apparaten open modellen serveren en KVR verdienen.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** is een gedecentraliseerd AI-inferentienetwerk: grote open modellen worden met de **linkcpp**-engine over gedeelde hardware verdeeld, zodat geen enkel knooppunt het hele model bezit. Iedereen kan een GPU, CPU, NPU — zelfs een telefoon — bijdragen en **KVR** verdienen voor de lagen of experts die zijn apparaat daadwerkelijk serveert. Ontwikkelaars bereiken het netwerk via OpenAI/Anthropic-compatibele gateways en betalen per inferentie.",
      },
      {
        t: "ul",
        items: [
          "**Engine met beschikbare broncode** — linkcpp is BSL-gelicentieerd (gratis voor ontwikkeling en testen, productiegebruik vereist een licentie); het inferentie-engine-datavlak eronder blijft ongewijzigd en inspecteerbaar.",
          "**Non-custodial** — beloningen worden verrekend naar de eigen Solana-wallet van elke node-eigenaar; sleutels verlaten de gebruiker nooit.",
          "**Bewezen op echte hardware** — een 122B-model draaide verdeeld over 4 AMD MI250-GPU's, in een heterogene vloot van GPU/CPU/NPU/mobiele nodes, met de bijdrage van elke node end-to-end bijgeschreven.",
          "**Vernoemd naar de Noorse mythe** — Kvasir, het wijste wezen, geboren uit de samengebrachte essentie van alle goden en het eigendom van geen enkele.",
        ],
      },
      { t: "h2", kick: "Eén verzoek, vele apparaten", text: "Hoe een inferentie stroomt" },
      {
        t: "code",
        caption: "Elke hop is gewoon HTTP/TCP; het model zelf is wat verdeeld wordt.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Rollen **stapelen**: één machine kan tegelijk rekennode, gateway-host en hub-host zijn, en de beloningen tellen op. De taak van het netwerk is om het geheel op één machine te laten lijken — één endpoint vooraan, duizenden imperfecte apparaten erachter.",
      },
      {
        t: "p",
        md: "Vandaag draait het netwerk op **Solana devnet**; KVR is een utility- / bijdrage-token, geen verhandelbaar activum of investering, en niets op deze pagina is financieel advies.",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "Het besturingsvlak: ontdekt apparaten, plant laagplaatsing, start workers, orkestreert de ring.",
    blocks: [
      {
        t: "p",
        md: "De **hub** is het besturingsvlak van het netwerk, door linkcpp geleverd als één Docker-image (`controller.hub:app`, een FastAPI-service op poort **19000**). Hij ontdekt apparaten, controleert runtime-compatibiliteit, plant plaatsing met de planner, start standaard inferentie-engine-workers en stelt de gateways per controller beschikbaar. Het is bewust saaie infrastructuur: request/response-HTTP, herstart-veilige staat, geen exotisch transport.",
      },
      { t: "h2", kick: "Drie deuren naar binnen", text: "Hoe machines zich bij een hub aansluiten" },
      {
        t: "ul",
        items: [
          "**Lokale node-slots** — vijf vaste slots per hub, gemapt op RPC-poorten **50052–50056**. Slots bestaan altijd; je bewerkt de GPU- + VRAM/RAM/CPU-budgetten van een slot in plaats van willekeurige nodes te maken, en resources zijn **alleen bewerkbaar zolang een slot niet gebonden is** — dat beschermt het capaciteitscontract onder een draaiende controller.",
          "**Remote units** — registreer een andere draaiende linkcpp-hub en importeer diens zichtbare nodes. Het datavlak-endpoint wordt altijd afgeleid van de geregistreerde *unit*-URL plus de door de unit blootgestelde worker-poort — nooit van een node-host die het externe systeem adverteert.",
          "**Beheerde node-agents** — worker-only services (`nodeagent.py`) die zich aansluiten via simpel request/response-HTTP (`/control/join|status|download|load|unload`) en rapporteren via `POST /api/node-reports`. Bewust **geen** persistente stream, zodat ze simpele LAN/VPN-routering overleven.",
        ],
      },
      { t: "h2", kick: "Niets laadt ongeverifieerd", text: "Compatibiliteitspoort" },
      {
        t: "p",
        md: "Elke unit, node en agent rapporteert een protocol- / runtime-pack-identiteit plus backend-details. Mismatches in unit, runtime-pack, inferentie-engine-revisie en RPC-ABI worden **hard geblokkeerd vóór bind, plan, load of infer**; backend-verschillen (CUDA/Metal/Vulkan/CPU) worden bijgehouden als node-capaciteiten, niet als afwijzingen. Adaptief laden wordt ook geblokkeerd wanneer een node de resource-monitoring niet kan leveren die een veilig plan nodig heeft.",
      },
      {
        t: "code",
        caption: "Wat een herstart overleeft, en wat niet.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Omdat de hub de meest kritieke rol is, verdienen hub-hosts de **hoogste uurlijkse uptime-beloning**. Een publieke hub draaien vereist het staken van **100.000 KVR**.",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Het publieke toegangspunt: OpenAI/Anthropic-compatibele API's en KVR-afrekening per inferentie.",
    blocks: [
      {
        t: "p",
        md: "De **gateway** is waar ontwikkelaars het netwerk ontmoeten. Elke controller stelt OpenAI-compatibele endpoints beschikbaar (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) en Anthropic-compatibele (`/anthropic/v1/messages`, `/anthropic/v1/models`), allemaal gedragen door hetzelfde geladen model — een bestaande client werkt door alleen de base-URL en sleutel te wisselen.",
      },
      {
        t: "code",
        caption: "Een standaard OpenAI-achtige aanroep tegen de Kvasir-gateway.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Metering", text: "Betalen per inferentie in KVR" },
      {
        t: "p",
        md: "Gebruik wordt in KVR verrekend via een driestappenflow — **quote → payment → inference** — zodat een verzoek geprijsd is vóór het draait en de nodes die het bedienden erna worden bijgeschreven. De gateway aggregeert ook een **live modelcatalogus** van elke bereikbare hub, zodat `/v1/models` weerspiegelt wat het netwerk nu echt kan serveren.",
      },
      {
        t: "ul",
        items: [
          "Gateway-hosts verdienen een **uurlijkse uptime-beloning** voor het online houden van het toegangspunt, plus een **×1.5-bonus** op elke inferentie die ze mee bedienen.",
          "Een publieke gateway draaien vereist het staken van **100.000 KVR** (net als een hub).",
          "Publieke deployments beschermen operator-toegang met **SIWS + 2FA**; kale hubs zijn alleen ontworpen voor vertrouwde host / LAN / VPN.",
        ],
      },
    ],
  },
  node: {
    title: "Node",
    summary: "Elk apparaat dat een deel van een model serveert — GPU, CPU, NPU of telefoon — en KVR verdient voor het werk dat het doet.",
    blocks: [
      {
        t: "p",
        md: "Een **node** is elk apparaat dat een deel van een model serveert: een GPU-bak, een CPU-machine, een NPU-apparaat of een telefoon. Een node bezit alleen zijn deel — een laagvenster op de ring, of een expertplak in de zwerm — en verdient KVR gewogen naar precies het geleverde werk. De live vloot mengt AMD MI250's, NVIDIA GB10's en RTX Pro 6000's, een MacBook, x86 Windows-CPU-machines en mobiele nodes in één netwerk.",
      },
      { t: "h2", kick: "Van download tot uitbetaling", text: "De levenscyclus van een node" },
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
          "**Rekennodes** verdienen per bijdrage-eenheid, gewogen naar laagaandeel en geschaald naar prestatieniveau — geen staking vereist.",
          "Nodes registreren onder de wallet van hun eigenaar; beloningen worden non-custodial naar die wallet verrekend. Vier verschillende eigenaarswallets die elk hun laagaandeel verdienen is end-to-end geverifieerd.",
          "Capaciteitsdata (backend, accumulatieprecisie, resourcebudgetten) bepaalt wat de planner op een node mag plaatsen — en, in de zwerm, welke rangen hij mag bedienen.",
          "Een node die geen resource-monitoring kan leveren wordt uitgesloten van adaptief laden in plaats van blind vertrouwd.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "443-relay",
    summary: "Het datavlak voor apparaten achter NAT: beide uiteinden bellen naar buiten via een WebSocket-brug op poort 443.",
    blocks: [
      {
        t: "p",
        md: "Telefoons achter carrier-NAT kunnen geen inkomende verbindingen accepteren, en edges zoals Cloudflare laten alleen poorten 80/443 door. De **443-relay** lost beide op: een WebSocket-brug per edge met een **1-byte rol-preamble** laat beide kanten **naar buiten** bellen, zodat een telefoon aan het datavlak deelneemt en daarbij **nul inkomende poorten** opent.",
      },
      {
        t: "code",
        caption: "Twee uitgaande verbindingen ontmoeten elkaar in het midden; de preamble zegt wie wie is.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "Gehard in productie", text: "Drie echte bugs, drie fixes" },
      {
        t: "ul",
        items: [
          "**Build-fingerprint-overeenstemming** — beide uiteinden moeten bewijzen dat ze hetzelfde runtime-pack draaien voordat er ook maar één tensor-byte stroomt.",
          "**Node-token-downloadauthenticatie** — partial-shard-downloads authenticeren met hetzelfde wallet-afgeleide node-token dat de app al bezit.",
          "**De `Int.ushr`-framestilstand** — Kotlins `ushr` gebruikt alleen de laagste 5 bits van de shift, dus `len ushr 56` werd `len ushr 24` en beschadigde stilletjes elk frame ≥ 64 KiB (een `result_output` van 593 KB was het eerste slachtoffer). Opgelost door de lengteverpakking naar `Long`-shifts te verplaatsen — dragend voor gebundelde expert-dispatch, die routinematig 64 KiB overschrijdt.",
        ],
      },
      {
        t: "p",
        md: "De relay draagt wat de topologie nodig heeft — ringlaaggrenzen of expert-dispatchstromen — en hetzelfde voor de ring geverifieerde mechanisme is wat productietelefoon-workers in de zwerm gebruiken.",
      },
      {
        t: "p",
        md: "Zowel de `/api/expert-relay`- als de `/api/ring-relay`-upgrades zijn **rauw doorgelust**: de gateway stuurt WebSocket-frames byte-voor-byte door zonder ze te parsen, zodat de relay een dunne, modelagnostische pijp blijft. Hij **meet nog steeds de bytes die hij per sessie overbrugt**, en dat gemeten werk vloeit in het bijdrageregister van de hub en wordt in **KVR** verrekend naar de eigen wallet van de worker — relayen voor een telefoon achter NAT verdient precies zoals een direct verbonden node.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "Het besturingsvlak met beschikbare broncode (BSL) dat alledaagse hardware in een gedistribueerde inferentie-engine verandert.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** is de engine achter Kvasir: een besturingsvlak rond het RPC-datavlak van inferentie-engine dat grote AI-modellen over meerdere GPU's en machines draait met *standaard* `ggml-rpc-server`- / `llama-server`-binaries. Alles wat het toevoegt is orkestratie — GPU-ontdekking, node-slots, laagplaatsingsplanning, worker-start en de OpenAI/Anthropic-gateways.",
      },
      { t: "h2", kick: "Architectuur", text: "Eén hub, standaard workers" },
      {
        t: "code",
        caption: "Het verzoekpad door een linkcpp-deployment.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Broncode beschikbaar onder de BSL** — lees hem, draai hem en bouw erop voort, gratis voor ontwikkeling en testen; productiegebruik vereist een licentie.",
          "Het inferentie-engine-datavlak blijft **ongevorkt** (op één vastgepinde mobiele GPU-over-RPC-patch na), zodat upstream-prestatieverbeteringen blijven binnenstromen.",
          "Geleverd als **één Docker-image**: de FastAPI-hub plus de twee inferentie-engine-binaries ingebakken; native worker-nodes bouwen buiten Docker voor CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "De planner", text: "GGUF-metadata erin, plaatsing eruit" },
      {
        t: "p",
        md: "De planner leest GGUF-metadata en produceert aaneengesloten laagvensters per node, de bijbehorende `--tensor-split` en KV-cache- / laag- / expert-VRAM-schattingen per node — plus optionele offload van MoE-expert-FFN's naar node-RAM, uitgegeven als inferentie-engine `-ot`-regels (bijv. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) en via `--override-tensor` naar de start gedragen. Een plan dat niet past wordt **vóór** het laden als **infeasible** gemeld, in plaats van als OOM tijdens runtime ontdekt.",
      },
      {
        t: "p",
        md: "Runtime-compatibiliteit is een eersteklas concept: protocol, runtime-pack, inferentie-engine-revisie en RPC-ABI worden geverifieerd, en mismatches worden hard geblokkeerd vóór elke bind, plan, load of inferentie.",
      },
    ],
  },
  "ring-runtime": {
    title: "Ring-runtime",
    summary: "Pipeline-inferentie zonder master: elk apparaat draait zijn laagvenster en geeft alleen grenzen door aan zijn buur.",
    blocks: [
      {
        t: "p",
        md: "De **ring-runtime** is Kvasirs serveertopologie met lage latentie. Elk apparaat laadt alleen zijn aaneengesloten **laagvenster** en opent precies twee verbindingen — voorganger en opvolger. Hidden-state-grenzen circuleren rond de ring; de laatste rang bemonstert het token en stuurt het terug. **Geen centrale master, en geen node bezit het hele model.**",
      },
      { t: "h2", kick: "Waarom geen ster", text: "Het RPC-masterprobleem" },
      {
        t: "p",
        md: "In de klassieke RPC-topologie opent één master de **hele GGUF** en belt naar elke worker. Dat breekt in een open netwerk op drie manieren: de master moet de hele checkpoint bezitten en serveren; elke worker moet belbaar zijn — telefoons achter carrier-NAT zijn dat niet; en de master is één eigenaar in een netwerk dat er geen zou moeten hebben. De ring verwijdert alle drie: elke stage bezit zijn venster, verbindingen lopen van buur naar buur, en de relay maakt NAT-apparaten bereikbaar.",
      },
      {
        t: "code",
        caption: "Eén decodeerstap rond een ring met 4 stages.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "Plaatsing komt uit het **rank manifest** van de planner — bijv. de 49 lagen van Qwen3.5-122B verdeeld over een GPU, CPU, NPU en telefoon.",
          "Grenzen zijn klein (één hidden-state-vector per token), dus hops zijn goedkoop, zelfs over zwakke verbindingen.",
          "Mobiele GPU's draaien ringstages **direct** (Adreno via OpenCL) — de RPC-route naar een telefoon-GPU bleek onhaalbaar omdat Adrenos bufferindeling RPC-serialisatie niet overleeft, maar een lokale stage bezit zijn backend, dus alleen grenzen kruisen de lijn.",
        ],
      },
      {
        t: "p",
        md: "De ring is het **latentie**-pad; zijn vloer is laaggranulariteit (~1.4 GB op de 122B). De expertzwerm verwijdert die vloer en sluit aan op hetzelfde serveerweefsel.",
      },
    ],
  },
  "layer-window": {
    title: "Laagvenster & partial shards",
    summary: "De aaneengesloten modelplak van een node — downloadbaar als mini-GGUF in plaats van de volledige checkpoint.",
    blocks: [
      {
        t: "p",
        md: "Een **laagvenster** is het aaneengesloten bereik van transformer-lagen dat een ringnode serveert. Een node heeft de volledige checkpoint niet nodig om er een te serveren — een **stage-mini-GGUF** draagt alleen de tensoren van het venster: voor de 122B **254 MB met 26 tensoren** (van 338 in totaal) tegenover het volledige model van 77.6 GB, of ~1.5 GB voor een éénlaagsvenster op een telefoon.",
      },
      {
        t: "code",
        caption: "Een rij uit het rank manifest: wie serveert wat, binnen welk budget.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Geclaimd, niet toegewezen", text: "Zelfinschrijving" },
      {
        t: "ul",
        items: [
          "Een node pollt de **dekkings-/vraagkaart** om te zien welke vensters onderbediend zijn en wat elk betaalt.",
          "Hij kiest het **hoogst belonende** ongedekte venster dat in zijn budget past, downloadt precies dat, en sluit aan.",
          "Dekking herstelt zichzelf: wanneer een node wegvalt, wordt zijn venster weer schaars — en dus weer lucratief.",
          "End-to-end geverifieerd met een telefoon achter NAT: poll → zelfinschrijving → gedeeltelijke download → Adreno-GPU-load → ringinferentie voltooid, bijdrage bijgeschreven.",
        ],
      },
      {
        t: "p",
        md: "De expertzwerm hergebruikt exact deze markt op de fijnere korrel van **(laag, expertbereik)** — dezelfde kaart, dezelfde zelfinschrijving, dezelfde beloningen, kleinere eenheden.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "Een model waarvan de FFN's honderden onafhankelijke experts zijn, waarvan er per token maar enkele afgaan.",
    blocks: [
      {
        t: "p",
        md: "Een **Mixture-of-Experts**-model vervangt de enkele FFN van elke laag door een bank onafhankelijke expert-FFN's plus een **router** die er per token enkele kiest. Qwen3.5-122B-A10B is het vlaggenschipvoorbeeld van het netwerk:",
      },
      {
        t: "stats",
        items: [
          { n: "49", l: "lagen" },
          { n: "256", l: "experts / laag" },
          { n: "8", l: "actief / token" },
          { n: "12,544", l: "experts totaal" },
          { n: "5.3 MB", l: "één expert (Q4)" },
          { n: "86%", l: "van het gewicht in experts" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "volledige checkpoint" },
        ],
      },
      {
        t: "p",
        md: "Elke laag splitst in een **dicht pad** — attention + KV, norms, de router (`ffn_gate_inp`), een gedeelde expert — en een **expertbank**, opgeslagen als drie gestapelde tensoren (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Het dichte pad is de minderheid van de bytes; de expertbank is 86% van het model.",
      },
      {
        t: "ul",
        items: [
          "De expertindex is de **buitenste GGUF-dimensie**, dus elke expert is een aaneengesloten, kwantblok-uitgelijnde plaat — extractie is een byte-range-kopie, geen dekwantisatie.",
          "Per token gaan slechts **8 van de 256** experts per laag af, dus het expertverkeer van een laag bij decoderen is een handvol kleine matrixvermenigvuldigingen over één hidden-vector (~6 KB dispatch).",
          "Experts zijn onderling onafhankelijk — eigendom kan over apparaten worden verspreid en vrij herverdeeld.",
        ],
      },
      {
        t: "p",
        md: "Daarom is MoE het natuurlijke substraat van de zwerm: de gewichten komen voorverpakt in apparaatformaat, onafhankelijk te bezitten eenheden.",
      },
    ],
  },
  "expert-sharding": {
    title: "Expert-sharding",
    summary: "Een MoE splitsen op expertkorrel, zodat een telefoon 42–340 MB aan experts draagt in plaats van een laag van 1.4 GB.",
    blocks: [
      {
        t: "p",
        md: "**Expert-sharding** verlaagt de draageenheid van de zwerm van een laag (~1.4 GB op de 122B) naar een expert (**5.3 MB**). Een zwak apparaat downloadt een plak van 8–64 experts (**42–340 MB**), laadt die als pure-functieworker — geen attention, geen KV, geen sampler — en berekent zijn experts wanneer de router van de backbone ze selecteert.",
      },
      { t: "h2", kick: "Twee rollen", text: "Backbone × worker" },
      {
        t: "code",
        caption: "Het snijpunt binnen één MoE-laag (de router draait één keer, op de backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "De **backbone** houdt het dichte pad (attention, norms, router, gedeelde expert, combine) en bewaart alle experts als RAM-geofflode fallback-replica voor churn-tolerantie.",
          "**Workers** (`linkcpp-expert-worker --serve`) beantwoorden `(n_used, n_tokens, cur, sel) → experts` over één langlevende TCP-stream — dezelfde stream die de 443-relay voor telefoons tunnelt.",
          "Dekking herstelt zichzelf via de **expertdekkingsmarkt**: `POST /api/expert-coverage` heartbeat het bezit, `GET /api/expert-demand` aggregeert schaarste, `POST /api/expert-volunteer` wijst het schaarste bereik toe, bijgesneden op het budget van de node.",
        ],
      },
      { t: "h2", kick: "Gemeten, niet beloofd", text: "Geverifieerd op echte hardware" },
      {
        t: "ul",
        items: [
          "Gesharde berekening == monolithisch tot **max|Δ| = 3.6e-12** (een exacte hergroepering, geen benadering).",
          "Cross-proces-dispatch op een live 122B-decodering: **argmax MATCH**, logit-cosine 0.99869 — byte-identiek aan in-process.",
          "Een Galaxy S25 downloadde autonoom zijn plak van 1.58 GB en berekende bij elk token de layer-0-experts: **8/8 tokens identiek** aan de lokale run.",
          "Een externe GPU over het publieke internet — één WAN-round-trip per token — bleef **greedy 8/8 identiek** (cosine 0.99773): **1.2% doorvoer-overhead** op een directe link, ~28% via een CDN-edge. De eerlijke prijs van seriële per-token-dispatch, en waarom de hefboom van het weefsel batching is, geen lagere latentie.",
          "Gebundelde dispatch haalt **53k tok/s per worker** bij batch 512 (ROCm) — de doorvoerweefsel-eigenschap die de zwerm praktisch maakt.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "Router-autoriteit",
    summary: "De coherentie-invariant van de zwerm: routering wordt één keer beslist, op de backbone — workers ontvangen alleen expert-id's.",
    blocks: [
      {
        t: "callout",
        md: "**De invariant:** de enige discrete beslissing in het netwerk is MoE-routering (top-8 van 256). Kvasir draait de router **precies één keer, op de backbone**, en stuurt workers alleen de id's van de geselecteerde experts. Een heterogene zwerm kan licht verschillen in de *grootte* van de uitvoer van elke expert — hij verschilt nooit in *welke experts draaien*.",
      },
      {
        t: "p",
        md: "Zonder deze regel zou elk backend de router opnieuw draaien en bij grensgevallen **andere experts** kiezen — echte, catastrofale divergentie, want vanaf dat token vertakt de berekening als met een andere random seed. Mét de regel reduceren hardwareverschillen tot een begrensde continue fout die de kansgewogen combine absorbeert.",
      },
      { t: "h2", kick: "Wat het voorkomt", text: "Divergentiemodi gesloten door één beslispunt" },
      {
        t: "table",
        head: ["Divergentiemodus", "Zonder autoriteit", "Met autoriteit"],
        rows: [
          ["Routeringsmismatch", "Backends kiezen verschillende top-8 op grenzen", "Id's één keer beslist, naar de eigenaren gestuurd"],
          ["Trajectvertakking", "Eén omgeslagen token vertakt de hele reeks", "Decoderen/samplen vastgepind op één node"],
          ["Verificatie", "Bitvergelijking tussen backends (onmogelijk)", "Tolerantiechecks op goed gedefinieerde residuen"],
        ],
      },
      {
        t: "p",
        md: "De kosten zijn verwaarloosbaar: de backbone berekende toch al `ffn_norm` en de router-logits; wat over de lijn gaat zijn alleen de hidden-rijen plus de geselecteerde id's — zo'n **6 KB per decodeerstap**.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "Numerieke equivalentie",
    summary: "Verschillende backends komen nooit bit-voor-bit overeen; de zwerm behandelt gemeten tolerantie als eersteklas contract.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno en CPU's berekenen dezelfde operatie met verschillende reductievolgordes, FMA-fusie, accumulatoren en benaderingen van transcendente functies — resultaten verschillen ~1e-6…1e-3 per operatie, **by design, nooit bit-identiek**. Een zwerm van willekeurig opdagende hardware kan geen bit-exactheid eisen, dus meet Kvasir in plaats daarvan equivalentie.",
      },
      {
        t: "table",
        head: ["Backend-paar (echte 122B, layer-0-experts)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Telefoon-ARM-CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "De volledige backend-matrix is gesloten: de twee GPU-backends (CUDA, ROCm) delen kernelbronnen en landen **feitelijk bit-identiek** (cosine 1.0000000000), terwijl GPU↔CPU-paren equivalent blijven op ~0.9997. Een CUDA-worker en een ROCm-worker zijn uitwisselbaar; een GPU-worker en een CPU-worker zijn numeriek equivalent.",
      },
      { t: "h2", kick: "Waarom ze verschillen", text: "Drijvende-komma-optelling is niet associatief" },
      {
        t: "ul",
        items: [
          "**Matmul-reductievolgorde** — tensor cores, MFMA-tegels, OpenCL-workgroups en SIMD-lanes accumuleren in verschillende volgordes.",
          "**Accumulatieprecisie** — F16/BF16-opslag met F32- of F16-accumulatoren: de grootste hefboom op divergentie.",
          "**Transcendente benaderingen** — exp (softmax), silu (swiglu) en rsqrt (norms) gebruiken per backend andere polynoom-/tabelvarianten.",
        ],
      },
      { t: "h2", kick: "Het contract", text: "Toleranties, capaciteiten, één autoriteit" },
      {
        t: "ul",
        items: [
          "Verificatie is een **tolerantie** — \"top-1-overeenstemming ≥ 99.x%, KL ≤ ε\" — nooit bitgelijkheid.",
          "Backends en accumulatieprecisie worden geadverteerd als node-**capaciteiten**; F32-accumulerende nodes krijgen voorrang voor uitvoergevoelige rangen.",
          "Nodes buiten tolerantie worden gemarkeerd als ongeschikt voor gevoelige rangen, niet botweg geweigerd.",
          "Discrete beslissingen (routering, sampling) worden vastgepind op enkele autoriteiten zodat continue fout nooit discrete divergentie kan worden.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "Het gekwantiseerde modelbestandsformaat van inferentie-engine — en de layout die partial- en expert-slicing goedkoop maakt.",
    blocks: [
      {
        t: "p",
        md: "**GGUF** is het één-bestandsmodelformaat van het inferentie-engine-ecosysteem: metadata (architectuur, laagaantal, dimensies, kwantisatie) plus de tensoren als rauwe gekwantiseerde bytes (bijv. Q4_K_M). De planner van linkcpp leest de metadata voor plaatsingen en grootteschattingen; de serveerkant snijdt de tensorbytes om downloads te produceren.",
      },
      {
        t: "ul",
        items: [
          "**Stage-mini-GGUF's** dragen de tensoren van één laagvenster — 254 MB in plaats van 77.6 GB voor een 122B-ringstage.",
          "**Expert-shard-GGUF's** dragen één (laag, expertbereik)-plak, geserveerd door `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` met node-token-authenticatie.",
          "Beide zijn **geldige GGUF-bestanden**: de reader op de node laadt ze met standaardtooling, geen eigen formaat.",
        ],
      },
      {
        t: "code",
        caption: "Waarom expert-slicing een byte-kopie is: de expertindex is de buitenste dimensie.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "De router (`ffn_gate_inp`) en de gedeelde expert zijn **uitgesloten** van expert-shards — ze horen bij de backbone, precies wat router-autoriteit vereist.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "Het utility-token van het netwerk: ontwikkelaars geven het uit aan inferentie, bijdragers verdienen het voor rekenwerk.",
    blocks: [
      {
        t: "p",
        md: "**KVR** (on-chain-naam \"Kvasir\", 6 decimalen, Solana) is één token dat beide kanten op stroomt: ontwikkelaars **geven** KVR uit om inferentie via de gateway te draaien, bijdragers **verdienen** KVR voor de rekenkracht die hun nodes leveren. Beloningen worden berekend uit echt werk — daadwerkelijk geserveerde lagen en experts — niet uit deelname.",
      },
      {
        t: "ul",
        items: [
          "**Uitgavenkant** — betalen per inferentie via de gateway: quote → payment → inference.",
          "**Verdienkant** — bijdrage-eenheden × laagaandeel × prestatieniveau voor rekenwerk; uurlijkse uptime voor hub-/gateway-rollen.",
          "**Verrekening** — op Solana, naar de eigen wallet van elke node-eigenaar; de verrekenservice schrijft elke node bij die een verzoek raakte.",
          "**Vernoemd naar de mythe** — de Mede der Poëzie, gebrouwen uit Kvasir, die wijsheid schonk aan ieder die ervan dronk: open toegang, en beloningen voor iedereen die bijschenkt.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, utility-token.** KVR draait momenteel op Solana devnet en is een utility- / bijdrage-token — geen verhandelbaar activum, prijs of investering. Niets hier is financieel advies of een rendementsbelofte.",
      },
    ],
  },
  "contribution-units": {
    title: "Bijdrage-eenheden",
    summary: "De beloningsformule: eenheden volgen geserveerde tokens gewogen naar laagaandeel, en schalen dan naar prestatieniveau.",
    blocks: [
      {
        t: "code",
        caption: "Hoe rekenbeloningen worden berekend.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "Eén **eenheid** ≈ 1k geserveerde tokens, gewogen naar het **laagaandeel** van de node in elke inferentie — een node die 12 van de 49 lagen draait, verdient 12/49 van de eenheden van elke inferentie. De niveauvermenigvuldiger beloont vervolgens gemeten snelheid, en infrastructuurrollen bouwen daarbovenop uurlijkse uptime op.",
      },
      { t: "h2", kick: "Rekenvoorbeeld", text: "Eén inferentie, vier nodes" },
      {
        t: "table",
        head: ["Node", "Lagen", "Aandeel", "Niveau", "Effectieve eenheden / 1k tokens"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Telefoon", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Beloningen volgen **echt werk**: een node die niets serveerde verdient niets, ongeacht uptime (bij rekenrollen).",
          "Rollen **stapelen** — één machine kan rekenwerk + gateway + hub zijn, en de stromen tellen op.",
          "Alles wordt in KVR verrekend naar de eigen eigenaarswallet van de node; het dashboard toont ruw × niveau = effectief en een opeisbaar saldo.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "Prestatieniveaus",
    summary: "Gemeten doorvoer bepaalt een vermenigvuldiger: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["Niveau", "Gemeten doorvoer", "Vermenigvuldiger"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "De gemeten decodeersnelheid van een node bepaalt zijn niveau, en het niveau vermenigvuldigt zijn verdiende eenheden — snellere hardware verdient proportioneel meer voor hetzelfde werk. De node-statusweergave van de wallet toont het niveau van elke node naast zijn bijdrage.",
      },
      {
        t: "ul",
        items: [
          "Niveaus worden **gemeten, niet zelf verklaard** — doorvoer komt uit de werkelijke serveerprestatie van de node en wordt in de loop van de tijd opnieuw gemeten.",
          "Een C-niveau-telefoon verdient nog steeds — ×0.7 van zijn laagaandeel — en dat is precies het punt: de vloer geldt voor alle deelname, niet alleen voor datacenters.",
          "Het niveau vermenigvuldigt *effectieve* eenheden, dus het componeert met laagaandeel en de gateway-bonus in plaats van ze te vervangen.",
        ],
      },
    ],
  },
  staking: {
    title: "Staking",
    summary: "Stake KVR om APR-rente te verdienen; 100.000 gestakete KVR kwalificeert een wallet om hub- of gateway-nodes te draaien.",
    blocks: [
      {
        t: "p",
        md: "Staking vergrendelt KVR in je eigen wallet om **APR-rente** te verdienen en je te kwalificeren voor node-beloningen. Een **hub**- of **gateway**-node draaien vereist een stake van **100.000 KVR**; gewone rekennodes doen mee zonder stake en verdienen voor de lagen die ze draaien.",
      },
      {
        t: "ul",
        items: [
          "Staken gebeurt in het staking-paneel van het wallet-dashboard: voer een bedrag in, **Stake**, en de positie begint APR plus node-beloningsgeschiktheid op te bouwen.",
          "De 100k-eis is een **skin-in-the-game-filter** voor de twee rollen waar het verkeer van anderen van afhangt — toegangspunten en het besturingsvlak.",
          "Staking is non-custodial zoals al het andere: de positie leeft in je eigen wallet, en hoofdsom, opgebouwde rente en node-beloningen zijn allemaal zichtbaar in het staking-paneel.",
          "Devnet-KVR om te staken komt via distributie of swap (SOL/ETH ↔ KVR-swap: binnenkort); devnet-SOL voor kosten komt uit de publieke faucet.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "Non-custodial wallet",
    summary: "Sleutels leven alleen op het apparaat van de gebruiker — web, desktop, iOS en Android — en beloningen worden er direct naartoe verrekend.",
    blocks: [
      {
        t: "p",
        md: "De Kvasir Wallet is **non-custodial by design**: de 12-woorden-herstelzin en de sleutels worden alleen op het eigen apparaat van de gebruiker bewaard, nooit bij een operator. Beloningen worden op Solana direct verrekend naar de eigenaarswallet van elke node — geverifieerd over vier verschillende eigenaarswallets, elk met zijn eigen laagaandeel.",
      },
      {
        t: "ul",
        items: [
          "**Platforms** — web, desktop (macOS/Windows/Linux, wallet + node in één Electron-app), iOS en Android.",
          "**Eén zin, elk apparaat** — dezelfde 12-woordenzin herstelt hetzelfde account op desktop, telefoon en web; een lokale passphrase ontgrendelt elke installatie.",
          "**Wallet = node-identiteit** — de wallet ondertekent de netwerkidentiteit van de node, dus \"wie verdient voor dit apparaat\" is cryptografisch, geen accountregel op iemands server.",
          "**Zin kwijt, account kwijt** — non-custody snijdt aan twee kanten; er is geen operator die het kan resetten.",
        ],
      },
      {
        t: "p",
        md: "De mobiele apps zijn tegelijk node-apps: dezelfde wallet die je KVR bewaart, configureert het reken-backend en de node-modus van de telefoon, staket en claimt beloningen.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "Operator-login is een wallet-handtekening (Sign-In With Solana) over een server-nonce, plus optionele TOTP-2FA.",
    blocks: [
      {
        t: "p",
        md: "Voor publieke deployments wordt operator-toegang tot hub en gateway geauthenticeerd met **Sign-In With Solana**: de wallet van de operator ondertekent een door de server uitgegeven nonce en bewijst eigendom zonder wachtwoord of bewaarde inloggegevens. Daarbovenop beschermen **TOTP-2FA** en eenmalige back-upcodes de sessie — op zowel hub als gateway.",
      },
      {
        t: "ul",
        items: [
          "**Nergens wachtwoorden** — de walletsleutel is de identiteit en de nonce voorkomt replay; er is serverzijdig niets te phishen of te lekken.",
          "**TOTP-inschrijving per wallet** wordt in de hub-staat gepersisteerd, dus 2FA overleeft herstarts samen met slots en bindingen.",
          "**Back-upcodes zijn eenmalig** — elke code wordt bij het inloggen verbruikt, voor herstel wanneer het authenticator-apparaat niet beschikbaar is.",
          "**Eerlijk verklaarde reikwijdte** — kale hub en RPC-poorten zijn ontworpen voor vertrouwde host / LAN / VPN; SIWS + 2FA is de laag die *publieke* domeinen veilig blootstelbaar maakt.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "De KVR-economie",
    summary: "Hoe consumentenkosten en node-beloningen samen één zelfversterkende lus vormen — de deugdzame cyclus die het netwerk goedkoper laat worden naarmate het groter wordt.",
    blocks: [
      {
        t: "p",
        md: "Kvasir is een **tweezijdige markt** die in één token wordt verrekend. Consumenten betalen **KVR** per inferentie aan de treasury; nodes verdienen **KVR** voor precies het werk dat ze hebben geserveerd, uitbetaald naar hun eigen wallets. Het ontwerpdoel is dat deze twee kanten niet concurreren — ze **versterken elkaar**: meer aanbod maakt het netwerk goedkoper en beter, wat meer vraag aantrekt, waarvan de betalingen rijkere beloningen financieren, wat weer meer aanbod aantrekt.",
      },
      { t: "h2", kick: "Het vliegwiel", text: "Gebruik en aanbod groeien samen" },
      {
        t: "p",
        md: "Omdat inferentie **verplicht** in KVR wordt betaald, is elke eenheid gebruik echte vraag naar het token — nut, geen speculatie. Die vraag ondersteunt de waarde van de KVR die nodes verdienen, wat bijdragen aantrekkelijk houdt, wat de capaciteit vergroot, wat prijs en latency verlaagt, wat meer gebruik aantrekt. Kvasirs scherpste voordeel trekt de lus nog strakker aan: een deelnemer kan **tegelijk consument en leverancier** zijn (een *prosumer*), dus de twee kanten groeien vaak binnen dezelfde mensen.",
      },
      {
        t: "callout",
        md: "**\"Gratis als je bijdraagt\" is netto-gratis, niet kosteloos.** Je betaalt voor wat je infereert en verdient voor wat je serveert; draag ongeveer evenveel bij als je verbruikt en de twee heffen elkaar op. Het netwerk is niet gratis — *jouw* rekening is dat.",
      },
      { t: "h2", kick: "Deugdzaam houden", text: "Drie invarianten, en de spiralen die ze voorkomen" },
      {
        t: "table",
        head: ["Invariant", "Spiraal die het voorkomt"],
        rows: [
          ["Beloningen gefinancierd uit echte inkomsten (emissie alleen om op te starten, daarna afbouwen)", "Inflatie holt KVR uit tot beide kanten instorten"],
          ["KVR is het verplichte medium voor inferentie", "Tokenwaarde ontkoppelt van gebruik en wordt pure speculatie"],
          ["Prijs zweeft tussen een kostenondergrens en een onder-marktbovengrens", "Te laag laat nodes verhongeren; te hoog verliest gebruikers aan gecentraliseerde API's"],
        ],
      },
      {
        t: "p",
        md: "Kvasir beloont al **echt werk** (KVR per geserveerde tokens × laagaandeel, niet louter aanwezigheid) en verrekent non-custodial, wat het moeilijke deel is van het eerlijk maken van uit inkomsten gefinancierde beloningen. De rest — een gebruiksgedreven prijs en een afbouw van emissie→inkomsten — is de economische routekaart die \"meer nodes → goedkoper\" van een intuïtie in een door het protocol afgedwongen regel verandert. Het item **Inferentieprijzen** behandelt de prijskant; **Bijdrage-eenheden** behandelt hoe werk beloning wordt.",
      },
    ],
  },
  "inference-pricing": {
    title: "Inferentieprijzen",
    summary: "Wat een inferentie vandaag in KVR kost, waarom een gedecentraliseerd netwerk structureel goedkoper is, en hoe de prijs geacht wordt te dalen naarmate het aanbod groeit.",
    blocks: [
      {
        t: "p",
        md: "Toegang tot het netwerk is **betalen-per-inferentie**: de gateway geeft een KVR-prijs op voor je verzoek, je wallet betaalt die on-chain, en pas daarna draait de hub het model. De prijsstelling is een kleine, transparante formule — een ondergrens per verzoek plus een tarief per token — vooraf opgegeven en verrekend op het **werkelijke** tokengebruik na generatie.",
      },
      {
        t: "code",
        caption: "De verrekeningsformule — vooraf opgegeven, achteraf in rekening gebracht op werkelijk gebruik.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Waarom het goedkoper kan", text: "Geen centrale marge om te betalen" },
      {
        t: "p",
        md: "Een gecentraliseerde API rekent de kostprijs **plus** een grote marge en kapitaalterugverdiening. Een gedecentraliseerd netwerk rekent dicht bij de **marginale kosten** van zijn bijdragers — elektriciteit en hardware-afschrijving — plus een dunne protocolvergoeding. Die structurele kloof bestaat ongeacht de omvang. Groei vergroot hem: **expert-sharding** betekent dat meer nodes elk een kleinere slice vasthouden, zodat goedkopere apparaten kunnen serveren, wat de marginale kosten van deelname verlaagt en het aanbod verdiept.",
      },
      {
        t: "callout",
        md: "**De prijs is bestuurd, geen vrijheid-blijheid.** Tarieven zijn een gevoelige economische parameter, alleen te wijzigen door de genesis-wallet met wallet-handtekening + 2FA — nooit via een omgevingsvariabele. Dit houdt de tokeneconomie stabiel en auditeerbaar.",
      },
      { t: "h2", kick: "Waar het naartoe gaat", text: "Gebruiksgedreven prijs" },
      {
        t: "p",
        md: "De ontwerprichting is een prijs die **meebeweegt met de netwerkbenutting** tussen een ondergrens (boven de marginale kosten van een node gehouden, zodat serveren de moeite waard blijft) en een bovengrens (onder gecentraliseerde alternatieven gehouden, zodat hij concurrerend blijft). Ongebruikt aanbod duwt de prijs omlaag; congestie duwt hem omhoog. Dat is het mechanisme dat **\"meer gedeelde nodes → lagere prijs\"** eindelijk waar maakt in code — de natuurlijke thermostaat van de **KVR-economie**.",
      },
    ],
  },
  "run-expert-worker": {
    title: "Een expert-worker draaien",
    summary: "Maak van een reserve-GPU, CPU of telefoon een expert-worker: bouw hem, meld je aan voor de schaarste plak, download hem, serveer, en bel naar buiten over 443 om KVR te verdienen.",
    blocks: [
      {
        t: "p",
        md: "Een **expert-worker** is een pure `(hidden, ids) → out`-functie — geen attention, geen KV-cache, geen sampler — die een plak van de experts van een MoE-model berekent wanneer de router van de backbone ze selecteert. Je kiest niet wat je serveert; de **dekkingsmarkt** overhandigt je het schaarste, hoogst belonende bereik bijgesneden op je budget, zodat een telefoon van 4 GB en een datacenter-GPU allebei een plek vinden.",
      },
      { t: "h2", kick: "Zeven stappen", text: "Bouwen → aanmelden → serveren → bellen → verdienen" },
      {
        t: "code",
        caption: "Het hele pad — het belscript wacht met een retry-lus op de backbone.",
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
          "**De plak is piepklein.** Een layer-0-plak met 128 experts is **794 MB** tegenover het volledige model van 72 GB — de korrel die zwakke apparaten laat deelnemen. Je downloadt alleen het bereik dat de markt heeft toegewezen.",
          "**Naar buiten bellen, nooit naar binnen.** Stap 5 opent één uitgaande WebSocket op 443, zodat carrier-NAT en CDN-edges hem doorlaten en je nul inkomende poorten blootstelt — hetzelfde pad dat een telefoon gebruikt.",
          "**De heartbeat is dragend.** Zonder `POST /api/expert-coverage` serveer je niets dat de vraagkaart kent, en niets wat je doet wordt bijgeschreven.",
          "**Beloning is per werk.** Overbrugd werk wordt bijgeboekt op het bijdrageregister van de hub; de gateway delta-crediteert KVR naar je **eigen** wallet (non-custodial). Je hebt een wallet-adres nodig om betaald te worden.",
        ],
      },
      {
        t: "callout",
        md: "De worker spreekt hetzelfde dispatch-protocol als een GPU in het datacenter — `(n_used, n_tokens, cur, sel) → experts` over één langlevende stream. Een partial-shard-worker zet gewoon `n_used = 1`. Die uniformiteit is waarom een telefoon, een CPU-bak en een Blackwell-kaart uitwisselbare leden van dezelfde zwerm zijn.",
      },
    ],
  },
  "hub-operations": {
    title: "Een hub beheren",
    summary: "Operatornotities voor het draaien van een hub en gateway: hot-patchen zonder rebuilds, herstarts overleven, de catalogus geregistreerd houden en het oppervlak afgrendelen tot 443.",
    blocks: [
      {
        t: "p",
        md: "De hub (besturingsvlak) en gateway (publiek toegangspunt) zijn de twee langlevende services die een operator gezond houdt. De hub en zijn RPC-poorten zijn **ongeauthenticeerd by design** — alleen vertrouwde host / LAN / VPN — en al het publieke verkeer komt samen op het enkele 443-oppervlak van de gateway. Dit zijn de operationele notities die die opzet stabiel houden bij codewijzigingen, herstarts en reboots.",
      },
      { t: "h2", kick: "Deployen & hot-patchen", text: "Code wijzigen zonder rebuild" },
      {
        t: "ul",
        items: [
          "**Snelle route:** werk hub-/gateway-code bij met `docker cp <file> <container>:/app/...` + `docker restart` — geen image-rebuild. Maar **een omgevingsvariabele toevoegen kan zo niet** (dat vereist een container-recreate); geef in plaats daarvan de voorkeur aan een runtime-config-API die naar de hub-staat persisteert.",
          "**Compose-drift:** een langdraaiende container kan afwijken van zijn compose-bestand (netwerkmodus, entrypoint, env). Doe altijd `docker inspect` op de echte config vóór een `docker compose up -d`-recreate — als hij is afgedreven, wist recreatie de productie-instellingen. Gebruik cp + restart.",
          "**Diff voordat je patcht:** `docker cp` het bestand uit de container naar buiten en diff het tegen repo-HEAD voordat je het vervangt, zodat de hot-patch van een eerdere sessie niet stilletjes verloren gaat.",
        ],
      },
      { t: "h2", kick: "Een herstart overleven", text: "Staat blijft; geladen modellen niet" },
      {
        t: "ul",
        items: [
          "Een hub-herstart **stopt met serveren.** Slots, controllers en bindingen herstellen uit `hub-state.json`, maar een geladen model is alleen runtime. Lees na een herstart de `last_load` van elke controller en vuur `POST /api/controllers/{cid}/serve` opnieuw af — zelfs een groot model komt in ~1 minuut terug dankzij de page cache.",
          "**Gateway-watchdog:** peil elk geserveerd model met een verzoek van 1 token elke 30 s en herlaad automatisch uit `last_load` bij falen (met een cooldown). **Peil *alle* modellen, niet `catalog[0]`** — zodra een gezond model van een andere hub naar voren sorteert, mist een peiling die alleen de eerste bekijkt een groot model dat uitvalt (een echte bug, sindsdien opgelost).",
          "**Catalogus-TTL:** `POST /api/pay/hub/register` heeft een TTL van 90 s, dus houd registratie levend met een heartbeat-lus van ~60 s, bestand tegen reboots gemaakt met een `@reboot`-cron of een systemd-unit.",
        ],
      },
      { t: "h2", kick: "Afgrendelen", text: "Alles wat publiek is gaat via 443" },
      {
        t: "ul",
        items: [
          "De hub (:19000) en RPC-poorten gaan uit van een vertrouwd netwerk; het enige dat naar het internet gericht mag zijn is de gateway op 443 (inclusief zijn WebSocket-relay-passthrough).",
          "Als een hub op een publiek IP moet staan, firewall hem dan tot vertrouwde IP's — maar Dockers gepubliceerde poorten worden **ge-DNAT vóór de INPUT-chain**, dus een regel op `dport` matcht niet. Filter in plaats daarvan in de `DOCKER-USER`-chain met de originele bestemmingspoort van conntrack (`--ctorigdstport`), en persisteer de regels met een systemd-oneshot geordend `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Verrekening & valkuilen", text: "Pull, niet push — en één shell-valkuil" },
      {
        t: "ul",
        items: [
          "**Verrekening is pull, geen push:** de hub accumuleert bijdragen; de gateway pollt `GET /api/contributions` en delta-crediteert KVR. Als een hub-herstart zijn tellers reset, herbaseert de gateway zodat niets dubbel wordt betaald. Het expert-werktarief wordt bepaald door `LINKCPP_EXPERT_UNITS_PER_MB`.",
          "**De `pkill`-valkuil:** `ssh host 'pkill -f X; ...'` matcht zijn *eigen* commandoregel en doodt zichzelf. Gebruik een character class in het patroon (`X[x]`), en zet nooit de spawn en de pkill in hetzelfde remote commando.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Hub-WAN-interconnect (200G-optiek)",
    summary: "Hoe hubs koppelen op 200 Gb/s over een kamer, een campus of een stad: welke optiek op welke afstand, wat waar in past, en wat er nodig is om echt line rate te halen.",
    blocks: [
      {
        t: "p",
        md: "Wanneer twee hubs allebei publieke routes hebben, hoort het expert-dispatch-datavlak een **directe link** te zijn — de 443-relay is voor edges achter NAT. Dit item is het concrete recept om die directe link 200 Gb/s-klasse te maken met catalogusonderdelen. Eén regel ordent alles: **de glasvezel is snelheidsneutraal glas; de snelheid zit in de pluggable aan elk uiteinde.**",
      },
      { t: "h2", kick: "Stap 1 · kies op afstand", text: "De reikwijdteladder" },
      {
        t: "table",
        head: ["afstand", "onderdeel", "past in"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC, geen switch"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / switch"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "NIC- of switch-QSFP56-cage"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "NIC- of switch-QSFP56-cage"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "switch-/router-QSFP-DD-cage (niet de NIC)"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "je switch draagt over aan de carrier"],
        ],
      },
      { t: "h2", kick: "Stap 2 · wat past waar", text: "NIC-kant vs switch-kant" },
      {
        t: "ul",
        items: [
          "**NIC-kant** — kaarten van de ConnectX-6/7-klasse bieden QSFP56-cages; DAC/AOC/FR4/LR4/ER4 zitten allemaal direct in de NIC. Een hub van de GB10-klasse heeft al twee 200 GbE QSFP-poorten aan boord, dus een link tussen twee hubs heeft precies één kabel en nul nieuwe hardware nodig.",
          "**Switch-kant** — coherente ZR+-optiek is QSFP-DD-formfactor en hoort in een switch of router; de NIC van de hub sluit dan op 200G via een korte DAC aan op die switch. Gebruik deze tier wanneer de verre hub tientallen kilometers weg is.",
          "**De glasvezel zelf** — standaard single-mode (G.652) duplex-LC-paren, geleased als dark fiber per streng. Hetzelfde glas draagt vandaag 100G en later 400G; upgrades zijn een moduleverwisseling, nooit graafwerk.",
          "**Voorbij ~120 km** — stop je met onderdelen kopen en ga je een wavelength leasen bij een carrier; de demarcatie is een Ethernet-overdracht op je switch.",
        ],
      },
      {
        t: "code",
        caption: "Drie referentieopstellingen, goedkoopste eerst.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Stap 3 · echt 200G halen", text: "Line rate is een configuratie, geen aankoop" },
      {
        t: "ul",
        items: [
          "Gebruik **RDMA (RoCE)** voor de dispatch-stream waar beschikbaar — hosts van de GB10-klasse voeden de NIC via gesplitste PCIe-links, en de gemeten volle snelheid (~185–190 Gb/s) verschijnt onder RoCE met een correct gemapte topologie; een verkeerd gemapt pad topt op bijna de helft van het tarief en ongetunede platte TCP landt veel lager.",
          "Schakel **jumbo frames (MTU 9000)** end-to-end in en houd `TCP_NODELAY` aan op de dispatch-sockets (de hub zet het al).",
          "Verwacht te *verifiëren*, niet aan te nemen: draai een perftest tussen hubs na elke fysieke wijziging — het verschil tussen 95 en 190 Gb/s is onzichtbaar tot het gemeten is.",
          "Houd de **443-relay als fallback-pad** — het belbeleid is direct-eerst voor publieke peers, relay voor NAT. De taak van de relay is bereik, de taak van de directe link is snelheid.",
        ],
      },
      {
        t: "p",
        md: "Waarom dit voor de architectuur uitmaakt: decodeerlatentie wordt begrensd door de round-trip-tijd (~5 µs/km in glasvezel — natuurkunde, onafhankelijk van bandbreedte), dus een dikke pijp koopt **prefill-snelheid, doorvoer van gebundelde dispatch en vrijwel directe distributie van expert-plakken**, geen lagere latentie per token. Dat is precies de hub-tier-rol in het twee-tier-ontwerp: capaciteit in de dikke-pijp-tier, bereik in de relay-tier.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Belasting-adaptieve schaling",
    summary: "Kvasirs MoE-serveerpad groeit en krimpt met het verkeer: de coördinator schakelt bewezen workers opnieuw in onder verzadiging, en de hub werft inactieve nodes door de expertvraag te verhogen — alles pull-gebaseerd, zodat ook NAT-apparaten meedoen.",
    blocks: [
      {
        t: "p",
        md: "Kvasirs MoE-serveerpad schaalt elastisch mee met de belasting, in twee samenwerkende lagen. Bij rust serveert de coördinator alles lokaal voor het snelste pad per token; bij verzadiging laten de twee lagen hieronder de zwerm groeien — en krimpen hem weer wanneer de piek voorbij is.",
      },
      { t: "h2", kick: "Laag 1", text: "Coördinatorkant: belasting-adaptieve dispatch" },
      {
        t: "p",
        md: "De backbone-coördinator (een `linkcpp-server` die het volledige model draait) serveert gerouteerde experts ofwel op zijn eigen GPU (snel, lokaal) ofwel door ze naar externe workers te dispatchen. Een achtergrondthread beslist elke paar seconden welke van de twee:",
      },
      {
        t: "ul",
        items: [
          "Hij pollt zijn **eigen** inferentieslots. Wanneer `busy >= saturation threshold` (standaard 2) staat de coördinator onder belasting.",
          "Onder belasting, als een **bewezen** worker live is — een waarvan `last_serve_ms > 0`, d.w.z. hij heeft daadwerkelijk eerder experts berekend — blijft de coördinator ernaartoe dispatchen voorbij de normale inactiviteitstimeout, en geeft zo voorrang aan totale doorvoer boven latentie per token.",
          "Een worker die verbinding maakte maar nooit serveerde (een telefoon die de relay belde maar nooit rekende) wordt **niet** geworven onder belasting, omdat ernaartoe dispatchen het snelle lokale pad zou vervangen door een trage fallback. Nieuwe workers krijgen nog steeds een eerste poging via een kort respijtvenster.",
          "De zelfquery is in tijd begrensd zodat een vastgelopen poll de dispatch nooit kan blokkeren.",
        ],
      },
      { t: "h2", kick: "Laag 2", text: "Hubkant: belasting-adaptieve werving" },
      {
        t: "p",
        md: "De control-hub bewaakt elke MoE-coördinator en laat de workerpool groeien wanneer dat nodig is:",
      },
      {
        t: "ul",
        items: [
          "Een achtergrondlus pollt de slots van elke coördinator en registreert verzadiging per model.",
          "Zolang een model verzadigd is, wordt zijn **effectief expert-replicadoel** verhoogd (base + boost). De dekkingsmarkt leest dan al-gedekte experts opnieuw als schaars, en een model met **geen** live workers wordt geseed vanuit zijn GGUF-metadata (aantal experts), zodat de vraag zelfs vanaf nul zichtbaar is.",
          "Inactieve nodes pollen de vraagmarkt (`/api/expert-volunteer`) en krijgen een `(layer, expert-range)`-plak toegewezen om te serveren. Ze downloaden de plak, bellen de relay en registreren dekking; de hub bedraadt ze automatisch in de dispatch-map van de coördinator.",
          "Wanneer de belasting wegebt, valt het doel terug en verdwijnt de vraag, zodat de extra workers niet meer gedispatcht worden en na verloop van tijd wegvallen.",
        ],
      },
      {
        t: "callout",
        md: "Het ontwerp is **pull-gebaseerd**: nodes vragen om werk in plaats van dat het naar hen wordt geduwd, zodat een worker achter NAT meedoet zonder enige inkomende connectiviteit. Een in Laag 2 geworven node die begint te serveren wordt een **bewezen** worker die Laag 1 vervolgens onder belasting ingeschakeld houdt — de twee lagen componeren tot één elastische lus.",
      },
      {
        t: "p",
        md: "**Observability:** `GET /api/moe/recruitment` rapporteert busy/saturation per model en de base- versus effectieve target; `/api/expert-demand` draagt een `recruiting`-vlag.",
      },
    ],
  },
};
