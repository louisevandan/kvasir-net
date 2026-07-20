/* German (Deutsch) — mirrors the shape of en.ts exactly.
   Technische Begriffe und Bezeichner bleiben unverändert (KVR, linkcpp,
   Inferenz-Engine, GPU, CPU, NPU, OpenAI, Anthropic, Solana, GGUF, MoE, SIWS, 2FA,
   TOTP, ring runtime, MIT, tok/s, Layer, Qwen3.5-122B usw.). Die Ehrlichkeits-
   Rahmung bleibt erhalten (Devnet, Utility-Token, keine Investition, non-custodial). */
import type { Dict } from "./types";

export const de: Dict = {
  nav: {
    why: "Warum",
    how: "Funktionsweise",
    contributors: "Mitwirkende",
    developers: "Entwickler",
    token: "KVR",
    rewards: "Belohnungen",
    tech: "Technik",
    roadmap: "Roadmap",
    careers: "Karriere",
    technology: "Technologie",
    blog: "Blog",
    wiki: "Wiki",
  },

  actions: {
    runNode: "Node betreiben",
    joinNode: "Beitreten",
    useApi: "API nutzen",
    getApiKey: "API-Schlüssel erhalten",
    getApiAccess: "API-Zugang erhalten",
    runNodeGuide: "Node-Anleitung",
    readDocs: "Dokumentation lesen",
    viewGithub: "Auf GitHub ansehen",
    github: "GitHub",
    menu: "Menü",
    copy: "kopieren",
    copied: "kopiert ✓",
    language: "Sprache",
  },

  hero: {
    eyebrow: "DePIN · Dezentrale KI — jenseits des Monopols",
    headline1: "Rechenleistung einbringen.",
    headline2: "KVR verdienen.",
    sub: "Kvasir verteilt große offene Modelle mit linkcpp über gemeinsam genutzte Hardware, sodass kein einzelner Node das gesamte Modell hält. Steuere eine GPU, CPU, NPU — oder sogar ein Smartphone — bei und verdiene KVR für die Layer, die du ausführst.",
    badges: [
      "Läuft auf GPU · CPU · NPU · Smartphone",
      "OpenAI- + Anthropic-kompatibel",
      "Quelloffen einsehbar (BSL)",
      "Solana-Devnet",
    ],
    ringCenter: "ein Ring · kein Master",
    topologyCaption:
      "Ein Ring aus Geräten — eine GPU, CPU, NPU und ein Smartphone — die jeweils einige der 49 Layer halten. Jeder Node führt seinen Abschnitt aus und gibt nur die Hidden-State-Grenze an seinen Nachbarn weiter; der letzte gibt das Token durch den Ring zurück. Kein Node hält das gesamte Modell, und es gibt keinen zentralen Master — illustrativ.",
  },

  thesis: {
    eyebrow: "Warum dezentrale KI",
    title: "KI sollte nicht einer Handvoll Unternehmen gehören",
    lede: "Spitzen-Inferenz konzentriert sich hinter wenigen abgeschotteten Rechenzentren — geschlossene Gewichte, nutzungsbasierte Abrechnung, eine Rechnung an einen Eigentümer. Kvasir weist in die andere Richtung: offene Modelle, bereitgestellt über ein erlaubnisfreies Netzwerk aus Alltagsgeräten — im Besitz der Menschen, die es betreiben, und von ihnen verdient.",
    centralizedLabel: "Zentralisierte KI",
    centralizedPoints: [
      "Wenige Hyperscaler besitzen die GPUs",
      "Modelle und Infrastruktur hinter geschlossenen APIs",
      "Du mietest Zugang; der Wert fließt nach oben",
      "Undurchsichtig — du vertraust dem Betreiber",
    ],
    kvasirLabel: "Kvasir",
    kvasirPoints: [
      "Jedes Gerät tritt einem Peer-to-Peer-Ring bei — kein zentraler Master",
      "Quelloffen einsehbare linkcpp-Engine — BSL-lizenziert und vollständig einsehbar",
      "Mitwirkende verdienen KVR für die tatsächliche Rechenleistung, die sie beisteuern",
      "Non-custodial — deine Schlüssel, dein Node, deine Belohnungen",
    ],
  },

  origin: {
    eyebrow: "Der Name",
    title: "Kvasir — Weisheit, aus vielen geboren, mit allen geteilt",
    mythLabel: "Nordischer Mythos",
    myth: "In der nordischen Mythologie wurde Kvasir geboren, als die Asen und die Wanen ihren Krieg beendeten und Frieden schlossen: Jeder spuckte in ein einziges Gefäß, und aus ihrer vereinten Essenz erhob sich das weiseste Wesen, das je gelebt hat — eines, das jede Frage beantworten konnte. Als er erschlagen wurde, wurde sein Blut zum Met der Dichtkunst gebraut, einem Trunk, der jedem, der davon trank, Weisheit verlieh.",
    whyLabel: "Warum wir ihn gewählt haben",
    mappings: [
      {
        from: "Aus jedem Gott zusammengeführt, keinem gehörend",
        to: "Intelligenz, zusammengesetzt aus den Geräten vieler Mitwirkender — kein einzelner Eigentümer.",
      },
      {
        from: "Das weiseste Wesen, das jede Frage beantwortet",
        to: "Ein offenes Inferenz-Netzwerk, das jeder abfragen kann.",
      },
      {
        from: "Ein Met, der Weisheit mit allen teilte",
        to: "Offener Zugang und KVR-Belohnungen für jeden Mitwirkenden, der sich einbringt.",
      },
    ],
    footnote: "On-Chain trägt der Token den Namen Kvasir (KVR) — der Met, verteilt.",
  },

  how: {
    eyebrow: "Funktionsweise",
    title: "Ein Modell, viele Geräte, Bezahlung pro Layer",
    lede: "Kein einzelner Node hält das gesamte Modell. Eine Anfrage fließt über den Layer-Pfad, und jeder Node wird für genau die Arbeit belohnt, die er geleistet hat.",
    steps: [
      {
        title: "Aufteilen",
        body: "Das Modell wird in zusammenhängende Layer-Fenster unterteilt. Jedes Gerät speichert dasselbe Modell, lädt aber nur sein Fenster — kein Node hält das Ganze.",
        note: "Qwen3.5-122B · 49 Layer · Rank-Manifest",
      },
      {
        title: "Bereitstellen",
        body: "Die Anfrage tritt in den Ring ein. Jeder Node führt seine Layer aus und gibt nur die Hidden-State-Grenze an seinen Nachbarn weiter; der letzte Node sampelt das Token und schickt es durch den Ring zurück — kein zentraler Master.",
        note: "ring runtime · OpenAI/Anthropic-kompatibel",
      },
      {
        title: "Belohnen",
        body: "Jeder Node verdient KVR, gewichtet nach den ausgeführten Layern — sein Anteil an den erzeugten Tokens — abgerechnet auf Solana an die eigene Wallet des Nodes.",
        note: "units += (out_tokens / 1000) × layer_share",
      },
    ],
  },

  contributors: {
    pill: "Für Mitwirkende",
    title: "Ungenutzte Rechenleistung in KVR verwandeln",
    lede: "Richte ein unterstütztes Gerät auf das Netzwerk aus, und es beginnt, Layer bereitzustellen. Du verdienst KVR proportional zu den Layern, die dein Node ausführt — deine Schlüssel bleiben in deiner eigenen Wallet.",
    nonCustodial:
      "Non-custodial by Design — die Betreiber-Anmeldung erfolgt per Wallet-Signatur (Sign-In With Solana) mit optionaler 2FA.",
    points: [
      {
        title: "Jedes Gerät kann teilnehmen",
        body: "GPUs, CPUs, NPUs und Smartphones führen heute alle Layer aus. Eine ring runtime auf Peer-to-Peer-Basis lässt jedes Gerät nur wenige Layer halten und nur kleinen Grenzzustand an seinen Nachbarn weitergeben — also kein Gatekeeper und kein einzelner Eigentümer.",
      },
      {
        title: "Layer-Share-Belohnungen",
        body: "Belohnungen sind proportional zu den Layern, die dein Node ausführt, nicht zu vager Teilnahme. 1 Unit ≈ 1k Tokens × dein Layer-Anteil pro Inferenz.",
      },
      {
        title: "Leistungsstufen",
        body: "Der gemessene Durchsatz bestimmt eine Stufe — S (×1.5), A (×1.25), B (×1.0), C (×0.7) —, die deine verdienten Units multipliziert. Schnellere Hardware, höherer Multiplikator.",
      },
      {
        title: "Uptime für Infrastruktur-Rollen",
        body: "Nodes, die Infrastruktur-Rollen ausfüllen, erhalten zusätzlich stündliche Uptime-Belohnungen dafür, dass sie das Netzwerk erreichbar halten.",
      },
    ],
    devicesLabel: "Unterstützte Geräte",
    statusLive: "Live",
    statusComing: "Demnächst",
    deviceDetails: [
      "CUDA · ROCm · Metal · Vulkan",
      "x86-64 · ARM",
      "On-Device-Beschleuniger",
      "Smartphones & Edge · ring runtime",
    ],
  },

  developers: {
    pill: "Für Entwickler",
    title: "Ein Endpunkt, gestützt von vielen Geräten",
    lede: "Behalte deinen bestehenden OpenAI- oder Anthropic-Client. Richte ihn auf das Kvasir-Gateway aus und bezahle pro Inferenz in KVR — ohne Umschreiben.",
    points: [
      "OpenAI-kompatibel: Drop-in für /v1/chat/completions, /v1/responses, /v1/models",
      "Anthropic-kompatibel: /anthropic/v1/messages und /anthropic/v1/models",
      "Pay-per-Inference in KVR: quote → payment → inference",
      "Live-Modellkatalog, aggregiert aus erreichbaren Hubs",
    ],
    codeHeader: "POST /v1/chat/completions",
  },

  token: {
    eyebrow: "Token & Belohnungen",
    title: "KVR bezahlt für Rechenleistung — und belohnt sie",
    lede: "KVR ist die Einheit, die Entwickler für Inferenz ausgeben, und die Einheit, die Mitwirkende für die Layer verdienen, die sie ausführen. Belohnungen werden aus echter Arbeit berechnet, nicht aus Teilnahme.",
    facts: [
      { k: "Symbol", v: "KVR", note: "On-Chain-Name “Kvasir”, 6 decimals" },
      { k: "Chain", v: "Solana", note: "derzeit Devnet" },
      { k: "Bezahlt für", v: "Inferenz", note: "Pay-per-Request über das Gateway" },
      { k: "Belohnungen", v: "Rechenleistung", note: "Layer-Share × Leistungsstufe" },
    ],
    whatForTitle: "Wofür KVR da ist",
    whatForBody:
      "Ein Token, beide Richtungen: Entwickler geben KVR aus, um Inferenz über das Gateway auszuführen, und Mitwirkende verdienen KVR für die Rechenleistung, die ihre Nodes bereitstellen. Es ist die Verrechnungseinheit des Netzwerks für echte Arbeit — sieh unten, wie sich die Belohnungen nach Rolle aufschlüsseln.",
    whatForChips: ["pro Inferenz zahlen", "pro Layer belohnen", "auf Solana abrechnen"],
    custodyTitle: "Non-custodial Wallet",
    custodyBody:
      "Belohnungen werden an die eigene Owner-Wallet jedes Nodes abgerechnet. Die Schlüssel liegen in der Wallet des Nutzers — Browser, Desktop oder Mobil — niemals bei einem Betreiber. Über vier verschiedene Owner-Wallets verifiziert, die jeweils ihren Layer-Anteil verdienen.",
    custodyChips: ["web", "desktop", "iOS", "Android"],
    devnetStrong: "Devnet, Utility-Token.",
    devnetBody:
      "KVR läuft derzeit auf dem Solana-Devnet und ist ein Utility-/Beitrags-Token — kein handelbarer Vermögenswert, kein Preis und keine Investition. Nichts hier ist eine Finanzberatung oder ein Versprechen einer Rendite.",
  },

  network: {
    eyebrow: "Netzwerk & Belohnungen",
    title: "Jede Rolle im Netzwerk verdient KVR",
    lede: "Der Ring aus Compute-Nodes wird von Hub- und Gateway-Rollen koordiniert. Jede wird in KVR für das bezahlt, was sie tatsächlich leistet — Rechenleistung für die ausgeführten Layer, Infrastruktur für die aufrechterhaltene Uptime.",
    roles: [
      {
        role: "Compute-Node",
        tagline: "Führt die Layer des Modells aus",
        body: "Hält einige zusammenhängende Layer im Ring und führt sie für jede Anfrage aus. Verdient pro Beitrags-Unit (≈1k bereitgestellte Tokens), gewichtet nach seinem Layer-Anteil und skaliert nach seiner Leistungsstufe.",
        earns: "pro Unit × Layer-Anteil × Stufe",
      },
      {
        role: "Gateway-Host",
        tagline: "Öffentlicher Zugang + Abrechnung",
        body: "Stellt das OpenAI/Anthropic-Gateway bereit und rechnet KVR-Zahlungen ab. Verdient eine stündliche Uptime-Belohnung dafür, den Zugangspunkt online zu halten, plus einen ×1.5-Bonus auf jede Inferenz, an deren Bereitstellung er beteiligt ist.",
        earns: "stündliche Uptime + ×1.5 Inferenz-Bonus",
      },
      {
        role: "Hub-Host",
        tagline: "Die Steuerungsebene",
        body: "Erkennt Geräte, plant die Layer-Platzierung und orchestriert den Ring. Die kritischste Rolle — daher verdient er die höchste stündliche Uptime-Belohnung dafür, das Netzwerk koordiniert zu halten.",
        earns: "höchste stündliche Uptime",
      },
    ],
    rolesNote:
      "Rollen lassen sich kombinieren: Eine Maschine kann gleichzeitig Compute, Gateway und Hub sein, und ihre Belohnungen summieren sich. Alles wird in KVR an die eigene Wallet des Nodes abgerechnet.",
    formulaTitle: "Wie Belohnungen berechnet werden",
    formulaLabels: ["Compute-Units", "Effektiv", "Infra-Uptime"],
    tiersTitle: "Leistungsstufen",
    tiersBody:
      "Die gemessene Decode-Geschwindigkeit eines Nodes bestimmt seinen Multiplikator — schnellere Hardware verdient proportional mehr für dieselbe Arbeit.",
  },

  tech: {
    eyebrow: "Hinter den Kulissen",
    title: "linkcpp — die Engine hinter dem Netzwerk",
    lede: "linkcpp ist der offene Control-Hub, der Alltags-Hardware in eine verteilte Inferenz-Engine verwandelt. Seine ring runtime lässt jedes Gerät nur wenige Layer halten und den Hidden State an seinen Nachbarn weitergeben — kein zentraler Master — während die unveränderte Inferenz-Engine-Datenebene ungeforkt bleibt.",
    taglineCaption: "— linkcpp, in eigenen Worten",
    points: [
      {
        title: "Ring runtime",
        body: "Jedes Gerät speichert dasselbe Modell und lädt nur sein Layer-Fenster, dann öffnet es eine Verbindung zu seinem Vorgänger und eine zu seinem Nachfolger. Hidden-State-Grenzen zirkulieren durch den Ring, und der letzte Rank gibt das Token zurück — kein zentraler Master, kein Node hält alles.",
      },
      {
        title: "linkcpp-Control-Hub",
        body: "Ein einziger dockerisierter Hub — die Steuerungsebene, die der RPC-Datenebene von Inferenz-Engine fehlte. Er erkennt Geräte, plant die Layer-Platzierung, startet die unveränderten Worker und stellt die Gateways bereit. Quelle verfügbar unter der Business Source License (BSL).",
      },
      {
        title: "Verteilte Layer-Platzierung",
        body: "linkcpp liest GGUF-Metadaten und berechnet zusammenhängende Layer-Fenster pro Node über ein Rank-Manifest, plus optionales MoE-Expert-FFN-Offloading in den Node-RAM.",
      },
      {
        title: "SIWS- + 2FA-Sicherheit",
        body: "Für öffentliche Deployments erfolgt der Betreiber-Zugang über eine Sign-In-With-Solana-Signatur auf einer Server-Nonce, plus TOTP-2FA und Einmal-Backup-Codes — sowohl auf Hub als auch auf Gateway.",
      },
    ],
    openText:
      "Die Quelle ist unter der Business Source License (BSL) verfügbar — für Entwicklung und Tests kostenlos lesbar, ausführbar und erweiterbar. Produktive (kommerzielle) Nutzung erfordert eine gekaufte Lizenz.",
  },

  roadmap: {
    eyebrow: "Roadmap",
    title: "Heute live — und wohin es geht",
    lede: "Eine klare Trennlinie zwischen dem, was bereits läuft, und dem, was geplant ist. Wir stellen die Roadmap nicht als bereits ausgeliefert dar.",
    items: [
      {
        phase: "Jetzt",
        title: "Inferenz auf jedem Gerät, live",
        body: "GPUs, CPUs, NPUs und Smartphones stellen Layer über die ring runtime bereit. 122B lief aufgeteilt auf 4 GPUs; Beiträge werden durchgängig gutgeschrieben; Non-custodial Wallets sind für web/desktop/iOS/Android verfügbar; der Zugang ist auf öffentlichen Domains abgesichert.",
      },
      {
        phase: "Demnächst",
        title: "Mainnet & On-Chain-Abrechnung",
        body: "Heute läuft alles auf dem Solana-Devnet mit einem Off-Chain-Abrechnungsdienst. Ein On-Chain-Belohnungsprogramm und Mainnet sind geplant.",
      },
      {
        phase: "Demnächst",
        title: "Ein erlaubnisfreies globales Netzwerk",
        body: "Die aktuellen Demos liefen auf der Hardware eines einzelnen Betreibers. Als Nächstes folgt die Öffnung des Netzwerks, sodass jeder, überall, ein Gerät anschließen und verdienen kann — ohne Gatekeeper.",
      },
    ],
  },

  proof: {
    pill: "In diesem Build nachgewiesen",
    title: "Echte verteilte Inferenz, live auf öffentlichen Domains",
    items: [
      "Parameter, aufgeteilt auf 4 AMD MI250 GPUs bereitgestellt",
      "verschiedene Owner-Wallets, die jeweils ihren Layer-Anteil verdienen",
      "API-Schnittstellen — OpenAI- + Anthropic-kompatibel",
      "Wallet-Plattformen — web · desktop · iOS · Android",
    ],
    strip:
      "122B bereitgestellt auf 4 GPUs · OpenAI- + Anthropic-kompatibel · Wallets auf web / desktop / iOS / Android · live auf öffentlichen Domains",
  },

  footer: {
    ctaTitle: "Bring deine GPU ins Netzwerk.",
    ctaBody:
      "Betreibe einen Node und verdiene KVR für die Layer, die du bereitstellst, oder binde das Gateway mit einem OpenAI/Anthropic-kompatiblen Endpunkt in deine App ein.",
    tagline:
      "Die Netzwerk-Marke für dezentrale KI-Inferenz, angetrieben vom linkcpp-Control-Hub — eine quelloffen einsehbare (BSL) Engine, die große Modelle über Alltagsgeräte verteilt (auf einer unveränderten Inferenz-Engine-Datenebene).",
    disclaimerStrong: "Haftungsausschluss.",
    disclaimer:
      "KVR ist ein Utility-/Beitrags-Token, das zur Bezahlung von Inferenz und zur Belohnung von Rechenleistung dient. Es läuft heute auf dem Solana-Devnet — es ist kein handelbarer Mainnet-Vermögenswert, und nichts hier ist ein Angebot, ein Preis oder ein Versprechen einer finanziellen Rendite. Belohnungen spiegeln tatsächlich beigesteuerte Rechenleistung wider, nicht Teilnahme.",
    rights: "© 2026 Kvasir · linkcpp. Engine unter der Business Source License (BSL) — kostenlos für Entwicklung und Tests; produktive Nutzung erfordert eine Lizenz.",
  },

  guide: {
    home: "Start",
    eyebrow: "Anleitung für Node-Betreiber",
    headline1: "Rechenleistung einbringen,",
    headline2: "einen Node betreiben.",
    sub: "Erstelle eine Wallet, stake KVR und verbinde dann dein Gerät mit dem Kvasir-Netzwerk, um KVR für die von dir bereitgestellte Rechenleistung zu verdienen. Wähle unten deine Plattform für Download-, Installations- und Ausführungsschritte.",
    badgeCustody: "Non-custodial — deine Schlüssel",
    badgeDevices: "GPU · CPU · NPU",
    badgeToken: "Solana devnet · KVR",
    devnetNote: "KVR ist ein Utility-Token im Solana-devnet — kein handelbarer Vermögenswert im mainnet und keine finanzielle Rendite.",
    reqTitle: "Hub · Gateway-Betreiber-Voraussetzung",
    reqBody: "Um einen Hub-Node oder einen Gateway-Node zu betreiben, musst du 100.000 KVR in deiner Wallet staken. Reguläre Compute-Nodes treten ohne diese Voraussetzung bei und verdienen für die Layer, die sie ausführen.",
    tabDesktop: "Desktop",
    tabMobile: "Mobil",
    soon: "Demnächst verfügbar",
    download: "Download",
    desktopTitle: "Kvasir Wallet · Desktop-App",
    desktopSub: "macOS · Windows · Linux — Wallet und Node in einer App.",
    desktop: [
      { title: "App herunterladen", body: "Lade oben das Kvasir-Wallet-Installationsprogramm für dein Betriebssystem herunter. Eine GPU (NVIDIA / AMD / Apple Silicon) wird empfohlen, aber CPU funktioniert auch.", body2: "" },
      { title: "Installieren und öffnen", body: "Führe das Installationsprogramm aus und öffne dann Kvasir Wallet. Wenn unter macOS die Warnung „nicht verifizierter Entwickler“ erscheint, erlaube dies in den Systemeinstellungen → Datenschutz & Sicherheit.", body2: "" },
      { title: "Wallet erstellen", body: "Wähle Neue Wallet erstellen. Notiere deine 12-Wörter-Wiederherstellungsphrase und bewahre sie sicher auf — sie kann bei Verlust nicht wiederhergestellt werden. Lege anschließend eine Passphrase fest, um die App zu entsperren. Die Schlüssel sind non-custodial und werden ausschließlich auf diesem Gerät gespeichert.", body2: "" },
      { title: "KVR einzahlen & staken", body: "Empfange etwas devnet-SOL (für Gebühren) und KVR (zum Staken) an der Empfangsadresse deiner Wallet. Gib im Staking-Bereich des Dashboards einen Betrag ein und wähle Staken, um APR-Zinsen zu verdienen und dich für Node-Rewards zu qualifizieren.", body2: "" },
      { title: "Node konfigurieren", body: "Wähle in den Node-Einstellungen das Compute-Backend dieses Geräts (CUDA / ROCm / Metal / CPU) und entscheide dich für Lokaler Shard (empfohlen) — dabei läuft der Layer-Shard lokal, und es wird nur ein kleiner Grenzzustand weitergeleitet, der schnellste Modus.", body2: "" },
      { title: "Node ausführen", body: "Aktiviere Node ausführen (live), um dieses Gerät unter deiner Wallet (Eigentümer) im Netzwerk zu registrieren und online zu bringen.", body2: "Für einen echten GPU-Compute-Node führe zusätzlich den nativen Agenten unten aus. Der Planner des Hubs platziert Modell-Layer auf deinem Gerät, und dein Node verdient einen Layer-Anteil an KVR, der der Eigentümer-Wallet gutgeschrieben wird." },
      { title: "Beitrag & Rewards verfolgen", body: "Beobachte im Node-Status Nodes / Online / effektiver Beitrag / einlösbar. Nodes werden nach Durchsatz in Stufen eingeteilt (S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7); Rohwert × Stufe = effektiv. Nutze Rewards einlösen, um aufgelaufene KVR in deine Wallet zu übertragen.", body2: "" },
    ],
    faucetTitle: "Devnet-SOL erhalten (kostenloser Faucet)",
    faucetIntro: "Du benötigst etwas devnet-SOL für Transaktionsgebühren (nutze die Empfangsadresse deiner Wallet):",
    faucetWeb: "Web: faucet.solana.com — füge deine Adresse ein und wähle das Netzwerk Devnet",
    faucetCli: "CLI: solana airdrop 2 <your address> --url devnet",
    faucetAlt: "Alternativen: QuickNode · SolFaucet devnet",
    faucetKvr: "Erhalte KVR zum Staken per Distribution oder Swap (KVR-Swap: SOL/ETH ↔ KVR — demnächst verfügbar).",
    mobileTitle: "Kvasir Wallet · {0}-App",
    mobileSub: "Erstelle eine Wallet und verbinde dein Gerät mit dem Netzwerk.",
    mobile: [
      { title: "App installieren", body: "Installiere Kvasir Wallet über {0}. Nutze die Schaltfläche oben, um die Store-Seite zu öffnen. Ein aktuelles Gerät mit GPU/NPU wird empfohlen.", note: "" },
      { title: "Wallet erstellen / wiederherstellen", body: "Öffne die App und wähle Neue Wallet erstellen oder Aus Wiederherstellungsphrase wiederherstellen. Bewahre deine 12-Wörter-Phrase sicher auf und lege eine Passphrase fest — dasselbe Konto lässt sich mit dieser Phrase auf dem Desktop und anderen Geräten wiederherstellen. Die Schlüssel sind non-custodial und werden nur auf dem Gerät gespeichert.", note: "" },
      { title: "Node konfigurieren", body: "Wähle in den Einstellungen für den mobilen Node ein Compute-Backend (GPU · OpenCL/Vulkan · CPU) und Lokaler Shard (empfohlen). Der erwartete Durchsatz (tok/s) sowie die Auswirkungen auf Speicher / Temperatur / Leistung werden angezeigt.", note: "" },
      { title: "Staking & Rewards", body: "Stake unter Staking & Node-Rewards KVR und prüfe / löse die einlösbaren Rewards ein, die dein Node ansammelt. Der Node-Status zeigt deine Leistungsstufe und deinen Beitrag.", note: "Die Teilnahme an lokaler Shard-Inferenz auf Mobilgeräten wird schrittweise eingeführt; aktuell sind die wichtigsten Compute-Nodes GPU-/CPU-Geräte, auf denen der Agent läuft." },
    ],
    viewGithub: "Auf GitHub ansehen",
    capWelcome: "Willkommen — Wallet erstellen oder wiederherstellen",
    capRecovery: "Speichere deine 12-Wörter-Wiederherstellungsphrase (Wörter unscharf dargestellt)",
    capPassphrase: "Passphrase festlegen → Loslegen",
    capReceive: "Empfangen — Adresse & QR-Code (Adresse teilweise maskiert)",
    capBalances: "Wallet-Guthaben — KVR · SOL",
    capStaking: "Staking — APR · Kapital · Zinsen · Node-Rewards",
    capBackend: "Compute-Backend (CUDA · ROCm · Metal · CPU)",
    capMode: "Node-Modus — Lokaler Shard (empfohlen)",
    capRunlive: "Node ausführen (live) — Live-Anzeigen · Node-ID · Betriebssystem",
    capNodes: "Node-Status — Gesamtwerte · Stufen · Beitrag pro Node",
    capClaim: "Rewards einlösen — einlösbare KVR",
    capWallet: "Wallet-Startseite — KVR-Guthaben (Adresse maskiert)",
    capNodeset: "Node-Einstellungen — Backend / Modus / erwartete Ressourcen",
    capStakingM: "Staking & Rewards für Node-Betreiber",
  },

  techBlog: {
    docTitle: "Kvasir — Technologie",
    pill: "Tech-Blog",
    title: "Engineering des Schwarms",
    lede: "Designnotizen und auf echter Hardware verifizierte Meilensteine aus dem Aufbau der experten-geshardeten Schwarm-Inferenz auf linkcpp — wie ein 122B-Modell über GPUs, CPUs und Smartphones läuft.",
    langNote: "",
    sidebarTitle: "Artikel durchsuchen",
    allArticles: "Alle Artikel",
    read: "Lesen",
    notFound: "Diesen Artikel gibt es nicht.",
    categories: {
      overview: "Vision & Architektur",
      core: "Kerntechnologie",
      milestones: "Meilensteine",
      demos: "Praxis-Demos",
    },
  },

  wiki: {
    docTitle: "Kvasir — Wiki",
    pill: "Wiki",
    title: "Die Kvasir-Wissensbasis",
    lede: "Kurze, präzise Einträge zu jedem Konzept im Netzwerk — vom Ring-Runtime und Experten-Sharding bis zu KVR-Belohnungen.",
    langNote: "",
    sidebarTitle: "Einträge durchsuchen",
    allEntries: "Alle Einträge",
    notFound: "Diesen Eintrag gibt es nicht.",
    categories: {
      network: "Netzwerk & Rollen",
      inference: "Inferenz & Engine",
      token: "Token & Belohnungen",
    },
  },

  apiDocs: {
    docTitle: "Kvasir-API — nutzungsbasierte Inferenz mit KVR",
    pill: "Für Entwickler · devnet",
    title: "Rufe Kvasir-Inferenz auf, bezahlt in KVR",
    lede: "Die Schwarm-Modelle von Kvasir sind nicht direkt im offenen Internet erreichbar. Der einzige öffentliche Einstiegspunkt ist das KVR-Pay-per-Use-Gateway: Jede Inferenz wird durch eine von deiner Wallet signierte On-Chain-KVR-Zahlung freigeschaltet. Hier ist der gesamte Ablauf, in der Sprache, in der du entwickelst.",
    devnetNote: "Läuft auf dem Solana-Devnet — KVR ist kein echtes Asset. Statte vor dem Start eine Devnet-Wallet mit KVR und etwas SOL für die Gebühren aus.",
    baseLabel: "Basis-URL",
    flowTitle: "Vier Schritte",
    flowSteps: [
      { n: "1", title: "Modell entdecken", body: "Frage das Gateway, welche Modelle der Schwarm gerade bereitstellt. Die Liste ist live — codiere nichts fest." },
      { n: "2", title: "Angebot einholen", body: "Sende Modell-id und Prompt. Du erhältst eine an diesen Prompt gebundene requestId und einen KVR-Preis." },
      { n: "3", title: "On-Chain bezahlen", body: "Überweise die angebotenen KVR an das Token-Konto des Empfängers und signiere mit deiner Wallet. Bewahre die Signatur auf." },
      { n: "4", title: "Einlösen", body: "Sende requestId und Signatur zurück. Das Gateway prüft die Zahlung, führt die Inferenz aus und liefert das Ergebnis." },
    ],
    refTitle: "API-Referenz",
    requestLabel: "Anfrage",
    responseLabel: "Antwort",
    apiModels: "Listet die Modelle auf, die der Schwarm gerade bereitstellt — ein leeres Array, wenn keines läuft, also codiere niemals eine id fest.",
    apiQuote: "Hole ein Preisangebot und eine an deinen Prompt gebundene requestId. priceToken ist die zu zahlende KVR-Menge; die endgültige Abrechnung erfolgt nach tatsächlicher Token-Nutzung.",
    apiPay: "Überweise die angebotenen KVR an das zugehörige Token-Konto des Empfängers (das Vault) und signiere mit deiner Wallet. Die Signatur ist einmalig gültig.",
    apiInfer: "Das Gateway pollt die Chain, um die Zahlung zu prüfen, führt die Inferenz auf dem Hub aus und liefert das Ergebnis samt tatsächlicher Nutzung und Kosten.",
    codeTitle: "End-to-End-Beispiel",
    codeLede: "Lade den geheimen Wallet-Schlüssel aus der Umgebung, hole ein Angebot, bezahle und löse ein — ein in sich geschlossenes Snippet. Die Schritte 1, 2 und 4 sind reines HTTP; nur Schritt 3 (die SPL-Überweisung) unterscheidet sich je SDK.",
    adapterTitle: "OpenAI-kompatibler Adapter",
    adapterLede: "Du hast bereits einen OpenAI-Client (oder ein Tool, das nur OpenAI spricht)? Lass diesen Drop-in-Adapter neben deiner App laufen. Er stellt /v1/chat/completions bereit und bezahlt jeden Aufruf aus deiner eigenen Wallet — Angebot, Signatur, Einlösung — im Hintergrund. Richte die Base-URL deines Clients auf den Adapter und nutze einen beliebigen Dummy-API-Key.",
    adapterNote: "Nicht-verwahrend: Das Wallet-Secret (KVR_SECRET_KEY) bleibt in diesem Prozess und erreicht Kvasir nie. Es gibt keinen Kvasir-API-Key — die Authentifizierung ist die On-Chain-KVR-Zahlung, die dein Adapter signiert. Jeder Aufruf ist ein Angebot/Zahlung/Einlösung-Umlauf; cache oder bündle je nach Durchsatz.",
    prereqTitle: "Bevor du beginnst",
    prereqs: [
      "Eine Solana-Devnet-Wallet, die KVR (zum Bezahlen) und etwas SOL (für Gebühren) hält.",
      "Der KVR-Mint hat 6 Dezimalstellen — der On-Chain-Betrag ist round(priceToken × 1.000.000).",
      "Das Ziel ist das zugehörige Token-Konto des Empfängers; existiert es noch nicht, muss deine Überweisung es anlegen (kostet etwas SOL).",
    ],
    securityTitle: "Vom Gateway durchgesetzte Sicherheitsregeln",
    security: [
      "Jede Transaktionssignatur ist einmalig gültig — ein erneutes Einreichen liefert 409.",
      "Die Inferenz ist durch die requestId plus die einmalige Signatur geschützt, halte die requestId also privat — nur ihr Aussteller sollte sie einlösen.",
      "Das Vault muss mindestens priceToken KVR erhalten, sonst wird die Anfrage abgelehnt.",
      "Fehlerfälle: unbekannte requestId (404), Signatur bereits verwendet (409), Transaktion noch nicht bestätigt (400).",
    ],
    walletTitle: "Test-Wallet erstellen",
    walletLede: "Noch keine Devnet-Wallet? Erzeuge ein Schlüsselpaar, gib sein Secret für deine Umgebung aus und lade es mit SOL für die Gebühren auf — dann fordere KVR am Faucet unten an.",
    walletNote: "Halte das Secret aus der Versionskontrolle heraus und lade es aus einer Umgebungsvariable. Nur Devnet — verwende einen Testschlüssel niemals im Mainnet erneut. Du kannst auch in der Kvasir-App eine Wallet erstellen und ihre Adresse kopieren.",
    faucetTitle: "Test-KVR erhalten",
    faucetLede: "Füge eine Solana-Devnet-Adresse ein, um 100 KVR zu erhalten — genug, um den obigen Ablauf auszuprobieren. Eine Anfrage pro Adresse und Tag.",
    faucetPlaceholder: "Deine Solana-Devnet-Adresse",
    faucetButton: "100 KVR anfordern",
    faucetSending: "Senden…",
    faucetSuccess: "{0} KVR an deine Wallet gesendet",
    faucetViewTx: "Transaktion ansehen",
    faucetError: "KVR konnten nicht gesendet werden",
    ctaTitle: "Auf Kvasir aufbauen",
    ctaBody: "Derselbe /api/pay-Vertrag treibt die Desktop-, iOS- und Android-Wallets von Kvasir an. Lies die Referenzimplementierung und den Gateway-Quellcode auf GitHub.",
    ctaButton: "Auf GitHub ansehen",
    catRunNode: "Node betreiben → kostenlose Inferenz",
    catUseApi: "Die API nutzen",
    selfHostTitle: "Betreibe einen Node, bekomme kostenlose Inferenz",
    selfHostPitch: "Willst du KI-Modelle kostenlos nutzen? Lass deinen Coding-Agent deine Maschine als Node ins Netz einklinken — und dir dafür einen Inferenz-Endpunkt zurückgeben.",
    selfHostBody: "Ein Skript startet den Hub (und optional das KVR-Gateway) mit Docker. Füge in der Hub-UI deine GPU hinzu, lade ein offenes Modell und rufe dann einen standardmäßigen OpenAI-kompatiblen Endpunkt — /c/<id>/v1/chat/completions — auf, der auf deiner eigenen Hardware läuft. Richte jedes Tool, das OpenAI spricht, darauf aus.",
    selfHostNote: "Das stellt offene Modelle, die deine Maschine halten kann, kostenlos bereit — es ist deine Rechenleistung. Für Frontier-Modelle, die zu groß für eine Maschine sind, tritt dem Schwarm bei: dafür ist die KVR-Pay-per-Use-API unten da.",
    inferenceApiTitle: "Inferenz-API (Guthaben)",
    inferenceApiLede: "Der einfachste Weg: ein nativer OpenAI-Endpunkt mit einem API-Key. Streaming (SSE) und native Tool-Calls funktionieren einfach, und jeder Aufruf wird von einem im Voraus aufgeladenen KVR-Guthaben abgezogen — kein Wallet-Signieren pro Aufruf. Der Zugang wird über eine Wallet-Whitelist gesteuert.",
    inferenceApiSteps: [
      { title: "Guthaben aufladen", body: "Erhalte KVR-Guthaben über eine Betreiber-Zuteilung oder lade selbst auf: Überweise KVR an die Treasury und reiche die Signatur bei POST /api/credits/deposit ein." },
      { title: "API-Key holen", body: "Weise den Wallet-Besitz einmalig nach (SIWS): Fordere eine Challenge an, signiere die Nachricht und tausche sie gegen einen Key. Der apiKey wird nur einmal zurückgegeben — speichere ihn." },
      { title: "Rufe sie wie OpenAI auf", body: "Richte einen beliebigen OpenAI-Client mit deinem Key auf die Basis-URL. Streaming und tool_calls funktionieren unverändert; das Guthaben wird pro Aufruf belastet." },
    ],
    inferenceApiKeyLede: "Erstelle einen API-Key (eine Wallet-Signatur)",
    inferenceApiCallLede: "Rufe sie dann mit dem Standard-OpenAI-SDK auf — nur base_url und der Key ändern sich",
    inferenceApiRefTitle: "Referenz",
    inferenceApiRef: {
      base: "Basis-URL", auth: "Authentifizierung", endpoints: "Endpunkte", balance: "Guthaben",
      pricing: "Preise", errors: "Fehler", context: "Max. Kontext", model: "Modell",
    },
    inferenceApiThinkNote: "Das bereitgestellte Modell denkt standardmäßig nach. Für kurze Antworten oder Tool-Calls setze chat_template_kwargs.enable_thinking = false; lass das Nachdenken aktiviert (besser fürs Coding) mit einem großzügigen max_tokens. Tool-Calls funktionieren immer. Der aktuelle Tarif wird von der Governance festgelegt und kann sich ändern.",
    keyIssueTitle: "Hier einen Key ausstellen",
    keyIssueLede: "Lieber nicht selbst skripten? Führe den gesamten Ablauf direkt hier aus — gib deine Wallet ein, signiere jede Challenge und erhalte einen Key. Dieselben Endpunkte wie oben; dein Key verlässt niemals deinen Browser.",
    keyIssueWalletPh: "Deine Solana-Wallet-Adresse",
    keyIssueLabelPh: "Key-Label (z. B. my-app)",
    keyIssueStart: "Starten",
    keyIssueRegisterNote: "Diese Wallet ist noch nicht registriert — signiere einmal zur Selbstregistrierung und dann erneut, um den Key zu erhalten.",
    keyIssueSignPrompt: "Signiere genau diese Nachricht mit deiner Wallet (ed25519) und füge dann die base64-Signatur ein:",
    keyIssueSigPh: "base64-Signatur",
    keyIssueSubmit: "Signatur einreichen",
    keyIssuePending: "Diese Wallet steht nicht auf der Whitelist und die Selbstregistrierung ist geschlossen. Ein Betreiber muss sie freigeben — bitte melde dich.",
    keyIssueKeyReady: "Dein API key — wird nur einmal angezeigt. Kopiere ihn jetzt.",
    keyIssueBalanceLabel: "Guthaben",
    keyIssueTopUp: "Guthaben ist 0 — lade KVR-Guthaben auf (überweise KVR an die Treasury und dann POST /api/credits/deposit), bevor du Aufrufe machst.",
    keyIssueError: "Anfrage fehlgeschlagen",
    keyIssueUseNote: "Jetzt nutzen: Es ist ein Bearer-Key für https://gate.kvasir-ai.net/v1. Führe den vorbereiteten Befehl unten aus oder richte ein beliebiges OpenAI-SDK auf diese Base-URL (vollständige Beispiele weiter unten).",
    selfIssueTitle: "Schlüssel selbst ausstellen (Agenten)",
    selfIssueLede: "Ein Coding-Agent kann alles headless aus einem Wallet-Secret erledigen — die SIWS-Challenges signieren, sich selbst registrieren, einen Key holen und dann den OpenAI-Endpunkt aufrufen. Kein Browser, keine Klicks.",
    gateTitle: "Mit einem Star die Entwickler-Docs freischalten",
    gateBody: "Die Docs bleiben offen — ein GitHub-Star hält dich nur auf dem Laufenden und hilft dem Projekt zu wachsen. Melde dich mit GitHub an und gib dem Repo einen Star, um fortzufahren.",
    gateSignIn: "Mit GitHub anmelden",
    gateStarBody: "Angemeldet als {0}. Gib dem Repo auf GitHub einen Star und prüfe erneut, um freizuschalten.",
    gateStarLink: "louisevandan/kvasir-net mit Star versehen ↗",
    gateRecheck: "Star gesetzt — erneut prüfen",
  },

  careers: {
    docTitle: "Marketing & Growth — Kvasir",
    pill: "Wir stellen ein",
    headline1: "Marketing & Growth",
    headline2: "lass das Netzwerk wachsen",
    sub: "Kvasir ist ein dezentrales KI-Inferenz-Netzwerk (DePIN) auf Solana. Die Open-Source-Engine linkcpp verteilt große offene Modelle auf viele beigesteuerte GPUs und Maschinen, und jeder Node verdient KVR für die Layer, die er tatsächlich bedient hat. Die Technik läuft — wir brauchen die Person, die es der Welt erzählt.",
    factRole: "Rolle",
    factRoleV: "Marketing & Growth — Vollzeit",
    factLocation: "Standort",
    factLocationV: "Remote · Zeitzone USA/Europa oder Südostasien · ≥3–4 h tägliche Überschneidung mit KST",
    factComp: "Vergütung",
    factCompV:
      "Early-Stage-Equity (4 Jahre Vesting / 1 Jahr Cliff) + TGE-bedingte Token-Zuteilung · bezahlte Probearbeit vor jeder Festlegung",
    factEngine: "Engine",
    liveTitle: "Was bereits live ist",
    liveLede: "Du steigst nicht in ein Whitepaper ein. Verifiziert und heute in Betrieb:",
    liveProof: [
      "Auf dem Netzwerk getestete Modelle: Qwen3.5 122B, Qwen3.5 35B und Gemma4 12B — jeweils Layer für Layer über mehrere Maschinen verteilt, sodass kein Node das ganze Modell hält.",
      "Eine heterogene Live-Flotte von insgesamt 21 Nodes: 4× AMD MI250 (ARM-Host), 4× NVIDIA GB10, 4× NVIDIA RTX Pro 6000, 1 MacBook Pro, 6 x86-Windows-CPU-Maschinen und 2 mobile Nodes (iOS + Android).",
      "Beitragsabrechnung pro Node: Jeder Node verdient KVR, gewichtet nach seinem Layer-Anteil an jeder bedienten Inferenz, abgerechnet auf seine eigene Wallet.",
      "OpenAI- und Anthropic-kompatibles Pay-per-Inference-Gateway, live auf unserer eigenen Domain.",
      "Non-Custodial-Wallets für Web, Desktop, iOS und Android, mit Wallet-Signatur-Login (Sign-In With Solana) + 2FA.",
    ],
    devnetNote:
      "KVR läuft derzeit auf dem Solana-Devnet. Es ist ein Utility-/Beitrags-Token — nichts auf dieser Seite ist ein Wertpapierangebot oder ein Versprechen über den Token-Wert.",
    ownsTitle: "Was du verantwortest",
    ownsLede:
      "Ein zweiseitiges Netzwerk braucht zweiseitiges Wachstum: Node-Betreiber auf der Angebotsseite, Entwickler auf der Nachfrageseite. Beides beginnt bei null — genau das ist der Job.",
    owns: [
      {
        title: "Community & Social",
        body: "X und Discord von null aufbauen. KOL-Beziehungen in der DePIN-/KI-Krypto-Nische pflegen und einen stetigen Content-Rhythmus halten.",
      },
      {
        title: "Node-Betreiber-Wachstum",
        body: "Akquise auf der Angebotsseite: GPU-Besitzer und Homelab-Communities erreichen und mit Kampagnen in aktive Kvasir-Nodes verwandeln.",
      },
      {
        title: "Entwickler-Nachfrage",
        body: "Nachfrageseitiges Marketing für Entwickler und KI-Startups, die OpenAI/Anthropic-kompatible Inferenz-Endpunkte brauchen — doku-nahe Inhalte, Launch-Posts, Integrations-Showcases.",
      },
      {
        title: "Kampagnen & Analytics",
        body: "Growth-Experimente entwerfen, ehrlich messen und auf die Kanäle setzen, die Node-Zahl und API-Nutzung wirklich bewegen.",
      },
      {
        title: "Launch- & Partnerschafts-Support",
        body: "Das Token-Launch-Marketing unterstützen, wenn das Netzwerk das Devnet verlässt, und bei der Partnerakquise helfen (GPU-Flotten, Wallets, Modellanbieter).",
      },
    ],
    profileTitle: "Wen wir suchen",
    profile: [
      "Krypto-nativer Marketer: Du hast eine web3-Community oder ein Produkt von null aufgebaut — nachweisbar auf X, Discord oder on-chain.",
      "DePIN- oder KI-Krypto-Erfahrung stark bevorzugt; du kannst einem GPU-Besitzer erklären, warum er einen Node betreiben sollte.",
      "Englisch auf Muttersprachler-Niveau oder fließend; Zeitzone USA/Europa oder Südostasien mit ≥3–4 h täglicher Überschneidung mit KST (UTC+9).",
      "Einverstanden mit Early-Stage-Vergütung: substanzielles Equity + Token-Upside statt hohem Gehalt.",
      "Hands-on-Umsetzer — Posts, Kampagnen und Experimente lieferst du selbst.",
    ],
    processTitle: "So stellen wir ein",
    processLede:
      "Jede Kandidatin und jeder Kandidat durchläuft vor jedem Equity-Gespräch eine bezahlte Probearbeit — das schützt beide Seiten.",
    process: [
      {
        title: "Kennenlern-Call",
        body: "Wir zeigen das laufende Netzwerk und die Roadmap; du zeigst eine Community oder Kampagne, die du wirklich aufgebaut hast.",
      },
      {
        title: "Bezahlte Probearbeit (2–4 Wochen)",
        body: "Eine echte, bezahlte Arbeitsprobe — z. B. ein Node-Betreiber-Akquiseplan mit Kanalrechnung oder ein Live-Growth-Experiment auf X/Discord. Wir bewerten Ergebnis, Tempo und Eigenständigkeit.",
      },
      {
        title: "Angebot",
        body: "Marketing & Growth: Equity mit standardmäßigem 4-Jahres-Vesting (1 Jahr Cliff) plus TGE-bedingter Token-Zuteilung; ein Cash-Grundgehalt, sobald Funding eintrifft.",
      },
      {
        title: "Gemeinsam bauen",
        body: "Erster Meilenstein als Team: unseren nächsten Hackathon-Beitrag gemeinsam bauen und die erste Kohorte von Node-Betreibern aufbauen.",
      },
    ],
    applyTitle: "So bewirbst du dich",
    applyBody:
      "Schick eine kurze Vorstellung per E-Mail mit Links, die das obige Profil belegen — die Community oder Kampagne, die du aufgebaut hast, dein X-Handle, alles On-Chain. Ein Lebenslauf ist optional; Belege sind es nicht.",
    applyCta: "Bewerben",
    readCode: "Erst den Code lesen",
    seeProduct: "Das Produkt ansehen",
  },
};
