/* Nederlands — mirror van de Engelse brondictionary (en.ts). Zelfde structuur:
   dezelfde keys, nesting, arraylengtes en volgorde; alleen de tekstwaarden zijn
   vertaald. Technische termen en identifiers blijven verbatim (KVR, linkcpp,
   inferentie-engine, GPU, CPU, NPU, OpenAI, Anthropic, Solana, GGUF, MoE, SIWS, 2FA,
   TOTP, ring runtime, MIT, tok/s, layer, Qwen3.5-122B, etc.). De eerlijkheids-
   framing blijft intact (devnet, utility-token, geen investering, non-custodial). */
import type { Dict } from "./types";

export const nl: Dict = {
  nav: {
    why: "Waarom",
    how: "Hoe het werkt",
    contributors: "Bijdragers",
    developers: "Ontwikkelaars",
    token: "KVR",
    rewards: "Beloningen",
    tech: "Techniek",
    roadmap: "Roadmap",
    careers: "Vacatures",
    technology: "Technologie",
    blog: "Blog",
    wiki: "Wiki",
  },

  actions: {
    runNode: "Node draaien",
    joinNode: "Meedoen",
    useApi: "Gebruik de API",
    getApiKey: "Vraag een API-sleutel aan",
    getApiAccess: "Vraag API-toegang aan",
    runNodeGuide: "Handleiding: node draaien",
    readDocs: "Lees de documentatie",
    viewGithub: "Bekijk op GitHub",
    github: "GitHub",
    menu: "Menu",
    copy: "kopiëren",
    copied: "gekopieerd ✓",
    language: "Taal",
  },

  hero: {
    eyebrow: "DePIN · Gedecentraliseerde AI — voorbij het monopolie",
    headline1: "Lever rekenkracht.",
    headline2: "Verdien KVR.",
    sub: "Kvasir verdeelt grote open modellen over gedeelde hardware met linkcpp, zodat geen enkele node het hele model bevat. Draag een GPU, CPU, NPU — zelfs een telefoon — bij en verdien KVR voor de layers die je draait.",
    badges: [
      "Draait op GPU · CPU · NPU · telefoon",
      "OpenAI + Anthropic compatibel",
      "Broncode beschikbaar (BSL)",
      "Solana devnet",
    ],
    ringCenter: "één ring · geen master",
    topologyCaption:
      "Een ring van apparaten — een GPU, CPU, NPU en telefoon — die elk een paar van de 49 layers bevatten. Elke node draait zijn deel en geeft alleen de hidden-state-grens door aan zijn buur; de laatste stuurt de token via de ring terug. Geen enkele node bevat het hele model, en er is geen centrale master — ter illustratie.",
  },

  thesis: {
    eyebrow: "Waarom gedecentraliseerde AI",
    title: "AI hoort niet in handen te zijn van een handvol bedrijven",
    lede: "Frontier-inferentie concentreert zich achter een paar afgeschermde datacenters — gesloten gewichten, afgemeten toegang, één rekening betaald aan één eigenaar. Kvasir wijst de andere kant op: open modellen bediend via een permissionless netwerk van alledaagse apparaten, in bezit van en verdiend door de mensen die het draaien.",
    centralizedLabel: "Gecentraliseerde AI",
    centralizedPoints: [
      "Een paar hyperscalers bezitten de GPU’s",
      "Modellen en infrastructuur achter gesloten API’s",
      "Je huurt toegang; waarde stroomt naar boven",
      "Ondoorzichtig — je vertrouwt op de operator",
    ],
    kvasirLabel: "Kvasir",
    kvasirPoints: [
      "Elk apparaat sluit zich aan bij een peer-to-peer-ring — geen centrale master",
      "linkcpp-engine met beschikbare broncode — BSL-gelicentieerd en volledig inspecteerbaar",
      "Bijdragers verdienen KVR voor de echte rekenkracht die ze leveren",
      "Non-custodial — jouw sleutels, jouw node, jouw beloningen",
    ],
  },

  origin: {
    eyebrow: "De naam",
    title: "Kvasir — wijsheid geboren uit velen, gedeeld met allen",
    mythLabel: "Noordse mythe",
    myth: "In de Noordse mythologie werd Kvasir geboren toen de Æsir- en Vanir-goden hun oorlog beëindigden en vrede sloten: elk spuwde in één vat, en uit hun samengevoegde essentie ontstond het wijste wezen dat ooit leefde — een wezen dat elke vraag kon beantwoorden. Toen hij werd gedood, werd zijn bloed gebrouwen tot de Mede der poëzie, een drank die iedereen die ervan dronk wijsheid schonk.",
    whyLabel: "Waarom we hem kozen",
    mappings: [
      {
        from: "Samengebracht uit elke god, van niemand",
        to: "Intelligentie samengesteld uit de apparaten van vele bijdragers — geen enkele eigenaar.",
      },
      {
        from: "Het wijste wezen, dat elke vraag beantwoordt",
        to: "Een open inferentienetwerk dat iedereen kan bevragen.",
      },
      {
        from: "Een mede die wijsheid met allen deelde",
        to: "Open toegang, en KVR-beloningen voor elke bijdrager die meeschenkt.",
      },
    ],
    footnote: "On-chain heet de token Kvasir (KVR) — de mede, gedistribueerd.",
  },

  how: {
    eyebrow: "Hoe het werkt",
    title: "Eén model, veel apparaten, betaald per layer",
    lede: "Geen enkele node bevat het hele model. Een verzoek stroomt over het layer-pad en elke node wordt beloond voor precies het werk dat het deed.",
    steps: [
      {
        title: "Splitsen",
        body: "Het model wordt verdeeld in aaneengesloten layer-vensters. Elk apparaat slaat hetzelfde model op, maar laadt alleen zijn eigen venster — geen enkele node bevat het geheel.",
        note: "Qwen3.5-122B · 49 layers · rank manifest",
      },
      {
        title: "Bedienen",
        body: "Het verzoek komt de ring binnen. Elke node draait zijn layers en geeft alleen de hidden-state-grens door aan zijn buur; de laatste node sampelt de token en stuurt hem via de ring terug — geen centrale master.",
        note: "ring runtime · OpenAI/Anthropic compatibel",
      },
      {
        title: "Belonen",
        body: "Elke node verdient KVR gewogen naar de layers die het draaide — zijn aandeel in de geproduceerde tokens — afgerekend op Solana naar de eigen wallet van die node.",
        note: "units += (out_tokens / 1000) × layer_share",
      },
    ],
  },

  contributors: {
    pill: "Voor bijdragers",
    title: "Zet ongebruikte rekenkracht om in KVR",
    lede: "Richt een ondersteund apparaat op het netwerk en het begint layers te bedienen. Je verdient KVR in verhouding tot de layers die je node draait — je sleutels blijven in je eigen wallet.",
    nonCustodial:
      "Non-custodial by design — de operator-login is een wallet-handtekening (Sign-In With Solana) met optionele 2FA.",
    points: [
      {
        title: "Elk apparaat kan meedoen",
        body: "GPU’s, CPU’s, NPU’s en telefoons draaien vandaag allemaal layers. Een peer-to-peer ring runtime laat elk apparaat slechts een paar layers bevatten en alleen kleine grenstoestand doorgeven aan zijn buur — dus geen poortwachter, en geen enkele eigenaar.",
      },
      {
        title: "Beloningen naar layer-aandeel",
        body: "Beloningen zijn evenredig aan de layers die je node draait, niet aan vage deelname. 1 unit ≈ 1k tokens × jouw layer-aandeel van elke inferentie.",
      },
      {
        title: "Prestatieklassen",
        body: "Gemeten doorvoer bepaalt een klasse — S (×1.5), A (×1.25), B (×1.0), C (×0.7) — die je verdiende units vermenigvuldigt. Snellere hardware, hogere vermenigvuldiger.",
      },
      {
        title: "Uptime voor infra-rollen",
        body: "Nodes die infrastructuurrollen vervullen bouwen ook uurlijkse uptime-beloningen op voor het bereikbaar houden van het netwerk.",
      },
    ],
    devicesLabel: "Ondersteunde apparaten",
    statusLive: "Live",
    statusComing: "Binnenkort",
    deviceDetails: [
      "CUDA · ROCm · Metal · Vulkan",
      "x86-64 · ARM",
      "versnellers op het apparaat",
      "telefoons & edge · ring runtime",
    ],
  },

  developers: {
    pill: "Voor ontwikkelaars",
    title: "Eén endpoint, ondersteund door veel apparaten",
    lede: "Behoud je bestaande OpenAI- of Anthropic-client. Richt hem op de Kvasir-gateway en betaal per inferentie in KVR — zonder te herschrijven.",
    points: [
      "OpenAI-compatibel: drop-in voor /v1/chat/completions, /v1/responses, /v1/models",
      "Anthropic-compatibel: /anthropic/v1/messages en /anthropic/v1/models",
      "Betalen per inferentie in KVR: prijsopgave → betaling → inferentie",
      "Live modelcatalogus samengesteld uit bereikbare hubs",
    ],
    codeHeader: "POST /v1/chat/completions",
  },

  token: {
    eyebrow: "Token & beloningen",
    title: "KVR betaalt voor rekenkracht — en beloont het",
    lede: "KVR is de eenheid die ontwikkelaars uitgeven aan inferentie en de eenheid die bijdragers verdienen voor de layers die ze draaien. Beloningen worden berekend op basis van echt werk, niet van deelname.",
    facts: [
      { k: "Symbool", v: "KVR", note: "on-chain naam “Kvasir”, 6 decimals" },
      { k: "Chain", v: "Solana", note: "vandaag devnet" },
      { k: "Betaalt voor", v: "Inferentie", note: "betalen per verzoek via de gateway" },
      { k: "Beloont", v: "Rekenkracht", note: "layer-aandeel × prestatieklasse" },
    ],
    whatForTitle: "Waar KVR voor is",
    whatForBody:
      "Eén token, beide richtingen: ontwikkelaars geven KVR uit om inferentie via de gateway te draaien, en bijdragers verdienen KVR voor de rekenkracht die hun nodes leveren. Het is de rekeneenheid van het netwerk voor echt werk — zie hieronder hoe beloningen per rol zijn opgebouwd.",
    whatForChips: ["betalen per inferentie", "belonen per layer", "afrekenen op Solana"],
    custodyTitle: "Non-custodial wallet",
    custodyBody:
      "Beloningen worden afgerekend naar de eigen eigenaarswallet van elke node. Sleutels bevinden zich in de wallet van de gebruiker — browser, desktop of mobiel — nooit bij een operator. Geverifieerd over vier afzonderlijke eigenaarswallets, die elk hun layer-aandeel verdienen.",
    custodyChips: ["web", "desktop", "iOS", "Android"],
    devnetStrong: "Devnet, utility-token.",
    devnetBody:
      "KVR draait momenteel op Solana devnet en is een utility- / bijdrage-token — geen verhandelbaar bezit, prijs of investering. Niets hierin is financieel advies of een belofte van rendement.",
  },

  network: {
    eyebrow: "Netwerk & beloningen",
    title: "Elke rol in het netwerk verdient KVR",
    lede: "De ring van compute-nodes wordt gecoördineerd door hub- en gateway-rollen. Elk wordt in KVR betaald voor wat het daadwerkelijk doet — rekenkracht voor de layers die het draait, infrastructuur voor de uptime die het behoudt.",
    roles: [
      {
        role: "Compute-node",
        tagline: "Draait de layers van het model",
        body: "Bevat een paar aaneengesloten layers in de ring en draait ze voor elk verzoek. Verdient per bijdrage-unit (≈1k tokens bediend), gewogen naar zijn layer-aandeel en geschaald naar zijn prestatieklasse.",
        earns: "per unit × layer-aandeel × klasse",
      },
      {
        role: "Gateway-host",
        tagline: "Publieke ingang + afrekening",
        body: "Bedient de OpenAI/Anthropic-gateway en rekent KVR-betalingen af. Verdient een uurlijkse uptime-beloning voor het online houden van de ingang, plus een ×1.5-bonus op elke inferentie die het helpt bedienen.",
        earns: "uurlijkse uptime + ×1.5-inferentiebonus",
      },
      {
        role: "Hub-host",
        tagline: "De control plane",
        body: "Ontdekt apparaten, plant layer-plaatsing en orkestreert de ring. De meest kritieke rol — daarom verdient het de hoogste uurlijkse uptime-beloning voor het gecoördineerd houden van het netwerk.",
        earns: "hoogste uurlijkse uptime",
      },
    ],
    rolesNote:
      "Rollen stapelen: één machine kan tegelijk compute, gateway en hub zijn, en zijn beloningen tellen op. Alles wordt in KVR afgerekend naar de eigen wallet van die node.",
    formulaTitle: "Hoe beloningen worden berekend",
    formulaLabels: ["Compute-units", "Effectief", "Infra-uptime"],
    tiersTitle: "Prestatieklassen",
    tiersBody:
      "De gemeten decode-snelheid van een node bepaalt zijn vermenigvuldiger — snellere hardware verdient evenredig meer voor hetzelfde werk.",
  },

  tech: {
    eyebrow: "Onder de motorkap",
    title: "linkcpp — de engine achter het netwerk",
    lede: "linkcpp is de open control hub die alledaagse hardware verandert in een gedistribueerde inferentie-engine. De ring runtime laat elk apparaat slechts een paar layers bevatten en hidden state doorgeven aan zijn buur — geen centrale master — terwijl de standaard inferentie-engine data plane ongeforkt blijft.",
    taglineCaption: "— linkcpp, in zijn eigen woorden",
    points: [
      {
        title: "Ring runtime",
        body: "Elk apparaat slaat hetzelfde model op en laadt alleen zijn layer-venster, en opent dan één link naar zijn voorganger en één naar zijn opvolger. Hidden-state-grenzen circuleren door de ring en de laatste rank stuurt de token terug — geen centrale master, geen node bevat alles.",
      },
      {
        title: "linkcpp control hub",
        body: "Eén Dockerized hub — de control plane die het RPC data plane van inferentie-engine miste. Het ontdekt apparaten, plant layer-plaatsing, start de standaard workers en stelt de gateways beschikbaar. Broncode beschikbaar onder de Business Source License (BSL).",
      },
      {
        title: "Gedistribueerde layer-plaatsing",
        body: "linkcpp leest GGUF-metadata en berekent aaneengesloten layer-vensters per node via een rank manifest, plus optionele MoE-expert-FFN-offload naar node-RAM.",
      },
      {
        title: "SIWS + 2FA-beveiliging",
        body: "Bij publieke deployments is operator-toegang een Sign-In With Solana-handtekening over een server-nonce, plus TOTP-2FA en eenmalige back-upcodes — op zowel hub als gateway.",
      },
    ],
    openText:
      "De broncode is beschikbaar onder de Business Source License (BSL) — lees hem, draai hem en bouw erop voort, gratis voor ontwikkeling en testen. Productie- (commercieel) gebruik vereist een aangeschafte licentie.",
  },

  roadmap: {
    eyebrow: "Roadmap",
    title: "Vandaag live, en waar het naartoe gaat",
    lede: "Een duidelijke grens tussen wat al draait en wat gepland is. We presenteren de roadmap niet als afgeleverd.",
    items: [
      {
        phase: "Nu",
        title: "Inferentie op elk apparaat, live",
        body: "GPU’s, CPU’s, NPU’s en telefoons bedienen layers via de ring runtime. 122B draaide gesplitst over 4 GPU’s; bijdrage wordt end-to-end gecrediteerd; non-custodial wallets zijn beschikbaar op web/desktop/iOS/Android; toegang is beveiligd op publieke domeinen.",
      },
      {
        phase: "Binnenkort",
        title: "Mainnet & on-chain-afrekening",
        body: "Alles draait vandaag op Solana devnet met een off-chain afrekenservice. Een on-chain beloningsprogramma en mainnet zijn gepland.",
      },
      {
        phase: "Binnenkort",
        title: "Een permissionless wereldwijd netwerk",
        body: "De huidige demo’s draaiden op de hardware van één operator. Het netwerk openstellen zodat iedereen, waar dan ook, een apparaat kan aansluiten en verdienen — zonder poortwachter — is de volgende stap.",
      },
    ],
  },

  proof: {
    pill: "Bewezen in deze build",
    title: "Echte gedistribueerde inferentie, draaiend op publieke domeinen",
    items: [
      "parameters bediend, gesplitst over 4 AMD MI250-GPU’s",
      "afzonderlijke eigenaarswallets die elk hun layer-aandeel verdienen",
      "API-oppervlakken — OpenAI + Anthropic compatibel",
      "wallet-platforms — web · desktop · iOS · Android",
    ],
    strip:
      "122B bediend over 4 GPU’s · OpenAI + Anthropic compatibel · wallets op web / desktop / iOS / Android · live op publieke domeinen",
  },

  footer: {
    ctaTitle: "Zet je GPU op het netwerk.",
    ctaBody:
      "Draai een node en verdien KVR voor de layers die je bedient, of koppel de gateway aan je app via een OpenAI/Anthropic-compatibel endpoint.",
    tagline:
      "Het netwerkmerk voor gedecentraliseerde AI-inferentie, aangedreven door de linkcpp control hub — een engine met beschikbare broncode (BSL) die grote modellen splitst over alledaagse apparaten (op een standaard inferentie-engine data plane).",
    disclaimerStrong: "Disclaimer.",
    disclaimer:
      "KVR is een utility- / bijdrage-token dat wordt gebruikt om voor inferentie te betalen en om rekenkracht te belonen. Het draait vandaag op Solana devnet — het is geen verhandelbaar mainnet-bezit en niets hierin is een aanbod, prijs of belofte van financieel rendement. Beloningen weerspiegelen echt geleverde rekenkracht, niet deelname.",
    rights: "© 2026 Kvasir · linkcpp. Engine onder de Business Source License (BSL) — gratis voor ontwikkeling en testen; productiegebruik vereist een licentie.",
  },

  guide: {
    home: "Home",
    eyebrow: "Node-operator gids",
    headline1: "Breng rekenkracht,",
    headline2: "draai een node.",
    sub: "Maak een wallet aan, stake KVR en verbind vervolgens je apparaat met het Kvasir-netwerk om KVR te verdienen met de rekenkracht die je bijdraagt. Kies hieronder je platform voor download-, installatie- en uitvoerstappen.",
    badgeCustody: "Non-custodial — jouw sleutels",
    badgeDevices: "GPU · CPU · NPU",
    badgeToken: "Solana devnet · KVR",
    devnetNote: "KVR is een Solana devnet-utility-token — geen verhandelbaar mainnet-asset en geen financieel rendement.",
    reqTitle: "Vereiste voor hub- en gateway-operators",
    reqBody: "Om een hub-node of een gateway-node te draaien, moet je 100.000 KVR staken in je wallet. Reguliere compute-nodes kunnen zonder deze vereiste meedoen en verdienen voor de lagen die ze draaien.",
    tabDesktop: "Desktop",
    tabMobile: "Mobiel",
    soon: "Binnenkort beschikbaar",
    download: "Download",
    desktopTitle: "Kvasir Wallet · Desktop-app",
    desktopSub: "macOS · Windows · Linux — wallet en node in één app.",
    desktop: [
      { title: "Download de app", body: "Download hierboven het installatieprogramma van Kvasir Wallet voor jouw besturingssysteem. Een GPU (NVIDIA / AMD / Apple Silicon) wordt aanbevolen, maar CPU werkt ook.", body2: "" },
      { title: "Installeren en openen", body: "Voer het installatieprogramma uit en open daarna Kvasir Wallet. Zie je op macOS de waarschuwing “niet-geïdentificeerde ontwikkelaar”, sta dit dan toe via Systeeminstellingen → Privacy en beveiliging.", body2: "" },
      { title: "Maak je wallet aan", body: "Kies Nieuwe wallet aanmaken. Schrijf je herstelzin van 12 woorden op en bewaar deze veilig — bij verlies kan deze niet worden hersteld. Stel vervolgens een wachtwoordzin in om de app te ontgrendelen. Sleutels zijn non-custodial en worden alleen op dit apparaat opgeslagen.", body2: "" },
      { title: "Wallet vullen & KVR staken", body: "Ontvang wat devnet SOL (voor kosten) en KVR (om te staken) op het ontvangstadres van je wallet. Voer in het staking-paneel van het dashboard een bedrag in en klik op Staken om APR-rente te verdienen en in aanmerking te komen voor node-beloningen.", body2: "" },
      { title: "Configureer de node", body: "Kies in Node-instellingen de compute-backend van deze machine (CUDA / ROCm / Metal / CPU) en selecteer Lokale shard (aanbevolen) — dit draait de laag-shard lokaal en stuurt alleen kleine randstatus door, de snelste modus.", body2: "" },
      { title: "Start de node", body: "Schakel Node draaien (live) in om deze machine onder je wallet (eigenaar) op het netwerk te registreren en online te brengen.", body2: "Voor een echte GPU-compute-node draai je ook de onderstaande native agent. De planner van de hub plaatst modellagen op je machine, en je node verdient een laag-aandeel KVR dat wordt bijgeschreven op de eigenaarswallet." },
      { title: "Volg bijdrage & beloningen", body: "Bekijk in Node-status: nodes / online / effectieve bijdrage / opeisbaar. Nodes worden ingedeeld in tiers op basis van doorvoer (S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7); ruw × tier = effectief. Gebruik Beloningen claimen om opgebouwde KVR naar je wallet over te maken.", body2: "" },
    ],
    faucetTitle: "Devnet SOL verkrijgen (gratis faucet)",
    faucetIntro: "Je hebt wat devnet SOL nodig voor transactiekosten (gebruik het ontvangstadres van je wallet):",
    faucetWeb: "Web: faucet.solana.com — plak je adres en kies netwerk Devnet",
    faucetCli: "CLI: solana airdrop 2 <your address> --url devnet",
    faucetAlt: "Alternatieven: QuickNode · SolFaucet devnet",
    faucetKvr: "Verkrijg KVR om te staken via distributie of swap (KVR-swap: SOL/ETH ↔ KVR — binnenkort beschikbaar).",
    mobileTitle: "Kvasir Wallet · {0}-app",
    mobileSub: "Maak een wallet aan en verbind je apparaat met het netwerk.",
    mobile: [
      { title: "Installeer de app", body: "Installeer Kvasir Wallet vanuit {0}. Gebruik de knop hierboven om de winkelpagina te openen. Een recent apparaat met een GPU/NPU wordt aanbevolen.", note: "" },
      { title: "Wallet aanmaken / herstellen", body: "Open de app en kies Nieuwe wallet aanmaken of Herstellen via herstelzin. Bewaar je zin van 12 woorden veilig en stel een wachtwoordzin in — met deze zin herstel je hetzelfde account op desktop en andere apparaten. Sleutels zijn non-custodial en worden alleen op het apparaat opgeslagen.", note: "" },
      { title: "Configureer de node", body: "Kies in Mobiele node-instellingen een compute-backend (GPU · OpenCL/Vulkan · CPU) en Lokale shard (aanbevolen). De verwachte doorvoer (tok/s) en de impact op geheugen / temperatuur / prestaties worden weergegeven.", note: "" },
      { title: "Staken & beloningen", body: "Stake in Staking & node-beloningen je KVR en bekijk / claim de opeisbare beloningen die je node opbouwt. Node-status toont je prestatie-tier en bijdrage.", note: "Deelname aan mobiele local-shard-inferentie wordt geleidelijk uitgerold; op dit moment zijn de belangrijkste compute-nodes GPU/CPU-machines waarop de agent draait." },
    ],
    viewGithub: "Bekijk op GitHub",
    capWelcome: "Welkom — wallet aanmaken of herstellen",
    capRecovery: "Bewaar je herstelzin van 12 woorden (woorden vervaagd)",
    capPassphrase: "Wachtwoordzin instellen → Aan de slag",
    capReceive: "Ontvangen — adres & QR (adres deels gemaskeerd)",
    capBalances: "Walletsaldo — KVR · SOL",
    capStaking: "Staking — APR · hoofdsom · rente · node-beloningen",
    capBackend: "Compute-backend (CUDA · ROCm · Metal · CPU)",
    capMode: "Node-modus — Lokale shard (aanbevolen)",
    capRunlive: "Node draaien (live) — live meters · node-id · OS",
    capNodes: "Node-status — totalen · tiers · bijdrage per node",
    capClaim: "Beloningen claimen — opeisbare KVR",
    capWallet: "Wallet-start — KVR-saldo (adres gemaskeerd)",
    capNodeset: "Node-instellingen — backend / modus / verwachte resources",
    capStakingM: "Staking & beloningen voor node-operators",
  },

  techBlog: {
    docTitle: "Kvasir — Technologie",
    pill: "Techblog",
    title: "De engineering van de zwerm",
    lede: "Ontwerpnotities en op echte hardware geverifieerde mijlpalen uit de bouw van expert-gesharde zwerm-inferentie op linkcpp — hoe een 122B-model over GPU’s, CPU’s en telefoons draait.",
    langNote: "",
    sidebarTitle: "Artikelen bekijken",
    allArticles: "Alle artikelen",
    read: "Lezen",
    notFound: "Dat artikel bestaat niet.",
    categories: {
      overview: "Visie & architectuur",
      core: "Kerntechnologie",
      milestones: "Mijlpalen",
      demos: "Demo’s op echte apparaten",
    },
  },

  wiki: {
    docTitle: "Kvasir — Wiki",
    pill: "Wiki",
    title: "De Kvasir-kennisbank",
    lede: "Korte, precieze items over elk concept in het netwerk — van de ring runtime en expert-sharding tot KVR-beloningen.",
    langNote: "",
    sidebarTitle: "Items bekijken",
    allEntries: "Alle items",
    notFound: "Dat item bestaat niet.",
    categories: {
      network: "Netwerk & rollen",
      inference: "Inferentie & engine",
      token: "Token & beloningen",
    },
  },

  apiDocs: {
    docTitle: "Kvasir-API — inferentie met betalen per gebruik in KVR",
    pill: "Voor ontwikkelaars · devnet",
    title: "Roep Kvasir-inferentie aan, betaald in KVR",
    lede: "De swarm-modellen van Kvasir staan niet direct bloot aan het open internet. Het enige publieke toegangspunt is de KVR-gateway met betalen per gebruik: elke inferentie wordt ontgrendeld door een on-chain KVR-betaling die je wallet ondertekent. Hier is de hele flow, in de taal waarin jij bouwt.",
    devnetNote: "Draait op het Solana-devnet — KVR is geen echt bezit. Voorzie voordat je begint een devnet-wallet van KVR en wat SOL voor de kosten.",
    baseLabel: "Basis-URL",
    flowTitle: "Vier stappen",
    flowSteps: [
      { n: "1", title: "Ontdek een model", body: "Vraag de gateway welke modellen de swarm nu bedient. De lijst is live — codeer niets hard." },
      { n: "2", title: "Vraag een offerte", body: "Stuur de model-id en je prompt. Je krijgt een requestId gekoppeld aan die prompt en een KVR-prijs." },
      { n: "3", title: "Betaal on-chain", body: "Stuur de geoffreerde KVR naar de tokenrekening van de ontvanger en onderteken met je wallet. Bewaar de handtekening." },
      { n: "4", title: "Verzilveren", body: "Stuur de requestId en handtekening terug. De gateway verifieert de betaling, voert de inferentie uit en geeft het resultaat terug." },
    ],
    refTitle: "API-referentie",
    requestLabel: "Verzoek",
    responseLabel: "Respons",
    apiModels: "Toont de modellen die de swarm nu bedient — een lege array wanneer er geen is, dus codeer nooit een id hard.",
    apiQuote: "Vraag een prijsofferte en een requestId gekoppeld aan je prompt. priceToken is de te betalen hoeveelheid KVR; de eindafrekening is op basis van werkelijk tokengebruik.",
    apiPay: "Stuur de geoffreerde KVR naar de bijbehorende tokenrekening van de ontvanger (de vault) en onderteken met je wallet. De handtekening is eenmalig te gebruiken.",
    apiInfer: "De gateway pollt de chain om de betaling te verifiëren, voert de inferentie uit op de hub en geeft het resultaat terug samen met het werkelijke gebruik en de kosten.",
    codeTitle: "End-to-end voorbeeld",
    codeLede: "Laad de geheime sleutel van je wallet uit de omgeving, offreer, betaal en verzilver — één op zichzelf staand fragment. Stappen 1, 2 en 4 zijn puur HTTP; alleen stap 3 (de SPL-overdracht) verschilt per SDK.",
    adapterTitle: "OpenAI-compatibele adapter",
    adapterLede: "Heb je al een OpenAI-client (of een tool die alleen OpenAI spreekt)? Draai deze plug-and-play adapter naast je app. Hij stelt /v1/chat/completions beschikbaar en betaalt elke aanroep vanuit je eigen wallet — offerte, ondertekenen, verzilveren — op de achtergrond. Wijs de base-URL van je client naar de adapter en gebruik een willekeurige dummy-API-sleutel.",
    adapterNote: "Non-custodial: het wallet-secret (KVR_SECRET_KEY) blijft in dit proces en bereikt Kvasir nooit. Er is geen Kvasir-API-sleutel — authenticatie is de on-chain KVR-betaling die je adapter ondertekent. Elke aanroep is één offerte/betaling/verzilvering-heen-en-weer; cache of batch naar je doorvoer.",
    prereqTitle: "Voordat je begint",
    prereqs: [
      "Een Solana-devnet-wallet met KVR (om te betalen) en wat SOL (voor de kosten).",
      "De KVR-mint heeft 6 decimalen — het on-chain bedrag is round(priceToken × 1.000.000).",
      "De bestemming is de bijbehorende tokenrekening van de ontvanger; bestaat die nog niet, dan moet je overdracht die aanmaken (kost wat SOL).",
    ],
    securityTitle: "Beveiligingsregels die de gateway afdwingt",
    security: [
      "Elke transactiehandtekening is eenmalig te gebruiken — hergebruik levert 409 op.",
      "Inferentie wordt beschermd door de requestId plus de eenmalige handtekening, dus houd de requestId privé — alleen de uitgever mag hem verzilveren.",
      "De vault moet minstens priceToken KVR ontvangen, anders wordt het verzoek geweigerd.",
      "Foutgevallen: onbekende requestId (404), handtekening al gebruikt (409), transactie nog niet bevestigd (400).",
    ],
    walletTitle: "Maak een test-wallet",
    walletLede: "Nog geen devnet-wallet? Genereer een sleutelpaar, print het secret voor je omgeving en vul het aan met SOL voor de kosten — vraag daarna KVR aan bij de faucet hieronder.",
    walletNote: "Houd het secret buiten versiebeheer en laad het uit een omgevingsvariabele. Alleen devnet — hergebruik een testsleutel nooit op mainnet. Je kunt ook een wallet aanmaken in de Kvasir-app en het adres kopiëren.",
    faucetTitle: "Test-KVR ophalen",
    faucetLede: "Plak een Solana-devnet-adres om 100 KVR te ontvangen — genoeg om de flow hierboven te proberen. Eén verzoek per adres per dag.",
    faucetPlaceholder: "Je Solana-devnet-adres",
    faucetButton: "100 KVR aanvragen",
    faucetSending: "Verzenden…",
    faucetSuccess: "{0} KVR naar je wallet gestuurd",
    faucetViewTx: "Transactie bekijken",
    faucetError: "Kon geen KVR versturen",
    ctaTitle: "Bouw op Kvasir",
    ctaBody: "Hetzelfde /api/pay-contract drijft de desktop-, iOS- en Android-wallets van Kvasir aan. Lees de referentie-implementatie en de gateway-broncode op GitHub.",
    ctaButton: "Bekijk op GitHub",
    catRunNode: "Draai een node → gratis inferentie",
    catUseApi: "De API gebruiken",
    selfHostTitle: "Draai een node, krijg gratis inferentie",
    selfHostPitch: "Wil je AI-modellen gratis gebruiken? Laat je coding-agent je machine als node aan het netwerk koppelen — en je er een inferentie-endpoint voor teruggeven.",
    selfHostBody: "Eén script start de hub (en optioneel de KVR-gateway) met Docker. Voeg je GPU toe en laad een open model in de hub-UI, en roep dan een standaard OpenAI-compatibel endpoint aan — /c/<id>/v1/chat/completions — dat op je eigen hardware draait. Richt elk hulpmiddel dat OpenAI spreekt erop.",
    selfHostNote: "Dit serveert de open modellen die je machine aankan gratis — het is jouw rekenkracht. Voor frontier-modellen die te groot zijn voor één machine, sluit je aan bij de swarm: daar is de KVR betalen-per-gebruik-API hieronder voor.",
    inferenceApiTitle: "Inference API (credits)",
    inferenceApiLede: "De eenvoudigste weg: een native OpenAI-endpoint met een API-sleutel. Streaming (SSE) en native tool calls werken meteen, en elke aanroep wordt afgeschreven van een vooruitbetaald KVR-saldo — geen wallet-ondertekening per aanroep. Toegang wordt geregeld door een wallet-whitelist.",
    inferenceApiSteps: [
      { title: "Credits bijvullen", body: "Krijg KVR-credit via een toewijzing van een operator, of stort zelf: maak KVR over naar de treasury en dien de handtekening in bij POST /api/credits/deposit." },
      { title: "Een API-sleutel ophalen", body: "Bewijs eenmalig het eigenaarschap van je wallet (SIWS): vraag een challenge aan, onderteken het bericht en wissel het in voor een sleutel. De apiKey wordt eenmalig teruggegeven — bewaar hem." },
      { title: "Aanroepen zoals OpenAI", body: "Richt een willekeurige OpenAI-client op de basis-URL met jouw sleutel. Streaming en tool_calls werken ongewijzigd; het saldo wordt per aanroep gedebiteerd." },
    ],
    inferenceApiKeyLede: "Een API-sleutel uitgeven (één wallet-handtekening)",
    inferenceApiCallLede: "Roep het daarna aan met de standaard OpenAI-SDK — alleen base_url en de sleutel veranderen",
    inferenceApiRefTitle: "Referentie",
    inferenceApiRef: {
      base: "Basis-URL", auth: "Auth", endpoints: "Endpoints", balance: "Saldo",
      pricing: "Prijzen", errors: "Fouten", context: "Max. context", model: "Model",
    },
    inferenceApiThinkNote: "Het bediende model redeneert standaard. Zet voor korte antwoorden of tool calls chat_template_kwargs.enable_thinking = false; laat het denken aan (beter voor coderen) met een royale max_tokens. Tool calls werken altijd. Het huidige tarief wordt bepaald door governance en kan wijzigen.",
    keyIssueTitle: "Geef hier een sleutel uit",
    keyIssueLede: "Liever niet scripten? Doorloop de hele flow direct hier — voer je wallet in, onderteken elke challenge en ontvang een sleutel. Dezelfde endpoints als hierboven; je sleutel verlaat je browser nooit.",
    keyIssueWalletPh: "Je Solana-wallet-adres",
    keyIssueLabelPh: "Sleutellabel (bijv. my-app)",
    keyIssueStart: "Start",
    keyIssueRegisterNote: "Deze wallet is nog niet geregistreerd — onderteken één keer om jezelf te registreren en daarna nogmaals om de sleutel op te halen.",
    keyIssueSignPrompt: "Onderteken dit exacte bericht met je wallet (ed25519) en plak daarna de base64-handtekening:",
    keyIssueSigPh: "base64-handtekening",
    keyIssueSubmit: "Handtekening indienen",
    keyIssuePending: "Deze wallet staat niet op de whitelist en zelfregistratie is gesloten. Een operator moet hem goedkeuren — neem contact op.",
    keyIssueKeyReady: "Je API key — wordt eenmalig getoond. Kopieer hem nu.",
    keyIssueBalanceLabel: "Creditsaldo",
    keyIssueTopUp: "Saldo is 0 — vul KVR-credit bij (maak KVR over naar de treasury en doe daarna POST /api/credits/deposit) voordat je aanroepen doet.",
    keyIssueError: "Verzoek mislukt",
    keyIssueUseNote: "Gebruik hem nu: het is een Bearer-sleutel voor https://gate.kvasir-ai.net/v1. Voer de kant-en-klare opdracht hieronder uit, of richt een OpenAI-SDK op die base-URL (volledige voorbeelden verderop).",
    selfIssueTitle: "Zelf een sleutel uitgeven (agents)",
    selfIssueLede: "Een coding-agent kan alles headless doen vanuit een wallet-secret: de SIWS-challenges ondertekenen, zichzelf registreren, een sleutel ophalen en dan het OpenAI-endpoint aanroepen. Geen browser, geen klikken.",
    gateTitle: "Geef een ster om de dev-docs te ontgrendelen",
    gateBody: "De docs blijven open — een GitHub-ster houdt je alleen op de hoogte en helpt het project groeien. Log in met GitHub en geef de repo een ster om verder te gaan.",
    gateSignIn: "Inloggen met GitHub",
    gateStarBody: "Ingelogd als {0}. Geef de repo een ster op GitHub en controleer opnieuw om te ontgrendelen.",
    gateStarLink: "Ster louisevandan/kvasir-net ↗",
    gateRecheck: "Ik heb een ster gegeven — opnieuw controleren",
  },

  careers: {
    docTitle: "Marketing & Growth — Kvasir",
    pill: "We nemen aan",
    headline1: "Marketing & Growth",
    headline2: "laat het netwerk groeien",
    sub: "Kvasir is een gedecentraliseerd AI-inferentienetwerk (DePIN) op Solana. De open-source linkcpp-engine verdeelt grote open modellen over vele bijgedragen GPU's en machines, en elke node verdient KVR voor de lagen die hij daadwerkelijk heeft bediend. De techniek werkt al — we zoeken de persoon die het de wereld vertelt.",
    factRole: "Rol",
    factRoleV: "Marketing & growth — fulltime",
    factLocation: "Locatie",
    factLocationV: "Remote · tijdzone VS/Europa of Zuidoost-Azië · ≥3–4 u dagelijkse overlap met KST",
    factComp: "Beloning",
    factCompV:
      "Early-stage equity (4 jaar vesting / 1 jaar cliff) + TGE-afhankelijke tokentoewijzing · betaalde proefopdracht vóór elke verbintenis",
    factEngine: "Engine",
    liveTitle: "Wat al live is",
    liveLede: "Je stapt niet in een whitepaper. Geverifieerd en vandaag draaiend:",
    liveProof: [
      "Modellen getest op het netwerk: Qwen3.5 122B, Qwen3.5 35B en Gemma4 12B — elk laag voor laag verdeeld over meerdere machines, zodat geen enkele node het hele model bevat.",
      "Een heterogene live vloot van in totaal 21 nodes: 4× AMD MI250 (ARM-host), 4× NVIDIA GB10, 4× NVIDIA RTX Pro 6000, 1 MacBook Pro, 6 x86 Windows-CPU-machines en 2 mobiele nodes (iOS + Android).",
      "Bijdrageverrekening per node: elke node verdient KVR gewogen naar zijn laagaandeel in elke bediende inferentie, uitbetaald naar zijn eigen wallet.",
      "OpenAI- en Anthropic-compatibele pay-per-inference gateway, live op ons eigen domein.",
      "Non-custodial wallets uitgebracht op web, desktop, iOS en Android, met wallet-handtekening-login (Sign-In With Solana) + 2FA.",
    ],
    devnetNote:
      "KVR draait momenteel op het Solana-devnet. Het is een utility-/bijdragetoken — niets op deze pagina is een effectenaanbod of een belofte over tokenwaarde.",
    ownsTitle: "Wat je gaat doen",
    ownsLede:
      "Een tweezijdig netwerk vraagt tweezijdige groei: node-operators aan de aanbodkant, ontwikkelaars aan de vraagkant. Beide beginnen bij nul — dat is de baan.",
    owns: [
      {
        title: "Community & social",
        body: "Bouw X en Discord vanaf nul op. Ontwikkel KOL-relaties in de DePIN-/AI-crypto-niche en houd een vast contentritme aan.",
      },
      {
        title: "Groei van node-operators",
        body: "Acquisitie aan de aanbodkant: bereik GPU-bezitters en homelab-community's en voer campagnes die hen omzetten in actieve Kvasir-nodes.",
      },
      {
        title: "Vraag van ontwikkelaars",
        body: "Vraagzijdemarketing voor ontwikkelaars en AI-startups die OpenAI/Anthropic-compatibele inferentie-endpoints nodig hebben — docs-achtige content, launchposts, integratie-showcases.",
      },
      {
        title: "Campagnes & analytics",
        body: "Ontwerp groeiexperimenten, meet ze eerlijk en zet vol in op de kanalen die het aantal nodes en API-gebruik echt bewegen.",
      },
      {
        title: "Launch- & partnershipsupport",
        body: "Ondersteun de tokenlaunch-marketing wanneer het netwerk het devnet ontgroeit, en help bij partneracquisitie (GPU-vloten, wallets, modelaanbieders).",
      },
    ],
    profileTitle: "Wie we zoeken",
    profile: [
      "Crypto-native marketeer: je hebt een web3-community of -product vanaf nul laten groeien — verifieerbaar op X, Discord of on-chain.",
      "Bekendheid met DePIN of AI-crypto heeft sterke voorkeur; je kunt een GPU-bezitter uitleggen waarom die een node zou draaien.",
      "Engels als moedertaal of vloeiend; tijdzone VS/Europa of Zuidoost-Azië met ≥3–4 u dagelijkse overlap met KST (UTC+9).",
      "Comfortabel met early-stage beloning: substantiële equity + token-upside boven een hoog salaris.",
      "Hands-on uitvoerder — je levert zelf posts, campagnes en experimenten.",
    ],
    processTitle: "Hoe we aannemen",
    processLede:
      "Elke kandidaat doorloopt een betaalde proefopdracht vóór enig equity-gesprek — dat beschermt beide kanten.",
    process: [
      {
        title: "Kennismakingscall",
        body: "Wij laten het live netwerk en de roadmap zien; jij vertelt over een community of campagne die je echt hebt opgebouwd.",
      },
      {
        title: "Betaalde proefopdracht (2–4 weken)",
        body: "Een echte, betaalde werkproef — bijv. een acquisitieplan voor node-operators met kanaalberekeningen, of één live groeiexperiment op X/Discord. We beoordelen output, snelheid en zelfstandigheid.",
      },
      {
        title: "Aanbod",
        body: "Marketing & Growth: equity met standaard 4 jaar vesting (1 jaar cliff) plus een TGE-afhankelijke tokentoewijzing; een cash-basis zodra funding binnenkomt.",
      },
      {
        title: "Samen bouwen",
        body: "Eerste mijlpaal als team: samen onze volgende hackathon-inzending bouwen en de eerste groep node-operators laten groeien.",
      },
    ],
    applyTitle: "Zo solliciteer je",
    applyBody:
      "Mail een korte introductie met links die het bovenstaande profiel bewijzen — de community of campagne die je bouwde, je X-handle, alles on-chain. Een cv is optioneel; bewijs niet.",
    applyCta: "Solliciteer",
    readCode: "Lees eerst de code",
    seeProduct: "Bekijk het product",
  },
};
