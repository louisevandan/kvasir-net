/* Nederlands — vertaling van de techblog. Structuur (slug, categorie,
   blokvolgorde, code, img-posities) spiegelt exact articles.ts (Engelse bron);
   technische termen, identifiers en cijfers blijven behouden. */
import type { TechTranslation } from "./articles";

export const nlTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "Zwerm-inferentie met expert-sharding: het ontwerp",
    dek: "86% van een 122B-MoE bestaat uit 12.544 onafhankelijke experts van 5.3 MB. Snijd het model op die korrel en een telefoon draagt een echt aandeel van frontier-inferentie.",
    blocks: [
      {
        t: "callout",
        md: "**De these:** 86% van het gewicht van Qwen3.5-122B bestaat uit 12.544 onderling onafhankelijke experts van 5.3 MB. Shard op de expertkorrel en een zwak apparaat draagt \"8–64 experts (42–340 MB)\" in plaats van \"een laag van 1.4 GB\" — precies de eenheid die een telefoon werkelijk kan vasthouden. MoE is het natuurlijke substraat van een zwerm.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "Substraat · Qwen3.5-122B-A10B (Q4_K_M)", text: "De gewichten zijn al verpakt in zwermformaat" },
      {
        t: "stats",
        items: [
          { n: "49", l: "lagen" },
          { n: "256", l: "experts / laag" },
          { n: "8", l: "actief / token" },
          { n: "5.3 MB", l: "één expert (Q4)" },
          { n: "12,544", l: "experts totaal" },
          { n: "86%", l: "van het gewicht in experts" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "expertdim. = buitenste" },
        ],
      },
      {
        t: "p",
        md: "De expertindex is de **buitenste dimensie** van elke MoE-tensor, dus elke expert is een aaneengesloten, kwantblok-uitgelijnde plaat. Een expert-gesneden mini-GGUF is een schone byte-range-kopie — geen dekwantisatie, geen herverpakking.",
      },
      { t: "h2", kick: "Twee rollen", text: "Backbone-stage × expert-worker" },
      {
        t: "ul",
        items: [
          "**Backbone-stage (sterke node):** attention + KV-cache, alle norms, de **router**, de gedeelde expert en de residual combine — het hele dichte pad. Hij houdt bovendien alle experts resident als fallback-replica (RAM-offload), wat de zwerm churn-tolerantie geeft.",
          "**Expert-worker (een telefoon):** geen transformer. Geen attention, geen KV, geen sampler — een pure functie `(hidden, local_ids) → out` van drie mat-muls, die alleen zijn eigen expertplak huisvest. Past in elk budget, tot een telefoon van 4 GB.",
        ],
      },
      {
        t: "code",
        caption: "Het snijpunt: de router draait één keer op de backbone, met autoriteit.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Omdat de router **precies één keer** op de backbone draait, wordt elke geselecteerde expert precies één keer berekend door de node die hem bezit. Er is **geen benadering** — sharding verplaatst alleen waar de mat-muls gebeuren.",
      },
      { t: "h2", kick: "Geen nieuw subsysteem", text: "De zwerm is de bewezen beloningsmarkt, fijner gekorreld" },
      {
        t: "p",
        md: "Kvasir draait al een autonome schaarstemarkt voor **laag**-shards, geverifieerd op echte apparaten: een telefoon achter NAT pollt de vraagkaart, schrijft zichzelf in op het **hoogst belonende** segment, downloadt alleen dat venster gedeeltelijk, laadt het op zijn Adreno-GPU en voltooit ringinferentie — en verdient bijdragebeloningen. Expert-sharding hergebruikt dat alles — dekkingskaart, zelfinschrijving op maximale beloning, gedeeltelijke download, beloningen per node — en verandert alleen de dekkingseenheid van *laagbereiken* naar *(laag, expertbereik)*.",
      },
      { t: "h2", kick: "Twee al geverifieerde innovaties", text: "Deelname met deelgewichten + de 443-relay" },
      {
        t: "ul",
        items: [
          "**Beloningsgedreven gedeeltelijke gewichtsdownload:** conventionele RPC/TP/PP-opstellingen sturen de volledige checkpoint naar elke rank en een scheduler dicteert de plaatsing. Bij Kvasir downloadt een node **alleen de plak die hij gaat berekenen**, en kiest die plak **zelf, op beloning** — een stage-mini-GGUF van 254 MB tegenover het volledige model van 77.6 GB. Zo sluit een telefoon van 4 GB zich aan bij een model dat veel groter is dan hijzelf.",
          "**443-relay-datavlak:** Cloudflares 80/443-only edge plus carrier-NAT betekent geen directe verbinding in beide richtingen. Een WebSocket-brug per edge met een 1-byte rol-preamble laat **beide kanten naar buiten bellen** (de telefoon opent nul inkomende poorten). De landing vergde drie echte bugfixes — build-fingerprint-overeenstemming, node-token-downloadauthenticatie, en een Kotlin `Int.ushr`-framelengtebug die elk frame ≥ 64 KiB stilletjes beschadigde (`ushr` gebruikt alleen de laagste 5 bits van de shift; `len ushr 56` werd `len ushr 24`) — opgelost met `Long`-shifts.",
        ],
      },
      { t: "h2", kick: "De eerlijke crux", text: "Een doorvoerweefsel, geen lagelatentie-decoder" },
      {
        t: "p",
        md: "Decoderen is 49 seriële lagen, en één internet-rondreis per laag kost 2.5–10 s per token. De wedstrijd van de zwerm is dus **modellen serveren die niemand alleen kan hosten**, gemeten in totale doorvoer: batch-dispatch amortiseert de RTT, de backbone houdt een hot-expert-cache, en verzoeken routeren naar nabije replica's. Het lagelatentiepad blijft bij de pipeline-ring.",
      },
      { t: "h2", kick: "Routekaart", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — backbone-expert-RAM-offload: draai de 122B op één coördinator, zonder graafchirurgie.",
          "**M1** — expert-parallel bewijs op één host: expert-gesneden mini-GGUF + worker-runtime + dispatch, logits exact gelijk aan monolithisch.",
          "**M2** — LAN- + NAT-telefoonworkers die echte 122B-experts berekenen door de 443-relay.",
          "**M3** — dekkingsmarkt op expertkorrel met replica's en churn-fallback.",
          "**M4** — doorvoer: batch-dispatch + hot-expert-cache, tokens/s schalend met het aantal workers.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "Van blauwdruk naar hardware: het geverifieerde, en de sluitsteen",
    dek: "Een terugblik op de verificatiecampagne — ontwerp tot en met M2-kern bewezen op een echte 122B — en het ene integratiestuk dat de rest ontgrendelt.",
    blocks: [
      {
        t: "p",
        md: "De afgelopen weken werden de moeilijke, nieuwe onderdelen van de expertzwerm één voor één bewezen op een echte **Qwen3.5-122B** — niet gesimuleerd, geen speelgoedformaat. Hier is het verificatiespoor tot nu toe, en de ene sluitsteen die overbleef.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "Het spoor · alles geverifieerd op de echte 122B", text: "Wat er al is geland" },
      {
        t: "ul",
        items: [
          "**Ontwerp (5 revisies sinds de blauwdruk)** — EP-architectuur, autonome deelname met deelgewichten, de relay, de worker-kernelspecificatie, en cross-backend numerieke equivalentie gecodificeerd als kerntechnologie. Router-autoriteit vastgelegd als de coherentie-invariant.",
          "**M0 — backbone-expert-RAM-offload (planner geverifieerd):** de 122B is *feasible* op één coördinator van 64 GB — 10 expertlagen naar RAM geoffload, VRAM 62.6 GiB / RAM 14.2 GiB, bedraad via `--override-tensor`.",
          "**M1 — expertplak-datapad:** mini-GGUF-slicing per expert (ne[2]-platen, bytekopie zonder dekwantisatie) + het `/expert-shard`-downloadendpoint.",
          "**M1 — numeriek orakel:** dispatch + combine == monolithisch met **max|Δ| = 3.6e-12** op echte layer-0-experts — sharding is een exacte hergroepering van dezelfde gewogen som.",
          "**M1 — C++-worker op hardware:** `linkcpp-expert-worker` (puur ggml/gguf) gebouwd en gedraaid op ROCm, **cosine 0.99995** vs het orakel; router → twee C++-workers → combine evenaart monolithisch op cosine 0.9997–0.9999.",
          "**M2-kern — een telefoon berekent echte 122B-experts:** Android-crossbuild, gedraaid op een SM-S938N, **cosine 0.99992** vs het orakel.",
          "**Numeriek — 3-backend-equivalentiematrix:** dezelfde 122B-berekening op ROCm × telefoon-ARM-CPU × numpy — ROCm↔telefoon cosine 0.99990, ROCm↔numpy 0.99996, telefoon↔numpy 0.99992. Alles equivalent, niets bit-identiek.",
        ],
      },
      { t: "h2", kick: "De sluitsteen", text: "Backbone-dispatch, geïntegreerd in live decoderen" },
      {
        t: "callout",
        md: "Elk **onderdeel** — plakken, workers, dispatch/combine-logica, numerieke equivalentie, telefoonberekening — was apparaat-geverifieerd. Wat restte was ze **binnen een echte inferentie-engine-decodering** te bedraden: een `build_moe_ffn`-hook die experts midden in de graaf naar hun eigenaarnodes dispatcht. Dat vergde aanpassing van de vastgepinde inferentie-engine-submodule en meerdere build-verify-cycli. Zodra deze sluitsteen staat, openen **M2-relay-integratie, de M3-expertdekkingsmarkt en M4-batchdoorvoer** zich achtereenvolgens — ze hangen allemaal van deze dispatch af.",
      },
      {
        t: "p",
        md: "De sluitsteen is inmiddels geland: de vervolgposts over M2, M3, M4 en de live telefoondemostratie zijn de resultaten van precies deze integratie.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "Numerieke equivalentie over heterogene backends",
    dek: "CUDA, ROCm, Adreno en CPU's zullen nooit bit-voor-bit overeenstemmen. Dat de zwerm toch één coherent model produceert, is een ontworpen eigenschap, geen geluk.",
    blocks: [
      { t: "h2", kick: "Het sleutelonderscheid", text: "Exact vs equivalent — twee verschillende eigenschappen" },
      {
        t: "ul",
        items: [
          "**Binnen één backend — exact (3.6e-12):** experts over nodes verdelen en combineren is dezelfde gewogen som hergegroepeerd; het enige verschil is de drijvende-komma-accumulatievolgorde. Orakel-geverifieerd.",
          "**Tussen backends — equivalent (1e-3…1e-6):** dezelfde operatie op andere hardware draagt een relatieve fout per op van ~1e-3–1e-6, en is nooit nul. **De zwerm leeft in dit regime.**",
        ],
      },
      {
        t: "p",
        md: "\"Exact\" is wat decompositie binnen één apparaat garandeert. \"Equivalent\" is wat heterogene hardware je geeft. De taak van de zwerm is te voorkomen dat equivalentie zich opstapelt tot divergentie.",
      },
      { t: "h2", kick: "Gemeten · echte 122B, drie backends", text: "Geen theorie — gemeten op hardware" },
      {
        t: "p",
        md: "Dezelfde layer-0-expert-FFN van Qwen3.5-122B, berekend door `linkcpp-expert-worker` op een MI250 (**ROCm**), de **ARM-CPU** van een telefoon (SM-S938N) en een x86-**numpy**-referentie — dezelfde invoer, dezelfde gewichten, andere instructiesets en reductievolgordes:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Backend-paar", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Telefoon-ARM-CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["ROCm-GPU vs telefoon-ARM-CPU", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Drie instructiesets, één berekening — elk paar equivalent (cosine ≈ 0.9999), geen paar bit-identiek (Δ ≈ 1e-6). De residuen zijn klein **omdat router-autoriteit de invoer en de expertselectie heeft vastgepind**.",
      },
      {
        t: "p",
        md: "Een latere run op echte **NVIDIA GB10 Grace Blackwell**-hardware sloot de matrix op het laatste backend: CUDA ↔ ROCm landde op **cosine 1.0000000000** (max abs 3.5e-10, feitelijk bit-identiek, aangezien beide GPU-backends kernelbronnen delen), en CUDA ↔ Grace ARM CPU op cosine 0.99975 — hetzelfde GPU↔CPU-patroon als hierboven.",
      },
      { t: "h2", kick: "Waarom backends verschillen", text: "Drijvende-komma-optelling is niet associatief" },
      {
        t: "ul",
        items: [
          "**Matmul-reductievolgorde** — tensor cores, MFMA-tegels, OpenCL-workgroups en SIMD-lanes accumuleren elk in andere volgordes en tegelingen.",
          "**FMA-fusie** — `a*b+c` één keer (FMA) of twee keer afgerond, per backend anders gefuseerd.",
          "**Accumulatieprecisie** — F16/BF16-opslag met F32- vs F16-accumulatoren (de grootste hefboom op divergentie).",
          "**Transcendente benaderingen** — polynoom-/tabelvarianten van exp (softmax), silu/sigmoid (swiglu), rsqrt (norms).",
          "**Dequant + matmul-pad** — dekwantiseren-dan-vermenigvuldigen vs gefuseerde gekwantiseerde kernels ronden tussenwaarden anders af.",
          "**Niet-deterministische kernels** — atomic/split-K-reducties kunnen op hetzelfde apparaat per run verschillen.",
        ],
      },
      { t: "p", md: "Niets hiervan is een bug. Het is de prijs die het snelle pad van elke versneller betaalt." },
      { t: "h2", kick: "Waarom het toch werkt", text: "Eén autoriteit voor beslissingen, genoeg precisie voor accumulatie" },
      {
        t: "callout",
        md: "**ROUTER-AUTORITEIT — de kerninvariant.** De enige discrete beslissing in het netwerk is MoE-routering (top-8 van 256). Als elk backend de router opnieuw zou draaien, zouden grens-tokens **andere experts** kiezen en werkelijk divergeren. Kvasir draait de router **één keer, op de backbone**, en stuurt workers alleen de geselecteerde expert-id's. Een heterogene zwerm kan verschillen in de *grootte* van de uitvoer van elke expert — hij verschilt nooit in *welke experts draaien*. Dit zet catastrofale discrete divergentie om in begrensde continue fout, en is de coherentieregel van heterogeen expert-sharding.",
      },
      {
        t: "ul",
        items: [
          "**Discrete argmax:** decoderen is een argmax over logits. Een wiebel van 1e-3 kantelt een token alleen als twee kandidaten binnen 1e-3 liggen — op de meeste posities is de marge veel groter, dus **komen de tokens identiek uit**; de zeldzame kantelingen zijn posities zo ambigu als een andere seed.",
          "**Combine is optelling:** deelresultaten versmelten als een kansgewogen **som**. Onafhankelijke fouten van ~1e-4 tellen incoherent op — ze groeien als √k, niet k — en er is geen wegvallen van grote waarden, dus het residu blijft goed geconditioneerd.",
        ],
      },
      { t: "h2", kick: "Waar het kan breken · en de regels die dat stoppen", text: "Divergentiemodi en verdedigingen" },
      {
        t: "table",
        head: ["Divergentiemodus", "Mechanisme", "Regel"],
        rows: [
          ["Routeringsmismatch", "Backends kiezen andere top-8 voor grens-tokens", "Router-autoriteit — één keer beslist op de backbone, id's gedispatcht"],
          ["Trajectvertakking", "Logit-gewiebel per token kantelt er uiteindelijk één; de reeks vertakt als een nieuwe seed", "Decoderen/samplen vastgepind op één node"],
          ["Diepte-accumulatie", "49 lagen × elk ~1e-4 → tot 1e-2 bij de finale logits", "F32-accumulatie bij grenzen en combine"],
          ["Zelf-non-determinisme", "Atomic/split-K-kernels variëren per run", "Deterministische combine-kernels; verificatie met toleranties"],
          ["Precisiemismatch", "De ene node accumuleert F16, de andere F32", "Accumulatieprecisie geadverteerd als capaciteit; F32-nodes voorkeur voor uitvoerrangen"],
        ],
      },
      { t: "h2", kick: "Equivalentie is een getal", text: "Het meetprotocol" },
      {
        t: "ul",
        items: [
          "**Delta per op** — dezelfde invoer, relatieve fout A vs B op matmul, swiglu, softmax, norm.",
          "**Laaggrens-drift** — residual-delta na één laag, gestapeld om te zien of de diepte als √L of L accumuleert.",
          "**End-to-end logit-divergentie** — L∞, L2 en **KL-divergentie** over de volledige forward.",
          "**Beslissingsovereenstemming** — top-1-tokenovereenstemming plus top-8-routeringsovereenstemming (valideert waarom router-autoriteit nodig is).",
          "**Generatiestabiliteit** — greedy N tokens; de eerste index waar A en B divergeren.",
          "**Taakniveau** — perplexity- en eval-scoredelta's: de enige metriek die een gebruiker echt voelt.",
        ],
      },
      {
        t: "p",
        md: "Een voldoende is een **tolerantie** — \"top-1-overeenstemming ≥ 99.x%, KL ≤ ε\". Een node buiten tolerantie wordt gemarkeerd als ongeschikt voor gevoelige rangen, niet botweg afgewezen.",
      },
      { t: "h2", kick: "Waarom dit kernzwermtechnologie is", text: "Bit-overeenstemming is onmogelijk — en onnodig" },
      {
        t: "p",
        md: "Een homogene cluster kan bit-exactheid aannemen; een zwerm niet — zijn premisse is *welke hardware er ook opduikt*. Dus behandelt Kvasir numerieke equivalentie precies als protocolcompatibiliteit: een **eersteklas, gemeten contract**. Backends en accumulatieprecisie worden geadverteerd als nodecapaciteiten, router-autoriteit wordt afgedwongen als invariant, en elke verificatie gebruikt toleranties in plaats van bitgelijkheid. **Gemeten numerieke equivalentie + discrete beslissingen van één autoriteit** — dat is wat één model tegelijk op elke GPU op aarde laat draaien. Dat is de zwerm.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell sloot zich aan bij de zwerm",
    dek: "Een GB10 Grace Blackwell berekende echte 122B-expert-FFN-plakken in CUDA en evenaarde AMD ROCm bit voor bit (cosine 1.0000000000), en de Grace ARM CPU binnen tolerantie. De cross-backend-matrix is compleet.",
    blocks: [
      {
        t: "p",
        md: "De premisse van een zwerm is *welke hardware er ook opduikt*. Numerieke equivalentie — het bewijs dat CUDA-, ROCm-, Adreno- en CPU-workers allemaal hetzelfde token uitzenden — was al gemeten op ROCm, telefoon-ARM en numpy. NVIDIA is het **standaard en best geoptimaliseerde** pad in de standaard-ggml/inferentie-engine, maar was het ene backend waarop de matrix niet gesloten was. Echte Blackwell-hardware draaien sluit hem.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. Op dezelfde echte Qwen3.5-122B-layer-0-expertplak zijn de twee GPU-backends feitelijk bit-identiek.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Gemeten · echte Qwen3.5-122B-A10B, layer-0-expertplak", text: "De cross-backend-matrix" },
      {
        t: "table",
        head: ["Vergelijking", "Hardware", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "De twee GPU-backends (CUDA, ROCm) delen kernelbronnen, dus landen ze **feitelijk bit-identiek** (10⁻¹⁰). GPU↔CPU draagt een verstoring van ~10⁻³ per op door een andere accumulatievolgorde, maar blijft equivalent op **cosine 0.99975** — hetzelfde patroon als het eerdere ROCm↔telefoon-ARM 0.99992. Het router-autoriteitsprincipe houdt opnieuw stand op NVIDIA: **de discrete beslissingen (argmax, expertselectie) zijn invariant over deze continue verstoring.**",
      },
      { t: "h2", kick: "Opstelling", text: "Wat draaide, waarop" },
      {
        t: "ul",
        items: [
          "**Apparaat** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124,5 GB unified memory.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; de pure ggml/gguf-expert-worker gebouwd met Blackwell-kernels.",
          "**Model** — Qwen3.5-122B-A10B-Q4_K_M, layer-0 alle experts (256 experts, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Methode** — de 1,58 GB L0-plak MI250 → GB10 gestreamd (verliesloze vergelijking); dezelfde invoer (h/ids) door CUDA, CPU en ROCm; float32-uitvoervectoren (36.864) vergeleken op cosine, relatieve L2 en max-abs.",
        ],
      },
      {
        t: "callout",
        md: "**Eén echte-hardware-valkuil:** de geïntegreerde GPU van de GB10 wordt door ggml geclassificeerd als apparaattype `ACCEL`, niet `GPU` — dus `init_by_type(GPU)` vond niets. Opgelost door het eerste niet-CPU-apparaat te kiezen in plaats van het GPU-type hard te coderen.",
      },
      { t: "h2", kick: "Waarom het ertoe doet", text: "De matrix is gesloten" },
      {
        t: "p",
        md: "Om heterogene workers één model te laten serveren, moeten een CUDA-bak en een ROCm-bak **uitwisselbaar** zijn, en moeten een GPU en een CPU **numeriek equivalent** zijn. Met Blackwell gemeten gelden beide over de volledige backend-matrix: CUDA↔ROCm-workers kunnen elkaar vervangen, en GPU↔CPU-workers komen overeen binnen een begrensde, goed geconditioneerde tolerantie. De meest voorkomende versneller ter wereld is nu een geverifieerde zwermburger.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "Het geldpad verharden: transactiebeveiliging in KVR-settlement",
    dek: "Drie echte kwetsbaarheidsklassen — replay van betaalhandtekeningen, ongeauthenticeerd beloningen slaan en race-double-spends — gevonden, in tests uitgebuit en gesloten in de settlement-dienst van de gateway.",
    blocks: [
      {
        t: "p",
        md: "In een DePIN is het geldpad precies zo vijandig als het rekenpad: elk endpoint dat KVR bijschrijft wordt vroeg of laat afgetast door iemand die KVR wil zonder het werk te doen. Een beveiligingsronde over de settlement-dienst van de gateway — het proces dat on-chain betalingen verifieert en stakes, node-beloningen en inferentiekosten bijschrijft — vond en sloot **drie echte kwetsbaarheidsklassen**. Elk werd vóór de fix gedemonstreerd met een exploit-achtige test en erna opnieuw geverifieerd.",
      },
      { t: "h2", kick: "Het vertrouwensmodel", text: "Verifieer on-chain feiten, geen clientbeweringen" },
      {
        t: "p",
        md: "Kvasirs bewaringsmodel laat de sleutels bij de gebruikers: wallets ondertekenen transacties, Solana registreert ze, en de enige taak van de settlement-dienst is **verifiëren wat er werkelijk on-chain gebeurde** voordat hij een saldo aanraakt. Betalingen volgen *quote → payment → inference*, waarbij elke verbruikte transactiehandtekening in een eenmalig `usedSignatures`-register wordt genoteerd zodat ze nooit twee keer kan worden aangeboden. Dat maakt de settlement-dienst het knelpunt — en de regel die hij nooit mag breken: alleen bijschrijven wat de chain bewijst, nooit wat de client beweert.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Fix #1 · afzenderbinding", text: "Bind de betaling aan de betaler" },
      {
        t: "p",
        md: "Solana-handtekeningen zijn **openbaar**. Het stake-verificatiepad controleerde dat de vault het verwachte KVR *ontving* — maar nooit *wie het stuurde*. Een aanvaller kon op devnet de KVR→vault-overdracht van een slachtoffer afwachten en dan `{owner: aanvaller, signature: die van het slachtoffer}` indienen: de vault-ontvangstcheck slaagde, de hoofdsom werd aan de aanvaller bijgeschreven, en één unstake later was het geld van hem. Directe diefstal, met niets meer dan een block explorer.",
      },
      {
        t: "code",
        caption: "De fix: het KVR moet zijn afgeschreven van tokenrekeningen waarvan de eigenaar de bijgeschreven owner is.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Fix #2 · vertrouwde rapporteur", text: "Beloningen alleen uit geauthenticeerde bronnen" },
      {
        t: "p",
        md: "De node-belonings-endpoints sloegen claimbaar KVR uit **zelfgerapporteerde invoer**: `POST /api/node/contribution` schreef de `units` bij die de client beweerde — `units: 1e9` plus één claim-aanroep kon de vault leegtrekken — en register/heartbeat geloofden zelfverklaarde hub-/gatewayrollen (uurlijkse infra-beloningen) en prestatiescores (beloningsvermenigvuldigers). De fix zet elke beloningsbeïnvloedende bewering achter een **vertrouwde rapporteur**: alleen het M2M-servicetoken van de contributiepoll van de hub, of een geauthenticeerde admin, mag units, infra-rollen of prestatieniveaus beweren — afgedwongen zelfs in open LAN-modus, omdat dit KVR slaat. De tokenvergelijking is constant-tijd, en wallet↔node-koppeling blijft vrij; ze kan alleen niet meer haar eigen beloningen beweren.",
      },
      { t: "h2", kick: "Fix #3 · settlement-serialisatie", text: "Eén schrijver per saldo" },
      {
        t: "p",
        md: "De settlement-status was een lock-vrij read-modify-write, en elke geldoperatie *awaitet* halverwege een on-chain uitbetaling of verificatie — en geeft de event loop op met een verouderd saldo in de hand. Twee gelijktijdige claims konden hetzelfde openstaande saldo van 100 KVR lezen en het beide uitbetalen. Niet theoretisch: de exploit-test toonde **drie gelijktijdige claims die 300 uitbetaalden voor een saldo van 100**.",
      },
      {
        t: "code",
        caption: "Asynchrone serialisatie per sleutel: geldoperaties met dezelfde sleutel draaien strikt na elkaar.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Verdediging in de diepte", text: "Waar elke laag nu staat" },
      {
        t: "table",
        head: ["Laag", "Mechanisme"],
        rows: [
          ["Identiteit", "SIWS-wallet-handtekeninglogin over een server-nonce + TOTP-2FA + eenmalige back-upcodes"],
          ["Transport", "Wallet-afgeleide node-tokens bij shard-downloads; build-fingerprint-overeenstemming op de relay"],
          ["Betaling", "Afzenderbinding op stake-overdrachten; eenmalige usedSignatures; privé requestId bij inferentie"],
          ["Settlement", "Per-sleutel-locks rond elke saldoschrijving; eerst uitbetalen, dan afschrijven; idempotent opnieuw indienen"],
          ["Rapportage", "Beloningsbeïnvloedende feiten alleen van het M2M-servicetoken of admin, constant-tijd vergeleken"],
          ["Bewaring", "Non-custodial wallets — de dienst kan alleen bewegen wat de vault bevat, nooit gebruikerssleutels"],
        ],
      },
      { t: "h2", kick: "Gemeten, niet aangenomen", text: "Elke fix draagt zijn eigen exploit-test" },
      {
        t: "ul",
        items: [
          "Het opnieuw afspelen van de overdrachtshandtekening van een slachtoffer onder de wallet van een aanvaller wordt nu geweigerd (\"not sent by owner\"); legitieme stakes, over-claims en inferentiebetalingen gedragen zich ongewijzigd.",
          "Drie gelijktijdige claims op één saldo betalen **precies één keer**; het opnieuw indienen van een al betaalde inferentie geeft idempotent hetzelfde resultaat terug.",
          "Zelfbeweerde `units`, hub-/gatewayrollen en prestatieniveaus van ongeauthenticeerde clients verplaatsen geen enkele lamport aan beloningen meer.",
        ],
      },
      {
        t: "p",
        md: "De rode draad van alle drie de fixes is één principe, drie keer toegepast: **de chain is de bron van waarheid, de dienst is een verificateur, en elk saldo heeft precies één schrijver**. De settlement-dienst draait nog op Solana devnet — precies waar je deze klassen wilt vinden, uitbuiten en repareren voordat mainnet de inzet verhoogt.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — De engine: linkcpp, een besturingsvlak voor inferentie-engine",
    dek: "inferentie-engine levert een capabel RPC-datavlak maar geen besturingsvlak. linkcpp voegt de ontbrekende helft toe — ontdekking, planning, start en gateways — rond standaard binaries.",
    blocks: [
      {
        t: "p",
        md: "Alles waar Kvasir op draait begint hier. **linkcpp** is een besturingsvlak met beschikbare broncode (Business Source License) rond het RPC-datavlak van inferentie-engine: het draait grote AI-modellen over meerdere GPU's en machines met *standaard* `ggml-rpc-server`- / `llama-server`-binaries. Het datavlak blijft ongevorkt — alles wat linkcpp toevoegt is orkestratie.",
      },
      { t: "h2", kick: "Het gat", text: "Een datavlak zonder besturingsvlak" },
      {
        t: "p",
        md: "inferentie-engine kan een model al via RPC over machines verdelen — maar iemand moet de GPU's ontdekken, beslissen welke lagen waarheen gaan, de juiste workers met de juiste budgetten starten, controleren dat elke node hetzelfde protocol spreekt, en een API blootstellen die ontwikkelaars echt kunnen aanroepen. Dat met de hand doen voor één cluster is een klus; het doen voor een open netwerk van andermans apparaten is onmogelijk. Die coördinatielaag is linkcpp.",
      },
      { t: "h2", kick: "Architectuur", text: "Eén hub, standaard workers, standaard gateways" },
      {
        t: "code",
        caption: "Verzoekstroom — de hub orkestreert, standaard binaries rekenen.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock inferentie-engine engines below" },
      {
        t: "ul",
        items: [
          "**Drie manieren om aan te sluiten:** vaste **lokale node-slots** met bewerkbare VRAM/RAM/CPU-budgetten; **remote units** — registreer een andere hub en importeer zijn nodes; en **beheerde node-agents** — worker-only services die via simpel request/response-HTTP aansluiten, bewust zonder persistente stream, zodat ze simpele LAN/VPN-routering overleven.",
          "**Compatibiliteitsgating is eersteklas:** elke unit, node en agent rapporteert een protocol-/runtime-pack-identiteit plus backend-details. Mismatches in unit, runtime-pack, inferentie-engine-revisie en RPC-ABI worden **hard geblokkeerd vóór bind, plan, load of infer** — backend-verschillen (CUDA/Metal/Vulkan/CPU) worden als capaciteiten bijgehouden, niet als afwijzingen.",
          "**De planner** leest GGUF-metadata en produceert aaneengesloten laagplaatsing per node, `--tensor-split`, KV-cache-/laag-/expert-VRAM-schattingen, en optionele expert-FFN-offload naar RAM.",
          "**Gateways:** elke controller stelt OpenAI-compatibele (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) en Anthropic-compatibele (`/anthropic/v1/messages|models`) endpoints beschikbaar, gedragen door hetzelfde geladen model — bestaande clients werken ongewijzigd.",
        ],
      },
      {
        t: "p",
        md: "Deze bewuste scheiding — een ongewijzigd datavlak onder een open besturingsvlak — is waar al het latere op bouwt: de ring-runtime, de laagmarkt en uiteindelijk de expertzwerm zijn allemaal besturingsvlak-evoluties over dezelfde standaard rekenkracht.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — De ring: pipeline-inferentie zonder master",
    dek: "Elk apparaat laadt alleen zijn laagvenster en geeft een kleine hidden-state-grens door aan zijn buur. Geen node bezit het model; er bestaat geen centrale master.",
    blocks: [
      { t: "h2", kick: "Waarom geen ster", text: "De RPC-master is een knelpunt en een poortwachter" },
      {
        t: "p",
        md: "In de klassieke RPC-topologie opent één master de **hele GGUF** en belt naar elke worker. Die vorm breekt in een open netwerk op drie manieren: de master moet de hele checkpoint bezitten en serveren; elke worker moet belbaar zijn — telefoons achter carrier-NAT zijn dat niet; en de master is één eigenaar in een netwerk dat er geen zou moeten hebben.",
      },
      { t: "h2", kick: "De ring", text: "Laagvensters + grensdoorgave" },
      {
        t: "ul",
        items: [
          "Elk apparaat slaat hetzelfde model op maar **laadt alleen zijn aaneengesloten laagvenster**, en opent precies twee verbindingen: één naar zijn voorganger, één naar zijn opvolger.",
          "Een verzoek betreedt de ring; elke node draait zijn lagen en geeft alleen de **hidden-state-grens** door aan zijn buur. De laatste rank bemonstert het token en stuurt het terug — geen centrale master, en geen node bezit het hele model.",
          "Plaatsing komt uit het **rank manifest** van de planner — voor Qwen3.5-122B worden 49 lagen verdeeld over welke mix van GPU, CPU, NPU en telefoon er ook opduikt.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Zwakke apparaten tot echte leden maken", text: "Deelshards, mobiele GPU's en de 443-relay" },
      {
        t: "ul",
        items: [
          "**Deelshard-download:** een ringstage heeft de checkpoint niet nodig — hij heeft zijn venster nodig. Een stage-mini-GGUF draagt alleen die tensoren (**254 MB met 26 tensoren** tegenover het volledige model van 77.6 GB), dus een telefoon haalt ~1.5 GB voor een éénlaagsvenster in plaats van alles.",
          "**Mobiel GPU-pad:** de RPC-route naar een telefoon-GPU bleek onhaalbaar (Adrenos OpenCL-bufferindeling overleeft RPC-serialisatie niet), maar een **ringstage draait direct op de Adreno-GPU** — de stage bezit zijn backend lokaal, dus alleen grenzen kruisen de draad.",
          "**NAT-traversal:** telefoons kunnen geen inkomende verbindingen accepteren, dus het datavlak loopt door een **443-relay** — een WebSocket-brug per edge met een 1-byte rol-preamble die beide uiteinden naar buiten laat bellen. De telefoon opent nul inkomende poorten.",
          "**Zelfinschrijvingsmarkt:** stages worden geclaimd, niet toegewezen. Een node pollt de dekkings-/vraagkaart, kiest het **hoogst belonende** ongedekte venster, downloadt dat venster en sluit aan — end-to-end geverifieerd met een telefoon achter NAT die ringinferentie voltooide en zijn bijdrage verdiende.",
        ],
      },
      { t: "h2", kick: "Waar de ring past", text: "Het lagelatentiepad" },
      {
        t: "p",
        md: "De ring is Kvasirs **latentie**-pad: grenzen zijn klein, hops zijn weinig, en decoderen stroomt rond de lus zonder iets centraal te verzamelen. Zijn beperking is granulariteit — de kleinste eenheid die een node kan dragen is een laag (~1.4 GB op de 122B). Die vloer wegnemen is wat de expertzwerm doet; de ring blijft de serveerruggegraat waar hij op aansluit.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — Binnenin een 122B-MoE: waarom de gewichten geshard willen worden",
    dek: "Een analyse op tensorniveau van Qwen3.5-122B: 86% van de bytes zijn 12.544 onafhankelijke expertplaten, elk één schone byte-range-kopie verwijderd van zelfstandigheid.",
    blocks: [
      {
        t: "p",
        md: "Voordat we iets ontwierpen, haalden we de 122B op schijf uit elkaar. De vraag: als een zwerm zwakke apparaten dit model moet dragen, wat is dan de natuurlijke draageenheid? Het antwoord viel uit de GGUF-tensorlayout zelf.",
      },
      { t: "h2", kick: "Anatomie · Qwen3.5-122B-A10B (Q4_K_M)", text: "Waar een MoE-laag werkelijk uit bestaat" },
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
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Elke laag splitst in een **dicht pad** — attention + KV, de norms, de router (`ffn_gate_inp`), een gedeelde expert — en een **expertbank**: 256 onafhankelijke FFN's opgeslagen als drie gestapelde tensoren (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Het dichte pad is de minderheid van de bytes; de expertbank is 86% van het model.",
      },
      { t: "h2", kick: "Het cadeau van de layout", text: "Experts zijn aaneengesloten, blok-uitgelijnde platen" },
      {
        t: "ul",
        items: [
          "De expertindex is de **buitenste ggml-dimensie** (`ne[2]`) van elke experttensor — expert *e* beslaat één aaneengesloten, kwantblok-uitgelijnde plaat rauwe gekwantiseerde bytes.",
          "Dat maakt extractie per expert een **byte-range-kopie**: `data[a:b]`, geen dekwantisatie, geen herverpakking — een expert-gesneden mini-GGUF is goedkoop te maken en bit-getrouw.",
          "Per token vuren slechts **8 van de 256** experts per laag, gekozen door de router — het expertverkeer van een laag bij decoderen is dus een handvol kleine matrixvermenigvuldigingen over één hidden-vector.",
        ],
      },
      { t: "h2", kick: "De implicatie", text: "De draageenheid daalt van 1.4 GB naar 5.3 MB" },
      {
        t: "p",
        md: "Op laagkorrel is het minste dat een node kan houden ~**1.4 GB** — buiten bereik van de meeste telefoons zodra app, KV en OS hun deel nemen. Op expertkorrel is de eenheid **5.3 MB**, en een realistische bijdrage is 8–64 experts (**42–340 MB**) — ruim binnen elk modern apparaat. De experts zijn onderling onafhankelijk, dus eigendom kan willekeurig verspreid en vrij herverdeeld worden. Deze analyse maakte sharding op expertniveau tot de ontwerpgok: de gewichten waren al verpakt in zwermformaat — het netwerk hoefde de verpakking alleen te eren.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — Backbone-expert-RAM-offload (M0)",
    dek: "Stream MoE-expert-FFN's uit CPU-RAM in plaats van VRAM, en één coördinator van 64 GB houdt een 122B vast — zonder graafchirurgie.",
    blocks: [
      {
        t: "p",
        md: "Expert-FFN's hoeven niet in VRAM te wonen. Ze uit CPU-RAM streamen laat één coördinator een model vasthouden waarvan de experts zijn VRAM overstijgen — het fundament dat zwakke nodes überhaupt aan een grote MoE laat deelnemen.",
      },
      { t: "h2", kick: "Planner geverifieerd · echte 122B-GGUF", text: "Een 122B past op één coördinator van 64 GB" },
      {
        t: "p",
        md: "Voorheen plaatste de ring gewichten alleen in VRAM, dus de 122B (77.6 GB) was **infeasible** op een 64-GB-GCD. Met expert-offload-regels komt de dry-run **feasible** terug:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "122B-ringplan" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (experts)" },
          { n: "10", l: "geoffloade lagen" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Planner-uitvoer — inferentie-engine -ot-regelformaat.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # inferentie-engine -ot format`,
      },
      { t: "h2", kick: "Wat er bedraad werd · puur Python, geen C++-rebuild", text: "De offload-regels van de planner naar een echte load dragen" },
      {
        t: "ul",
        items: [
          "**planner** — zendt al `ot` (komma-gevoegde `-ot`-regels) uit in elke plaatsing.",
          "**protocol.py** — veld `StageStartRequest.ot` toegevoegd.",
          "**runtime.py** — stuurt de `ot` van de plaatsing door naar het stage-verzoek.",
          "**stage_service.py** — de coördinator start met `--override-tensor`.",
          "`linkcpp-server` geeft onbekende argumenten door aan de standaard llama-server, dus `-ot` grijpt ongewijzigd aan.",
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
        md: "Geverifieerd met een `ot`-protocol-rondreistest plus de bevestiging dat het coördinatorcommando `--override-tensor` uitzendt; de hub herdeployde schoon zonder regressies. Wat toen restte: de volledige 2-node-77-GB-load (coördinator met offload + een telefoon met een ~1.5-GB-éénlaagsvenster), wachtend op serverbeschikbaarheid. De kern van M0 — de backbone-offload die zwakke nodes aan een grote MoE laat deelnemen — was op code- en plannerniveau compleet.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — Het expertplak-datapad (M1)",
    dek: "Een zwak apparaat downloadt een paar experts van 6 MB, geen laag van 1.4 GB — en gesharde berekening evenaart monolithisch tot 3.6e-12.",
    blocks: [
      { t: "h2", kick: "Geverifieerd · echte Qwen3.5-122B-A10B", text: "Een expertplak is een bytekopie — zonder dekwantisatie" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "plak in expertdim." },
          { n: "~6.1", l: "MB / expert (Q4+Q6)" },
          { n: "206 MB", l: "download 2 lagen × 16 exp." },
          { n: "200", l: "HTTP, geldige GGUF" },
        ],
      },
      {
        t: "p",
        md: "MoE-experttensoren stapelen alle experts langs de buitenste ggml-dimensie, dus de reader stelt `(n_expert, rows, row_bytes)` rauwe gekwantiseerde bytes bloot. Expert *e* is een kwantblok-uitgelijnde aaneengesloten plaat — de plak is letterlijk `data[a:b]`, zonder dekwantisatie en zonder herverpakking.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — de geverifieerde rondreis.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "Het numerieke orakel", text: "dispatch + combine == monolithisch, exact" },
      {
        t: "p",
        md: "Met echte layer-0-experts van de 122B (gedekwantiseerde referentie) evenaart het splitsen van de experts in 4 shards, elk afzonderlijk berekenen en combineren **de monolithische MoE-FFN**: sharding is een exacte hergroepering van dezelfde gewogen som, geen benadering.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "relatieve fout" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "aangeraakte experts" },
        ],
      },
      { t: "h2", kick: "C++-worker, hardware-geverifieerd", text: "linkcpp-expert-worker reproduceert het orakel op ROCm" },
      {
        t: "ul",
        items: [
          "**Puur ggml/gguf** (geen libllama): laadt de plak in een GPU-backend en draait `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`.",
          "**ROCm-build + run** op een MI250: 122B layer-0, experts [0,8), 4 tokens.",
          "**Cosine 0.99995 vs het orakel**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — dit residu is zelf het eerste gemeten geval van cross-backend-equivalentie (ROCm vs numpy).",
          "Hetzelfde codepad dekt CUDA/Metal/Vulkan/CPU (`mul_mat_id`/`swiglu` zijn standaard ggml; CUDA heeft een speciale MoE-kernel).",
        ],
      },
      {
        t: "p",
        md: "Het moeilijkste, riskantste stuk — de worker-kernel op het apparaat — werd hier geverifieerd. Wat restte was backbone↔worker-orkestratie; de worker is een bewezen pure functie die deze plakken consumeert.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — Gedistribueerde expert-dispatch (M2)",
    dek: "Een live 122B-decodering geeft de expertberekening van één laag via TCP aan een apart workerproces — en voorspelt exact hetzelfde token.",
    blocks: [
      { t: "h2", kick: "Geverifieerd · echte 122B, twee processen", text: "Backbone-decodering → TCP → worker → experts → hetzelfde token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "logit-cosine" },
          { n: "0", l: "transportverlies (byte-identical)" },
          { n: "2", l: "processen (backbone + worker)" },
        ],
      },
      {
        t: "p",
        md: "De expert-worker serveert de layer-0-plak als **apart proces** (ROCm), en de `build_moe_ffn`-dispatch-callback van de 122B-backbone verstuurt `(cur, sel)` over TCP en ontvangt de expertuitvoer. De logit-cosine is **exact de in-process-waarde** (0.99868775) — het transport is verliesloos. Expert-parallelle zwermberekening werkt over een procesgrens heen.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "Eén langlevende TCP-verbinding — dezelfde stream die ring/443-relay kunnen tunnelen.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Klaar", text: "De gedistribueerde dispatch-pipeline" },
      {
        t: "ul",
        items: [
          "`--serve`-modus: laad de plak, luister op TCP, beantwoord `(n_used, n_tokens, cur, sel) → experts`.",
          "`--dispatch-port`: de backbone-callback zendt/ontvangt via TCP naar een aparte worker, ter vervanging van in-process-berekening.",
          "Gemeten op een live 122B-decodering met layer-0 buiten het proces gedispatcht → **argmax MATCH**, cosine 0.99869 (= in-process, verliesloos).",
          "M2-kern (eerder): de ARM van de telefoon berekende echte 122B-experts op cosine 0.99992 (Android-crossbuild).",
        ],
      },
      {
        t: "p",
        md: "Hierna: dezelfde TCP-stream door de **443-relay** tunnelen naar workers op andere machines en telefoons (het transport is al bewezen in het ringwerk), dan de M3-dekkingsmarkt en M4-batchdoorvoer.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — De expertdekkingsmarkt (M3)",
    dek: "Zwakke nodes zien welk (laag, expertbereik) het schaarst en best betaald is, en vullen het zelf — de bewezen laagmarkt, fijner gekorreld.",
    blocks: [
      {
        t: "p",
        md: "Kvasirs laagshard-markt — vraagkaart, zelfinschrijving op maximale beloning, gedeeltelijke download, beloningen per node — was al apparaat-geverifieerd. M3 herparametriseert hetzelfde mechanisme op de korrel van **(laag, expertbereik)**, zodat de dekking zichzelf herstelt richting de meest ondergerepliceerde, best betaalde expertbereiken.",
      },
      { t: "h2", kick: "Geverifieerd · API", text: "Schaarste-aggregatie → toewijzing van het best belonende bereik" },
      {
        t: "p",
        md: "Drie workers registreren op laag 0: A = [0,128), B = [128,256), C = [0,128) als tweede replica, met `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["laag", "experts", "replica's", "schaarste"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (doel gehaald)"],
          ["0", "[128, 256)", "1", "0.5 (onder doel)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → snijdt het schaarste bereik bij op het budget van de node.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Klaar · puur Python (hub)", text: "Een vraag-/aanbodmarkt op expertkorrel" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — workers heartbeaten hun (laag, expertbereik)-bezit.",
          "`GET /api/expert-demand` — replica-aggregatie per expert → aaneengesloten expertbereik-segmenten met schaarste-scores.",
          "`POST /api/expert-volunteer` — wijst het schaarste bereik toe, bijgesneden op het budget van de node.",
          "De bestaande laagmarkt (zelfinschrijving · gedeeltelijke download · beloningen) geherparametriseerd naar (laag, expertbereik).",
        ],
      },
      {
        t: "p",
        md: "Wat volgt in M4: batch-dispatch voor gelijktijdige verzoeken plus een hot-expert-cache — tokens/s evenredig met het aantal workers — en replica-routering (dichtstbijzijnde/snelste worker) met churn-fallback.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — Batch-dispatch-doorvoer (M4)",
    dek: "De zwerm is een doorvoerweefsel, geen latentiespel: dispatch-aanroepen bundelen amortiseert de overhead per verzoek 77× per token.",
    blocks: [
      { t: "h2", kick: "Gemeten · ROCm, expert-FFN, n_used = 8", text: "Grotere batches, meer tok/s per worker" },
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
        md: "Van **1.45 ms/tok** bij batch 1 naar **0.019 ms/tok** bij batch 512 — een verbetering van 77× per token. De tijd per aanroep beweegt nauwelijks (1.45 → 9.6 ms) terwijl de batch 512× groeit — de GPU verwerkt de batch bijna gratis achter een vaste overhead. Dit is de **doorvoerweefsel-eigenschap** die expert-parallel praktisch maakt: batch-dispatch amortiseert RTT en overhead per verzoek.",
      },
      { t: "h2", kick: "Klaar", text: "Batch-dispatch-doorvoer" },
      {
        t: "ul",
        items: [
          "Worker `--bench`: compute_dispatch-timings voor batch 1…512 → tok/s.",
          "**53k tok/s per worker** bij batch 512 (ROCm) — bundelen amortiseert de overhead.",
          "Hierbovenop stapelen hot-expert-caching en multi-worker-aggregaatschaling (replica-routering).",
        ],
      },
      {
        t: "callout",
        md: "Met M4 is de **hele M0 → M4-pipeline gedemonstreerd op een echte 122B**: backbone-offload · expertplakken · geverifieerde workers · live decode-dispatch (argmax MATCH) · gedistribueerde processen · dekkingsmarkt · batchdoorvoer.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "Een telefoon deed mee aan 122B-inferentie",
    dek: "Een Galaxy S25 downloadde autonoom zijn expertplak van de hub en berekende bij elke stap van een live 122B-decodering de experts van één laag. De uitvoer was correct.",
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → gegenereerd (met de telefoon in de lus): **\" Paris.\"** — 8/8 tokens identiek aan de lokale run.",
      },
      { t: "h2", kick: "Gemeten · echte 122B, telefoon berekent layer-0", text: "Correctheid + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens identiek aan lokaal" },
          { n: "4.01", l: "TPS lokaal (basislijn)" },
          { n: "3.13", l: "TPS met telefoon" },
          { n: "1.58 GB", l: "autonome download" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Zelfs met de telefoon die voor elk token de layer-0-experts berekent, zijn **de gegenereerde tokens exact de lokale** — het juiste \"Paris.\". De TPS zakt van 4.01 naar 3.13 — de telefoon-dispatch-rondreis (MI250 → tunnel → telefoon, ~100 ms/token) kost 22%. De doorvoer komt terug met batching en replica's (M4).",
      },
      { t: "h2", kick: "De autonome deelnamestroom", text: "Ontdekken → beloningsgedreven download → meedoen aan de berekening" },
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
      { t: "h2", kick: "Geverifieerd vs resterend", text: "Het mechanisme is compleet; de in-app-lus is productisering" },
      {
        t: "ul",
        items: [
          "Gedeeltelijke download (het expert-shard-endpoint), worker-serving, backbone-dispatch, live 122B-generatie en TPS — allemaal geverifieerd op het echte apparaat.",
          "Correctheid: met de telefoon in de lus zijn 8/8 tokens gelijk aan de lokale run, met het juiste antwoord.",
          "Resterend: de autonome in-app-lus (expert-demand pollen → volunteer → downloaden → serve → registreren) is Kotlin-bedrading — deze demo dreef het mechanisme direct aan.",
          "Transport: deze demo gebruikte een SSH-tunnel; productie gebruikt de 443-relay (al geverifieerd in het ringwerk).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "De Kvasir-economie: een deugdzame cyclus van kosten en beloning",
    dek: "Een gedecentraliseerd inferentienetwerk werkt alleen als de prijs die consumenten betalen en de beloning die nodes verdienen elkaar versterken. Dit is het vliegwiel waar we naartoe bouwen, de spiralen die het om zeep helpen, en de drie invarianten die het draaiende houden.",
    blocks: [
      {
        t: "callout",
        md: "**These:** Kvasir is een tweezijdige markt die in één token wordt verrekend — consumenten betalen KVR om te infereren, nodes verdienen KVR om te serveren. Het hele ontwerp slaagt of faalt op één eigenschap: die twee kanten moeten een **deugdzame cyclus** vormen, waarbij elke omwenteling de volgende makkelijker maakt. Krijg dat verkeerd en elk prijsbeleid stort uiteindelijk in; krijg het goed en het netwerk wordt *goedkoper* naarmate het *groter* wordt.",
      },
      {
        t: "p",
        md: "Het is verleidelijk om kosten en beloning als een touwtrekwedstrijd te zien — elke euro die een consument bespaart, is een euro die een node niet verdient. Die framing is een valstrik. In een gezond netwerk zijn het **hetzelfde vliegwiel** vanuit twee kanten bekeken: betalingen worden beloningen, beloningen worden aanbod, aanbod wordt capaciteit en lagere prijzen, lagere prijzen worden meer gebruik, en meer gebruik wordt meer betalingen. De vraag is niet hoe je een vaste taart verdeelt; het is hoe je het wiel draaiende houdt zodat de taart groeit.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "Het vliegwiel", text: "Waarom gebruik en aanbod samen groeien" },
      {
        t: "p",
        md: "De motor van de cyclus is één regel die in Kvasir al waar is: **inferentie moet in KVR worden betaald**. Dat maakt elke eenheid gebruik een eenheid echte vraag naar het token — nut, geen speculatie. Tokenvraag ondersteunt de waarde van de KVR die nodes verdienen; aantrekkelijke beloningen trekken aanbod aan; aanbod vergroot de capaciteit en drijft, via concurrentie en fijnere expert-sharding, de marginale kosten van serveren omlaag; goedkopere, snellere, capabelere service trekt meer gebruik aan. Kvasir trekt de lus strakker aan met een eigenschap die geen enkele gecentraliseerde API kan kopiëren: een deelnemer kan **tegelijk consument en leverancier** zijn. De vraagkant en de aanbodkant groeien vaak binnen *dezelfde mensen*, wat de onevenwichtigheden dempt die eenzijdige markten kapotmaken.",
      },
      { t: "h2", kick: "De faalmodi", text: "Vier spiralen die het wiel achteruit laten draaien" },
      {
        t: "p",
        md: "Een vliegwiel kan net zo makkelijk afremmen als versnellen. De doodsspiralen benoemen is hoe je ertegen ontwerpt:",
      },
      {
        t: "table",
        head: ["Spiraal", "Hoe het begint", "Waar het eindigt"],
        rows: [
          ["Beloningsverwatering", "Meer nodes jagen op vlakke vraag", "Beloning per node daalt, nodes vertrekken, capaciteit zakt"],
          ["Prijs-te-laag", "Goedkope prijs, beloningen onder nodekosten", "Serveren levert niets meer op, aanbod en kwaliteit storten in"],
          ["Prijs-te-hoog", "Goede beloningen, maar boven de markt", "Gebruikers kiezen een goedkopere API, inkomsten drogen op"],
          ["Emissieafhankelijkheid", "Beloningen betaald door bijmunten, niet uit inkomsten", "Inflatie holt KVR uit tot beide kanten opgeven"],
        ],
      },
      { t: "h2", kick: "De invarianten", text: "Drie regels die de cyclus deugdzaam houden" },
      {
        t: "ul",
        items: [
          "**Beloningen worden gefinancierd uit echte inkomsten.** In de stabiele toestand komt wat nodes verdienen uit wat consumenten betalen — niet uit onbegrensde token-emissie. Emissie is een opstartsubsidie die moet *afbouwen* naarmate de vergoedingsinkomsten groeien. Kvasir helpt hier al door **echt werk** te belonen — KVR per daadwerkelijk geserveerde tokens × laagaandeel, niet louter aanwezigheid — zodat subsidie niet kan weglekken naar inactieve 'huurlingen'-nodes.",
          "**KVR is het verplichte medium.** Omdat je niet kunt infereren zonder KVR te betalen, is gebruik een permanente vraagput voor het token. Dat verankert de tokenwaarde aan echt nut in plaats van speculatie — het verschil tussen een munt en een fiche.",
          "**De prijs zweeft binnen een band.** Een ondergrens boven de marginale kosten van een node houdt serveren de moeite waard; een bovengrens onder gecentraliseerde alternatieven houdt Kvasir concurrerend. Daartussen beweegt de prijs — en dat is waar de groei van het netwerk zich eindelijk als lagere kosten toont.",
        ],
      },
      { t: "h2", kick: "De thermostaat", text: "\"meer nodes → goedkoper\" waar maken in code" },
      {
        t: "p",
        md: "Vandaag is de prijs een bestuurde constante — verstandig voor een devnet, maar het betekent dat nodes toevoegen de *capaciteit* verhoogt, niet de betaalbaarheid. De ontwerprichting is een **gebruiksgedreven prijs**: ongebruikt aanbod duwt de prijs omlaag richting de ondergrens, congestie duwt hem omhoog richting de bovengrens. Dat ene signaal verandert de intuïtie *\"hoe meer mensen rekenkracht delen, hoe goedkoper het wordt\"* in een regel die het protocol afdwingt — terwijl de ondergrens operators solvabel houdt zodat het aanbod dat het goedkoop maakte niet verdampt. Omdat de prijs een gevoelige economische parameter is, wijzigt hij alleen onder **genesis-wallet-autoriteit met wallet-handtekening + 2FA**, nooit via een verdwaalde omgevingsvariabele.",
      },
      {
        t: "callout",
        md: "**\"Gratis\" is het netto, niet de prijs.** Je betaalt voor wat je infereert en verdient voor wat je serveert; draag ongeveer evenveel bij als je verbruikt en je rekening komt netto op nul uit. Geen enkele abonnements-API — Claude Max, een Codex-plek — kan dat bieden, want je kunt nooit hun aanbodkant zijn. Met Kvasir kun je modellen draaien die je eigen machine niet kan vasthouden *én* betaald krijgen voor het helpen draaien van die van anderen.",
      },
      {
        t: "p",
        md: "Niets hiervan vereist exotisch mechanismeontwerp. Het vereist discipline op drie punten: beloning uit inkomsten, waarde uit gebruik, balans uit een begrensde zwevende prijs. Kvasir levert al de moeilijke, eerlijke delen — non-custodial verrekening, werk-evenredige beloningen, een token dat je daadwerkelijk moet uitgeven om het netwerk te gebruiken. De rest is de economische routekaart: de afbouw, de vergoedingssplitsing die een verzekeringspool voor mislukte inferenties financiert, en de thermostaat. In die volgorde gebouwd, houden kosten en beloning op met vechten en beginnen ze elkaar te versterken.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "Een GPU aan de andere kant van het internet deed mee aan 122B-inferentie",
    dek: "Een Blackwell-werkstation in een andere stad belde één uitgaande 443-verbinding en berekende experts voor een live 122B-decodering — byte-identiek aan een lokale run, en in KVR betaald voor het werk dat het deed.",
    blocks: [
      {
        t: "callout",
        md: "**Wat er gebeurde:** een 122B-decodering die op een AMD-backbone op de ene plek draaide, stuurde zijn expertwerk per token naar een NVIDIA GB10 (Grace Blackwell)-machine in een andere stad — over één uitgaande WebSocket op poort 443 — en kreeg expertuitvoer terug die **exact dezelfde tokens** opleverde als lokaal berekenen. Geen tunnel, geen port-forwarding, geen inkomend firewallgat. De externe machine verdiende KVR voor de bytes die het serveerde.",
      },
      {
        t: "p",
        md: "De premisse van Kvasir is *welke hardware er ook opduikt* — inclusief hardware achter carrier-NAT, op het publieke internet, in een andere stad. Qwen3.5-122B-A10B draagt **86% van zijn gewicht in 12.544 onafhankelijke experts** (48 lagen × 256, top-8), elk een pure functie van 5.3 MB. Die korrel is wat een verre, ongerelateerde machine een plak laat vasthouden en laat bijdragen. De open vraag was nooit *kunnen we het splitsen* — het was *kan een worker aan de andere kant van het open internet werkelijk meedoen aan een live decodering, correct en verantwoord*. Nu is dat gebeurd.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "Eén uitgaande verbinding", text: "Geen tunnel, geen inkomende poorten" },
      {
        t: "p",
        md: "De externe worker opent **één** verbinding — uitgaand `wss://` naar de publieke gateway op 443, de enige poort die carrier-NAT en CDN-edges betrouwbaar doorlaten. De gateway parset de stream niet; hij **raw-splicet** de WebSocket naar de LAN-only hub, die hem doorbrugt naar de expert-dispatch-listener van de backbone. Beide uiteinden belden naar buiten en ontmoetten elkaar in het midden. De worker stelt nul inkomende poorten bloot en heeft geen publiek adres nodig.",
      },
      {
        t: "code",
        caption: "Twee uitgaande verbindingen, gesplicet tot één gewone dispatch-stream.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Byte-identiek over het internet", text: "De router beslist één keer; de wiskunde hergroepeert exact" },
      {
        t: "p",
        md: "De backbone draait de router **één keer** en met autoriteit; de worker is een pure functie `(hidden, ids) → out`. Die functie over een continent verplaatsen verandert dus *waar* de vermenigvuldiging gebeurt, niet *wat* ze berekent. Op een live 122B-decodering met layer-0-experts op afstand geserveerd: de greedy tokenstroom was **8/8 identiek** (\" Paris.\"), logit **cosine 0.99773**, argmax-match. Dit is dezelfde router-autoriteitseigenschap die de discrete beslissingen van CUDA↔ROCm↔CPU invariant houdt — heterogene backends blijven begrensd door continue fout, nooit een catastrofale vertakking.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "greedy tokens identiek" },
          { n: "0.99773", l: "logit-cosine, extern vs lokaal" },
          { n: "1.2%", l: "TPS-overhead, direct (13 ms RTT)" },
          { n: "0.00895", l: "KVR naar de worker, eerste externe sessie" },
        ],
      },
      { t: "h2", kick: "De eerlijke kost is RTT", text: "Waarom de zwerm een doorvoerweefsel is, geen lagelatentie-decoder" },
      {
        t: "p",
        md: "Seriële dispatch per token betaalt een rondreis per stap. Gemeten: met een directe verbinding (13 ms RTT) was de doorvoer-overhead **1.2%** (4.220 → 4.169 tok/s); gerouteerd door een CDN-edge op 443 was het **~28%**. Dat publiceren we eerlijk, want het wijst op de ontwerpwaarheid — een WAN-zwerm is **RTT-gebonden**, dus zijn kracht is niet de latentie van één stream maar de **totale capaciteit**. Batching amortiseert de rondreis: batch-expert-dispatch bereikt **77× de doorvoer per token** bij batch 512. Bytes zijn speelruimte; rondreizen zijn wat je moet verbergen — het onderwerp van de begeleidende routekaartpost.",
      },
      { t: "h2", kick: "Betaald voor precies het werk", text: "Gemeten bytes worden KVR" },
      {
        t: "p",
        md: "Deelname is waardeloos als ze niet verantwoord is. De relay **meet de gebridgede bytes per sessie** in het contributieregister van de hub; de gateway pollt dat register en schrijft delta-gewijs KVR bij op de **eigen** wallet van de worker — non-custodial, zoals al het andere. De eerste cross-internet-sessie leverde daadwerkelijk op: **1.28 MB werk → 1.277952 units → 0.00895 KVR** aan openstaande beloningen. Klein, en dat is het punt — het is echte verrekening per werk, geen deelnametrofee.",
      },
      {
        t: "p",
        md: "Precies hetzelfde uitgaande-443-pad is hoe een **telefoon** aansluit: een Galaxy S25 heeft er al 122B-experts over berekend (8/8 identieke tokens, cosine 0.99992). Een model op frontier-schaal, geserveerd door een backbone op de ene plek, een datacenter-GPU in een andere stad, en een telefoon in iemands zak — allemaal dezelfde tokens producerend, elk betaald voor zijn aandeel. Wat nu volgt is de WAN-rondreis goedkoop maken; die routekaart is gefundeerd op de productiecijfers van anderen en onze eigen metingen.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "WAN-dispatch goedkoop maken: een gefundeerde routekaart",
    dek: "Externe expert-dispatch werkt en is byte-identiek — maar een WAN-decodering is rondreis-gebonden. Dit is het plan om de kosten te drukken, gefundeerd op productiecijfers van DeepSeek, Petals en anderen (routekaart, niet geleverd).",
    blocks: [
      {
        t: "callout",
        md: "**Kader:** de cijfers *die wij gemeten hebben* worden als gemeten gepresenteerd; alles wat als plan wordt beschreven is een **routekaart**, geen geleverd resultaat. Het doel is externe expert-dispatch — al correct en betaald (zie de begeleidende post) — te nemen en de WAN-rondreis goedkoop genoeg te maken zodat een verre GPU of een telefoon een eersteklas zwermlid is, geen traag lid.",
      },
      {
        t: "p",
        md: "Onze eigen meting, gewoon gepubliceerd: dispatch kost ongeveer **110 KB per token per laag** — 12.3 KB eruit (dispatch) plus 98.3 KB terug (combine). De asymmetrie van 8× komt doordat elke geselecteerde expert zijn volledige uitvoer teruggeeft *vóór* de gewogen som. Direct verbonden is dat een doorvoer-overhead van **1.2%**; via een CDN-relay **~28%**. Dat zijn de feiten. De rest van deze post gaat over hoe we het gat willen dichten — en waarom bytes het makkelijke deel zijn.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "De dominante wet", text: "Een WAN-decodering is rondreis-gebonden" },
      {
        t: "p",
        md: "Het allerbelangrijkste gepubliceerde resultaat hier is niet van ons — het is dat van Petals: als de RTT van <5 ms naar 100 ms gaat, zakt het decoderen van **1.24 naar 0.57 stappen/s**, terwijl een **bandbreedtereductie van 10× het met ~0 verandert**. Latentie domineert; bandbreedte is speling. Dat herformuleert het hele probleem: bytes afknabbelen is speelruimte, maar **rondreizen wegsnijden is de kern**. Elk item hieronder is gerangschikt naar hoeveel rondreis-kosten het wegneemt.",
      },
      { t: "h2", kick: "Goedkoper op de draad", text: "Bytereductie met nauwkeurigheid voorop" },
      {
        t: "ul",
        items: [
          "**Retourneer gewogen deelsommen, geen rauwe expertuitvoer.** Door lineariteit is de combine van de backbone hoe dan ook exact, maar de worker retourneert één opgetelde vector in plaats van 8 — dat is de ~8×-combine-reductie, en precies wat DeepSeek-V3 / DeepEP in productie doen.",
          "**Parallelle ster, geen seriële keten** over meerdere workers: ΣRTT valt samen tot max RTT.",
          "**F16 op de draad** — we accepteren al een cross-backend-cosine van ~0.998, dus F16-transport valt binnen de bestaande tolerantie; **blockwise INT8/FP8 later**, nadat onze eigen argmax/cosine-poort het tegen Q4_K_M-gewichten heeft goedgekeurd (Petals liet INT8 over het echte internet zien zonder kwaliteitsverlies).",
          "Samen mikken deze op **~110 KB → 9–12 KB per token (~12×)** — reëel, maar onthoud dat het de *speelruimte* is, niet het knelpunt.",
        ],
      },
      { t: "h2", kick: "De rondreis amortiseren", text: "De kern: minder reizen, verborgen reizen" },
      {
        t: "ul",
        items: [
          "**Speculatief decoderen** verandert veel tokens in één rondreis. Bij een gemeten WAN van 80 ms ligt het break-even op slechts **~1.15–1.2 geaccepteerde tokens/stap** — dus zelfs een zwakke n-gram-gok wint (vanilla Jacobi kan averechts werken; techniekkeuze doet ertoe). Ons dispatch-protocol draagt al `n_tokens > 1`, dus er is geen wijziging op de draad nodig.",
          "**Continue batching bij de gateway** vouwt gelijktijdige verzoeken in één reis; **slot-affiniteit-prefix-caching** houdt een sessie op dezelfde replica's.",
          "**Latentie verbergen**: de gedeelde expert is een onafhankelijke additieve term, dus de backbone berekent hem *lokaal* tijdens de externe rondreis (ScMoE rapporteert 1.82× over PCIe, zonder hertrainen). Houd **hot experts lokaal**, stuur alleen koude op afstand (EPLB repliceert de ~32 heetste voor een decodeerversnelling van 2.54× in productie).",
        ],
      },
      { t: "h2", kick: "Beleid & de dikke-pijp-toekomst", text: "Routeer per peer, en wat 200 Gb/s verandert" },
      {
        t: "p",
        md: "Padbeleid: publiek routeerbare peers nemen het **directe** pad (de 1.2%-route); de relay is alleen voor NAT-gebonden apparaten. En als brede 200-Gb/s-verbindingen arriveren, serialiseert de 110 KB in **~4.4 µs** — de bandbreedteterm verdwijnt nog vóór de bovenstaande reducties, en een plak van 794 MB wordt in ~32 ms verstuurd. Maar **RTT is fysica; hij krimpt niet** — dus speculatief decoderen en overlap blijven de echte hefbomen, zelfs bij 200 G. Waar de dikke pijp werkelijk toe doet, is multi-backbone-federatie (meerdere backbones die één expert-pool delen) en bandbreedtegebonden werk: prefill van lange prompts en grote-batch-doorvoer.",
      },
      {
        t: "callout",
        md: "**Eén voorbehoud, eerlijk gesteld:** de transportlaag zelf (WebSocket vs QUIC, masking-overhead, NAT-hole-punching) heeft **geen extern resultaat dat we kunnen citeren** — dat is engineering die we zelf gaan meten voordat we iets beweren. Alles hierboven rust op gepubliceerde productiecijfers (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) plus onze eigen metingen; wanneer een routekaartitem wordt geleverd, worden de cijfers en werkwoordstijd hier bijgewerkt.",
      },
      { t: "h2", kick: "Wat nu volgt", text: "Onboarding-doelen" },
      {
        t: "p",
        md: "Onze dispatch-hook zit op `build_moe_ffn` — **één functie die 43 MoE-architecturen delen** in de inferentie-engine. Drie invarianten zijn modelonafhankelijk: de MoE-wiskunde (routed = Σ wᵢ·Eᵢ(x), lineair), het gedeelde codepad, en GGUF's standaard gestapelde experttensoren (buitenste `ne[2]` → blok-uitgelijnd snijden). Een nieuw model onboarden is dus geen herontwerp — het is één passage door een argmax/cosine-verificatiepoort per model.",
      },
      {
        t: "table",
        head: ["model", "experts · routering", "per expert (Q4≈)", "gedeeld", "status"],
        rows: [
          ["Qwen3.5-122B (vandaag in bedrijf)", "256 · top-8", "5.3 MB (gemeten)", "ja", "in productie"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "ja", "klaar — eerste kandidaat"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "ja", "klaar (hook geverifieerd)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "nee", "klaar (hook geverifieerd)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "ja", "klaar (deepseek2-graaf)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "ja", "klaar (deepseek-familie)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "nee", "klaar"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "nee", "klaar"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "ja", "klaar (MoE om de laag)"],
          ["MiniMax M3 428B", "128 · top-4", "TBD (GGUF)", "ja", "wacht op upstream-engine"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "nee", "werkt — alleen GPU-workers"],
        ],
      },
      {
        t: "p",
        md: "De industrie convergeert naar fijnkorrelige MoE — kleinere experts, meer ervan, hogere sparsity (DeepSeek, Qwen, Kimi, GLM, gpt-oss gingen allemaal deze kant op). Elke stap in die richting maakt de deelname-eenheid van de zwerm kleiner en de korrel van de schaarstemarkt fijner. De bovenstaande modellen zijn geen verlanglijst; elk stroomt al door dezelfde dispatch-hook die we in productie draaien — onboarding is een verificatiepoort, geen technisch project.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "De 200G-vraag",
    dek: "Onze zwerm-hubs kunnen nu al met 200 Gb/s verbinden met onderdelen uit het schap — één zit ingebouwd in de GB10. Dit is wat een dikke pijp een gedistribueerde MoE oplevert, en het ene ding dat het niet kan.",
    blocks: [
      {
        t: "callout",
        md: "**De premisse:** WAN-decodering is RTT-gebonden, niet bandbreedtegebonden — onze comm-routekaart liet zien dat bytes het makkelijke deel zijn. Dus wat verandert er werkelijk als hubs 200-Gb/s-verbindingen krijgen? Bijna alles aan *capaciteit*, en bijna niets aan *latentie*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Al in de doos · ConnectX-7", text: "De hardware is niet futuristisch — één zit in onze GB10-worker" },
      {
        t: "p",
        md: "De GB10 Grace Blackwell die onze 122B-experts berekent, draagt aan boord een **NVIDIA ConnectX-7 met twee 200-GbE-QSFP-poorten**. Twee van deze machines verbinden direct met één QSFP56-DAC-kabel van ~$100 — een 200G-cluster van twee hubs zonder switches. ARM is hier een eersteklas burger: dezelfde `mlx5`-driverstack die deze NIC's in x86-datacenters draait, draait ze op aarch64, wat precies is wat de GB10 is.",
      },
      {
        t: "callout",
        md: "**De kleine lettertjes:** de GB10 voedt zijn ConnectX-7 via twee PCIe-Gen5-x4-verbindingen in multi-host-modus. Gemeten volle snelheid (~185–190 Gb/s) vereist **RoCE (RDMA) en een correct gemapte topologie** — naïeve TCP over een verkeerd gemapt pad landt op ~95 Gb/s of slechter. Dikke pijpen worden gekocht met configuratie, niet alleen met kabels.",
      },
      { t: "h2", kick: "De afstandsladder", text: "200G is een catalogusartikel op elke afstand" },
      {
        t: "table",
        head: ["afstand", "onderdeel", "vormfactor"],
        rows: [
          ["rack (0.5–3 m)", "QSFP56-DAC-koper", "kabel, ~$100"],
          ["kamer (~30 m)", "AOC actief optisch", "kabel"],
          ["campus (2–10 km)", "200G FR4-/LR4-optiek", "QSFP56-module"],
          ["metro (~40 km)", "200G ER4-optiek", "QSFP56-module"],
          ["regio (~120 km)", "400G ZR+ coherent, gedraaid op 200G-lijnsnelheid", "QSFP-DD-module"],
          ["long-haul (100en km)", "carrier-200G-golflengte / DWDM-lijnsysteem", "geleaste dienst"],
        ],
      },
      {
        t: "p",
        md: "In het WAN is de *kabel* gewoon standaard single-mode glasvezel — snelheidsneutraal glas dat al elke stad overspant. De snelheid zit in de pluggable optiek aan elk uiteinde, en **OpenZR+ maakte 200G-over-120 km een module die je in een switch plugt**, geen telecomproject. Daarbuiten least je een golflengte.",
      },
      { t: "h2", kick: "Wat het oplevert", text: "Elke bandbreedteterm in de zwerm verdwijnt" },
      {
        t: "ul",
        items: [
          "Een dispatch-payload (~110 KB/token/laag vandaag, ~10 KB na de wire-routekaart) serialiseert in **microseconden** — payloadgrootte houdt volledig op een ontwerpbeperking te zijn.",
          "Een **expertplak wordt in ~32 ms verstuurd** (794 MB, theoretisch) en een heel 122B-model synchroniseert in **~3 s** — herbalancering van de dekkingsmarkt en onboarding van nieuwe hubs worden bijna-direct.",
          "Prefill van lange context — de enige echt bandbreedtezware fase — beweegt op wire speed, dus de tijd tot het eerste token op 100K-token-prompts wordt backbone-rekengebonden.",
          "**Batch-dispatch schaalt zonder wire-plafond**: expert-pool-verkeer geaggregeerd over vele gebruikersstromen is precies de bandbreedtezware, latentietolerante belasting die een dikke pijp opslokt. Dit is wat een multi-backbone-federatie — meerdere hubs, elk met KV voor hun eigen gebruikers, die één expert-pool delen — praktisch maakt.",
        ],
      },
      { t: "h2", kick: "Wat het niet kan kopen", text: "Licht haast zich niet" },
      {
        t: "p",
        md: "Glasvezel draagt licht met ~5 µs/km, en geen hoeveelheid bandbreedte verandert dat. Een rondreis van 13 ms is 13 ms bij 200 Gb/s. Autoregressief decoderen betaalt die rondreis per gesharde laag, per token — daarom blijven **speculatief decoderen (k tokens per rondreis) en shared-expert-overlap (rekenen terwijl de dispatch onderweg is) essentieel**, zelfs tussen hubs verbonden door de dikste pijp op de markt. Bandbreedte koopt doorvoer; alleen rondreis-discipline koopt latentie.",
      },
      {
        t: "p",
        md: "Zo bezinkt de architectuur in twee lagen. Een **hublaag** — backbones en hot experts verbonden door 200G-klasse-links, waar de capaciteit feitelijk onbegrensd is — en een **edgelaag** — telefoons en kleine apparaten op de 443-relay, die de lange staart van experts dragen die de schaarstemarkt hun toewijst. De dikke pijp laat de eerste laag als één machine aanvoelen; de relay houdt de tweede laag open voor iedereen. Geen van beide vervangt de ander: die splitsing *is* het ontwerp.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "De zwerm die groeit onder belasting",
    dek: "Een reusachtig model dat alleen hulp inroept wanneer het die nodig heeft — de MoE-zwerm schaalt zichzelf nu naar het verkeer: strak en snel bij rust, breed en parallel bij drukte.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir serveert modellen die veel groter zijn dan enige afzonderlijke machine kan bevatten — een Mixture-of-Experts-model van 122B parameters draait over een coördinator plus een zwerm workers: GPU's op het LAN, GPU's over een 200 Gb/s-link, zelfs telefoons die over het internet inbellen. Omdat een MoE-model elk token naar slechts een handvol van zijn experts routeert, ligt het merendeel van de gewichten op elk moment stil, en die inactieve experts kunnen **buiten** de hoofdnode leven — op welke hardware zich ook aanmeldde om ze vast te houden.",
      },
      {
        t: "callout",
        md: "Het nieuwe is dat de zwerm zichzelf nu **naar de belasting schaalt**.",
      },
      { t: "h2", kick: "Hoe hij zich gedraagt", text: "Strak bij rust, breed bij drukte" },
      {
        t: "p",
        md: "Wanneer het verkeer licht is, serveert de coördinator alles op zijn eigen GPU — het snelste pad per token, geen netwerkhops. Wanneer verzoeken zich beginnen op te stapelen en zijn inferentieslots verzadigd raken, gebeuren er automatisch twee dingen:",
      },
      {
        t: "ul",
        items: [
          "**Hij schakelt de workers die hij al heeft opnieuw in.** De coördinator bewaakt zijn eigen wachtrij. Onder verzadiging blijft hij gerouteerd-expertwerk uitsturen naar **bewezen** workers — die daadwerkelijk eerder hebben geserveerd — waarbij hij een beetje latentie per token inruilt voor veel meer totale doorvoer. Een worker die alleen verbinding maakte maar nooit rekende, wordt nooit met belasting vertrouwd; een gloednieuwe worker krijgt nog steeds een eerlijke eerste kans.",
          "**De hub werft nieuwe.** De control-hub merkt dezelfde verzadiging op en verhoogt de \"vraag\" naar de experts van dat model. Inactieve nodes — een telefoon in iemands zak, een reserve-GPU aan de andere kant van de stad — pollen die vraagmarkt al. Zodra de vraag stijgt, krijgen ze een plak experts aangeboden om te serveren, downloaden die, bellen in en sluiten aan. Wanneer de piek voorbij is, zakt de vraag terug en vallen de extra workers stilletjes weg.",
        ],
      },
      {
        t: "p",
        md: "Niemand plant dit. Geen node wordt geduwd. De zwerm ademt met de belasting mee: strak en snel bij rust, breed en parallel bij drukte — en het werkt zelfs voor nodes achter thuisrouters, omdat alles pull-gebaseerd is.",
      },
      {
        t: "p",
        md: "Dat is de vorm van een netwerk dat modellen met biljoenen parameters kan serveren op hardware die niemand alleen bezit: inactieve capaciteit wordt precies dan uitgenodigd wanneer dat de moeite waard is, en alleen dan.",
      },
    ],
  },
};
