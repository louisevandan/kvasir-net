/* Deutsch — Übersetzung des Tech-Blogs. Struktur (Slug, Kategorie,
   Blockreihenfolge, Code, img-Positionen) spiegelt exakt articles.ts
   (englische Quelle); Fachbegriffe, Bezeichner und Zahlen bleiben erhalten. */
import type { TechTranslation } from "./articles";

export const deTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "Experten-geshardete Schwarm-Inferenz: das Design",
    dek: "86 % eines 122B-MoE sind 12.544 unabhängige 5.3-MB-Experten. Schneide das Modell an diesem Korn, und ein Smartphone trägt einen echten Anteil der Frontier-Inferenz.",
    blocks: [
      {
        t: "callout",
        md: "**Die These:** 86 % des Gewichts von Qwen3.5-122B sind 12.544 wechselseitig unabhängige 5.3-MB-Experten. Sharde am Expertenkorn, und ein schwaches Gerät trägt „8–64 Experten (42–340 MB)\" statt „eine 1.4-GB-Layer\" — genau die Einheit, die ein Smartphone wirklich halten kann. MoE ist das natürliche Substrat eines Schwarms.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "Substrat · Qwen3.5-122B-A10B (Q4_K_M)", text: "Die Gewichte sind bereits in schwarmgroße Einheiten verpackt" },
      {
        t: "stats",
        items: [
          { n: "49", l: "Layer" },
          { n: "256", l: "Experten / Layer" },
          { n: "8", l: "aktiv / Token" },
          { n: "5.3 MB", l: "ein Experte (Q4)" },
          { n: "12,544", l: "Experten gesamt" },
          { n: "86%", l: "des Gewichts in Experten" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "Expertendim. = äußerste" },
        ],
      },
      {
        t: "p",
        md: "Der Expertenindex ist die **äußerste Dimension** jedes MoE-Tensors, jeder Experte also eine zusammenhängende, quantblock-ausgerichtete Platte. Ein experten-geschnittenes mini-GGUF ist eine saubere Byte-Range-Kopie — keine Dequantisierung, kein Umpacken.",
      },
      { t: "h2", kick: "Zwei Rollen", text: "Backbone-Stage × Experten-Worker" },
      {
        t: "ul",
        items: [
          "**Backbone-Stage (starker Knoten):** Attention + KV-Cache, alle Norms, der **Router**, der geteilte Experte und das Residual-Combine — der gesamte dichte Pfad. Er hält zudem alle Experten als Fallback-Replik resident (RAM-Offload), was dem Schwarm Churn-Toleranz gibt.",
          "**Experten-Worker (ein Smartphone):** kein Transformer. Keine Attention, kein KV, kein Sampler — eine reine Funktion `(hidden, local_ids) → out` aus drei Mat-Muls, die nur die eigene Expertenscheibe hält. Passt in jedes Budget, bis hinunter zum 4-GB-Smartphone.",
        ],
      },
      {
        t: "code",
        caption: "Die Schnittstelle: Der Router läuft einmal auf dem Backbone, mit Autorität.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Weil der Router **genau einmal** auf dem Backbone läuft, wird jeder ausgewählte Experte genau einmal von dem Knoten berechnet, der ihn besitzt. Es gibt **keine Näherung** — das Sharding verschiebt nur, wo die Mat-Muls stattfinden.",
      },
      { t: "h2", kick: "Kein neues Subsystem", text: "Der Schwarm ist der bewährte Belohnungsmarkt, nur feiner gekörnt" },
      {
        t: "p",
        md: "Kvasir betreibt bereits einen autonomen Knappheitsmarkt für **Layer**-Shards, verifiziert auf echten Geräten: Ein Smartphone hinter NAT pollt die Nachfragekarte, schreibt sich in das **bestbelohnte** Segment ein, lädt nur dieses Fenster partiell herunter, lädt es auf seine Adreno-GPU und vollendet die Ring-Inferenz — und verdient Beitragsbelohnungen. Das Experten-Sharding nutzt all das wieder — Abdeckungskarte, Selbsteinschreibung nach Maximalbelohnung, Teil-Download, Belohnungen je Knoten — und ändert nur die Abdeckungseinheit von *Layer-Bereichen* zu *(Layer, Experten-Bereich)*.",
      },
      { t: "h2", kick: "Zwei bereits geräteverifizierte Innovationen", text: "Teilgewicht-Teilnahme + das 443-Relay" },
      {
        t: "ul",
        items: [
          "**Belohnungsgetriebener Teilgewicht-Download:** Konventionelle RPC/TP/PP-Aufbauten schicken den vollen Checkpoint an jeden Rang, und ein Scheduler diktiert die Platzierung. Bei Kvasir lädt ein Knoten **nur die Scheibe, die er berechnen wird**, und wählt diese Scheibe **selbst, nach Belohnung** — ein 254-MB-Stage-mini-GGUF gegenüber dem 77.6-GB-Vollmodell. So tritt ein 4-GB-Smartphone einem Modell bei, das weit größer ist als es selbst.",
          "**443-Relay-Datenebene:** Cloudflares 80/443-only-Edge plus Carrier-NAT verhindern das direkte Anwählen in beide Richtungen. Eine WebSocket-Brücke pro Edge mit 1-Byte-Rollen-Preamble lässt **beide Seiten ausgehend wählen** (das Smartphone öffnet null eingehende Ports). Die Landung erforderte drei echte Bugfixes — Build-Fingerprint-Abgleich, Node-Token-Download-Auth und ein Kotlin-`Int.ushr`-Framelängen-Bug, der jeden Frame ≥ 64 KiB still beschädigte (`ushr` nutzt nur die unteren 5 Bits des Shifts; `len ushr 56` wurde zu `len ushr 24`) — behoben durch `Long`-Shifts.",
        ],
      },
      { t: "h2", kick: "Der ehrliche Kern", text: "Ein Durchsatzgewebe, kein Niedriglatenz-Decoder" },
      {
        t: "p",
        md: "Dekodieren sind 49 serielle Layer, und ein Internet-Roundtrip pro Layer kostet 2.5–10 s pro Token. Der Wettkampf des Schwarms ist also, **Modelle zu servieren, die niemand allein hosten kann**, gemessen am Gesamtdurchsatz: Batch-Dispatch amortisiert das RTT, das Backbone hält einen Hot-Expert-Cache, und Anfragen werden zu nahen Repliken geroutet. Der Niedriglatenz-Pfad bleibt beim Pipeline-Ring.",
      },
      { t: "h2", kick: "Fahrplan", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — Backbone-Experten-RAM-Offload: 122B auf einem Koordinator, ohne Graph-Chirurgie.",
          "**M1** — Expert-Parallel-Beweis auf einem Host: experten-geschnittenes mini-GGUF + Worker-Runtime + Dispatch, Logits exakt gleich dem Monolithen.",
          "**M2** — LAN- + NAT-Smartphone-Worker berechnen echte 122B-Experten durch das 443-Relay.",
          "**M3** — Abdeckungsmarkt am Expertenkorn mit Repliken und Churn-Fallback.",
          "**M4** — Durchsatz: Batch-Dispatch + Hot-Expert-Cache, tokens/s skalierend mit der Worker-Zahl.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "Vom Bauplan zur Hardware: das Verifizierte und der Schlussstein",
    dek: "Rückblick auf die Verifikationskampagne — Design bis M2-Kern auf einem echten 122B bewiesen — und das eine Integrationsstück, das den Rest freischaltet.",
    blocks: [
      {
        t: "p",
        md: "In den vergangenen Wochen wurden die schweren, neuartigen Teile des Experten-Schwarms eines nach dem anderen auf einem echten **Qwen3.5-122B** bewiesen — nicht simuliert, nicht spielzeuggroß. Hier die bisherige Verifikationsspur und der eine Schlussstein, der noch fehlte.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "Die Spur · alles auf dem echten 122B verifiziert", text: "Was bislang gelandet ist" },
      {
        t: "ul",
        items: [
          "**Design (5 Revisionen seit dem Bauplan)** — EP-Architektur, autonome Teilgewicht-Teilnahme, das Relay, die Worker-Kernel-Spezifikation und die als Kerntechnologie kodifizierte Cross-Backend-Äquivalenz. Router-Autorität als Kohärenz-Invariante festgeschrieben.",
          "**M0 — Backbone-Experten-RAM-Offload (Planner verifiziert):** 122B ist auf einem einzelnen 64-GB-Koordinator *feasible* — 10 Expertenlayer in RAM ausgelagert, VRAM 62.6 GiB / RAM 14.2 GiB, verdrahtet via `--override-tensor`.",
          "**M1 — Experten-Scheiben-Datenpfad:** mini-GGUF-Slicing je Experte (ne[2]-Platten, Bytekopie ohne Dequantisierung) + der `/expert-shard`-Download-Endpoint.",
          "**M1 — numerisches Orakel:** dispatch + combine == monolithisch mit **max|Δ| = 3.6e-12** auf echten Layer-0-Experten — Sharding ist eine exakte Umgruppierung derselben gewichteten Summe.",
          "**M1 — C++-Worker auf Hardware:** `linkcpp-expert-worker` (reines ggml/gguf) auf ROCm gebaut und ausgeführt, **cosine 0.99995** vs. das Orakel; Router → zwei C++-Worker → Combine gleicht dem Monolithen bei cosine 0.9997–0.9999.",
          "**M2-Kern — ein Smartphone berechnet echte 122B-Experten:** Android-Crossbuild, ausgeführt auf einem SM-S938N, **cosine 0.99992** vs. das Orakel.",
          "**Numerik — 3-Backend-Äquivalenzmatrix:** dieselbe 122B-Berechnung auf ROCm × Smartphone-ARM-CPU × numpy — ROCm↔Smartphone cosine 0.99990, ROCm↔numpy 0.99996, Smartphone↔numpy 0.99992. Alles äquivalent, nichts bitidentisch.",
        ],
      },
      { t: "h2", kick: "Der Schlussstein", text: "Backbone-Dispatch, integriert in die Live-Dekodierung" },
      {
        t: "callout",
        md: "Jede **Komponente** — Scheiben, Worker, Dispatch/Combine-Logik, numerische Äquivalenz, Smartphone-Berechnung — war geräteverifiziert. Es blieb, sie **in einer echten Inferenz-Engine-Dekodierung** zu verdrahten: ein `build_moe_ffn`-Hook, der Experten mitten im Graphen an ihre Besitzerknoten dispatcht. Das erforderte die Änderung des gepinnten Inferenz-Engine-Submoduls und mehrere Build-Verify-Zyklen. Steht dieser Schlussstein, öffnen sich **M2-Relay-Integration, der M3-Experten-Abdeckungsmarkt und M4-Batch-Durchsatz** der Reihe nach — alle hängen an diesem Dispatch.",
      },
      {
        t: "p",
        md: "Der Schlussstein ist inzwischen gelandet: Die Folgebeiträge zu M2, M3, M4 und der Live-Smartphone-Demo sind die Ergebnisse genau dieser Integration.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "Numerische Äquivalenz über heterogene Backends",
    dek: "CUDA, ROCm, Adreno und CPUs werden nie bitgenau übereinstimmen. Dass der Schwarm dennoch ein kohärentes Modell liefert, ist eine entworfene Eigenschaft, kein Glück.",
    blocks: [
      { t: "h2", kick: "Die Schlüsselunterscheidung", text: "Exakt vs. äquivalent — zwei verschiedene Eigenschaften" },
      {
        t: "ul",
        items: [
          "**Innerhalb eines Backends — exakt (3.6e-12):** Experten über Knoten zu verteilen und zu kombinieren ist dieselbe gewichtete Summe, nur umgruppiert; der einzige Unterschied ist die Gleitkomma-Akkumulationsreihenfolge. Orakelverifiziert.",
          "**Zwischen Backends — äquivalent (1e-3…1e-6):** dieselbe Operation auf anderer Hardware trägt einen relativen Fehler pro Op von ~1e-3–1e-6 und ist nie null. **In diesem Regime lebt der Schwarm.**",
        ],
      },
      {
        t: "p",
        md: "„Exakt\" garantiert die Zerlegung innerhalb eines Geräts. „Äquivalent\" gibt einem heterogene Hardware. Die Aufgabe des Schwarms ist, zu verhindern, dass sich Äquivalenz zu Divergenz aufschaukelt.",
      },
      { t: "h2", kick: "Gemessen · echter 122B, drei Backends", text: "Keine Theorie — auf Hardware gemessen" },
      {
        t: "p",
        md: "Dieselbe Layer-0-Experten-FFN von Qwen3.5-122B, berechnet von `linkcpp-expert-worker` auf einem MI250 (**ROCm**), der **ARM-CPU** eines Smartphones (SM-S938N) und einer x86-**numpy**-Referenz — gleiche Eingaben, gleiche Gewichte, andere Befehlssätze und Reduktionsreihenfolgen:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Backend-Paar", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Smartphone-ARM-CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["ROCm-GPU vs Smartphone-ARM-CPU", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Drei Befehlssätze, eine Berechnung — jedes Paar äquivalent (cosine ≈ 0.9999), kein Paar bitidentisch (Δ ≈ 1e-6). Die Residuen sind klein, **weil die Router-Autorität Eingaben und Expertenauswahl festgenagelt hat**.",
      },
      {
        t: "p",
        md: "Ein späterer Lauf auf echter **NVIDIA GB10 Grace Blackwell**-Hardware schloss die Matrix auf dem letzten Backend: CUDA ↔ ROCm landete bei **cosine 1.0000000000** (max abs 3.5e-10, praktisch bitidentisch, da beide GPU-Backends Kernel-Quellen teilen), und CUDA ↔ Grace ARM CPU bei cosine 0.99975 — dasselbe GPU↔CPU-Muster wie oben.",
      },
      { t: "h2", kick: "Warum Backends abweichen", text: "Gleitkomma-Addition ist nicht assoziativ" },
      {
        t: "ul",
        items: [
          "**Matmul-Reduktionsreihenfolge** — Tensor-Cores, MFMA-Kacheln, OpenCL-Workgroups und SIMD-Lanes akkumulieren in verschiedenen Reihenfolgen und Kachelungen.",
          "**FMA-Fusion** — `a*b+c` einmal (FMA) oder zweimal gerundet, je Backend anders fusioniert.",
          "**Akkumulationspräzision** — F16/BF16-Speicherung mit F32- vs. F16-Akkumulatoren (der größte Hebel der Divergenz).",
          "**Transzendente Näherungen** — Polynom-/Tabellenvarianten von exp (Softmax), silu/sigmoid (SwiGLU), rsqrt (Norms).",
          "**Dequant + Matmul-Pfad** — dequantisieren-dann-multiplizieren vs. fusionierte quantisierte Kernel runden Zwischenwerte anders.",
          "**Nichtdeterministische Kernel** — atomic/split-K-Reduktionen können auf demselben Gerät von Lauf zu Lauf variieren.",
        ],
      },
      { t: "p", md: "Nichts davon ist ein Bug. Es ist der Preis, den der schnelle Pfad jedes Beschleunigers zahlt." },
      { t: "h2", kick: "Warum es trotzdem funktioniert", text: "Eine Autorität für Entscheidungen, genug Präzision fürs Akkumulieren" },
      {
        t: "callout",
        md: "**ROUTER-AUTORITÄT — die zentrale Invariante.** Die einzige diskrete Entscheidung im Netzwerk ist das MoE-Routing (Top-8 von 256). Würde jedes Backend den Router neu ausführen, wählten Grenz-Token **verschiedene Experten** und würden wirklich divergieren. Kvasir führt den Router **einmal, auf dem Backbone** aus und schickt den Workern nur die ausgewählten Experten-IDs. Ein heterogener Schwarm mag sich in der *Größe* der Ausgabe jedes Experten unterscheiden — nie darin, *welche Experten laufen*. Das verwandelt katastrophale diskrete Divergenz in beschränkten kontinuierlichen Fehler und ist die Kohärenzregel des heterogenen Experten-Shardings.",
      },
      {
        t: "ul",
        items: [
          "**Diskretes Argmax:** Dekodieren ist ein Argmax über Logits. Ein 1e-3-Wackeln kippt einen Token nur, wenn zwei Kandidaten näher als 1e-3 liegen — an den meisten Positionen ist die Marge weit größer, also **kommen die Tokens identisch heraus**; die seltenen Kipper sind Positionen, so mehrdeutig wie ein anderer Seed.",
          "**Combine ist Addition:** Teilergebnisse verschmelzen als wahrscheinlichkeitsgewichtete **Summe**. Unabhängige ~1e-4-Fehler addieren sich inkohärent — sie wachsen wie √k, nicht k — und ohne Auslöschung großer Werte bleibt das Residuum gut konditioniert.",
        ],
      },
      { t: "h2", kick: "Wo es brechen kann · und die Regeln dagegen", text: "Divergenzmodi und Verteidigungen" },
      {
        t: "table",
        head: ["Divergenzmodus", "Mechanismus", "Regel"],
        rows: [
          ["Routing-Abweichung", "Backends wählen bei Grenz-Token unterschiedliche Top-8", "Router-Autorität — einmal auf dem Backbone entschieden, IDs dispatcht"],
          ["Trajektorien-Gabelung", "Das Logit-Wackeln pro Token kippt irgendwann einen; die Sequenz gabelt wie ein neuer Seed", "Dekodieren/Sampling an einen Knoten gepinnt"],
          ["Tiefenakkumulation", "49 Layer × je ~1e-4 → bis zu 1e-2 an den finalen Logits", "F32-Akkumulation an Grenzen und beim Combine"],
          ["Selbst-Nichtdeterminismus", "Atomic/split-K-Kernel variieren von Lauf zu Lauf", "Deterministische Combine-Kernel; Verifikation mit Toleranzen"],
          ["Präzisions-Abweichung", "Ein Knoten akkumuliert F16, ein anderer F32", "Akkumulationspräzision als Fähigkeit ausgewiesen; F32-Knoten für Ausgaberänge bevorzugt"],
        ],
      },
      { t: "h2", kick: "Äquivalenz ist eine Zahl", text: "Das Messprotokoll" },
      {
        t: "ul",
        items: [
          "**Delta pro Op** — gleiche Eingaben, relativer Fehler A vs. B bei matmul, swiglu, softmax, norm.",
          "**Layer-Grenzdrift** — Residual-Delta nach einer Layer, gestapelt, um zu sehen, ob die Tiefe wie √L oder L akkumuliert.",
          "**Ende-zu-Ende-Logit-Divergenz** — L∞, L2 und **KL-Divergenz** über den vollen Forward.",
          "**Entscheidungsübereinstimmung** — Top-1-Token-Übereinstimmung plus Top-8-Routing-Übereinstimmung (validiert, warum Router-Autorität nötig ist).",
          "**Generationsstabilität** — greedy N Tokens; der erste Index, an dem A und B divergieren.",
          "**Task-Ebene** — Perplexity- und Eval-Score-Deltas: die einzige Metrik, die ein Nutzer wirklich spürt.",
        ],
      },
      {
        t: "p",
        md: "Ein Bestehen ist eine **Toleranz** — „Top-1-Übereinstimmung ≥ 99.x %, KL ≤ ε\". Ein Knoten außerhalb der Toleranz wird für sensible Ränge als ungeeignet markiert, nicht pauschal abgelehnt.",
      },
      { t: "h2", kick: "Warum das Kern-Schwarmtechnologie ist", text: "Bit-Übereinstimmung ist unmöglich — und unnötig" },
      {
        t: "p",
        md: "Ein homogener Cluster kann Bitgenauigkeit annehmen; ein Schwarm nicht — seine Prämisse ist *welche Hardware auch immer auftaucht*. Also behandelt Kvasir numerische Äquivalenz genau wie Protokollkompatibilität: als **erstklassigen, gemessenen Vertrag**. Backends und Akkumulationspräzision werden als Knotenfähigkeiten ausgewiesen, Router-Autorität wird als Invariante erzwungen, und jede Verifikation nutzt Toleranzen statt Bitgleichheit. **Gemessene numerische Äquivalenz + diskrete Entscheidungen einer einzigen Autorität** — das lässt ein Modell auf jeder GPU der Erde zugleich laufen. Das ist der Schwarm.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell trat dem Schwarm bei",
    dek: "Eine GB10 Grace Blackwell berechnete echte 122B-Experten-FFN-Scheiben in CUDA und glich AMD ROCm bitgenau ab (cosine 1.0000000000) sowie die Grace ARM CPU innerhalb der Toleranz. Die Cross-Backend-Matrix ist vollständig.",
    blocks: [
      {
        t: "p",
        md: "Die Prämisse eines Schwarms ist *welche Hardware auch immer auftaucht*. Numerische Äquivalenz — der Beweis, dass CUDA-, ROCm-, Adreno- und CPU-Worker alle denselben Token ausgeben — war bereits auf ROCm, Smartphone-ARM und numpy gemessen. NVIDIA ist der **voreingestellte und am besten optimierte** Pfad im Standard-ggml/Inferenz-Engine, war aber das einzige Backend, auf dem die Matrix nicht geschlossen war. Echte Blackwell-Hardware laufen zu lassen schließt sie.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. Auf derselben echten Qwen3.5-122B-Layer-0-Expertenscheibe sind die zwei GPU-Backends praktisch bitidentisch.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Gemessen · echtes Qwen3.5-122B-A10B, Layer-0-Expertenscheibe", text: "Die Cross-Backend-Matrix" },
      {
        t: "table",
        head: ["Vergleich", "Hardware", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "Die zwei GPU-Backends (CUDA, ROCm) teilen sich Kernel-Quellen und landen daher **praktisch bitidentisch** (10⁻¹⁰). GPU↔CPU trägt durch eine andere Akkumulationsreihenfolge eine Störung von ~10⁻³ pro Op, bleibt aber bei **cosine 0.99975** äquivalent — dasselbe Muster wie das frühere ROCm↔Smartphone-ARM 0.99992. Das Router-Autoritätsprinzip hält auch auf NVIDIA: **die diskreten Entscheidungen (argmax, Expertenauswahl) sind über diese kontinuierliche Störung invariant.**",
      },
      { t: "h2", kick: "Setup", text: "Was lief, worauf" },
      {
        t: "ul",
        items: [
          "**Gerät** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124,5 GB Unified Memory.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; der reine ggml/gguf-Expert-Worker mit Blackwell-Kernels gebaut.",
          "**Modell** — Qwen3.5-122B-A10B-Q4_K_M, Layer-0 alle Experten (256 Experten, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Methode** — die 1,58-GB-L0-Scheibe MI250 → GB10 gestreamt (verlustfreier Vergleich); dieselbe Eingabe (h/ids) durch CUDA, CPU und ROCm; float32-Ausgabevektoren (36.864) per cosine, relativer L2 und max-abs verglichen.",
        ],
      },
      {
        t: "callout",
        md: "**Eine Echt-Hardware-Falle:** die integrierte GPU des GB10 wird von ggml als Gerätetyp `ACCEL` klassifiziert, nicht `GPU` — also fand `init_by_type(GPU)` nichts. Behoben, indem statt eines hartkodierten GPU-Typs das erste Nicht-CPU-Gerät gewählt wird.",
      },
      { t: "h2", kick: "Warum es zählt", text: "Die Matrix ist geschlossen" },
      {
        t: "p",
        md: "Damit heterogene Worker ein Modell servieren, müssen eine CUDA-Maschine und eine ROCm-Maschine **austauschbar** sein, und eine GPU und eine CPU müssen **numerisch äquivalent** sein. Mit gemessenem Blackwell gilt beides über die volle Backend-Matrix: CUDA↔ROCm-Worker sind gegeneinander austauschbar, und GPU↔CPU-Worker stimmen innerhalb einer beschränkten, gut konditionierten Toleranz überein. Der weltweit verbreitetste Beschleuniger ist nun ein verifizierter Schwarmbürger.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "Den Geldpfad härten: Transaktionssicherheit im KVR-Settlement",
    dek: "Drei echte Schwachstellenklassen — Replay von Zahlungssignaturen, unauthentifiziertes Belohnungsprägen und Race-Double-Spends — gefunden, in Tests ausgenutzt und im Settlement-Dienst des Gateways geschlossen.",
    blocks: [
      {
        t: "p",
        md: "In einer DePIN ist der Geldpfad genauso feindlich wie der Rechenpfad: Jeder Endpoint, der KVR gutschreibt, wird irgendwann von jemandem abgetastet, der KVR ohne Arbeit will. Ein Sicherheitsdurchgang über den Settlement-Dienst des Gateways — den Prozess, der On-Chain-Zahlungen verifiziert und Stakes, Node-Belohnungen und Inferenzgebühren gutschreibt — fand und schloss **drei echte Schwachstellenklassen**. Jede wurde vor dem Fix mit einem Exploit-artigen Test demonstriert und danach erneut verifiziert.",
      },
      { t: "h2", kick: "Das Vertrauensmodell", text: "On-Chain-Fakten verifizieren, nicht Client-Behauptungen" },
      {
        t: "p",
        md: "Kvasirs Verwahrmodell lässt die Schlüssel bei den Nutzern: Wallets signieren Transaktionen, Solana zeichnet sie auf, und die einzige Aufgabe des Settlement-Dienstes ist, **zu verifizieren, was tatsächlich auf der Chain geschah**, bevor er ein Guthaben anfasst. Zahlungen folgen *quote → payment → inference*, wobei jede verbrauchte Transaktionssignatur in einem Einmal-Register `usedSignatures` vermerkt wird und nie zweimal vorgelegt werden kann. Das macht den Settlement-Dienst zum Nadelöhr — und die Regel, die er nie brechen darf: nur gutschreiben, was die Chain beweist, nie, was der Client behauptet.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Fix #1 · Sender-Bindung", text: "Die Zahlung an den Zahler binden" },
      {
        t: "p",
        md: "Solana-Signaturen sind **öffentlich**. Der Stake-Verifikationspfad prüfte, dass der Vault das erwartete KVR *erhielt* — aber nie, *wer es sandte*. Ein Angreifer konnte auf devnet die KVR→Vault-Überweisung eines Opfers beobachten und dann `{owner: Angreifer, signature: die des Opfers}` einreichen: Die Vault-Empfangsprüfung bestand, das Kapital wurde dem Angreifer gutgeschrieben, und ein Unstake später gehörten die Mittel ihm. Direkter Diebstahl, mit nichts als einem Block-Explorer.",
      },
      {
        t: "code",
        caption: "Der Fix: Das KVR muss von Token-Konten abgebucht worden sein, deren Besitzer der gutgeschriebene Owner ist.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Fix #2 · vertrauenswürdiger Melder", text: "Belohnungen nur aus authentifizierten Quellen" },
      {
        t: "p",
        md: "Die Node-Belohnungs-Endpoints prägten einforderbares KVR aus **selbstgemeldeten Eingaben**: `POST /api/node/contribution` schrieb die vom Client behaupteten `units` gut — `units: 1e9` plus ein Claim-Aufruf konnten den Vault leeren — und register/heartbeat glaubten selbstdeklarierten Hub-/Gateway-Rollen (stündliche Infra-Belohnungen) und Performance-Scores (Belohnungsmultiplikatoren). Der Fix stellt jede belohnungswirksame Behauptung hinter einen **vertrauenswürdigen Melder**: Nur das M2M-Service-Token des Hub-Beitrags-Polls oder ein authentifizierter Admin darf Units, Infra-Rollen oder Perf-Stufen behaupten — erzwungen selbst im offenen LAN-Modus, weil diese KVR prägen. Der Token-Vergleich ist zeitkonstant, und die Wallet↔Node-Verknüpfung bleibt frei; sie kann nur ihre eigenen Belohnungen nicht mehr selbst behaupten.",
      },
      { t: "h2", kick: "Fix #3 · Settlement-Serialisierung", text: "Ein Schreiber pro Guthaben" },
      {
        t: "p",
        md: "Der Settlement-Zustand war ein sperrfreies Read-Modify-Write, und jede Geldoperation *awaitet* mittendrin eine On-Chain-Auszahlung oder Verifikation — und gibt die Event-Loop mit veraltetem Guthaben in der Hand ab. Zwei nebenläufige Claims konnten dasselbe 100-KVR-Guthaben lesen und beide auszahlen. Nicht theoretisch: Der Exploit-Test zeigte **drei nebenläufige Claims, die 300 für ein 100er-Guthaben auszahlten**.",
      },
      {
        t: "code",
        caption: "Asynchrone Serialisierung je Schlüssel: gleichschlüsselige Geldoperationen laufen strikt nacheinander.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Verteidigung in der Tiefe", text: "Wo jede Schicht jetzt steht" },
      {
        t: "table",
        head: ["Schicht", "Mechanismus"],
        rows: [
          ["Identität", "SIWS-Wallet-Signatur-Login über eine Server-Nonce + TOTP-2FA + Einmal-Backup-Codes"],
          ["Transport", "Wallet-abgeleitete Node-Tokens bei Shard-Downloads; Build-Fingerprint-Abgleich am Relay"],
          ["Zahlung", "Sender-Bindung bei Stake-Transfers; Einmal-usedSignatures; private requestId bei Inferenz"],
          ["Settlement", "Schlüssel-Locks um jede Guthaben-Schreibung; erst auszahlen, dann abbuchen; idempotentes Wiedereinreichen"],
          ["Meldung", "Belohnungswirksame Fakten nur vom M2M-Service-Token oder Admin, zeitkonstant verglichen"],
          ["Verwahrung", "Non-custodial Wallets — der Dienst kann nur bewegen, was der Vault hält, nie Nutzerschlüssel"],
        ],
      },
      { t: "h2", kick: "Gemessen, nicht angenommen", text: "Jeder Fix trägt seinen eigenen Exploit-Test" },
      {
        t: "ul",
        items: [
          "Das Abspielen der Transfersignatur eines Opfers unter der Wallet eines Angreifers wird nun abgelehnt („not sent by owner\"); legitime Stakes, Überforderungen und Inferenzzahlungen verhalten sich unverändert.",
          "Drei nebenläufige Claims gegen ein Guthaben zahlen **genau einmal**; das Wiedereinreichen einer bereits bezahlten Inferenz liefert idempotent dasselbe Ergebnis.",
          "Selbstbehauptete `units`, Hub-/Gateway-Rollen und Perf-Stufen unauthentifizierter Clients bewegen keinen einzigen Lamport an Belohnungen mehr.",
        ],
      },
      {
        t: "p",
        md: "Der rote Faden aller drei Fixes ist ein Prinzip in drei Anwendungen: **Die Chain ist die Wahrheitsquelle, der Dienst ist ein Verifizierer, und jedes Guthaben hat genau einen Schreiber.** Der Settlement-Dienst läuft noch auf Solana devnet — genau dort will man diese Klassen finden, ausnutzen und beheben, bevor das Mainnet den Einsatz erhöht.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — Die Engine: linkcpp, eine Steuerungsebene für Inferenz-Engine",
    dek: "Inferenz-Engine liefert eine fähige RPC-Datenebene, aber keine Steuerungsebene. linkcpp ergänzt die fehlende Hälfte — Discovery, Planung, Start und Gateways — um unveränderte Binaries.",
    blocks: [
      {
        t: "p",
        md: "Alles, worauf Kvasir läuft, beginnt hier. **linkcpp** ist eine quelloffen einsehbare Steuerungsebene (Business Source License) um die RPC-Datenebene von Inferenz-Engine: Sie führt große KI-Modelle über mehrere GPUs und Maschinen mit *unveränderten* `ggml-rpc-server`- / `llama-server`-Binaries aus. Die Datenebene bleibt ungeforkt — alles, was linkcpp hinzufügt, ist Orchestrierung.",
      },
      { t: "h2", kick: "Die Lücke", text: "Eine Datenebene ohne Steuerungsebene" },
      {
        t: "p",
        md: "Inferenz-Engine kann ein Modell bereits per RPC über Maschinen verteilen — aber jemand muss die GPUs entdecken, entscheiden, welche Layer wohin gehen, die richtigen Worker mit den richtigen Budgets starten, prüfen, dass jeder Knoten dasselbe Protokoll spricht, und eine API bereitstellen, die Entwickler wirklich aufrufen können. Für einen Cluster ist das von Hand lästig; für ein offenes Netzwerk fremder Geräte unmöglich. Diese Koordinationsschicht ist linkcpp.",
      },
      { t: "h2", kick: "Architektur", text: "Ein Hub, unveränderte Worker, Standard-Gateways" },
      {
        t: "code",
        caption: "Anfragefluss — der Hub orchestriert, unveränderte Binaries rechnen.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock Inferenz-Engine engines below" },
      {
        t: "ul",
        items: [
          "**Drei Beitrittswege:** feste **lokale Node-Slots** mit editierbaren VRAM/RAM/CPU-Budgets; **Remote-Units** — einen anderen Hub registrieren und dessen Knoten importieren; und **verwaltete Node-Agents** — reine Worker-Dienste über schlichtes Request/Response-HTTP, bewusst ohne persistenten Stream, damit sie einfache LAN/VPN-Routings überleben.",
          "**Kompatibilitäts-Gating ist erstklassig:** Jede Unit, jeder Knoten und Agent meldet eine Protokoll-/Runtime-Pack-Identität plus Backend-Details. Abweichungen bei Unit, Runtime-Pack, Inferenz-Engine-Revision und RPC-ABI werden **vor bind, plan, load oder infer hart blockiert** — Backend-Unterschiede (CUDA/Metal/Vulkan/CPU) werden als Fähigkeiten geführt, nicht als Ablehnungen.",
          "**Der Planner** liest GGUF-Metadaten und erzeugt zusammenhängende Layer-Platzierung je Knoten, `--tensor-split`, KV-Cache/Layer/Experten-VRAM-Schätzungen und optionales Experten-FFN-Offload in RAM.",
          "**Gateways:** Jeder Controller exponiert OpenAI-kompatible (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) und Anthropic-kompatible (`/anthropic/v1/messages|models`) Endpoints, getragen vom selben geladenen Modell — bestehende Clients funktionieren unverändert.",
        ],
      },
      {
        t: "p",
        md: "Diese bewusste Trennung — eine unveränderte Datenebene unter einer offenen Steuerungsebene — ist das Fundament für alles Spätere: Ring-Runtime, Layer-Markt und schließlich der Experten-Schwarm sind allesamt Steuerungsebenen-Evolutionen über derselben unveränderten Rechenleistung.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — Der Ring: Pipeline-Inferenz ohne Master",
    dek: "Jedes Gerät lädt nur sein Layer-Fenster und reicht eine kleine Hidden-State-Grenze an seinen Nachbarn weiter. Kein Knoten hält das Modell; kein zentraler Master existiert.",
    blocks: [
      { t: "h2", kick: "Warum kein Stern", text: "Der RPC-Master ist Engpass und Türsteher" },
      {
        t: "p",
        md: "In der klassischen RPC-Topologie öffnet ein Master das **gesamte GGUF** und wählt jeden Worker an. Diese Form bricht in einem offenen Netzwerk dreifach: Der Master muss den ganzen Checkpoint halten und ausliefern; jeder Worker muss anwählbar sein — Smartphones hinter Carrier-NAT sind es nicht; und der Master ist ein einzelner Eigentümer in einem Netzwerk, das keinen haben sollte.",
      },
      { t: "h2", kick: "Der Ring", text: "Layer-Fenster + Grenzweitergabe" },
      {
        t: "ul",
        items: [
          "Jedes Gerät speichert dasselbe Modell, **lädt aber nur sein zusammenhängendes Layer-Fenster** und öffnet genau zwei Verbindungen: eine zum Vorgänger, eine zum Nachfolger.",
          "Eine Anfrage betritt den Ring; jeder Knoten führt seine Layer aus und reicht nur die **Hidden-State-Grenze** an den Nachbarn weiter. Der letzte Rang sampelt den Token und schickt ihn zurück — kein zentraler Master, kein Knoten hält das ganze Modell.",
          "Die Platzierung stammt aus dem **rank manifest** des Planners — bei Qwen3.5-122B werden 49 Layer auf jede auftauchende Mischung aus GPU, CPU, NPU und Smartphone verteilt.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Schwache Geräte zu echten Mitgliedern machen", text: "Teil-Shards, mobile GPUs und das 443-Relay" },
      {
        t: "ul",
        items: [
          "**Teil-Shard-Download:** Eine Ring-Stage braucht nicht den Checkpoint — sie braucht ihr Fenster. Ein Stage-mini-GGUF trägt nur diese Tensoren (**254 MB mit 26 Tensoren** gegenüber dem 77.6-GB-Vollmodell), ein Smartphone zieht also ~1.5 GB für ein Ein-Layer-Fenster statt alles.",
          "**Mobiler GPU-Pfad:** Die RPC-Route zur Smartphone-GPU erwies sich als unmöglich (Adrenos OpenCL-Buffer-Layout übersteht die RPC-Serialisierung nicht), aber eine **Ring-Stage läuft direkt auf der Adreno-GPU** — die Stage besitzt ihr Backend lokal, über die Leitung gehen nur Grenzen.",
          "**NAT-Traversierung:** Smartphones können keine eingehenden Verbindungen annehmen, die Datenebene läuft daher durch ein **443-Relay** — eine WebSocket-Brücke pro Edge mit 1-Byte-Rollen-Preamble, die beide Enden ausgehend wählen lässt. Das Smartphone öffnet null eingehende Ports.",
          "**Selbsteinschreibungsmarkt:** Stages werden beansprucht, nicht zugewiesen. Ein Knoten pollt die Abdeckungs-/Nachfragekarte, wählt das **bestbelohnte** unabgedeckte Fenster, lädt es und tritt bei — Ende-zu-Ende verifiziert mit einem NAT-Smartphone, das die Ring-Inferenz vollendete und seinen Beitrag verdiente.",
        ],
      },
      { t: "h2", kick: "Wo der Ring hingehört", text: "Der Niedriglatenz-Pfad" },
      {
        t: "p",
        md: "Der Ring ist Kvasirs **Latenz**-Pfad: Grenzen sind klein, Hops wenige, und die Dekodierung fließt um die Schleife, ohne zentral etwas einzusammeln. Seine Grenze ist die Granularität — die kleinste Einheit, die ein Knoten tragen kann, ist eine Layer (~1.4 GB beim 122B). Diesen Boden zu entfernen ist die Aufgabe des Experten-Schwarms; der Ring bleibt das Serving-Rückgrat, an das er andockt.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — Im Inneren eines 122B-MoE: Warum die Gewichte geshardet werden wollen",
    dek: "Eine Analyse von Qwen3.5-122B auf Tensorebene: 86 % der Bytes sind 12.544 unabhängige Expertenplatten, jede nur eine saubere Byte-Range-Kopie von der Eigenständigkeit entfernt.",
    blocks: [
      {
        t: "p",
        md: "Bevor wir irgendetwas entwarfen, nahmen wir den 122B auf der Platte auseinander. Die Frage: Wenn ein Schwarm schwacher Geräte dieses Modell tragen soll, was ist die natürliche Trageeinheit? Die Antwort fiel aus dem GGUF-Tensorlayout selbst heraus.",
      },
      { t: "h2", kick: "Anatomie · Qwen3.5-122B-A10B (Q4_K_M)", text: "Woraus eine MoE-Layer wirklich besteht" },
      {
        t: "stats",
        items: [
          { n: "49", l: "Layer" },
          { n: "256", l: "Experten / Layer" },
          { n: "8", l: "aktiv / Token" },
          { n: "12,544", l: "Experten gesamt" },
          { n: "5.3 MB", l: "ein Experte (Q4)" },
          { n: "86%", l: "des Gewichts in Experten" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "voller Checkpoint" },
        ],
      },
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Jede Layer teilt sich in einen **dichten Pfad** — Attention + KV, die Norms, der Router (`ffn_gate_inp`), ein geteilter Experte — und eine **Expertenbank**: 256 unabhängige FFNs, gespeichert als drei gestapelte Tensoren (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Der dichte Pfad ist die Minderheit der Bytes; die Expertenbank sind 86 % des Modells.",
      },
      { t: "h2", kick: "Das Geschenk des Layouts", text: "Experten sind zusammenhängende, blockausgerichtete Platten" },
      {
        t: "ul",
        items: [
          "Der Expertenindex ist die **äußerste ggml-Dimension** (`ne[2]`) jedes Expertentensors — Experte *e* belegt eine zusammenhängende, quantblock-ausgerichtete Platte roher quantisierter Bytes.",
          "Das macht die Extraktion je Experte zu einer **Byte-Range-Kopie**: `data[a:b]`, keine Dequantisierung, kein Umpacken — ein experten-geschnittenes mini-GGUF ist billig zu erzeugen und bittreu.",
          "Pro Token feuern je Layer nur **8 von 256** Experten, vom Router gewählt — der Expertenverkehr einer Layer beim Dekodieren ist eine Handvoll kleiner Matrixmultiplikationen über einen Hidden-Vektor.",
        ],
      },
      { t: "h2", kick: "Die Konsequenz", text: "Die Trageeinheit fällt von 1.4 GB auf 5.3 MB" },
      {
        t: "p",
        md: "Am Layer-Korn ist das Minimum, das ein Knoten halten kann, ~**1.4 GB** — für die meisten Smartphones unerreichbar, sobald App, KV und OS ihren Anteil nehmen. Am Experten-Korn ist die Einheit **5.3 MB**, und ein realistischer Beitrag sind 8–64 Experten (**42–340 MB**) — bequem in jedem modernen Gerät. Die Experten sind wechselseitig unabhängig, Eigentum kann also beliebig verstreut und frei umverteilt werden. Diese Analyse machte das Sharding auf Expertenebene zur Design-Wette: Die Gewichte waren bereits in schwarmgroße Einheiten verpackt — das Netzwerk musste die Verpackung nur ehren.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — Backbone-Experten-RAM-Offload (M0)",
    dek: "Streame die MoE-Experten-FFNs aus dem CPU-RAM statt der VRAM, und ein einzelner 64-GB-Koordinator hält einen 122B — ohne Graph-Chirurgie.",
    blocks: [
      {
        t: "p",
        md: "Experten-FFNs müssen nicht in der VRAM leben. Sie aus dem CPU-RAM zu streamen lässt einen Koordinator ein Modell halten, dessen Experten seine VRAM übersteigen — das Fundament, das schwachen Knoten den Beitritt zu einem großen MoE überhaupt erlaubt.",
      },
      { t: "h2", kick: "Planner verifiziert · echtes 122B-GGUF", text: "Ein 122B passt auf einen einzelnen 64-GB-Koordinator" },
      {
        t: "p",
        md: "Zuvor platzierte der Ring Gewichte nur in VRAM, der 122B (77.6 GB) war auf einem 64-GB-GCD also **infeasible**. Mit Experten-Offload-Regeln kommt der Dry-Run **feasible** zurück:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "122B-Ring-Plan" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (Experten)" },
          { n: "10", l: "ausgelagerte Layer" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Planner-Ausgabe — Inferenz-Engine -ot-Regelformat.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # Inferenz-Engine -ot format`,
      },
      { t: "h2", kick: "Was verdrahtet wurde · reines Python, kein C++-Rebuild", text: "Die Offload-Regeln des Planners in einen echten Load tragen" },
      {
        t: "ul",
        items: [
          "**planner** — emittiert bereits `ot` (kommagetrennte `-ot`-Regeln) in jeder Platzierung.",
          "**protocol.py** — Feld `StageStartRequest.ot` hinzugefügt.",
          "**runtime.py** — leitet das `ot` der Platzierung in die Stage-Anfrage weiter.",
          "**stage_service.py** — der Koordinator startet mit `--override-tensor`.",
          "`linkcpp-server` reicht unbekannte Argumente an den unveränderten llama-server durch, `-ot` greift also unangetastet.",
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
        md: "Verifiziert mit einem `ot`-Protokoll-Roundtrip-Test plus der Bestätigung, dass das Koordinator-Kommando `--override-tensor` emittiert; der Hub wurde sauber und regressionsfrei neu deployt. Was damals blieb: der volle 2-Knoten-77-GB-Load (Koordinator mit Offload + ein Smartphone mit einem ~1.5-GB-Ein-Layer-Fenster), gated auf Serververfügbarkeit. Der Kern von M0 — das Backbone-Offload, das schwache Knoten an einem großen MoE teilnehmen lässt — war auf Code- und Planner-Ebene abgeschlossen.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — Der Experten-Scheiben-Datenpfad (M1)",
    dek: "Ein schwaches Gerät lädt ein paar 6-MB-Experten, keine 1.4-GB-Layer — und die geshardete Rechnung gleicht der monolithischen bis auf 3.6e-12.",
    blocks: [
      { t: "h2", kick: "Verifiziert · echtes Qwen3.5-122B-A10B", text: "Eine Expertenscheibe ist eine Bytekopie — ohne Dequantisierung" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "Scheibe in Expertendim." },
          { n: "~6.1", l: "MB / Experte (Q4+Q6)" },
          { n: "206 MB", l: "Download 2 Layer × 16 Exp." },
          { n: "200", l: "HTTP, gültiges GGUF" },
        ],
      },
      {
        t: "p",
        md: "MoE-Expertentensoren stapeln alle Experten entlang der äußersten ggml-Dimension, der Reader exponiert also `(n_expert, rows, row_bytes)` roher quantisierter Bytes. Experte *e* ist eine quantblock-ausgerichtete zusammenhängende Platte — die Scheibe ist buchstäblich `data[a:b]`, ohne Dequantisierung und ohne Umpacken.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — der verifizierte Roundtrip.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "Das numerische Orakel", text: "dispatch + combine == monolithisch, exakt" },
      {
        t: "p",
        md: "Mit echten Layer-0-Experten des 122B (dequantisierte Referenz) gleicht das Aufteilen der Experten in 4 Shards, separates Berechnen und Kombinieren **der monolithischen MoE-FFN**: Sharding ist eine exakte Umgruppierung derselben gewichteten Summe, keine Näherung.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "relativer Fehler" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "berührte Experten" },
        ],
      },
      { t: "h2", kick: "C++-Worker, hardwareverifiziert", text: "linkcpp-expert-worker reproduziert das Orakel auf ROCm" },
      {
        t: "ul",
        items: [
          "**Reines ggml/gguf** (kein libllama): lädt die Scheibe in ein GPU-Backend und führt `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)` aus.",
          "**ROCm-Build + Lauf** auf einem MI250: 122B Layer-0, Experten [0,8), 4 Tokens.",
          "**Cosine 0.99995 vs. das Orakel**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — dieses Residuum ist selbst der erste gemessene Fall von Cross-Backend-Äquivalenz (ROCm vs. numpy).",
          "Derselbe Codepfad deckt CUDA/Metal/Vulkan/CPU ab (`mul_mat_id`/`swiglu` sind Standard-ggml; CUDA hat einen dedizierten MoE-Kernel).",
        ],
      },
      {
        t: "p",
        md: "Das schwerste, riskanteste Stück — der Worker-Kernel auf dem Gerät — wurde hier verifiziert. Was blieb, war die Backbone↔Worker-Orchestrierung; der Worker ist eine bewiesene reine Funktion, die diese Scheiben konsumiert.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — Verteilter Experten-Dispatch (M2)",
    dek: "Eine Live-122B-Dekodierung übergibt die Expertenrechnung einer Layer per TCP an einen separaten Worker-Prozess — und sagt exakt denselben Token voraus.",
    blocks: [
      { t: "h2", kick: "Verifiziert · echter 122B, zwei Prozesse", text: "Backbone-Dekodierung → TCP → Worker → Experten → derselbe Token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "Logit-Cosine" },
          { n: "0", l: "Transportverlust (byte-identical)" },
          { n: "2", l: "Prozesse (Backbone + Worker)" },
        ],
      },
      {
        t: "p",
        md: "Der Experten-Worker serviert die Layer-0-Scheibe als **separaten Prozess** (ROCm), und der `build_moe_ffn`-Dispatch-Callback des 122B-Backbones verschickt `(cur, sel)` über TCP und empfängt die Expertenausgaben. Der Logit-Cosine ist **exakt der In-Process-Wert** (0.99868775) — der Transport ist verlustfrei. Expert-paralleles Schwarmrechnen funktioniert über eine Prozessgrenze hinweg.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "Eine langlebige TCP-Verbindung — derselbe Stream, den Ring/443-Relay tunneln können.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Erledigt", text: "Die verteilte Dispatch-Pipeline" },
      {
        t: "ul",
        items: [
          "`--serve`-Modus: Scheibe laden, auf TCP lauschen, `(n_used, n_tokens, cur, sel) → experts` beantworten.",
          "`--dispatch-port`: der Backbone-Callback sendet/empfängt per TCP an einen separaten Worker und ersetzt die In-Process-Rechnung.",
          "Gemessen auf einer Live-122B-Dekodierung mit Layer-0 außerprozesslich dispatcht → **argmax MATCH**, cosine 0.99869 (= in-process, verlustfrei).",
          "M2-Kern (früher): das ARM des Smartphones berechnete echte 122B-Experten bei cosine 0.99992 (Android-Crossbuild).",
        ],
      },
      {
        t: "p",
        md: "Als Nächstes: denselben TCP-Stream durch das **443-Relay** zu Workern auf anderen Maschinen und Smartphones tunneln (der Transport wurde bereits in der Ring-Arbeit bewiesen), dann der M3-Abdeckungsmarkt und M4-Batch-Durchsatz.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — Der Experten-Abdeckungsmarkt (M3)",
    dek: "Schwache Knoten sehen, welcher (Layer, Experten-Bereich) am knappsten und bestbezahlt ist, und füllen ihn selbst — der bewährte Layer-Markt, feiner gekörnt.",
    blocks: [
      {
        t: "p",
        md: "Kvasirs Layer-Shard-Markt — Nachfragekarte, Selbsteinschreibung nach Maximalbelohnung, Teil-Download, Belohnungen je Knoten — war bereits geräteverifiziert. M3 parametrisiert denselben Mechanismus am Korn der **(Layer, Experten-Bereich)** neu, sodass sich die Abdeckung selbst zu den am stärksten unterreplizierten, bestbezahlten Expertenbereichen hin heilt.",
      },
      { t: "h2", kick: "Verifiziert · API", text: "Knappheitsaggregation → Zuteilung des bestbelohnten Bereichs" },
      {
        t: "p",
        md: "Drei Worker registrieren sich auf Layer 0: A = [0,128), B = [128,256), C = [0,128) als zweite Replik, mit `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["Layer", "Experten", "Repliken", "Knappheit"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (Ziel erreicht)"],
          ["0", "[128, 256)", "1", "0.5 (unter Ziel)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → schneidet den knappsten Bereich aufs Budget des Knotens zu.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Erledigt · reines Python (Hub)", text: "Ein Angebots-/Nachfragemarkt am Expertenkorn" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — Worker heartbeaten ihre (Layer, Experten-Bereich)-Bestände.",
          "`GET /api/expert-demand` — Replik-Aggregation je Experte → zusammenhängende Expertenbereichs-Segmente mit Knappheitswerten.",
          "`POST /api/expert-volunteer` — teilt den knappsten Bereich zu, zugeschnitten aufs Budget des Knotens.",
          "Der bestehende Layer-Markt (Selbsteinschreibung · Teil-Download · Belohnungen) neu parametrisiert auf (Layer, Experten-Bereich).",
        ],
      },
      {
        t: "p",
        md: "Was in M4 folgt: Batch-Dispatch für nebenläufige Anfragen plus ein Hot-Expert-Cache — tokens/s proportional zur Worker-Zahl — und Replik-Routing (nächster/schnellster Worker) mit Churn-Fallback.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — Batch-Dispatch-Durchsatz (M4)",
    dek: "Der Schwarm ist ein Durchsatzgewebe, kein Latenzspiel: Dispatch-Aufrufe zu bündeln amortisiert den Overhead pro Anfrage 77× pro Token.",
    blocks: [
      { t: "h2", kick: "Gemessen · ROCm, Experten-FFN, n_used = 8", text: "Größere Batches, mehr tok/s pro Worker" },
      {
        t: "table",
        head: ["Batch", "tok/s pro Worker"],
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
        md: "Von **1.45 ms/tok** bei Batch 1 zu **0.019 ms/tok** bei Batch 512 — eine 77×-Verbesserung pro Token. Die Zeit pro Aufruf bewegt sich kaum (1.45 → 9.6 ms), während der Batch um 512× wächst — die GPU verarbeitet den Batch hinter einem fixen Overhead nahezu gratis. Das ist die **Durchsatzgewebe-Eigenschaft**, die Expert-Parallel praktikabel macht: Batch-Dispatch amortisiert RTT und Overhead pro Anfrage.",
      },
      { t: "h2", kick: "Erledigt", text: "Batch-Dispatch-Durchsatz" },
      {
        t: "ul",
        items: [
          "Worker `--bench`: compute_dispatch-Zeitmessungen für Batch 1…512 → tok/s.",
          "**53k tok/s pro Worker** bei Batch 512 (ROCm) — das Bündeln amortisiert den Overhead.",
          "Darauf stapeln sich Hot-Expert-Caching und Multi-Worker-Aggregatskalierung (Replik-Routing).",
        ],
      },
      {
        t: "callout",
        md: "Mit M4 ist die **gesamte M0 → M4-Pipeline auf einem echten 122B demonstriert**: Backbone-Offload · Expertenscheiben · verifizierte Worker · Live-Decode-Dispatch (argmax MATCH) · verteilte Prozesse · Abdeckungsmarkt · Batch-Durchsatz.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "Ein Smartphone trat einer 122B-Inferenz bei",
    dek: "Ein Galaxy S25 lud autonom seine Expertenscheibe vom Hub und berechnete bei jedem Schritt einer Live-122B-Dekodierung die Experten einer Layer. Die Ausgabe war korrekt.",
    blocks: [
      {
        t: "callout",
        md: "Prompt: **\"The capital of France is\"** → generiert (mit dem Smartphone in der Schleife): **\" Paris.\"** — 8/8 Tokens identisch zum lokalen Lauf.",
      },
      { t: "h2", kick: "Gemessen · echter 122B, Smartphone rechnet Layer-0", text: "Korrektheit + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "Tokens identisch zum Lokalen" },
          { n: "4.01", l: "TPS lokal (Basislinie)" },
          { n: "3.13", l: "TPS mit Smartphone" },
          { n: "1.58 GB", l: "autonomer Download" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Selbst wenn das Smartphone für jeden Token die Layer-0-Experten rechnet, sind **die generierten Tokens exakt die lokalen** — das korrekte \"Paris.\". Die TPS fällt von 4.01 auf 3.13 — der Smartphone-Dispatch-Roundtrip (MI250 → Tunnel → Smartphone, ~100 ms/Token) kostet 22 %. Der Durchsatz kommt mit Batching und Repliken zurück (M4).",
      },
      { t: "h2", kick: "Der autonome Teilnahmefluss", text: "Entdecken → belohnungsgetriebener Download → dem Rechnen beitreten" },
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
      { t: "h2", kick: "Verifiziert vs. verbleibend", text: "Der Mechanismus ist vollständig; die In-App-Schleife ist Produktisierung" },
      {
        t: "ul",
        items: [
          "Teil-Download (der expert-shard-Endpoint), Worker-Serving, Backbone-Dispatch, Live-122B-Generierung und TPS — alles auf dem echten Gerät verifiziert.",
          "Korrektheit: Mit dem Smartphone in der Schleife gleichen 8/8 Tokens dem lokalen Lauf, mit der richtigen Antwort.",
          "Verbleibend: die autonome In-App-Schleife (expert-demand pollen → volunteer → herunterladen → serve → registrieren) ist Kotlin-Verdrahtung — diese Demo trieb den Mechanismus direkt.",
          "Transport: Diese Demo nutzte einen SSH-Tunnel; die Produktion nutzt das 443-Relay (bereits in der Ring-Arbeit verifiziert).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "Die Kvasir-Ökonomie: Ein tugendhafter Kreislauf aus Kosten und Belohnung",
    dek: "Ein dezentrales Inferenz-Netzwerk funktioniert nur, wenn der Preis, den Verbraucher zahlen, und die Belohnung, die Knoten verdienen, sich gegenseitig verstärken. Hier ist das Schwungrad, auf das wir hinarbeiten, die Spiralen, die es töten, und die drei Invarianten, die es am Drehen halten.",
    blocks: [
      {
        t: "callout",
        md: "**These:** Kvasir ist ein zweiseitiger Markt, abgerechnet in einem einzigen Token — Verbraucher zahlen KVR, um zu inferieren, Knoten verdienen KVR, um zu bedienen. Das gesamte Design gelingt oder scheitert an einer Eigenschaft: Diese zwei Seiten müssen einen **tugendhaften Kreislauf** bilden, in dem jede Drehung die nächste leichter macht. Mach das falsch, und jede Preispolitik kollabiert irgendwann; mach es richtig, und das Netzwerk wird *billiger*, während es *größer* wird.",
      },
      {
        t: "p",
        md: "Es ist verlockend, Kosten und Belohnung als Tauziehen zu behandeln — jeder Dollar, den ein Verbraucher spart, ist ein Dollar, den ein Knoten nicht verdient. Diese Rahmung ist eine Falle. In einem gesunden Netzwerk sind sie **dasselbe Schwungrad** von zwei Enden her gesehen: Zahlungen werden zu Belohnungen, Belohnungen werden zu Angebot, Angebot wird zu Kapazität und niedrigeren Preisen, niedrigere Preise werden zu mehr Nutzung, und mehr Nutzung wird zu mehr Zahlungen. Die Frage ist nicht, wie man einen festen Kuchen aufteilt; sie ist, wie man das Rad am Drehen hält, sodass der Kuchen wächst.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "Das Schwungrad", text: "Warum Nutzung und Angebot gemeinsam wachsen" },
      {
        t: "p",
        md: "Der Motor des Kreislaufs ist eine einzige Regel, die in Kvasir bereits gilt: **Inferenz muss in KVR bezahlt werden**. Das macht jede Nutzungseinheit zu einer Einheit echter Nachfrage nach dem Token — Utility, keine Spekulation. Token-Nachfrage stützt den Wert der KVR, die Knoten verdienen; attraktive Belohnungen ziehen Angebot an; Angebot erweitert die Kapazität und treibt, durch Wettbewerb und feineres Experten-Sharding, die Grenzkosten des Bedienens nach unten; billigerer, schnellerer, fähigerer Dienst zieht mehr Nutzung an. Kvasir zieht die Schleife mit einer Eigenschaft enger, die keine zentralisierte API kopieren kann: ein Teilnehmer kann **zugleich Verbraucher und Anbieter** sein. Die Nachfrageseite und die Angebotsseite wachsen oft in *denselben Menschen*, was die Ungleichgewichte dämpft, die einseitige Märkte zerstören.",
      },
      { t: "h2", kick: "Die Versagensmodi", text: "Vier Spiralen, die das Rad rückwärts laufen lassen" },
      {
        t: "p",
        md: "Ein Schwungrad kann sich ebenso leicht herunterdrehen wie hinauf. Die Todesspiralen zu benennen ist die Art, gegen sie zu entwerfen:",
      },
      {
        t: "table",
        head: ["Spirale", "Wie sie beginnt", "Wo sie endet"],
        rows: [
          ["Belohnungsverwässerung", "Mehr Knoten jagen flache Nachfrage", "Belohnung je Knoten fällt, Knoten gehen, Kapazität sinkt"],
          ["Preis-zu-niedrig", "Billiger Preis, Belohnungen unter den Knotenkosten", "Bedienen zahlt sich nicht mehr, Angebot und Qualität kollabieren"],
          ["Preis-zu-hoch", "Gute Belohnungen, aber über Marktniveau", "Nutzer wählen eine billigere API, der Umsatz versiegt"],
          ["Emissionsabhängigkeit", "Belohnungen aus Prägung bezahlt, nicht aus Umsatz", "Inflation erodiert KVR, bis beide Seiten aufgeben"],
        ],
      },
      { t: "h2", kick: "Die Invarianten", text: "Drei Regeln, die den Kreislauf tugendhaft halten" },
      {
        t: "ul",
        items: [
          "**Belohnungen werden aus echtem Umsatz finanziert.** Im stationären Zustand kommt das, was Knoten verdienen, aus dem, was Verbraucher zahlen — nicht aus offener Token-Emission. Emission ist eine Bootstrap-Subvention, die *tapern* muss, während der Gebührenumsatz wächst. Kvasir hilft hier bereits, indem es **echte Arbeit** belohnt — KVR pro tatsächlich bediente Tokens × Layer-Anteil, nicht bloße Präsenz — sodass die Subvention nicht an leerlaufende 'Söldner'-Knoten lecken kann.",
          "**KVR ist das verpflichtende Medium.** Weil man ohne KVR-Zahlung nicht inferieren kann, ist Nutzung eine dauerhafte Nachfragesenke für den Token. Das verankert den Token-Wert an echter Utility statt an Spekulation — der Unterschied zwischen einer Währung und einem Spielchip.",
          "**Der Preis schwebt innerhalb eines Bandes.** Ein Boden, über den Grenzkosten der Knoten gehalten, hält das Bedienen lohnend; eine Decke, unter zentralisierten Alternativen gehalten, hält Kvasir wettbewerbsfähig. Zwischen ihnen bewegt sich der Preis — und dort zeigt sich das Wachstum des Netzwerks endlich als niedrigere Kosten.",
        ],
      },
      { t: "h2", kick: "Der Thermostat", text: "\"Mehr Knoten → billiger\" im Code wahr machen" },
      {
        t: "p",
        md: "Heute ist der Preis eine regierte Konstante — sinnvoll für ein Devnet, aber es bedeutet, dass das Hinzufügen von Knoten die *Kapazität* erhöht, nicht die Erschwinglichkeit. Die Designrichtung ist ein **auslastungsgetriebener Preis**: leerlaufendes Angebot drückt den Preis hinunter zum Boden, Überlastung drückt ihn hinauf zur Decke. Dieses eine Signal verwandelt die Intuition *\"je mehr Menschen Rechenleistung teilen, desto billiger wird es\"* in eine Regel, die das Protokoll erzwingt — während der Boden die Betreiber solvent hält, sodass das Angebot, das es billig machte, nicht verdampft. Weil der Preis ein sensibler ökonomischer Parameter ist, ändert er sich nur unter **genesis-Wallet-Autorität mit Wallet-Signatur + 2FA**, nie durch eine verirrte Umgebungsvariable.",
      },
      {
        t: "callout",
        md: "**\"Kostenlos\" ist das Netto, nicht der Preis.** Du zahlst für das, was du inferierst, und verdienst für das, was du bedienst; trage ungefähr so viel bei, wie du verbrauchst, und deine Rechnung geht netto auf null. Keine Abo-API — Claude Max, ein Codex-Platz — kann das bieten, denn du kannst nie ihre Angebotsseite sein. Mit Kvasir kannst du Modelle betreiben, die deine eigene Maschine nicht halten kann, *und* dafür bezahlt werden, anderen beim Betreiben ihrer zu helfen.",
      },
      {
        t: "p",
        md: "Nichts davon erfordert exotisches Mechanismusdesign. Es erfordert Disziplin bei drei Dingen: Belohnung aus Umsatz, Wert aus Nutzung, Balance aus einem beschränkten schwebenden Preis. Kvasir liefert bereits die schweren, ehrlichen Teile — non-custodial Abrechnung, arbeitsproportionale Belohnungen, einen Token, den man tatsächlich ausgeben muss, um das Netzwerk zu nutzen. Der Rest ist die ökonomische Roadmap: der Taper, der Gebühren-Split, der einen Versicherungspool für fehlgeschlagene Inferenzen finanziert, und der Thermostat. In dieser Reihenfolge gebaut, hören Kosten und Belohnung auf zu kämpfen und fangen an, sich zu verstärken.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "Eine GPU über das Internet trat einer 122B-Inferenz bei",
    dek: "Eine Blackwell-Workstation in einer anderen Stadt wählte eine einzige ausgehende 443-Verbindung und berechnete Experten für eine Live-122B-Dekodierung — byte-identisch zu einem lokalen Lauf und in KVR bezahlt für die geleistete Arbeit.",
    blocks: [
      {
        t: "callout",
        md: "**Was geschah:** Eine 122B-Dekodierung, laufend auf einem AMD-Backbone an einem Ort, schickte ihre Experten-Arbeit je Token an eine NVIDIA-GB10-Maschine (Grace Blackwell) in einer anderen Stadt — über eine einzige ausgehende WebSocket-Verbindung auf Port 443 — und erhielt Expertenausgaben zurück, die **exakt dieselben Tokens** erzeugten wie die lokale Berechnung. Kein Tunnel, keine Portweiterleitung, kein eingehendes Firewall-Loch. Die entfernte Maschine verdiente KVR für die Bytes, die sie bediente.",
      },
      {
        t: "p",
        md: "Kvasirs Prämisse ist *welche Hardware auch immer auftaucht* — auch Hardware hinter Carrier-NAT, im öffentlichen Internet, in einer anderen Stadt. Qwen3.5-122B-A10B trägt **86 % seines Gewichts in 12.544 unabhängigen Experten** (48 Layer × 256, top-8), jeder eine reine 5.3-MB-Funktion. Genau dieses Korn lässt eine entfernte, fremde Maschine eine Scheibe halten und beitragen. Die offene Frage war nie *können wir es aufteilen* — sondern *kann ein Worker über das offene Internet wirklich an einer Live-Dekodierung teilnehmen, korrekt und abrechenbar*. Nun hat er es.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "Ein ausgehender Wählvorgang", text: "Kein Tunnel, keine eingehenden Ports" },
      {
        t: "p",
        md: "Der entfernte Worker öffnet **eine** Verbindung — ausgehend `wss://` zum öffentlichen Gateway auf 443, dem einzigen Port, den Carrier-NAT und CDN-Edges zuverlässig durchlassen. Das Gateway parst den Stream nicht; es **spleißt** die WebSocket **roh** zum LAN-only-Hub durch, der sie zum Experten-Dispatch-Lauscher des Backbones brückt. Beide Enden wählten nach außen und trafen sich in der Mitte. Der Worker exponiert null eingehende Ports und braucht keine öffentliche Adresse.",
      },
      {
        t: "code",
        caption: "Zwei ausgehende Wählvorgänge, gespleißt zu einem gewöhnlichen Dispatch-Stream.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Byte-identisch über das Internet", text: "Der Router entscheidet einmal; die Rechnung gruppiert exakt um" },
      {
        t: "p",
        md: "Der Backbone führt den Router **einmal** und autoritativ aus; der Worker ist eine reine `(hidden, ids) → out`-Funktion. Diese Funktion über einen Kontinent zu verschieben ändert also, *wo* die Multiplikation geschieht, nicht *was* sie berechnet. Auf einer Live-122B-Dekodierung mit remote bedienten Layer-0-Experten: Der Greedy-Token-Stream war **8/8 identisch** (\" Paris.\"), Logit-**cosine 0.99773**, argmax-Übereinstimmung. Das ist dieselbe Router-Autoritäts-Eigenschaft, die die diskreten Entscheidungen von CUDA↔ROCm↔CPU invariant hält — heterogene Backends bleiben durch kontinuierlichen Fehler beschränkt, nie durch einen katastrophalen Zweig.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "Greedy-Tokens identisch" },
          { n: "0.99773", l: "Logit-cosine, remote vs. lokal" },
          { n: "1.2%", l: "TPS-Overhead, direkt (13 ms RTT)" },
          { n: "0.00895", l: "KVR an den Worker, erste Remote-Session" },
        ],
      },
      { t: "h2", kick: "Die ehrlichen Kosten: RTT", text: "Warum der Schwarm ein Durchsatzgewebe ist, kein Niedriglatenz-Decoder" },
      {
        t: "p",
        md: "Serieller Dispatch je Token zahlt einen Roundtrip pro Schritt. Gemessen: mit einer direkten Verbindung (13 ms RTT) betrug der Durchsatz-Overhead **1.2 %** (4.220 → 4.169 tok/s); durch einen CDN-Edge auf 443 geroutet waren es **~28 %**. Wir veröffentlichen das ehrlich, weil es auf die Design-Wahrheit zeigt — ein WAN-Schwarm ist **RTT-gebunden**, seine Stärke ist also nicht die Latenz eines Streams, sondern die **aggregierte Kapazität**. Batching amortisiert den Roundtrip: gebündelter Experten-Dispatch erreicht bei Batch 512 den **77-fachen Durchsatz je Token**. Bytes sind Spielraum; Roundtrips sind das, was zu verbergen ist — das Thema des begleitenden Roadmap-Beitrags.",
      },
      { t: "h2", kick: "Bezahlt für genau die Arbeit", text: "Gemessene Bytes werden zu KVR" },
      {
        t: "p",
        md: "Teilnahme ist wertlos, wenn sie nicht abrechenbar ist. Das Relay **misst die je Session gebrückten Bytes** in das Beitrags-Ledger des Hubs; das Gateway pollt dieses Ledger und schreibt KVR als Delta der **eigenen** Wallet des Workers gut — non-custodial, wie alles andere. Die erste Cross-Internet-Session akkumulierte tatsächlich: **1.28 MB Arbeit → 1.277952 Units → 0.00895 KVR** an ausstehenden Belohnungen. Klein, und genau das ist der Punkt — es ist echtes Settlement je Arbeit, keine Teilnahme-Trophäe.",
      },
      {
        t: "p",
        md: "Derselbe ausgehende 443-Pfad ist genau der Weg, auf dem ein **Smartphone** beitritt: Ein Galaxy S25 hat darüber bereits 122B-Experten berechnet (8/8 identische Tokens, cosine 0.99992). Ein Modell von Frontier-Größe, bedient von einem Backbone an einem Ort, einer Rechenzentrums-GPU in einer anderen Stadt und einem Smartphone in jemandes Tasche — alle produzieren dieselben Tokens, jeder bezahlt für seinen Anteil. Was als Nächstes kommt, ist, den WAN-Roundtrip billig zu machen; diese Roadmap gründet in den Produktionszahlen anderer und unseren eigenen Messungen.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "WAN-Dispatch billig machen: eine fundierte Roadmap",
    dek: "Remote-Experten-Dispatch funktioniert und ist byte-identisch — aber eine WAN-Dekodierung ist roundtrip-gebunden. Hier ist der Plan, die Kosten zu senken, fundiert in Produktionszahlen von DeepSeek, Petals und anderen (Roadmap, nicht ausgeliefert).",
    blocks: [
      {
        t: "callout",
        md: "**Rahmung:** Die Zahlen, die *wir gemessen* haben, sind als gemessen ausgewiesen; alles, was als Plan beschrieben ist, ist eine **Roadmap**, kein ausgeliefertes Ergebnis. Das Ziel ist, den Remote-Experten-Dispatch — bereits korrekt und bezahlt (siehe den Begleitbeitrag) — zu nehmen und den WAN-Roundtrip billig genug zu machen, dass eine entfernte GPU oder ein Smartphone ein erstklassiges Schwarmmitglied ist, kein langsames.",
      },
      {
        t: "p",
        md: "Unsere eigene Messung, schlicht veröffentlicht: Dispatch kostet etwa **110 KB pro Token pro Layer** — 12.3 KB hinaus (Dispatch) plus 98.3 KB zurück (Combine). Die 8×-Asymmetrie rührt daher, dass jeder ausgewählte Experte seine volle Ausgabe *vor* der gewichteten Summe zurückgibt. Direkt verbunden sind das **1.2 %** Durchsatz-Overhead; durch ein CDN-Relay **~28 %**. Das sind die Fakten. Der Rest dieses Beitrags ist, wie wir die Lücke zu schließen gedenken — und warum Bytes der leichte Teil sind.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "Das beherrschende Gesetz", text: "Eine WAN-Dekodierung ist roundtrip-gebunden" },
      {
        t: "p",
        md: "Das mit Abstand wichtigste veröffentlichte Ergebnis hier ist nicht unseres — es ist das von Petals: Wenn die RTT von <5 ms auf 100 ms steigt, fällt die Dekodierung von **1.24 auf 0.57 steps/s**, während ein **10×-Bandbreitenschnitt sie um ~0 ändert**. Latenz dominiert; Bandbreite ist Reserve. Das rahmt das ganze Problem neu: Bytes zu schaben ist Spielraum, aber **Roundtrips zu senken ist die Substanz**. Jeder Punkt unten ist danach gereiht, wie viel Roundtrip-Kosten er entfernt.",
      },
      { t: "h2", kick: "Billiger auf der Leitung", text: "Bytereduktion, Genauigkeit zuerst" },
      {
        t: "ul",
        items: [
          "**Gewichtete Teilsummen zurückgeben, nicht rohe Expertenausgaben.** Durch Linearität ist das Combine des Backbones so oder so exakt, aber der Worker gibt einen summierten Vektor statt 8 zurück — das ist die ~8×-Combine-Reduktion, und genau das tun DeepSeek-V3 / DeepEP in Produktion.",
          "**Paralleler Stern, keine serielle Kette** über mehrere Worker: ΣRTT kollabiert zu max RTT.",
          "**F16 auf der Leitung** — wir akzeptieren bereits einen Cross-Backend-cosine von ~0.998, F16-Transport liegt also innerhalb der bestehenden Toleranz; **blockweise INT8/FP8 später**, nachdem unser eigenes argmax/cosine-Gate es gegen Q4_K_M-Gewichte freigibt (Petals zeigte INT8 über das echte Internet ohne Qualitätsverlust).",
          "Zusammen zielen diese auf **~110 KB → 9–12 KB pro Token (~12×)** — real, aber es bleibt der *Spielraum*, nicht der Engpass.",
        ],
      },
      { t: "h2", kick: "Den Roundtrip amortisieren", text: "Die Substanz: weniger Trips, verborgene Trips" },
      {
        t: "ul",
        items: [
          "**Spekulatives Dekodieren** verwandelt viele Tokens in einen Roundtrip. Bei gemessenen 80 ms WAN liegt der Break-even bei nur **~1.15–1.2 akzeptierten Tokens/Schritt** — selbst ein schwacher n-Gramm-Tipp gewinnt also (Vanilla-Jacobi kann nach hinten losgehen; die Technikwahl zählt). Unser Dispatch-Protokoll trägt bereits `n_tokens > 1`, es ist also keine Leitungsänderung nötig.",
          "**Continuous Batching am Gateway** faltet nebenläufige Anfragen in einen Trip; **Slot-Affinity-Prefix-Caching** hält eine Session auf denselben Repliken.",
          "**Latenzverbergung**: Der geteilte Experte ist ein unabhängiger additiver Term, der Backbone berechnet ihn also *lokal* während des Remote-Roundtrips (ScMoE berichtet 1.82× über PCIe, ohne Retraining). Halte **heiße Experten lokal**, schicke nur kalte remote (EPLB repliziert die ~32 heißesten für einen 2.54×-Dekodier-Speedup in Produktion).",
        ],
      },
      { t: "h2", kick: "Policy & die Fat-Pipe-Zukunft", text: "Nach Peer routen, und was 200 Gb/s ändert" },
      {
        t: "p",
        md: "Pfad-Policy: öffentlich routbare Peers nehmen den **direkten** Pfad (die 1.2%-Route); das Relay ist nur für NAT-gebundene Geräte. Und wenn breite 200-Gb/s-Verbindungen ankommen, serialisiert sich die 110 KB in **~4.4 µs** — der Bandbreitenterm verschwindet schon vor den obigen Reduktionen, und eine 794-MB-Scheibe geht in ~32 ms über die Leitung. Aber **RTT ist Physik; sie schrumpft nicht** — spekulatives Dekodieren und Overlap bleiben also die echten Hebel selbst bei 200 G. Wo die Fat Pipe wirklich zählt, ist Multi-Backbone-Föderation (mehrere Backbones teilen sich einen Expertenpool) und bandbreitengebundene Arbeit: Langprompt-Prefill und Groß-Batch-Durchsatz.",
      },
      {
        t: "callout",
        md: "**Ein Vorbehalt, ehrlich gesagt:** Die Transportschicht selbst (WebSocket vs. QUIC, Masking-Overhead, NAT-Hole-Punching) hat **kein externes Ergebnis, das wir zitieren können** — das ist Engineering, das wir selbst messen werden, bevor wir irgendetwas behaupten. Alles oben ruht auf veröffentlichten Produktionszahlen (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) plus unseren eigenen Messungen; wenn ein Roadmap-Punkt ausgeliefert wird, werden seine Zahlen und sein Tempus hier aktualisiert.",
      },
      { t: "h2", kick: "Was als Nächstes kommt", text: "Onboarding-Ziele" },
      {
        t: "p",
        md: "Unser Dispatch-Hook sitzt auf `build_moe_ffn` — **einer einzigen Funktion, die 43 MoE-Architekturen** in der Inferenz-Engine teilen. Drei Invarianten sind modellunabhängig: die MoE-Mathematik (routed = Σ wᵢ·Eᵢ(x), linear), der geteilte Codepfad und GGUFs standardmäßig gestapelte Expertentensoren (äußerstes `ne[2]` → blockausgerichtetes Slicing). Ein neues Modell zu onboarden ist also kein Redesign — es ist ein Durchlauf durch ein argmax/cosine-Verifikations-Gate je Modell.",
      },
      {
        t: "table",
        head: ["Modell", "Experten · Routing", "je Experte (Q4≈)", "geteilt", "Status"],
        rows: [
          ["Qwen3.5-122B (heute im Einsatz)", "256 · top-8", "5.3 MB (gemessen)", "ja", "in Produktion"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "ja", "bereit — erster Kandidat"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "ja", "bereit (Hook verifiziert)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "nein", "bereit (Hook verifiziert)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "ja", "bereit (deepseek2-Graph)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "ja", "bereit (deepseek-Familie)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "nein", "bereit"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "nein", "bereit"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "ja", "bereit (MoE in jeder zweiten Layer)"],
          ["MiniMax M3 428B", "128 · top-4", "TBD (GGUF)", "ja", "wartet auf Upstream-Engine"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "nein", "funktioniert — nur GPU-Worker"],
        ],
      },
      {
        t: "p",
        md: "Die Industrie konvergiert auf feinkörniges MoE — kleinere Experten, mehr davon, höhere Sparsity (DeepSeek, Qwen, Kimi, GLM, gpt-oss sind alle diesen Weg gegangen). Jeder Schritt in diese Richtung macht die Teilnahmeeinheit des Schwarms kleiner und das Korn des Knappheitsmarkts feiner. Die Modelle oben sind keine Wunschliste; jedes fließt bereits durch denselben Dispatch-Hook, den wir in Produktion betreiben — Onboarding ist ein Verifikations-Gate, kein Engineering-Projekt.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "Die 200G-Frage",
    dek: "Unsere Schwarm-Hubs können mit Teilen aus dem Regal bereits bei 200 Gb/s verbinden — eines ist in den GB10 eingebaut. Hier ist, was eine Fat Pipe einem verteilten MoE bringt, und das eine, was sie nicht kann.",
    blocks: [
      {
        t: "callout",
        md: "**Die Prämisse:** WAN-Dekodierung ist RTT-gebunden, nicht bandbreitengebunden — unsere Kommunikations-Roadmap zeigte, dass Bytes der leichte Teil sind. Was ändert sich also wirklich, wenn Hubs 200-Gb/s-Verbindungen bekommen? Fast alles an *Kapazität* und fast nichts an *Latenz*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Schon in der Box · ConnectX-7", text: "Die Hardware ist nicht futuristisch — eine steckt in unserem GB10-Worker" },
      {
        t: "p",
        md: "Der GB10 Grace Blackwell, der unsere 122B-Experten berechnet, trägt an Bord eine **NVIDIA ConnectX-7 mit zwei 200-GbE-QSFP-Ports**. Zwei dieser Maschinen verbinden sich direkt mit einem einzigen ~$100-QSFP56-DAC-Kabel — ein 200G-Zwei-Hub-Cluster ohne einen einzigen Switch. ARM ist hier ein erstklassiger Bürger: Derselbe `mlx5`-Treiberstack, der diese NICs in x86-Rechenzentren betreibt, betreibt sie auf aarch64 — genau das ist der GB10.",
      },
      {
        t: "callout",
        md: "**Das Kleingedruckte:** Der GB10 speist seine ConnectX-7 über zwei PCIe-Gen5-x4-Links im Multi-Host-Modus. Die gemessene volle Geschwindigkeit (~185–190 Gb/s) erfordert **RoCE (RDMA) und eine korrekt gemappte Topologie** — naives TCP über einen fehlgemappten Pfad landet bei ~95 Gb/s oder schlechter. Fat Pipes werden mit Konfiguration gekauft, nicht nur mit Kabeln.",
      },
      { t: "h2", kick: "Die Distanzleiter", text: "200G ist bei jeder Reichweite ein Katalogartikel" },
      {
        t: "table",
        head: ["Reichweite", "Teil", "Formfaktor"],
        rows: [
          ["Rack (0.5–3 m)", "QSFP56-DAC-Kupfer", "Kabel, ~$100"],
          ["Raum (~30 m)", "AOC (aktiv-optisch)", "Kabel"],
          ["Campus (2–10 km)", "200G-FR4/LR4-Optik", "QSFP56-Modul"],
          ["Metro (~40 km)", "200G-ER4-Optik", "QSFP56-Modul"],
          ["Region (~120 km)", "400G ZR+ kohärent, betrieben mit 200G-Leitungsrate", "QSFP-DD-Modul"],
          ["Long-Haul (Hunderte km)", "Carrier-200G-Wellenlänge / DWDM-Leitungssystem", "gemieteter Dienst"],
        ],
      },
      {
        t: "p",
        md: "Im WAN ist das *Kabel* nur standardmäßige Singlemode-Faser — geschwindigkeitsneutrales Glas, das bereits jede Stadt überspannt. Die Geschwindigkeit steckt in der steckbaren Optik an jedem Ende, und **OpenZR+ machte 200G-über-120 km zu einem Modul, das man in einen Switch steckt**, nicht zu einem Telekom-Projekt. Darüber hinaus mietet man eine Wellenlänge.",
      },
      { t: "h2", kick: "Was sie bringt", text: "Jeder Bandbreitenterm im Schwarm verschwindet" },
      {
        t: "ul",
        items: [
          "Ein Dispatch-Payload (~110 KB/Token/Layer heute, ~10 KB nach der Leitungs-Roadmap) serialisiert sich in **Mikrosekunden** — die Payload-Größe ist überhaupt keine Design-Beschränkung mehr.",
          "Eine **Expertenscheibe geht in ~32 ms über die Leitung** (794 MB, theoretisch), und ein ganzes 122B-Modell synchronisiert sich in **~3 s** — Abdeckungsmarkt-Rebalancing und Neu-Hub-Onboarding werden nahezu sofortig.",
          "Langkontext-Prefill — die eine wirklich bandbreitenschwere Phase — bewegt sich mit Leitungsgeschwindigkeit, die Zeit bis zum ersten Token bei 100K-Token-Prompts wird also backbone-rechengebunden.",
          "**Batch-Dispatch skaliert ohne Leitungsdecke**: Expertenpool-Verkehr, aggregiert über viele Nutzerstreams, ist genau die bandbreitenschwere, latenztolerante Last, die eine Fat Pipe schluckt. Das macht eine Multi-Backbone-Föderation — mehrere Hubs, jeder hält KV für seine eigenen Nutzer, teilen sich einen Expertenpool — praktikabel.",
        ],
      },
      { t: "h2", kick: "Was sie nicht kaufen kann", text: "Licht hat es nicht eilig" },
      {
        t: "p",
        md: "Faser trägt Licht mit ~5 µs/km, und keine Menge Bandbreite ändert das. Ein 13-ms-Roundtrip ist 13 ms bei 200 Gb/s. Autoregressives Dekodieren zahlt diesen Roundtrip je geshardeter Layer, je Token — deshalb bleiben **spekulatives Dekodieren (k Tokens pro Roundtrip) und Shared-Expert-Overlap (rechnen, während der Dispatch unterwegs ist) essenziell**, selbst zwischen Hubs, die durch die fetteste Pipe am Markt verbunden sind. Bandbreite kauft Durchsatz; nur Roundtrip-Disziplin kauft Latenz.",
      },
      {
        t: "p",
        md: "So setzt sich die Architektur in zwei Ebenen ab. Eine **Hub-Ebene** — Backbones und heiße Experten, verbunden durch 200G-Klasse-Links, wo die Kapazität faktisch unbeschränkt ist — und eine **Edge-Ebene** — Smartphones und kleine Geräte am 443-Relay, die den langen Schwanz der Experten halten, den ihnen der Knappheitsmarkt zuweist. Die Fat Pipe lässt die erste Ebene wie eine einzige Maschine wirken; das Relay hält die zweite Ebene für jeden offen. Keine ersetzt die andere: Genau dieser Split *ist* das Design.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "Der Schwarm, der unter Last wächst",
    dek: "Ein riesiges Modell, das sich nur dann Hilfe borgt, wenn es sie braucht — der MoE-Schwarm skaliert sich jetzt selbst an den Verkehr: eng und schnell in ruhigen Zeiten, breit und parallel bei Andrang.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir bedient Modelle, die weit größer sind, als eine einzelne Maschine sie fassen kann — ein 122B-Parameter-Mixture-of-Experts-Modell läuft über einen Koordinator plus einen Schwarm von Workern: GPUs im LAN, GPUs über eine 200-Gb/s-Verbindung, sogar Smartphones, die sich über das Internet einwählen. Weil ein MoE-Modell jedes Token nur an eine Handvoll seiner Experten routet, liegen die meisten Gewichte zu jedem Zeitpunkt brach, und diese brachliegenden Experten können **außerhalb** des Hauptknotens leben — auf welcher Hardware auch immer sich freiwillig gemeldet hat, sie zu halten.",
      },
      {
        t: "callout",
        md: "Das Neue ist, dass sich der Schwarm jetzt **selbst an die Last skaliert**.",
      },
      { t: "h2", kick: "Wie er sich verhält", text: "Eng bei Ruhe, breit bei Andrang" },
      {
        t: "p",
        md: "Bei geringem Verkehr bedient der Koordinator alles auf seiner eigenen GPU — der schnellste Pfad pro Token, keine Netzwerk-Hops. Wenn sich Anfragen zu stauen beginnen und seine Inferenz-Slots sättigen, passieren automatisch zwei Dinge:",
      },
      {
        t: "ul",
        items: [
          "**Er reaktiviert die Worker, die er bereits hat.** Der Koordinator beobachtet seine eigene Warteschlange. Unter Sättigung streamt er weiterhin Routed-Expert-Arbeit an **bewährte** Worker hinaus — solche, die tatsächlich schon einmal bedient haben — und tauscht ein wenig Latenz pro Token gegen deutlich mehr Gesamtdurchsatz. Einem Worker, der sich nur verbunden, aber nie gerechnet hat, wird niemals Last anvertraut; ein brandneuer Worker bekommt dennoch einen fairen ersten Versuch.",
          "**Der Hub rekrutiert neue.** Der Steuerungs-Hub bemerkt dieselbe Sättigung und erhöht die „Nachfrage“ nach den Experten dieses Modells. Brachliegende Knoten — ein Smartphone in jemandes Tasche, eine freie GPU in der Nachbarschaft — pollen diesen Nachfragemarkt bereits. Sobald die Nachfrage steigt, wird ihnen eine Expertenscheibe zum Bedienen angeboten; sie laden sie herunter, wählen sich ein und treten bei. Wenn der Andrang vorbei ist, fällt die Nachfrage zurück, und die zusätzlichen Worker fallen still wieder ab.",
        ],
      },
      {
        t: "p",
        md: "Niemand plant das. Kein Knoten wird gepusht. Der Schwarm atmet mit der Last: eng und schnell in ruhigen Zeiten, breit und parallel bei Andrang — und es funktioniert sogar für Knoten hinter Heim-Routern, weil alles pull-basiert ist.",
      },
      {
        t: "p",
        md: "Das ist die Gestalt eines Netzwerks, das Modelle mit Billionen Parametern auf Hardware bedienen kann, die keine einzelne Person besitzt: Brachliegende Kapazität wird genau dann eingeladen, wenn es sich lohnt, sie einzuladen — und nur dann.",
      },
    ],
  },
};
