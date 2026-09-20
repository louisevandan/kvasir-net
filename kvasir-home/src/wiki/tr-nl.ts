/* Nederlands — vertaling van de wiki-items. Structuur (slug, categorie,
   blokvolgorde, code) spiegelt exact entries.ts (Engelse bron); technische
   termen en identifiers (KVR, p4, GGUF, MoE, bridge, tok/s enz.) blijven
   letterlijk. De compliance-framing (devnet, utility-token, non-custodial)
   blijft intact. */
import type { WikiTranslation } from "./entries";

export const nlWiki: Record<string, WikiTranslation> = {
  "node-relay": {
    title: "Node-relay",
    summary: "Een publiek adres dat wordt aangehouden namens een machine die er geen heeft, zodat een node achter NAT bereikbaar is zonder ook maar één poort te openen.",
    blocks: [
      { t: "p", md: "Een **node-relay** geeft de machine van een deelnemer een adres dat het netwerk kan bellen. p4 levert werk af door een verbinding *naar* een node te openen, en een thuismachine achter netwerkadresvertaling heeft zo'n adres niet. De relay houdt er publiek één aan, de node onderhoudt één uitgaande verbinding daarheen, en werk dat op het publieke adres binnenkomt daalt af langs de verbinding die de node al had." },
      { t: "p", md: "Geen van beide uiteinden van p4 merkt dat de relay bestaat. De beller ziet een gewoon adres; de agent van de node blijft gebonden aan `127.0.0.1` en luistert nergens anders op." },
      { t: "h2", text: "Waarom een tunnel en geen doorgeschakelde poort" },
      { t: "p", md: "p4 draagt geen enkele authenticatie: elke host die de poort van een agent bereikt mag `NODE_LOAD`, `NODE_UNLOAD` of `INSPECT` sturen. Een poort op de thuisrouter daarheen doorschakelen zou de machine blootstellen aan iedereen die haar vindt. Achter een relay luistert de node nergens op en bewijst hij een operator-wallet voordat zijn verbinding iets vervoert, met hetzelfde handtekeningschema als de afrekengateway: bereikbaarheid en authenticatie worden door hetzelfde mechanisme opgelost." },
      { t: "h2", text: "Wat het niet doet" },
      { t: "ul", items: [
        "**Het leest het verkeer niet.** Payloads gaan byte voor byte door en worden nooit ontleed, dus de relay kan opdrachten niet onderscheiden — en mag dat niet, want het verkeer begrijpen zou hem in staat stellen het te wijzigen.",
        "**Het plant niet.** Plaatsing blijft bij het plan van de operator; voor de relay is een node een adres en verder niets.",
        "**Het is geen bewijs van werk.** Bytes die een relay passeren zeggen niets over verrichte inferentie en tellen nooit mee als bijdrage.",
      ] },
      { t: "h2", text: "Zie ook" },
      { t: "p", md: "**p4-agent**, het proces dat de machine van een deelnemer draait, en **node-operator**, de wallet die een relay verifieert voordat een adres wordt toegekend." },
    ],
  },
  "kvasir-network": {
    title: "Kvasir-netwerk",
    summary: "Een gedecentraliseerd AI-inferentienetwerk (DePIN) waar alledaagse apparaten open modellen serveren en KVR verdienen.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** is een gedecentraliseerd AI-inferentienetwerk: grote open modellen worden met de **p4**-engine over gedeelde hardware verdeeld, zodat geen enkel knooppunt het hele model hoeft te bezitten. Iedereen kan een GPU, CPU, NPU — zelfs een telefoon — bijdragen en **KVR** verdienen voor de lagen of experts die zijn apparaat daadwerkelijk serveert. Ontwikkelaars bereiken het netwerk via OpenAI/Anthropic-compatibele gateways en betalen per inferentie.",
      },
      {
        t: "ul",
        items: [
          "**Engine met beschikbare broncode** — p4 valt onder de Business Source License 1.1 (niet-gemonetiseerd intern gebruik is toegestaan; gehost of inkomstengenererend gebruik vereist een commerciële licentie); het llama.cpp-datavlak eronder blijft dicht bij upstream en inspecteerbaar.",
          "**Wallet in eigen beheer** — sleutels verlaten nooit het apparaat van de gebruiker, en beloningen worden uitbetaald naar de eigen Solana-wallet van elke node-eigenaar. Op devnet worden gestakete KVR en vooraf betaalde credits aangehouden door de treasury van de gateway en bijgehouden in zijn grootboek, totdat een on-chain stakingprogramma wordt uitgebracht.",
          "**Bewezen op echte hardware** — een 122B-model draaide end-to-end over 3 fysieke machines op onze testvloot, met de bijdrage van elke node end-to-end bijgeschreven.",
          "**Vernoemd naar de Noorse mythe** — Kvasir, het wijste wezen, geboren uit de samengebrachte essentie van alle goden en het eigendom van geen enkele.",
        ],
      },
      { t: "h2", kick: "Eén verzoek, vele apparaten", text: "Hoe een inferentie stroomt" },
      {
        t: "code",
        caption: "Elke hop is gewoon HTTP/TCP; het model zelf is wat verdeeld wordt.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Rollen **stapelen**: één machine kan tegelijk rekennode, gateway-host en bridge-host zijn, en de beloningen tellen op. De taak van het netwerk is om het geheel op één machine te laten lijken — één endpoint vooraan, duizenden imperfecte apparaten erachter.",
      },
      {
        t: "p",
        md: "Vandaag draait het netwerk op **Solana devnet**; KVR is een utility- / bijdrage-token, geen verhandelbaar activum of investering, en niets op deze pagina is financieel advies.",
      },
    ],
  },
  architecture: {
    title: "Kvasir-architectuur",
    summary: "Eén kaart van het hele systeem: wallets, de gateway die de betaling aanneemt, de bridge die de engine afschermt, en het p4-netwerk dat het model draait.",
    blocks: [
      {
        t: "p",
        md: "Kvasir bestaat uit vier lagen met elk één naad. **Wallets** houden de sleutels. De **gateway** neemt de betaling aan en houdt het grootboek bij. De **bridge** zet een HTTP-gezicht op de inferentie-engine. Het **p4-netwerk** draait het model daadwerkelijk. Alles hieronder volgt uit waar die naden liggen — en het diagram markeert wat vandaag draait tegenover wat nog ontwerp is.",
      },
      { t: "h2", kick: "Wallets", text: "Sleutels verlaten het apparaat nooit" },
      {
        t: "p",
        md: "iOS (Swift), Android (Kotlin) en desktop (React + Electron) zijn aparte builds van dezelfde wallet, en de desktop-build is ook wat de gateway op `/` serveert als browser-wallet — een volwaardige wallet die in de pagina ondertekent, geen alleen-lezen console. Beloningen gaan naar het eigen Solana-adres van elke eigenaar; de gateway houdt nooit een gebruikerssleutel vast.",
      },
      { t: "h2", kick: "Gateway", text: "Eén proces, twee oppervlakken" },
      {
        t: "p",
        md: "`solana/staking-service` is tegelijk de **API-gateway** (een OpenAI-compatibele `/v1/chat/completions`, plus de betaal-per-verzoek-flow `/api/pay/quote` → `/api/inference`) en de **verrekengateway** (staking, het node-register, creditrekeningen, bijdragekrediet). Ze zijn één proces omdat ze één grootboek delen: een verzoek wordt pas bediend nadat de KVR-overdracht on-chain is geverifieerd, en hetzelfde grootboek schrijft de nodes bij die het bedienden.",
      },
      {
        t: "callout",
        md: "**De betaling wordt verrekend voordat de inferentie draait.** Faalt de bridge daarna, dan betaalt de gateway de betaler terug uit de treasury en geeft een 502 in plaats van te rekenen voor niets. Er zit geen mock-model en geen placeholder-catalogus achter: een model dat de app aanbiedt, is een model dat een bridge serveert — of de lijst is leeg.",
      },
      { t: "h2", kick: "Bridge", text: "Het HTTP-gezicht van de engine" },
      {
        t: "p",
        md: "De bridge (`p4bridge`) is in p4-termen een **OUTER**: hij installeert een sessie over de stages, dient in bij de kopstage en verzamelt de tokenstroom. Voor de gateway is hij een klein, vast contract — welke modellen geladen zijn, wie hoeveel bijdroeg, en completions.",
      },
      {
        t: "table",
        head: ["Route", "Wat het beantwoordt"],
        rows: [
          ["`/api/controllers`", "welke modellen geladen zijn, en de staat van elke stage"],
          ["`/api/runtime`", "de operator-wallet en de machines erachter"],
          ["`/api/contributions`", "rijen, eenheden, verzoeken en doorvoer per node"],
          ["`/c/<model>/v1/chat/completions`", "inferentie"],
        ],
      },
      {
        t: "p",
        md: "Twee taken die p4 bewust aan de bridge overlaat: **het chat-template** (p4 geeft de stage-server een ondoorzichtige prompt en past er geen enkele toe, dus een instruct-model zou je tekst voortzetten in plaats van hem te beantwoorden) en **het redeneerblok** (teruggegeven als `reasoning_content`, los van `content`, zodat een denkfase het tokenbudget niet stilletjes kan opeten en de betaler een leeg antwoord in rekening kan brengen).",
      },
      {
        t: "callout",
        md: "**De bridge wordt nooit gepubliceerd.** Zijn enige authenticatie is een gedeeld service-token, en alles wat hem bereikt kan de ring draaien. Hij bindt op loopback; de tunnel is de deur.",
      },
      { t: "h2", kick: "p4-netwerk", text: "Agents bezitten nodes, stage-servers houden lagen" },
      {
        t: "p",
        md: "Een **agent** bezit de nodes van een host; een **stage-server** is één proces dat een plak van de lagen van het model vasthoudt. Een stage geeft zijn resultaat door aan de volgende door zijn eigen agent te vragen de agent van die stage te bellen **op het adres dat die agent adverteert** — het geadverteerde adres moet dus bereikbaar zijn vanaf de andere hosts, en hoort het snelste netwerk te zijn dat ze delen. Op het MI250-rack is dat de InfiniBand-link, niet het kantoor-LAN en nooit loopback.",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` en `p4_staged_server` zijn één release.** Een agent die uit een nieuwere tree is gebouwd faalt bij READY op een ontbrekende HELLO-capaciteit — nadat het hele model is geladen.",
          "**Plaatsing is een operator-artefact.** Welke lagen op welke GPU zitten, onder welke load generation, komt uit een placement plan; de bridge antwoordt `409` aan wie hem vraagt te serveren, en de watchdog van de gateway zegt dat één keer en stopt met vragen.",
          "**Een pipeline heeft minstens twee stages nodig.** Het sessiecommando weigert een pipeline met één stage.",
        ],
      },
      { t: "h2", kick: "Relay", text: "Een belbaar adres voor een laptop" },
      {
        t: "p",
        md: "Edge-nodes — een desktop-app, een telefoon — hebben geen adres dat iemand kan bellen. De **relay** geeft ze er een: de node belt naar buiten, bewijst het wallet-sleutelpaar met een ed25519-challenge, en is daarna via de relay bereikbaar. De relay is de authenticatiegrens en parseert nooit payloads. De desktop-installer levert de p4-agent samen met de app, dus meedoen is geen tweede installatie.",
      },
      { t: "h2", kick: "Verrekening", text: "Krediet volgt deelname" },
      {
        t: "p",
        md: "Elke stage rapporteert de tokenrijen die hij heeft gedraaid. De bridge telt ze per node op, en de gateway pollt elke 30 seconden `/api/contributions` en schrijft bij op de wallet die de bridge noemt, als `rows / 1000` eenheden geschaald naar het prestatieniveau van de node. **In een pipeline ziet elke stage dezelfde rijen**, dus een ring met vier stages betaalt zijn vier stages gelijk, ongeacht hoeveel lagen elk vasthoudt — krediet volgt deelname, niet gewichtsaandeel. Expert-sharding, waarbij nodes verschillende fracties van een laag vasthouden, is het geval dat dit opnieuw ter discussie zal stellen.",
      },
      { t: "h2", kick: "P4 Studio", text: "Wat het diagram als voorstel markeert" },
      {
        t: "p",
        md: "**P4 Studio** is de eigen operatorconsole van p4. De observability-feed per verzoek die hij van de agents wil hebben, is een voorstel upstream en draait hier niet — daarom tekent het diagram hem gestippeld, naast expert-shards die vanaf edge-nodes worden geserveerd, wat ontworpen is maar nog niet draait.",
      },
    ],
  },
  bridge: {
    title: "Bridge",
    summary: "Het HTTP-gezicht van de inferentie-engine: wat er geladen is, wie heeft bijgedragen, en completions — en verder niets.",
    blocks: [
      {
        t: "p",
        md: "De **bridge** is het enige waar de verrekengateway voor inferentie mee praat. Hij is in p4-termen een **OUTER**: hij installeert een sessie over de stages van het model, dient een verzoek in bij de kopstage, verzamelt de tokenstroom en rapporteert wat elke node heeft bijgedragen. Hij bezit geen plaatsing, geen scheduling en geen staat behalve een catalogus van wat er geladen is — bewust klein, want alles wat hij niet beslist, kan ook niet afdrijven.",
      },
      { t: "h2", kick: "Het contract", text: "Vier routes, één token" },
      {
        t: "table",
        head: ["Route", "Wat het beantwoordt"],
        rows: [
          ["`/api/controllers`", "welke modellen geladen zijn, en de staat van elke stage"],
          ["`/api/runtime`", "de operator-wallet en de machines erachter"],
          ["`/api/contributions`", "rijen, eenheden, verzoeken en doorvoer per node"],
          ["`/c/<model>/v1/chat/completions`", "inferentie"],
        ],
      },
      {
        t: "p",
        md: "Elke route behalve `/api/health` vereist een gedeeld service-token, meegestuurd als `X-Kvasir-Service-Token`. Dat token is het **enige** dat tussen het open internet en gratis gebruik van de ring staat, en daarom bindt de bridge op loopback en wordt hij via een tunnel bereikt in plaats van gepubliceerd.",
      },
      { t: "h2", kick: "Wat p4 aan hem overlaat", text: "Twee taken die de engine niet doet" },
      {
        t: "ul",
        items: [
          "**Het chat-template.** p4 geeft de stage-server een ondoorzichtige prompt en past geen eigen beurtformaat toe. De bridge rendert dat van het model — uit de GGUF gelezen en in de catalogus benoemd als `prompt_format`. Sla het over en een instruct-model zet je tekst voort in plaats van hem te beantwoorden, stuurt nooit zijn einde-beurt-token, en loopt elke keer tot de tokenlimiet.",
          "**Het redeneerblok.** Een redeneermodel opent zijn antwoord met denken. De bridge geeft dat terug als `reasoning_content`, los van `content`, en respecteert `enable_thinking: false` door het blok in de prompt te sluiten — anders kan een lange denkfase het hele budget opsouperen en de aanroeper een leeg antwoord geven waarvoor hij al heeft betaald.",
        ],
      },
      { t: "h2", kick: "Plaatsing is niet zijn taak", text: "Waarom hij 409 antwoordt" },
      {
        t: "p",
        md: "De bridge vragen een model te serveren levert **409** op. Welke lagen op welke GPU zitten, onder welke load generation, komt uit een placement plan dat een operator heeft geschreven en geladen; er valt op afstand niets te herladen. De ring-watchdog van de gateway leert dat één keer en stopt met vragen in plaats van iets te blijven proberen dat niet kan werken.",
      },
      {
        t: "callout",
        md: "**Bijdragetellers leven in het geheugen.** Een herstart van de bridge verliest wat de gateway nog niet had opgehaald — hij pollt elke 30 seconden — en de gateway herbaseert in plaats van dubbel te tellen wanneer een teller terugloopt. Een node waarvan de bridge de eigenaar niet kent, wordt **stilzwijgend** overgeslagen, dus een niet-ingestelde operator-wallet leest als \"deze machines hebben niets verdiend\".",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Het publieke toegangspunt: OpenAI/Anthropic-compatibele API's en KVR-afrekening per inferentie.",
    blocks: [
      {
        t: "p",
        md: "De **gateway** is waar ontwikkelaars het netwerk ontmoeten. Elke controller stelt OpenAI-compatibele endpoints beschikbaar (`/v1/chat/completions`, `/v1/models`) en Anthropic-compatibele (`/anthropic/v1/messages`, `/anthropic/v1/models`), allemaal gedragen door hetzelfde geladen model — een bestaande client werkt door alleen de base-URL en sleutel te wisselen.",
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
        md: "Gebruik wordt in KVR verrekend via een driestappenflow — **quote → payment → inference** — zodat een verzoek geprijsd is vóór het draait en de nodes die het bedienden erna worden bijgeschreven. De gateway aggregeert ook een **live modelcatalogus** van elke bereikbare bridge, zodat `/v1/models` weerspiegelt wat het netwerk nu echt kan serveren.",
      },
      {
        t: "ul",
        items: [
          "Gateway-hosts verdienen een **uurlijkse uptime-beloning** voor het online houden van het toegangspunt, plus een **×1.5-bonus** op elke inferentie die ze mee bedienen.",
          "Een publieke gateway draaien vereist het staken van **100.000 KVR** (net als een bridge).",
          "Publieke deployments beschermen operator-toegang met **SIWS + 2FA**; een kale bridge is alleen ontworpen voor vertrouwde host / LAN / VPN.",
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
          "Nodes registreren onder de wallet van hun eigenaar; beloningen worden naar die wallet uitbetaald. Vier afzonderlijke eigenaarswallets, die elk hun laagaandeel verdienen, zijn end-to-end geverifieerd op de testvloot van één operator.",
          "Capaciteitsdata (backend, accumulatieprecisie, resourcebudgetten) bepaalt wat de planner op een node mag plaatsen — en, in de zwerm, welke rangen hij mag bedienen.",
          "Een node die geen resource-monitoring kan leveren wordt uitgesloten van adaptief laden in plaats van blind vertrouwd.",
        ],
      },
    ],
  },

  p4: {
    title: "p4",
    summary: "De engine achter Kvasir: een event-geadresseerd protocol waarin agents nodes bezitten, stage-servers lagen vasthouden, en plaatsing iets is wat een operator vastlegt in plaats van iets wat het netwerk raadt.",
    blocks: [
      {
        t: "p",
        md: "**p4** draait één model over meerdere machines door het in **stages** te knippen — aaneengesloten plakken van zijn lagen — en elke stage een eigen proces te geven. Een **agent** bezit de nodes op een host: hij start stage-servers, routeert events ertussen en staat in voor hun levenscyclus. Er is geen scheduler die bepaalt waar iets heen gaat; een operator schrijft een placement plan, laadt het, en het netwerk serveert daarna precies dat.",
      },
      {
        t: "code",
        caption: "Het verzoekpad door een p4-deployment.",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "Adressering", text: "Een stage belt de agent van de volgende stage" },
      {
        t: "p",
        md: "Wanneer een stage zijn lagen af heeft, geeft hij het resultaat door aan de volgende stage door zijn eigen agent te vragen een verbinding te openen naar **de agent van die stage, op het adres dat die agent adverteert**. Het geadverteerde adres is daarom niet cosmetisch: het moet bereikbaar zijn vanaf elke andere host in de ring, en het hoort het snelste netwerk te noemen dat ze delen. Adverteer loopback en een ring van twee hosts belt stilletjes zichzelf.",
      },
      { t: "h2", kick: "Levenscyclus", text: "Eén getal bindt een load samen" },
      {
        t: "ul",
        items: [
          "**De load generation wordt gekozen door wie laadt** en wordt op exacte gelijkheid gecontroleerd bij elke sessie, inferentie, verrekening en unload. Hij wordt nergens op de machines vastgelegd, dus de lader schrijft hem naar schijf *voordat* het eerste commando vertrekt — zonder dat getal kan een geladen model niet eens meer worden afgebroken.",
          "**De generation van een node en de load generation zijn hetzelfde getal.** De adapter vergelijkt de bron-generation van een release-bon met de load waar die bij hoort en stopt de node wanneer ze verschillen, dus een ring die met twee verschillende getallen is geladen serveert één verzoek en verliest dan zijn kop.",
          "**Een operationeel journaal is verplicht** voordat een model überhaupt laadt: het is de toelatingsregistratie die een load replay-veilig maakt, geen debughulpmiddel.",
        ],
      },
      { t: "h2", kick: "Wat het niet doet", text: "Bewuste weglatingen" },
      {
        t: "p",
        md: "p4 past **geen chat-template** toe — het stuurt een ondoorzichtige prompt door en verwacht dat de aanroeper het beurtformaat van het model al heeft gerenderd. Het neemt **geen plaatsingsbeslissingen**. En het kent geen begrip van wie er betaald moet worden: stages rapporteren de tokenrijen die ze hebben gedraaid, en verrekening is andermans contract. Elk daarvan is een naad die Kvasir invult in de [bridge](/wiki/bridge), waardoor de engine smal genoeg blijft om upstream te kunnen volgen.",
      },
      {
        t: "callout",
        md: "**De agent en de native stage-server zijn één release.** Een agent die uit een nieuwere tree is gebouwd faalt bij READY op een ontbrekende capaciteit in de HELLO van de stage-server — nadat het hele model is geladen. Bouw beide uit dezelfde checkout.",
      },
    ],
  },
  "in-flight-ring": {
    title: "In-flight ring",
    summary: "Een pipeline die nooit leegloopt: meerdere verzoeken bezetten tegelijk verschillende stages, zodat geen stage hoeft te wachten op de stage ervoor.",
    blocks: [
      {
        t: "p",
        md: "Kvasir serveert een model als een **pipeline van stages**, die elk een aaneengesloten plak van zijn lagen vasthouden. Een stage draait zijn lagen en geeft de grens door — een hidden state, geen gewichten — aan de volgende. Geen enkele stage bezit het hele model, en er zit niets midden in het datapad: de bridge dient in bij de kop en leest van de staart, terwijl de stages hun resultaten via hun eigen agents aan elkaar doorgeven.",
      },
      { t: "h2", kick: "Het in-flight-deel", text: "Waarom een leeggelopen pipeline het grootste deel van de machine verspilt" },
      {
        t: "p",
        md: "Als een pipeline eerst één verzoek afmaakt voordat hij het volgende toelaat, staat op elk moment elke stage op één na stil — een ring met vier stages draait op een kwart van zijn hardware. Het **in-flight**-ontwerp houdt meerdere verzoeken tegelijk in beweging: terwijl stage 3 het ene verzoek decodeert, is stage 0 al bezig met de prefill van een ander. Stages rapporteren hoe lang ze een batch vasthielden en hoe lang ze niets hadden om in te dienen, zodat een uitgehongerde ring er anders uitziet dan een verzadigde.",
      },
      {
        t: "code",
        caption: "Vier stages, drie verzoeken, één moment in de tijd.",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "Samenstelling", text: "Wat een batch precies is" },
      {
        t: "p",
        md: "Rijen uit verschillende verzoeken worden in één fysieke batch verpakt, en die exacte samenstelling wordt doorgegeven aan elke stroomafwaartse stage in plaats van per hop opnieuw bepaald. Dat is wat een prefill en meerdere decodes samen één pass laat delen, en daarom is de grootte van een batch een eigenschap van de load: het plan legt de rij- en micro-batchbreedtes vooraf vast, en die breedtes bepalen het grootste resultaat dat een stage ooit kan teruggeven.",
      },
      {
        t: "callout",
        md: "**Een pipeline heeft minstens twee stages nodig.** Een pipeline met één stage wordt botweg geweigerd — de kop en de staart zijn verschillende rollen, en één node die beide samenvouwt is een andere engine, geen kleinere ring.",
      },
      {
        t: "p",
        md: "De ring is het **latentie**-pad, en zijn granulariteit is een laag. Expert-sharding verwijdert die vloer door binnen een laag te snijden, en sluit aan op hetzelfde serveerweefsel.",
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
        md: "Een **Mixture-of-Experts**-model vervangt de enkele FFN van elke laag door een bank onafhankelijke expert-FFN's plus een **router** die er per token enkele kiest. Step-3.7-Flash, de 428B-MoE die vandaag in bedrijf is, heeft 288 experts per laag met top-8-routering. Qwen3.5-122B-A10B is het uitgewerkte voorbeeld hieronder, omdat dat het model is waarvan de cijfers end-to-end zijn gemeten:",
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
      {
        t: "callout",
        md: "**Engine-status.** Sharding op expertkorrel is gebouwd en gedemonstreerd op Kvasirs vorige engine, en de resultaten hieronder komen uit dat werk. De huidige engine, [p4](/wiki/p4), serveert vandaag op laagkorrel; het overzetten van expert-sharding daarnaartoe is ontworpen en in uitvoering. Waar een detail een tool of een route noemt, is dat de tool of route die op de vorige engine draaide.",
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
        t: "callout",
        md: "**Engine-status.** Sharding op expertkorrel is gebouwd en gedemonstreerd op Kvasirs vorige engine, en de resultaten hieronder komen uit dat werk. De huidige engine, [p4](/wiki/p4), serveert vandaag op laagkorrel; het overzetten van expert-sharding daarnaartoe is ontworpen en in uitvoering. Waar een detail een tool of een route noemt, is dat de tool of route die op de vorige engine draaide.",
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
        md: "**GGUF** is het één-bestandsmodelformaat van het inferentie-engine-ecosysteem: metadata (architectuur, laagaantal, dimensies, kwantisatie) plus de tensoren als rauwe gekwantiseerde bytes (bijv. Q4_K_M). Een placement plan wordt tegen die metadata geschreven — laagbereiken, apparaattoewijzing en grootteschattingen; de serveerkant snijdt de tensorbytes om downloads te produceren.",
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
          "**Verdienkant** — bijdrage-eenheden × laagaandeel × prestatieniveau voor rekenwerk; uurlijkse uptime voor bridge-/gateway-rollen.",
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
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "Rollen **stapelen** — één machine kan rekenwerk + gateway + bridge zijn, en de stromen tellen op.",
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
    summary: "Het staken van 100.000 KVR kwalificeert een wallet om bridge- of gateway-nodes te draaien.",
    blocks: [
      {
        t: "p",
        md: "Staking vergrendelt KVR om een wallet te kwalificeren voor operatorrollen en node-beloningen. Een **bridge**- of **gateway**-node draaien vereist een stake van **100.000 KVR**; gewone rekennodes doen mee zonder stake en verdienen voor de lagen die ze draaien.",
      },
      {
        t: "ul",
        items: [
          "Staken gebeurt in het staking-paneel van het wallet-dashboard: voer een bedrag in, **Stake**, en de positie telt mee voor operatorgeschiktheid en node-beloningen.",
          "De 100k-eis is een **skin-in-the-game-filter** voor de twee rollen waar het verkeer van anderen van afhangt — toegangspunten en het besturingsvlak.",
          "Op devnet wordt gestakete KVR in de staking-vault bewaard; het gestakete bedrag en de node-beloningen zijn zichtbaar in het staking-paneel.",
          "Devnet-KVR om te staken komt uit de distributie-faucet; devnet-SOL voor kosten komt uit de publieke faucet.",
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
        md: "De Kvasir Wallet is **non-custodial by design**: de 12-woorden-herstelzin en de sleutels worden alleen op het eigen apparaat van de gebruiker bewaard, nooit bij een operator. Beloningen worden op Solana direct verrekend naar de eigenaarswallet van elke node — geverifieerd op een testvloot over vier verschillende eigenaarswallets, elk met zijn eigen laagaandeel. Staking werkt op devnet anders: gestakete KVR wordt aangehouden in de treasury van de gateway en bijgehouden in zijn grootboek, totdat een on-chain stakingprogramma wordt uitgebracht.",
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
        md: "Voor publieke deployments wordt operator-toegang tot de gateway geauthenticeerd met **Sign-In With Solana**: de wallet van de operator ondertekent een door de server uitgegeven nonce en bewijst eigendom zonder wachtwoord of bewaarde inloggegevens. Daarbovenop beschermen **TOTP-2FA** en eenmalige back-upcodes de sessie.",
      },
      {
        t: "ul",
        items: [
          "**Nergens wachtwoorden** — de walletsleutel is de identiteit en de nonce voorkomt replay; er is serverzijdig niets te phishen of te lekken.",
          "**TOTP-inschrijving per wallet** wordt in het grootboek van de gateway gepersisteerd, dus 2FA overleeft een herstart.",
          "**Back-upcodes zijn eenmalig** — elke code wordt bij het inloggen verbruikt, voor herstel wanneer het authenticator-apparaat niet beschikbaar is.",
          "**Eerlijk verklaarde reikwijdte** — de bridge en de engine-poorten gaan uit van een vertrouwde host / LAN / VPN; SIWS + 2FA is de laag die *publieke* domeinen veilig blootstelbaar maakt.",
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
        md: "Omdat inferentie **verplicht** in KVR wordt betaald, is het token verbonden met echt gebruik — nut, geen speculatie. Gebruik financiert de KVR die nodes verdienen, wat bijdragen aantrekkelijk houdt, wat de capaciteit vergroot, wat prijs en latency verlaagt, wat meer gebruik aantrekt. Kvasirs scherpste voordeel trekt de lus nog strakker aan: een deelnemer kan **tegelijk consument en leverancier** zijn (een *prosumer*), dus de twee kanten groeien vaak binnen dezelfde mensen.",
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
        md: "Kvasir beloont al **echt werk** (KVR per geserveerde tokens × laagaandeel, niet louter aanwezigheid) en betaalt uit naar de eigen wallet van elke node, wat het moeilijke deel is van het eerlijk maken van uit inkomsten gefinancierde beloningen. De rest — een gebruiksgedreven prijs en een afbouw van emissie→inkomsten — is de economische routekaart die \"meer nodes → goedkoper\" van een intuïtie in een door het protocol afgedwongen regel verandert. Het item **Inferentieprijzen** behandelt de prijskant; **Bijdrage-eenheden** behandelt hoe werk beloning wordt.",
      },
    ],
  },
  "inference-pricing": {
    title: "Inferentieprijzen",
    summary: "Wat een inferentie vandaag in KVR kost, waarom een gedecentraliseerd netwerk structureel goedkoper is, en hoe de prijs geacht wordt te dalen naarmate het aanbod groeit.",
    blocks: [
      {
        t: "p",
        md: "Toegang tot het netwerk is **betalen-per-inferentie**: de gateway geeft een KVR-prijs op voor je verzoek, je wallet betaalt die on-chain, en pas daarna draait de ring het model. De prijsstelling is een kleine, transparante formule — een ondergrens per verzoek plus een tarief per token — vooraf opgegeven en verrekend op het **werkelijke** tokengebruik na generatie.",
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
      {
        t: "callout",
        md: "**Engine-status.** Sharding op expertkorrel is gebouwd en gedemonstreerd op Kvasirs vorige engine, en de resultaten hieronder komen uit dat werk. De huidige engine, [p4](/wiki/p4), serveert vandaag op laagkorrel; het overzetten van expert-sharding daarnaartoe is ontworpen en in uitvoering. Waar een detail een tool of een route noemt, is dat de tool of route die op de vorige engine draaide.",
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
          "**Beloning is per werk.** Overbrugd werk wordt bijgeboekt op het bijdrageregister van de bridge; de gateway delta-crediteert KVR naar je **eigen** wallet. Je hebt een wallet-adres nodig om betaald te worden.",
        ],
      },
      {
        t: "callout",
        md: "De worker spreekt hetzelfde dispatch-protocol als een GPU in het datacenter — `(n_used, n_tokens, cur, sel) → experts` over één langlevende stream. Een partial-shard-worker zet gewoon `n_used = 1`. Die uniformiteit is waarom een telefoon, een CPU-bak en een Blackwell-kaart uitwisselbare leden van dezelfde zwerm zijn.",
      },
    ],
  },
  "bridge-operations": {
    title: "Een bridge beheren",
    summary: "Operatornotities voor het draaien van de bridge en de ring erachter: een plan laden, een herstart overleven, bijdrage laten doorstromen en de engine van het internet af houden.",
    blocks: [
      {
        t: "p",
        md: "De **gateway** (publiek toegangspunt) en de **bridge** (het gezicht van de engine) zijn de twee langlevende services die een operator gezond houdt, met daarachter de p4-agents en hun stage-servers. De engine spreekt helemaal geen authenticatie — hij gaat ervan uit dat machines die elkaar kunnen bereiken dat ook horen te doen — dus alles wat publiek is komt samen op de gateway, en de bridge wordt via een tunnel bereikt in plaats van gepubliceerd.",
      },
      { t: "h2", kick: "Laden", text: "Een plan, en het getal waaronder het geladen wordt" },
      {
        t: "ul",
        items: [
          "**Plaatsing is een plan dat je schrijft**, geen verzoek dat je doet: de bridge antwoordt `409` aan wie hem vraagt te serveren. Genereer het plan, draai het droog, en laad het daarna met `--confirm`.",
          "**De load generation wordt naar schijf geschreven voordat het eerste commando vertrekt.** Hij wordt gekozen door de lader, bij elke sessie en bij elke unload op exacte gelijkheid gecontroleerd, en nergens op de machines vastgelegd — raak je hem kwijt, dan kan een geladen model niet eens meer worden afgebroken.",
          "**De node generation is datzelfde getal.** Laad een ring met twee verschillende getallen en hij serveert precies één verzoek voordat de kop stopt; de volgende sessie blijft halfgeladen hangen. Gebruik per load een verse waarde, anders botst een registratie die een mislukte poging heeft achtergelaten ermee.",
          "**De agent weigert te laden zonder zijn operationele journaal.** Dat is de toelatingsregistratie die een replay-veilige load nodig heeft, geen debugvlag.",
        ],
      },
      { t: "h2", kick: "Een herstart overleven", text: "Wat terugkomt en wat niet" },
      {
        t: "ul",
        items: [
          "**Een herstart van een agent laat zijn nodes vallen.** Stage-servers bestaan alleen tijdens runtime; het model moet opnieuw uit het plan worden geladen. Dat is de herstelprocedure, niet het falen ervan.",
          "**De gateway herlaadt het niet voor je.** Zijn ring-watchdog merkt op dat een model niet meer serveert, leert uit de `409` van de bridge dat plaatsing extern is, zegt dat één keer en stopt met vragen.",
          "**Bijdragetellers leven in het geheugen van de bridge.** De gateway pollt elke 30 s en schrijft het verschil bij; een herstart verliest alleen wat nog niet was opgehaald, en de gateway herbaseert in plaats van dubbel te betalen wanneer een teller terugloopt.",
        ],
      },
      { t: "h2", kick: "Afgrendelen", text: "De engine staat niet naar het internet gericht" },
      {
        t: "ul",
        items: [
          "Bind de bridge op loopback en geef hem een service-token. Zonder dat token authenticeert hij niemand, en alles wat hem bereikt kan de ring gratis draaien — hij zegt dat bij het opstarten, in plaats van je het later te laten ontdekken.",
          "Agents adverteren het adres dat andere agents bellen. Gebruik het snelste netwerk dat de hosts delen, nooit loopback tussen hosts, en houd dat netwerk van het publieke internet af.",
          "Moet er iets op een publiek IP staan, onthoud dan dat Dockers gepubliceerde poorten **ge-DNAT worden vóór de INPUT-chain**, dus een regel op `dport` matcht niet. Filter in de `DOCKER-USER`-chain op de originele bestemmingspoort van conntrack (`--ctorigdstport`), en persisteer met een systemd-oneshot geordend `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Valkuilen", text: "Twee die echt tijd kosten" },
      {
        t: "ul",
        items: [
          "**Een niet-ingestelde operator-wallet leest als nul verdiensten.** De gateway slaat elke bijdrageregel zonder eigenaar over en logt er niets over. De nodes lijken stil te staan terwijl ze serveren.",
          "**`pkill` matcht zijn eigen commandoregel.** `ssh host 'pkill -f server.js; ...'` doodt de shell die het draait. Zet het patroon in een scriptbestand in plaats van in het remote commando, gebruik een character class (`server[.]js`), en onthoud dat een proces dat als kale `node server.js` is gestart geen pad heeft om op te matchen — vind het dan via zijn luisterpoort.",
        ],
      },
    ],
  },
  "wan-interconnect": {
    title: "WAN-interconnect (200G-optiek)",
    summary: "Hoe rekensites koppelen op 200 Gb/s over een kamer, een campus of een stad: welke optiek op welke afstand, wat waar in past, en wat er nodig is om echt line rate te halen.",
    blocks: [
      {
        t: "p",
        md: "Wanneer twee sites allebei publieke routes hebben, hoort het expert-dispatch-datavlak een **directe link** te zijn — de relay is voor edges zonder eigen adres. Dit item is het concrete recept om die directe link 200 Gb/s-klasse te maken met catalogusonderdelen. Eén regel ordent alles: **de glasvezel is snelheidsneutraal glas; de snelheid zit in de pluggable aan elk uiteinde.**",
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
          "**NIC-kant** — kaarten van de ConnectX-6/7-klasse bieden QSFP56-cages; DAC/AOC/FR4/LR4/ER4 zitten allemaal direct in de NIC. Een host van de GB10-klasse heeft al twee 200 GbE QSFP-poorten aan boord, dus een link tussen twee sites heeft precies één kabel en nul nieuwe hardware nodig.",
          "**Switch-kant** — coherente ZR+-optiek is QSFP-DD-formfactor en hoort in een switch of router; de NIC van de site sluit dan op 200G via een korte DAC aan op die switch. Gebruik deze tier wanneer de verre site tientallen kilometers weg is.",
          "**De glasvezel zelf** — standaard single-mode (G.652) duplex-LC-paren, geleased als dark fiber per streng. Hetzelfde glas draagt vandaag 100G en later 400G; upgrades zijn een moduleverwisseling, nooit graafwerk.",
          "**Voorbij ~120 km** — stop je met onderdelen kopen en ga je een wavelength leasen bij een carrier; de demarcatie is een Ethernet-overdracht op je switch.",
        ],
      },
      {
        t: "code",
        caption: "Drie referentieopstellingen, goedkoopste eerst.",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "Stap 3 · echt 200G halen", text: "Line rate is een configuratie, geen aankoop" },
      {
        t: "ul",
        items: [
          "Gebruik **RDMA (RoCE)** voor de dispatch-stream waar beschikbaar — hosts van de GB10-klasse voeden de NIC via gesplitste PCIe-links, en de gemeten volle snelheid (~185–190 Gb/s) verschijnt onder RoCE met een correct gemapte topologie; een verkeerd gemapt pad topt op bijna de helft van het tarief en ongetunede platte TCP landt veel lager.",
          "Schakel **jumbo frames (MTU 9000)** end-to-end in en houd `TCP_NODELAY` aan op de dispatch-sockets (de bridge zet het al).",
          "Verwacht te *verifiëren*, niet aan te nemen: draai een perftest tussen de sites na elke fysieke wijziging — het verschil tussen 95 en 190 Gb/s is onzichtbaar tot het gemeten is.",
          "Houd de **443-relay als fallback-pad** — het belbeleid is direct-eerst voor publieke peers, relay voor NAT. De taak van de relay is bereik, de taak van de directe link is snelheid.",
        ],
      },
      {
        t: "p",
        md: "Waarom dit voor de architectuur uitmaakt: decodeerlatentie wordt begrensd door de round-trip-tijd (~5 µs/km in glasvezel — natuurkunde, onafhankelijk van bandbreedte), dus een dikke pijp koopt **prefill-snelheid, doorvoer van gebundelde dispatch en vrijwel directe distributie van expert-plakken**, geen lagere latentie per token. Dat is precies de rol van de site-tier in het twee-tier-ontwerp: capaciteit in de dikke-pijp-tier, bereik in de relay-tier.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Belasting-adaptieve schaling",
    summary: "Kvasirs MoE-serveerpad groeit en krimpt met het verkeer: de coördinator schakelt bewezen workers opnieuw in onder verzadiging, en de bridge werft inactieve nodes door de expertvraag te verhogen — alles pull-gebaseerd, zodat ook NAT-apparaten meedoen.",
    blocks: [
      {
        t: "p",
        md: "Kvasirs MoE-serveerpad schaalt elastisch mee met de belasting, in twee samenwerkende lagen. Bij rust serveert de coördinator alles lokaal voor het snelste pad per token; bij verzadiging laten de twee lagen hieronder de zwerm groeien — en krimpen hem weer wanneer de piek voorbij is.",
      },
      {
        t: "callout",
        md: "**Engine-status.** Sharding op expertkorrel is gebouwd en gedemonstreerd op Kvasirs vorige engine, en de resultaten hieronder komen uit dat werk. De huidige engine, [p4](/wiki/p4), serveert vandaag op laagkorrel; het overzetten van expert-sharding daarnaartoe is ontworpen en in uitvoering. Waar een detail een tool of een route noemt, is dat de tool of route die op de vorige engine draaide.",
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
      { t: "h2", kick: "Laag 2", text: "Besturingsvlakkant: belasting-adaptieve werving" },
      {
        t: "p",
        md: "Het besturingsvlak bewaakt elke MoE-coördinator en laat de workerpool groeien wanneer dat nodig is:",
      },
      {
        t: "ul",
        items: [
          "Een achtergrondlus pollt de slots van elke coördinator en registreert verzadiging per model.",
          "Zolang een model verzadigd is, wordt zijn **effectief expert-replicadoel** verhoogd (base + boost). De dekkingsmarkt leest dan al-gedekte experts opnieuw als schaars, en een model met **geen** live workers wordt geseed vanuit zijn GGUF-metadata (aantal experts), zodat de vraag zelfs vanaf nul zichtbaar is.",
          "Inactieve nodes pollen de vraagmarkt (`/api/expert-volunteer`) en krijgen een `(layer, expert-range)`-plak toegewezen om te serveren. Ze downloaden de plak, bellen de relay en registreren dekking; het besturingsvlak bedraadt ze automatisch in de dispatch-map van de coördinator.",
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
