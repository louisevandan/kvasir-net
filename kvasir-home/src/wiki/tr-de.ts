/* Deutsch — Übersetzung der Wiki-Einträge. Struktur (Slug, Kategorie,
   Blockreihenfolge, Code) spiegelt exakt entries.ts (englische Quelle);
   technische Begriffe und Bezeichner (KVR, linkcpp, Inferenz-Engine, GGUF, MoE,
   ring runtime, tok/s usw.) bleiben unverändert. Die Compliance-Rahmung
   (Devnet, Utility-Token, non-custodial) bleibt erhalten. */
import type { WikiTranslation } from "./entries";

export const deWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Kvasir-Netzwerk",
    summary: "Ein dezentrales KI-Inferenznetzwerk (DePIN), in dem Alltagsgeräte offene Modelle bedienen und KVR verdienen.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** ist ein dezentrales KI-Inferenznetzwerk: Große offene Modelle werden mit der **linkcpp**-Engine über geteilte Hardware verteilt, sodass kein einzelner Knoten das ganze Modell hält. Jeder kann eine GPU, CPU, NPU — sogar ein Smartphone — beisteuern und **KVR** für die Layer oder Experten verdienen, die sein Gerät tatsächlich bedient. Entwickler erreichen das Netzwerk über OpenAI/Anthropic-kompatible Gateways und zahlen pro Inferenz.",
      },
      {
        t: "ul",
        items: [
          "**Quelloffen einsehbare Engine** — linkcpp ist BSL-lizenziert (kostenlos für Entwicklung und Tests, produktive Nutzung erfordert eine Lizenz); die Inferenz-Engine-Datenebene darunter bleibt unverändert und einsehbar.",
          "**Non-custodial** — Belohnungen werden in die eigene Solana-Wallet jedes Knoten-Besitzers abgerechnet; Schlüssel verlassen den Nutzer nie.",
          "**Auf echter Hardware bewiesen** — ein 122B-Modell lief verteilt über 4 AMD-MI250-GPUs, in einer heterogenen Flotte aus GPU/CPU/NPU/Mobil-Knoten, mit Ende-zu-Ende gutgeschriebenem Beitrag jedes Knotens.",
          "**Benannt nach dem nordischen Mythos** — Kvasir, das weiseste Wesen, geboren aus der gesammelten Essenz aller Götter und im Besitz von keinem.",
        ],
      },
      { t: "h2", kick: "Eine Anfrage, viele Geräte", text: "Wie eine Inferenz fließt" },
      {
        t: "code",
        caption: "Jeder Hop ist gewöhnliches HTTP/TCP; verteilt wird das Modell selbst.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Rollen **stapeln sich**: Eine Maschine kann zugleich Rechenknoten, Gateway-Host und Hub-Host sein, und ihre Belohnungen summieren sich. Die Aufgabe des Netzwerks ist es, das Aggregat wie eine einzige Maschine aussehen zu lassen — ein Endpoint vorn, Tausende unvollkommener Geräte dahinter.",
      },
      {
        t: "p",
        md: "Heute läuft das Netzwerk auf **Solana devnet**; KVR ist ein Utility- / Beitrags-Token, kein handelbares Asset und keine Investition, und nichts auf dieser Seite ist Finanzberatung.",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "Die Steuerungsebene: entdeckt Geräte, plant die Layer-Platzierung, startet Worker, orchestriert den Ring.",
    blocks: [
      {
        t: "p",
        md: "Der **Hub** ist die Steuerungsebene des Netzwerks, von linkcpp als einzelnes Docker-Image bereitgestellt (`controller.hub:app`, ein FastAPI-Dienst auf Port **19000**). Er entdeckt Geräte, prüft Runtime-Kompatibilität, plant Platzierung mit dem Planner, startet unveränderte Inferenz-Engine-Worker und exponiert die Gateways je Controller. Es ist bewusst langweilige Infrastruktur: Request/Response-HTTP, neustartsicherer Zustand, kein exotischer Transport.",
      },
      { t: "h2", kick: "Drei Türen hinein", text: "Wie Maschinen einem Hub beitreten" },
      {
        t: "ul",
        items: [
          "**Lokale Node-Slots** — fünf feste Slots pro Hub, gemappt auf die RPC-Ports **50052–50056**. Slots existieren immer; man editiert die GPU- + VRAM/RAM/CPU-Budgets eines Slots, statt beliebige Knoten anzulegen, und Ressourcen sind **nur bei ungebundenem Slot** editierbar — das schützt den Kapazitätsvertrag unter einem laufenden Controller.",
          "**Remote-Units** — einen anderen laufenden linkcpp-Hub registrieren und dessen sichtbare Knoten importieren. Der Datenebenen-Endpoint leitet sich immer aus der registrierten *Unit*-URL plus dem von der Unit exponierten Worker-Port ab — nie aus einem Node-Host, den das entfernte System bewirbt.",
          "**Verwaltete Node-Agents** — reine Worker-Dienste (`nodeagent.py`), die über schlichtes Request/Response-HTTP (`/control/join|status|download|load|unload`) beitreten und via `POST /api/node-reports` berichten. Bewusst **kein** persistenter Stream, damit sie einfache LAN/VPN-Routings überleben.",
        ],
      },
      { t: "h2", kick: "Nichts lädt ungeprüft", text: "Kompatibilitäts-Gating" },
      {
        t: "p",
        md: "Jede Unit, jeder Knoten und Agent meldet eine Protokoll-/Runtime-Pack-Identität plus Backend-Details. Abweichungen bei Unit, Runtime-Pack, Inferenz-Engine-Revision und RPC-ABI werden **vor bind, plan, load oder infer hart blockiert**; Backend-Unterschiede (CUDA/Metal/Vulkan/CPU) werden als Knoten-Fähigkeiten geführt, nicht als Ablehnungen. Adaptives Laden wird ebenfalls blockiert, wenn ein Knoten das Ressourcen-Monitoring nicht liefern kann, das ein sicherer Plan braucht.",
      },
      {
        t: "code",
        caption: "Was einen Neustart überlebt — und was nicht.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Weil der Hub die kritischste Rolle ist, verdienen Hub-Hosts die **höchste stündliche Uptime-Belohnung**. Der Betrieb eines öffentlichen Hubs erfordert das Staking von **100.000 KVR**.",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Der öffentliche Eingang: OpenAI/Anthropic-kompatible APIs und KVR-Abrechnung pro Inferenz.",
    blocks: [
      {
        t: "p",
        md: "Das **Gateway** ist der Ort, an dem Entwickler auf das Netzwerk treffen. Jeder Controller exponiert OpenAI-kompatible Endpoints (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) und Anthropic-kompatible (`/anthropic/v1/messages`, `/anthropic/v1/models`), alle vom selben geladenen Modell getragen — ein bestehender Client funktioniert, indem nur Base-URL und Key getauscht werden.",
      },
      {
        t: "code",
        caption: "Ein gewöhnlicher OpenAI-Aufruf gegen das Kvasir-Gateway.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Abrechnung", text: "Pay-per-Inference in KVR" },
      {
        t: "p",
        md: "Die Nutzung wird in KVR über einen dreistufigen Fluss abgerechnet — **quote → payment → inference** — eine Anfrage wird also vor der Ausführung bepreist, und die Knoten, die sie bedient haben, werden danach gutgeschrieben. Das Gateway aggregiert außerdem einen **Live-Modellkatalog** aus jedem erreichbaren Hub, sodass `/v1/models` widerspiegelt, was das Netzwerk gerade wirklich bedienen kann.",
      },
      {
        t: "ul",
        items: [
          "Gateway-Hosts verdienen eine **stündliche Uptime-Belohnung** dafür, den Eingang online zu halten, plus einen **×1.5-Bonus** auf jede Inferenz, die sie mit bedienen.",
          "Der Betrieb eines öffentlichen Gateways erfordert das Staking von **100.000 KVR** (wie beim Hub).",
          "Öffentliche Deployments schützen den Operator-Zugang mit **SIWS + 2FA**; nackte Hubs sind nur für vertrauenswürdige Hosts / LAN / VPN gedacht.",
        ],
      },
    ],
  },
  node: {
    title: "Node",
    summary: "Jedes Gerät, das einen Anteil eines Modells bedient — GPU, CPU, NPU oder Smartphone — und für seine Arbeit KVR verdient.",
    blocks: [
      {
        t: "p",
        md: "Ein **Node** ist jedes Gerät, das einen Teil eines Modells bedient: eine GPU-Kiste, eine CPU-Maschine, ein NPU-Gerät oder ein Smartphone. Ein Node hält nur seinen Anteil — ein Layer-Fenster im Ring oder eine Experten-Scheibe im Schwarm — und verdient KVR, gewichtet nach genau der geleisteten Arbeit. Die Live-Flotte mischt AMD MI250, NVIDIA GB10 und RTX Pro 6000, ein MacBook, x86-Windows-CPU-Maschinen und mobile Nodes in einem Netzwerk.",
      },
      { t: "h2", kick: "Vom Download zur Auszahlung", text: "Der Lebenszyklus eines Nodes" },
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
          "**Rechenknoten** verdienen pro Beitragseinheit, gewichtet nach Layer-Anteil und skaliert nach Leistungsstufe — kein Staking nötig.",
          "Nodes registrieren sich unter der Wallet ihres Besitzers; Belohnungen werden non-custodial dorthin abgerechnet. Vier verschiedene Besitzer-Wallets, die jeweils ihren Layer-Anteil verdienen, wurden Ende-zu-Ende verifiziert.",
          "Fähigkeitsdaten (Backend, Akkumulationspräzision, Ressourcenbudgets) entscheiden, was der Planner auf einem Node platzieren darf — und im Schwarm, welche Ränge er bedienen darf.",
          "Ein Node, der kein Ressourcen-Monitoring liefern kann, wird vom adaptiven Laden ausgeschlossen, statt blind Vertrauen zu genießen.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "443-Relay",
    summary: "Die Datenebene für Geräte hinter NAT: Beide Enden wählen sich ausgehend über eine WebSocket-Brücke auf Port 443 ein.",
    blocks: [
      {
        t: "p",
        md: "Smartphones hinter Carrier-NAT können keine eingehenden Verbindungen annehmen, und Edges wie Cloudflare lassen nur die Ports 80/443 durch. Das **443-Relay** löst beides: Eine WebSocket-Brücke pro Edge mit einem **1-Byte-Rollen-Preamble** lässt beide Seiten **ausgehend** wählen, sodass ein Smartphone an der Datenebene teilnimmt und dabei **null eingehende Ports** öffnet.",
      },
      {
        t: "code",
        caption: "Zwei ausgehende Verbindungen treffen sich in der Mitte; das Preamble sagt, wer wer ist.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "In der Produktion gehärtet", text: "Drei echte Bugs, drei Fixes" },
      {
        t: "ul",
        items: [
          "**Build-Fingerprint-Abgleich** — beide Enden müssen beweisen, dass sie dasselbe Runtime-Pack ausführen, bevor auch nur ein Tensor-Byte fließt.",
          "**Node-Token-Download-Auth** — Teil-Shard-Downloads authentifizieren sich mit demselben Wallet-abgeleiteten Node-Token, das die App bereits besitzt.",
          "**Der `Int.ushr`-Frame-Stillstand** — Kotlins `ushr` nutzt nur die unteren 5 Bits des Shifts, sodass `len ushr 56` zu `len ushr 24` wurde und jeden Frame ≥ 64 KiB still beschädigte (ein 593-KB-`result_output` war das erste Opfer). Behoben durch Umstellung der Längenkodierung auf `Long`-Shifts — tragend für den gebündelten Experten-Dispatch, der 64 KiB routinemäßig überschreitet.",
        ],
      },
      {
        t: "p",
        md: "Das Relay trägt, was die Topologie braucht — Ring-Layer-Grenzen oder Experten-Dispatch-Ströme — und derselbe für den Ring verifizierte Mechanismus ist der, den Produktions-Smartphone-Worker im Schwarm verwenden.",
      },
      {
        t: "p",
        md: "Sowohl die `/api/expert-relay`- als auch die `/api/ring-relay`-Upgrades sind **roh gespleißt**: Das Gateway leitet WebSocket-Frames Byte für Byte weiter, ohne sie zu parsen, sodass das Relay eine schlanke, modell-agnostische Leitung bleibt. Es **misst dennoch die Bytes, die es pro Sitzung überbrückt**, und diese gemessene Arbeit fließt in das Beitragsbuch des Hubs und wird in **KVR** in die eigene Wallet des Workers abgerechnet — Relaying für ein NAT-Smartphone verdient genauso wie ein direkt verbundener Node.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "Die quelloffen einsehbare (BSL) Steuerungsebene, die Alltagshardware in eine verteilte Inferenz-Engine verwandelt.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** ist die Engine hinter Kvasir: eine Steuerungsebene um die RPC-Datenebene von Inferenz-Engine, die große KI-Modelle über mehrere GPUs und Maschinen mit *unveränderten* `ggml-rpc-server`- / `llama-server`-Binaries ausführt. Alles, was sie hinzufügt, ist Orchestrierung — GPU-Discovery, Node-Slots, Layer-Platzierungsplanung, Worker-Start und die OpenAI/Anthropic-Gateways.",
      },
      { t: "h2", kick: "Architektur", text: "Ein Hub, unveränderte Worker" },
      {
        t: "code",
        caption: "Der Anfragepfad durch ein linkcpp-Deployment.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Quelle verfügbar unter der BSL** — für Entwicklung und Tests kostenlos lesbar, ausführbar und erweiterbar; produktive Nutzung erfordert eine Lizenz.",
          "Die Inferenz-Engine-Datenebene bleibt **ungeforkt** (bis auf einen gepinnten mobilen GPU-over-RPC-Patch), sodass Upstream-Performancearbeit weiter einfließt.",
          "Ausgeliefert als **ein einziges Docker-Image**: der FastAPI-Hub plus die zwei Inferenz-Engine-Binaries eingebacken; native Worker-Nodes bauen außerhalb von Docker für CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "Der Planner", text: "GGUF-Metadaten rein, Platzierung raus" },
      {
        t: "p",
        md: "Der Planner liest GGUF-Metadaten und erzeugt zusammenhängende Layer-Fenster je Node, das passende `--tensor-split` und KV-Cache- / Layer- / Experten-VRAM-Schätzungen je Node — plus optionales Offloading der MoE-Experten-FFNs in den Node-RAM, ausgegeben als Inferenz-Engine-`-ot`-Regeln (z. B. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) und via `--override-tensor` zum Start getragen. Ein Plan, der nicht passt, wird **vor** jedem Laden als **infeasible** gemeldet, statt zur Laufzeit als OOM entdeckt zu werden.",
      },
      {
        t: "p",
        md: "Runtime-Kompatibilität ist ein Konzept erster Klasse: Protokoll, Runtime-Pack, Inferenz-Engine-Revision und RPC-ABI werden verifiziert, und Abweichungen werden vor jedem bind, plan, load oder jeder Inferenz hart blockiert.",
      },
    ],
  },
  "ring-runtime": {
    title: "Ring-Runtime",
    summary: "Pipeline-Inferenz ohne Master: Jedes Gerät führt sein Layer-Fenster aus und reicht nur Grenzen an seinen Nachbarn weiter.",
    blocks: [
      {
        t: "p",
        md: "Die **Ring-Runtime** ist Kvasirs Serving-Topologie für niedrige Latenz. Jedes Gerät lädt nur sein zusammenhängendes **Layer-Fenster** und öffnet genau zwei Verbindungen — Vorgänger und Nachfolger. Hidden-State-Grenzen zirkulieren um den Ring; der letzte Rang sampelt den Token und gibt ihn zurück. **Kein zentraler Master, und kein Node hält das ganze Modell.**",
      },
      { t: "h2", kick: "Warum kein Stern", text: "Das RPC-Master-Problem" },
      {
        t: "p",
        md: "In der klassischen RPC-Topologie öffnet ein Master das **gesamte GGUF** und wählt jeden Worker an. Das bricht in einem offenen Netzwerk dreifach: Der Master muss den ganzen Checkpoint halten und ausliefern; jeder Worker muss anwählbar sein — Smartphones hinter Carrier-NAT sind es nicht; und der Master ist ein einzelner Eigentümer in einem Netzwerk, das keinen haben sollte. Der Ring beseitigt alle drei: Jede Stage besitzt ihr Fenster, Verbindungen laufen von Nachbar zu Nachbar, und das Relay macht NAT-Geräte erreichbar.",
      },
      {
        t: "code",
        caption: "Ein Dekodierschritt um einen 4-Stage-Ring.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "Die Platzierung stammt aus dem **rank manifest** des Planners — z. B. die 49 Layer von Qwen3.5-122B verteilt auf GPU, CPU, NPU und Smartphone.",
          "Grenzen sind klein (ein Hidden-State-Vektor pro Token), also sind Hops selbst über schwache Verbindungen billig.",
          "Mobile GPUs führen Ring-Stages **direkt** aus (Adreno via OpenCL) — der RPC-Weg zur Smartphone-GPU war unmöglich, weil Adrenos Buffer-Layout die RPC-Serialisierung nicht übersteht; eine lokale Stage besitzt aber ihr Backend, sodass nur Grenzen über die Leitung gehen.",
        ],
      },
      {
        t: "p",
        md: "Der Ring ist der **Latenz**-Pfad; sein Boden ist die Layer-Granularität (~1.4 GB beim 122B). Der Experten-Schwarm beseitigt diesen Boden und dockt an dasselbe Serving-Gewebe an.",
      },
    ],
  },
  "layer-window": {
    title: "Layer-Fenster & Teil-Shards",
    summary: "Die zusammenhängende Modellscheibe eines Nodes — als mini-GGUF ladbar statt als kompletter Checkpoint.",
    blocks: [
      {
        t: "p",
        md: "Ein **Layer-Fenster** ist der zusammenhängende Bereich von Transformer-Layern, den ein Ring-Node bedient. Ein Node braucht dafür nicht den ganzen Checkpoint — ein **Stage-mini-GGUF** trägt nur die Tensoren des Fensters: beim 122B **254 MB mit 26 Tensoren** (von insgesamt 338) gegenüber dem 77.6-GB-Vollmodell, oder ~1.5 GB für ein Ein-Layer-Fenster auf einem Smartphone.",
      },
      {
        t: "code",
        caption: "Eine Zeile des rank manifest: wer bedient was, in welchem Budget.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Beansprucht, nicht zugewiesen", text: "Selbsteinschreibung" },
      {
        t: "ul",
        items: [
          "Ein Node fragt die **Abdeckungs-/Nachfragekarte** ab, um zu sehen, welche Fenster unterversorgt sind und was jedes zahlt.",
          "Er wählt das **höchstbelohnte** unabgedeckte Fenster, das in sein Budget passt, lädt genau das herunter und tritt bei.",
          "Die Abdeckung heilt sich selbst: Wenn ein Node abwandert, wird sein Fenster wieder knapp — und damit wieder lukrativ.",
          "Ende-zu-Ende mit einem NAT-Smartphone verifiziert: Poll → Selbsteinschreibung → Teil-Download → Adreno-GPU-Load → Ring-Inferenz abgeschlossen, Beitrag gutgeschrieben.",
        ],
      },
      {
        t: "p",
        md: "Der Experten-Schwarm nutzt genau diesen Markt in der feineren **(Layer, Experten-Bereich)**-Körnung wieder — dieselbe Karte, dieselbe Selbsteinschreibung, dieselben Belohnungen, kleinere Einheiten.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "Ein Modell, dessen FFNs Hunderte unabhängiger Experten sind, von denen pro Token nur wenige feuern.",
    blocks: [
      {
        t: "p",
        md: "Ein **Mixture-of-Experts**-Modell ersetzt die einzelne FFN jeder Schicht durch eine Bank unabhängiger Experten-FFNs plus einen **Router**, der pro Token einige auswählt. Qwen3.5-122B-A10B ist das Flaggschiff-Beispiel des Netzwerks:",
      },
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
      {
        t: "p",
        md: "Jede Schicht teilt sich in einen **dichten Pfad** — Attention + KV, Norms, den Router (`ffn_gate_inp`), einen geteilten Experten — und eine **Expertenbank**, gespeichert als drei gestapelte Tensoren (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Der dichte Pfad ist die Minderheit der Bytes; die Expertenbank macht 86 % des Modells aus.",
      },
      {
        t: "ul",
        items: [
          "Der Expertenindex ist die **äußerste GGUF-Dimension**, jeder Experte also eine zusammenhängende, quantblock-ausgerichtete Platte — Extraktion ist eine Byte-Range-Kopie, keine Dequantisierung.",
          "Pro Token feuern nur **8 von 256** Experten je Layer, der Expertenverkehr einer Layer beim Dekodieren besteht also aus einer Handvoll kleiner Matrixmultiplikationen über einen Hidden-Vektor (~6 KB Dispatch).",
          "Experten sind voneinander unabhängig — Eigentum kann über Geräte verstreut und frei umverteilt werden.",
        ],
      },
      {
        t: "p",
        md: "Darum ist MoE das natürliche Substrat des Schwarms: Die Gewichte kommen bereits in gerätegroßen, unabhängig besitzbaren Einheiten verpackt.",
      },
    ],
  },
  "expert-sharding": {
    title: "Experten-Sharding",
    summary: "Ein MoE auf Expertenkörnung teilen, sodass ein Smartphone 42–340 MB Experten trägt statt einer 1.4-GB-Layer.",
    blocks: [
      {
        t: "p",
        md: "**Experten-Sharding** senkt die Trageeinheit des Schwarms von einer Layer (~1.4 GB beim 122B) auf einen Experten (**5.3 MB**). Ein schwaches Gerät lädt eine Scheibe von 8–64 Experten (**42–340 MB**), lädt sie als Pure-Function-Worker — ohne Attention, ohne KV, ohne Sampler — und rechnet seine Experten, wann immer der Router des Backbones sie auswählt.",
      },
      { t: "h2", kick: "Zwei Rollen", text: "Backbone × Worker" },
      {
        t: "code",
        caption: "Die Schnittstelle in einer MoE-Layer (der Router läuft einmal, auf dem Backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "Das **Backbone** behält den dichten Pfad (Attention, Norms, Router, geteilter Experte, Combine) und hält alle Experten als RAM-ausgelagerte Fallback-Replik für Churn-Toleranz.",
          "**Worker** (`linkcpp-expert-worker --serve`) beantworten `(n_used, n_tokens, cur, sel) → experts` über einen langlebigen TCP-Stream — denselben Stream, den das 443-Relay für Smartphones tunnelt.",
          "Die Abdeckung heilt sich über den **Experten-Abdeckungsmarkt** selbst: `POST /api/expert-coverage` heartbeatet Bestände, `GET /api/expert-demand` aggregiert Knappheit, `POST /api/expert-volunteer` weist den knappsten Bereich zu, zugeschnitten aufs Budget des Nodes.",
        ],
      },
      { t: "h2", kick: "Gemessen, nicht versprochen", text: "Auf echter Hardware verifiziert" },
      {
        t: "ul",
        items: [
          "Sharded-Rechnung == monolithisch bis **max|Δ| = 3.6e-12** (eine exakte Umgruppierung, keine Näherung).",
          "Prozessübergreifender Dispatch bei einer echten 122B-Dekodierung: **argmax MATCH**, Logit-Cosine 0.99869 — byteidentisch zu in-process.",
          "Ein Galaxy S25 lud autonom seine 1.58-GB-Scheibe und rechnete bei jedem Token die Layer-0-Experten: **8/8 Token identisch** zum lokalen Lauf.",
          "Eine entfernte GPU über das öffentliche Internet — ein WAN-Roundtrip pro Token — blieb **greedy 8/8 identisch** (cosine 0.99773): **1.2% Durchsatz-Overhead** auf einer direkten Verbindung, ~28% über einen CDN-Edge. Die ehrlichen Kosten des seriellen Per-Token-Dispatch, und warum der Hebel des Gewebes das Bündeln ist, nicht niedrigere Latenz.",
          "Gebündelter Dispatch erreicht **53k tok/s pro Worker** bei Batch 512 (ROCm) — die Durchsatz-Gewebe-Eigenschaft, die den Schwarm praktikabel macht.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "Router-Autorität",
    summary: "Die Kohärenz-Invariante des Schwarms: Routing wird einmal entschieden, auf dem Backbone — Worker erhalten nur Experten-IDs.",
    blocks: [
      {
        t: "callout",
        md: "**Die Invariante:** Die einzige diskrete Entscheidung im Netzwerk ist das MoE-Routing (Top-8 von 256). Kvasir führt den Router **genau einmal, auf dem Backbone** aus und schickt den Workern nur die IDs der ausgewählten Experten. Ein heterogener Schwarm mag sich in der *Größe* der Ausgabe jedes Experten leicht unterscheiden — er unterscheidet sich nie darin, *welche Experten laufen*.",
      },
      {
        t: "p",
        md: "Ohne diese Regel würde jedes Backend den Router neu ausführen und bei Grenz-Token **andere Experten** wählen — echte, katastrophale Divergenz, denn ab diesem Token verzweigt die Rechnung wie mit einem anderen Zufalls-Seed. Mit ihr schrumpfen Hardware-Unterschiede zu einem beschränkten kontinuierlichen Fehler, den das wahrscheinlichkeitsgewichtete Combine absorbiert.",
      },
      { t: "h2", kick: "Was sie verhindert", text: "Divergenzmodi, die ein einziger Entscheidungspunkt schließt" },
      {
        t: "table",
        head: ["Divergenzmodus", "Ohne Autorität", "Mit Autorität"],
        rows: [
          ["Routing-Abweichung", "Backends wählen an Grenzen unterschiedliche Top-8", "IDs einmal entschieden, an die Besitzer geschickt"],
          ["Trajektorien-Gabelung", "Ein gekippter Token gabelt die ganze Sequenz", "Dekodieren/Sampling an einen Node gepinnt"],
          ["Verifikation", "Bitvergleich zwischen Backends (unmöglich)", "Toleranzprüfungen auf wohldefinierten Residuen"],
        ],
      },
      {
        t: "p",
        md: "Die Kosten sind vernachlässigbar: Das Backbone berechnete ohnehin schon `ffn_norm` und die Router-Logits; über die Leitung gehen nur die Hidden-Zeilen plus die ausgewählten IDs — etwa **6 KB pro Dekodierschritt**.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "Numerische Äquivalenz",
    summary: "Verschiedene Backends stimmen nie bitgenau überein; der Schwarm behandelt gemessene Toleranz als Vertrag erster Klasse.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno und CPUs berechnen dieselbe Operation mit unterschiedlichen Reduktionsreihenfolgen, FMA-Fusionen, Akkumulatoren und Näherungen transzendenter Funktionen — die Ergebnisse weichen pro Operation um ~1e-6…1e-3 ab, **per Design, nie bitidentisch**. Ein Schwarm aus beliebig auftauchender Hardware kann keine Bitgenauigkeit verlangen, also misst Kvasir stattdessen Äquivalenz.",
      },
      {
        t: "table",
        head: ["Backend-Paar (echtes 122B, Layer-0-Experten)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["Smartphone-ARM-CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "Die volle Backend-Matrix ist geschlossen: die zwei GPU-Backends (CUDA, ROCm) teilen sich Kernel-Quellen und landen **praktisch bitidentisch** (cosine 1.0000000000), während GPU↔CPU-Paare bei ~0.9997 äquivalent bleiben. Ein CUDA-Worker und ein ROCm-Worker sind austauschbar; ein GPU-Worker und ein CPU-Worker sind numerisch äquivalent.",
      },
      { t: "h2", kick: "Warum sie abweichen", text: "Gleitkomma-Addition ist nicht assoziativ" },
      {
        t: "ul",
        items: [
          "**Matmul-Reduktionsreihenfolge** — Tensor-Cores, MFMA-Kacheln, OpenCL-Workgroups und SIMD-Lanes akkumulieren in unterschiedlichen Reihenfolgen.",
          "**Akkumulationspräzision** — F16/BF16-Speicherung mit F32- oder F16-Akkumulatoren: der größte Hebel der Divergenz.",
          "**Transzendente Näherungen** — exp (Softmax), silu (SwiGLU) und rsqrt (Norms) nutzen je Backend andere Polynom-/Tabellenvarianten.",
        ],
      },
      { t: "h2", kick: "Der Vertrag", text: "Toleranzen, Fähigkeiten, einzige Autorität" },
      {
        t: "ul",
        items: [
          "Verifikation ist eine **Toleranz** — „Top-1-Übereinstimmung ≥ 99.x %, KL ≤ ε\" — nie Bitgleichheit.",
          "Backends und Akkumulationspräzision werden als Node-**Fähigkeiten** ausgewiesen; F32-akkumulierende Nodes werden für ausgabesensible Ränge bevorzugt.",
          "Nodes außerhalb der Toleranz werden für sensible Ränge als ungeeignet markiert, nicht pauschal abgelehnt.",
          "Diskrete Entscheidungen (Routing, Sampling) werden an einzelne Autoritäten gepinnt, damit kontinuierlicher Fehler nie zu diskreter Divergenz werden kann.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "Das quantisierte Modell-Dateiformat von Inferenz-Engine — und das Layout, das Teil- und Experten-Slicing billig macht.",
    blocks: [
      {
        t: "p",
        md: "**GGUF** ist das Ein-Datei-Modellformat des Inferenz-Engine-Ökosystems: Metadaten (Architektur, Layer-Anzahl, Dimensionen, Quantisierung) plus die Tensoren als rohe quantisierte Bytes (z. B. Q4_K_M). Der Planner von linkcpp liest die Metadaten für Platzierungen und Größenschätzungen; die Serving-Seite schneidet die Tensor-Bytes, um Downloads zu erzeugen.",
      },
      {
        t: "ul",
        items: [
          "**Stage-mini-GGUFs** tragen die Tensoren eines Layer-Fensters — 254 MB statt 77.6 GB für eine 122B-Ring-Stage.",
          "**Experten-Shard-GGUFs** tragen eine (Layer, Experten-Bereich)-Scheibe, ausgeliefert von `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` mit Node-Token-Auth.",
          "Beide sind **gültige GGUF-Dateien**: Der Reader auf dem Node lädt sie mit Standard-Tooling, kein Eigenformat.",
        ],
      },
      {
        t: "code",
        caption: "Warum Experten-Slicing eine Byte-Kopie ist: Der Expertenindex ist die äußerste Dimension.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "Der Router (`ffn_gate_inp`) und der geteilte Experte sind aus Experten-Shards **ausgeschlossen** — sie gehören dem Backbone, genau wie es die Router-Autorität verlangt.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "Der Utility-Token des Netzwerks: Entwickler geben ihn für Inferenz aus, Mitwirkende verdienen ihn für Rechenleistung.",
    blocks: [
      {
        t: "p",
        md: "**KVR** (On-Chain-Name „Kvasir\", 6 Dezimalstellen, Solana) ist ein Token, der in beide Richtungen fließt: Entwickler **geben** KVR aus, um Inferenz über das Gateway auszuführen, Mitwirkende **verdienen** KVR für die Rechenleistung ihrer Nodes. Belohnungen werden aus echter Arbeit berechnet — tatsächlich bediente Layer und Experten — nicht aus Teilnahme.",
      },
      {
        t: "ul",
        items: [
          "**Ausgabenseite** — Pay-per-Inference über das Gateway: quote → payment → inference.",
          "**Verdienstseite** — Beitragseinheiten × Layer-Anteil × Leistungsstufe für Rechenleistung; stündliche Uptime für Hub-/Gateway-Rollen.",
          "**Abrechnung** — auf Solana, in die eigene Wallet jedes Node-Besitzers; der Abrechnungsdienst schreibt jedem Node gut, der eine Anfrage berührt hat.",
          "**Nach dem Mythos benannt** — der Met der Poesie, gebraut aus Kvasir, der jedem Trinkenden Weisheit schenkte: offener Zugang, und Belohnungen für alle, die einschenken.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, Utility-Token.** KVR läuft derzeit auf Solana devnet und ist ein Utility- / Beitrags-Token — kein handelbares Asset, kein Preis, keine Investition. Nichts hier ist Finanzberatung oder ein Renditeversprechen.",
      },
    ],
  },
  "contribution-units": {
    title: "Beitragseinheiten",
    summary: "Die Belohnungsformel: Einheiten folgen den bedienten Tokens, gewichtet nach Layer-Anteil, skaliert nach Leistungsstufe.",
    blocks: [
      {
        t: "code",
        caption: "Wie Rechenbelohnungen berechnet werden.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "Eine **Einheit** ≈ 1k bediente Tokens, gewichtet nach dem **Layer-Anteil** des Nodes an jeder Inferenz — ein Node mit 12 von 49 Layern verdient 12/49 der Einheiten jeder Inferenz. Der Stufenmultiplikator belohnt dann gemessene Geschwindigkeit, und Infrastruktur-Rollen sammeln obendrauf stündliche Uptime.",
      },
      { t: "h2", kick: "Rechenbeispiel", text: "Eine Inferenz, vier Nodes" },
      {
        t: "table",
        head: ["Node", "Layer", "Anteil", "Stufe", "Effektive Einheiten / 1k Tokens"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Smartphone", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Belohnungen folgen **echter Arbeit**: Ein Node, der nichts bedient hat, verdient nichts — unabhängig von der Uptime (bei Rechenrollen).",
          "Rollen **stapeln sich** — eine Maschine kann Rechenknoten + Gateway + Hub sein, und ihre Ströme summieren sich.",
          "Alles wird in KVR in die eigene Besitzer-Wallet des Nodes abgerechnet; das Dashboard zeigt roh × Stufe = effektiv und ein abrufbares Guthaben.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "Leistungsstufen",
    summary: "Gemessener Durchsatz bestimmt einen Multiplikator: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["Stufe", "Gemessener Durchsatz", "Multiplikator"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "Die gemessene Dekodiergeschwindigkeit eines Nodes bestimmt seine Stufe, und die Stufe multipliziert seine verdienten Einheiten — schnellere Hardware verdient für dieselbe Arbeit proportional mehr. Die Node-Status-Ansicht der Wallet zeigt die Stufe jedes Nodes neben seinem Beitrag.",
      },
      {
        t: "ul",
        items: [
          "Stufen werden **gemessen, nicht selbst deklariert** — der Durchsatz stammt aus der tatsächlichen Serving-Leistung des Nodes und wird über die Zeit neu gemessen.",
          "Ein C-Stufen-Smartphone verdient trotzdem — ×0.7 seines Layer-Anteils — und genau das ist der Punkt: Der Boden gilt für alle Teilnehmer, nicht nur für Rechenzentren.",
          "Die Stufe multipliziert *effektive* Einheiten, sie komponiert also mit Layer-Anteil und Gateway-Bonus, statt sie zu ersetzen.",
        ],
      },
    ],
  },
  staking: {
    title: "Staking",
    summary: "KVR staken, um APR-Zinsen zu verdienen; 100.000 gestakte KVR qualifizieren eine Wallet für den Betrieb von Hub- oder Gateway-Nodes.",
    blocks: [
      {
        t: "p",
        md: "Staking sperrt KVR in der eigenen Wallet, um **APR-Zinsen** zu verdienen und sich für Node-Belohnungen zu qualifizieren. Der Betrieb eines **Hub**- oder **Gateway**-Nodes erfordert einen Stake von **100.000 KVR**; normale Rechenknoten treten ohne Stake bei und verdienen für die Layer, die sie ausführen.",
      },
      {
        t: "ul",
        items: [
          "Gestakt wird im Staking-Panel des Wallet-Dashboards: Betrag eingeben, **Stake**, und die Position beginnt, APR plus Node-Belohnungs-Berechtigung anzusammeln.",
          "Die 100k-Anforderung ist ein **Haftungs-Filter** für die zwei Rollen, von denen der Verkehr anderer abhängt — Eingänge und die Steuerungsebene.",
          "Staking ist non-custodial wie alles andere: Die Position lebt in der eigenen Wallet, und Kapital, aufgelaufene Zinsen und Node-Belohnungen sind alle im Staking-Panel sichtbar.",
          "Devnet-KVR zum Staken kommt per Verteilung oder Swap (SOL/ETH ↔ KVR-Swap: bald verfügbar); Devnet-SOL für Gebühren kommt aus dem öffentlichen Faucet.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "Non-custodial Wallet",
    summary: "Schlüssel leben nur auf dem Gerät des Nutzers — Web, Desktop, iOS und Android — und Belohnungen werden direkt dorthin abgerechnet.",
    blocks: [
      {
        t: "p",
        md: "Die Kvasir Wallet ist **non-custodial by design**: Die 12-Wort-Wiederherstellungsphrase und die Schlüssel liegen nur auf dem Gerät des Nutzers, nie bei einem Betreiber. Belohnungen werden auf Solana direkt in die Besitzer-Wallet jedes Nodes abgerechnet — verifiziert über vier verschiedene Besitzer-Wallets, jede mit ihrem eigenen Layer-Anteil.",
      },
      {
        t: "ul",
        items: [
          "**Plattformen** — Web, Desktop (macOS/Windows/Linux, Wallet + Node in einer Electron-App), iOS und Android.",
          "**Eine Phrase, jedes Gerät** — dieselbe 12-Wort-Phrase stellt dasselbe Konto auf Desktop, Smartphone und Web wieder her; eine lokale Passphrase entsperrt jede Installation.",
          "**Wallet = Node-Identität** — die Wallet signiert die Netzwerkidentität des Nodes; „wer für dieses Gerät verdient\" ist kryptographisch, keine Kontozeile auf irgendjemandes Server.",
          "**Phrase verloren, Konto verloren** — Non-Custody schneidet in beide Richtungen; es gibt keinen Betreiber, der es zurücksetzen kann.",
        ],
      },
      {
        t: "p",
        md: "Die Mobil-Apps sind zugleich Node-Apps: Dieselbe Wallet, die deine KVR hält, konfiguriert Compute-Backend und Node-Modus des Smartphones, staket und ruft Belohnungen ab.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "Operator-Login ist eine Wallet-Signatur (Sign-In With Solana) über eine Server-Nonce, plus optionale TOTP-2FA.",
    blocks: [
      {
        t: "p",
        md: "Bei öffentlichen Deployments wird der Operator-Zugang zu Hub und Gateway per **Sign-In With Solana** authentifiziert: Die Wallet des Operators signiert eine vom Server ausgegebene Nonce und beweist Eigentum ohne Passwort oder hinterlegte Zugangsdaten. Obendrauf schützen **TOTP-2FA** und Einmal-Backup-Codes die Sitzung — auf Hub und Gateway gleichermaßen.",
      },
      {
        t: "ul",
        items: [
          "**Nirgendwo Passwörter** — der Wallet-Schlüssel ist die Identität, die Nonce verhindert Replays; serverseitig gibt es nichts zu phishen oder zu leaken.",
          "**TOTP-Registrierung je Wallet** wird im Hub-Zustand persistiert, 2FA überlebt Neustarts also zusammen mit Slots und Bindungen.",
          "**Backup-Codes sind Einmal-Codes** — jeder wird beim Login verbraucht, zur Wiederherstellung, wenn das Authenticator-Gerät nicht verfügbar ist.",
          "**Ehrlich deklarierter Geltungsbereich** — nackter Hub und RPC-Ports sind für vertrauenswürdige Hosts / LAN / VPN gedacht; SIWS + 2FA ist die Schicht, die *öffentliche* Domains sicher exponierbar macht.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "Die KVR-Ökonomie",
    summary: "Wie Verbraucherkosten und Knotenbelohnung eine einzige, sich selbst verstärkende Schleife bilden — der tugendhafte Kreislauf, der das Netzwerk billiger werden lässt, während es wächst.",
    blocks: [
      {
        t: "p",
        md: "Kvasir ist ein **zweiseitiger Markt**, abgerechnet in einem einzigen Token. Verbraucher zahlen pro Inferenz **KVR** in die Treasury; Knoten verdienen **KVR** für genau die Arbeit, die sie bedient haben, ausgezahlt zurück in ihre eigenen Wallets. Das Designziel ist, dass diese zwei Seiten nicht konkurrieren — sie **verstärken sich**: mehr Angebot macht das Netzwerk billiger und besser, was mehr Nachfrage anzieht, deren Zahlungen reichere Belohnungen finanzieren, was mehr Angebot anzieht.",
      },
      { t: "h2", kick: "Das Schwungrad", text: "Nutzung und Angebot wachsen gemeinsam" },
      {
        t: "p",
        md: "Weil Inferenz in KVR bezahlt werden **muss**, ist jede Nutzungseinheit echte Nachfrage nach dem Token — Utility, keine Spekulation. Diese Nachfrage stützt den Wert der KVR, die Knoten verdienen, was das Beitragen attraktiv hält, was die Kapazität wachsen lässt, was Preis und Latenz senkt, was mehr Nutzung anzieht. Kvasirs schärfster Vorteil zieht die Schleife noch enger: ein Teilnehmer kann **zugleich Verbraucher und Anbieter** sein (ein *Prosument*), sodass die zwei Seiten oft in denselben Menschen wachsen.",
      },
      {
        t: "callout",
        md: "**\"Kostenlos, wenn du beiträgst\" ist netto-frei, nicht null Kosten.** Du zahlst für das, was du inferierst, und verdienst für das, was du bedienst; trage ungefähr so viel bei, wie du verbrauchst, und beide heben sich auf. Das Netzwerk ist nicht kostenlos — *deine* Rechnung ist es.",
      },
      { t: "h2", kick: "Ihn tugendhaft halten", text: "Drei Invarianten und die Spiralen, die sie verhindern" },
      {
        t: "table",
        head: ["Invariante", "Verhinderte Spirale"],
        rows: [
          ["Belohnungen aus echtem Umsatz finanziert (Emission nur zum Bootstrapping, dann Taper)", "Inflation erodiert KVR, bis beide Seiten kollabieren"],
          ["KVR ist das verpflichtende Medium für Inferenz", "Token-Wert entkoppelt sich von der Nutzung hin zu reiner Spekulation"],
          ["Preis schwebt zwischen einem Kostenboden und einer untermarktlichen Decke", "Zu niedrig hungert Knoten aus; zu hoch verliert Nutzer an zentralisierte APIs"],
        ],
      },
      {
        t: "p",
        md: "Kvasir belohnt bereits **echte Arbeit** (KVR pro bediente Tokens × Layer-Anteil, nicht bloße Präsenz) und rechnet non-custodial ab, was der schwierige Teil daran ist, umsatzfinanzierte Belohnungen ehrlich zu machen. Der Rest — ein auslastungsgetriebener Preis und ein Taper von Emission → Umsatz — ist die ökonomische Roadmap, die \"mehr Knoten → billiger\" von einer Intuition in eine vom Protokoll erzwungene Regel verwandelt. Der Eintrag **Inferenz-Bepreisung** behandelt die Preisseite; **Beitragseinheiten** behandelt, wie Arbeit zu Belohnung wird.",
      },
    ],
  },
  "inference-pricing": {
    title: "Inferenz-Bepreisung",
    summary: "Was eine Inferenz heute in KVR kostet, warum ein dezentrales Netzwerk strukturell billiger ist und wie der Preis fallen soll, während das Angebot wächst.",
    blocks: [
      {
        t: "p",
        md: "Der Zugang zum Netzwerk ist **Pay-per-Inference**: Das Gateway nennt einen KVR-Preis für deine Anfrage, deine Wallet zahlt ihn on-chain, und erst dann führt der Hub das Modell aus. Die Bepreisung ist eine kleine, transparente Formel — ein Boden je Anfrage plus eine Rate je Token — vorab genannt und auf **tatsächlicher** Token-Nutzung nach der Generierung abgerechnet.",
      },
      {
        t: "code",
        caption: "Die Abrechnungsformel — vorab genannt, nach echter Nutzung berechnet.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Warum es billiger sein kann", text: "Keine zentrale Marge zu bezahlen" },
      {
        t: "p",
        md: "Eine zentralisierte API bepreist zu Kosten **plus** einer großen Marge und Kapitalrückgewinnung. Ein dezentrales Netzwerk bepreist nahe den **Grenzkosten** seiner Mitwirkenden — Strom und Hardware-Amortisation — plus einer dünnen Protokollgebühr. Diese strukturelle Lücke besteht unabhängig von der Größe. Wachstum verbreitert sie: **Experten-Sharding** bedeutet, dass mehr Knoten je eine kleinere Scheibe halten, sodass billigere Geräte bedienen können, was die Grenzkosten der Teilnahme senkt und das Angebot vertieft.",
      },
      {
        t: "callout",
        md: "**Der Preis wird regiert, kein Freibrief.** Raten sind ein sensibler ökonomischer Parameter, geändert nur von der genesis-Wallet unter Wallet-Signatur + 2FA — nie durch eine Umgebungsvariable. Das hält die Token-Ökonomie stabil und auditierbar.",
      },
      { t: "h2", kick: "Wohin es geht", text: "Auslastungsgetriebener Preis" },
      {
        t: "p",
        md: "Die Designrichtung ist ein Preis, der **mit der Netzwerkauslastung schwebt**, zwischen einem Boden (über den Grenzkosten der Knoten gehalten, damit Bedienen lohnend bleibt) und einer Decke (unter zentralisierten Alternativen gehalten, damit er wettbewerbsfähig bleibt). Leerlaufendes Angebot drückt den Preis nach unten; Überlastung drückt ihn nach oben. Das ist der Mechanismus, der **\"mehr geteilte Knoten → niedrigerer Preis\"** endlich im Code wahr macht — der natürliche Thermostat der **KVR-Ökonomie**.",
      },
    ],
  },
  "run-expert-worker": {
    title: "Einen Experten-Worker betreiben",
    summary: "Verwandle eine übrige GPU, CPU oder ein Smartphone in einen Experten-Worker: bauen, sich für die knappste Scheibe melden, sie herunterladen, bedienen und über 443 hinauswählen, um KVR zu verdienen.",
    blocks: [
      {
        t: "p",
        md: "Ein **Experten-Worker** ist eine reine `(hidden, ids) → out`-Funktion — ohne Attention, ohne KV-Cache, ohne Sampler — die eine Scheibe der Experten eines MoE-Modells rechnet, wann immer der Router des Backbones sie auswählt. Du wählst nicht, was du bedienst; der **Abdeckungsmarkt** reicht dir den knappsten, höchstbelohnten Bereich, zugeschnitten auf dein Budget, sodass ein 4-GB-Smartphone und eine Rechenzentrums-GPU beide einen Slot finden.",
      },
      { t: "h2", kick: "Sieben Schritte", text: "Bauen → melden → bedienen → wählen → verdienen" },
      {
        t: "code",
        caption: "Der ganze Weg — das Wähl-Skript wartet mit einer Retry-Schleife auf das Backbone.",
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
          "**Die Scheibe ist winzig.** Eine Layer-0-Scheibe mit 128 Experten ist **794 MB** gegenüber dem 72-GB-Vollmodell — die Körnung, die schwachen Geräten die Teilnahme erlaubt. Du lädst nur den Bereich herunter, den der Markt zugewiesen hat.",
          "**Ausgehend wählen, nie eingehend.** Schritt 5 öffnet einen ausgehenden WebSocket auf 443, sodass Carrier-NAT und CDN-Edges ihn durchlassen und du null eingehende Ports exponierst — denselben Weg nutzt ein Smartphone.",
          "**Der Heartbeat ist tragend.** Ohne `POST /api/expert-coverage` bedienst du nichts, wovon die Nachfragekarte weiß, und nichts, was du tust, wird gutgeschrieben.",
          "**Belohnung erfolgt pro Arbeit.** Gebrückte Arbeit sammelt sich im Beitrags-Ledger des Hubs; das Gateway schreibt KVR per Delta in deine **eigene** Wallet gut (non-custodial). Du brauchst eine Wallet-Adresse, um bezahlt zu werden.",
        ],
      },
      {
        t: "callout",
        md: "Der Worker spricht dasselbe Dispatch-Protokoll wie eine GPU im Rechenzentrum — `(n_used, n_tokens, cur, sel) → experts` über einen langlebigen Stream. Ein Teil-Shard-Worker setzt einfach `n_used = 1`. Diese Einheitlichkeit ist der Grund, warum ein Smartphone, eine CPU-Kiste und eine Blackwell-Karte austauschbare Mitglieder desselben Schwarms sind.",
      },
    ],
  },
  "hub-operations": {
    title: "Einen Hub betreiben",
    summary: "Betriebsnotizen für Hub und Gateway: hot-patchen ohne Neubau, Neustarts überstehen, den Katalog registriert halten und die Oberfläche auf 443 verriegeln.",
    blocks: [
      {
        t: "p",
        md: "Der Hub (Steuerungsebene) und das Gateway (öffentlicher Eingang) sind die zwei langlebigen Dienste, die ein Operator gesund hält. Der Hub und seine RPC-Ports sind **per Design unauthentifiziert** — nur vertrauenswürdiger Host / LAN / VPN — und aller öffentliche Verkehr konvergiert auf der einzigen 443-Oberfläche des Gateways. Dies sind die Betriebsnotizen, die diese Anordnung über Codeänderungen, Neustarts und Reboots hinweg stabil halten.",
      },
      { t: "h2", kick: "Deploy & Hot-Patch", text: "Code ändern ohne Neubau" },
      {
        t: "ul",
        items: [
          "**Schneller Weg:** Hub-/Gateway-Code mit `docker cp <file> <container>:/app/...` + `docker restart` aktualisieren — kein Image-Neubau. Aber **eine Umgebungsvariable hinzuzufügen geht so nicht** (es braucht ein Neuerstellen des Containers); bevorzuge stattdessen eine Runtime-Config-API, die in den Hub-Zustand persistiert.",
          "**Compose-Drift:** Ein lange laufender Container kann von seiner Compose-Datei abweichen (Network-Mode, Entrypoint, Env). Prüfe immer die echte Konfiguration mit `docker inspect`, bevor ein `docker compose up -d` neu erstellt — ist sie abgedriftet, löscht das Neuerstellen die Produktionseinstellungen. Nutze cp + restart.",
          "**Vor dem Patchen diffen:** Die containerinterne Datei per `docker cp` herausholen und gegen repo-HEAD diffen, bevor du sie ersetzt, damit der Hot-Patch einer früheren Sitzung nicht still verloren geht.",
        ],
      },
      { t: "h2", kick: "Einen Neustart überstehen", text: "Zustand bleibt; geladene Modelle nicht" },
      {
        t: "ul",
        items: [
          "Ein Hub-Neustart **stoppt das Serving.** Slots, Controller und Bindungen stellen sich aus `hub-state.json` wieder her, aber ein geladenes Modell ist runtime-only. Lies nach einem Neustart das `last_load` jedes Controllers aus und löse `POST /api/controllers/{cid}/serve` erneut aus — selbst ein großes Modell kommt dank Page-Cache in ~1 Minute zurück.",
          "**Gateway-Watchdog:** Jedes bediente Modell alle 30 s mit einer 1-Token-Anfrage prüfen und bei Ausfall aus `last_load` automatisch neu laden (mit Cooldown). **Prüfe *alle* Modelle, nicht `catalog[0]`** — sobald ein gesundes Modell eines anderen Hubs nach vorn sortiert, verpasst eine Nur-Erstes-Prüfung ein großes Modell, das ausfällt (ein echter Bug, inzwischen behoben).",
          "**Katalog-TTL:** `POST /api/pay/hub/register` hat ein TTL von 90 s, halte die Registrierung also mit einer ~60-s-Heartbeat-Schleife am Leben, über Reboots hinweg dauerhaft gemacht durch einen `@reboot`-Cron oder eine systemd-Unit.",
        ],
      },
      { t: "h2", kick: "Verriegeln", text: "Alles Öffentliche geht über 443" },
      {
        t: "ul",
        items: [
          "Der Hub (:19000) und die RPC-Ports setzen ein vertrauenswürdiges Netzwerk voraus; das Einzige, was zum Internet zeigen sollte, ist das Gateway auf 443 (inklusive seiner WebSocket-Relay-Durchleitung).",
          "Muss ein Hub auf einer öffentlichen IP sitzen, per Firewall auf vertrauenswürdige IPs beschränken — aber Dockers veröffentlichte Ports werden **vor der INPUT-Chain DNAT't**, eine Regel auf `dport` greift also nicht. Filtere stattdessen in der `DOCKER-USER`-Chain über den ursprünglichen Zielport von conntrack (`--ctorigdstport`) und persistiere die Regeln mit einem systemd-Oneshot, geordnet `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Abrechnung & Fußangeln", text: "Pull, nicht Push — und eine Shell-Falle" },
      {
        t: "ul",
        items: [
          "**Abrechnung ist Pull, nicht Push:** Der Hub sammelt Beiträge; das Gateway pollt `GET /api/contributions` und schreibt KVR per Delta gut. Setzt ein Hub-Neustart seine Zähler zurück, baselined das Gateway neu, sodass nichts doppelt bezahlt wird. Die Experten-Arbeitsrate wird durch `LINKCPP_EXPERT_UNITS_PER_MB` gesetzt.",
          "**Die `pkill`-Fußangel:** `ssh host 'pkill -f X; ...'` matcht seine *eigene* Kommandozeile und killt sich selbst. Nutze eine Zeichenklasse im Muster (`X[x]`) und packe Spawn und pkill nie in denselben Remote-Befehl.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Hub-WAN-Interconnect (200G-Optik)",
    summary: "Wie Hubs mit 200 Gb/s über einen Raum, einen Campus oder eine Stadt verbunden werden: welche Optik bei welcher Distanz, was wo eingesteckt wird und was es braucht, um die Line-Rate wirklich zu erreichen.",
    blocks: [
      {
        t: "p",
        md: "Wenn zwei Hubs beide öffentliche Routen haben, sollte die Experten-Dispatch-Datenebene eine **Direktverbindung** sein — das 443-Relay ist für NAT-Edges. Dieser Eintrag ist das konkrete Rezept, um diese Direktverbindung mit Katalogteilen in die 200-Gb/s-Klasse zu bringen. Eine Regel ordnet alles: **Die Faser ist geschwindigkeitsneutrales Glas; die Geschwindigkeit steckt im Pluggable an jedem Ende.**",
      },
      { t: "h2", kick: "Schritt 1 · nach Distanz wählen", text: "Die Reichweiten-Leiter" },
      {
        t: "table",
        head: ["distance", "part", "plugs into"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC, kein Switch"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / Switch"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "NIC- oder Switch-QSFP56-Käfig"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "NIC- oder Switch-QSFP56-Käfig"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "Switch-/Router-QSFP-DD-Käfig (nicht die NIC)"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "dein Switch übergibt an den Carrier"],
        ],
      },
      { t: "h2", kick: "Schritt 2 · was steckt wo", text: "NIC-Seite vs. Switch-Seite" },
      {
        t: "ul",
        items: [
          "**NIC-Seite** — Karten der ConnectX-6/7-Klasse bieten QSFP56-Käfige; DAC/AOC/FR4/LR4/ER4 sitzen alle direkt in der NIC. Ein Hub der GB10-Klasse hat bereits zwei 200-GbE-QSFP-Ports an Bord, eine Zwei-Hub-Verbindung braucht also genau ein Kabel und null neue Hardware.",
          "**Switch-Seite** — kohärente ZR+-Optiken haben den QSFP-DD-Formfaktor und gehören in einen Switch oder Router; die NIC des Hubs verbindet sich dann mit 200G über ein kurzes DAC mit diesem Switch. Nutze diese Stufe, wenn der ferne Hub zig Kilometer entfernt ist.",
          "**Die Faser selbst** — Standard-Single-Mode-(G.652)-Duplex-LC-Paare, als Dark Fiber pro Strang gemietet. Dasselbe Glas trägt heute 100G und später 400G; Upgrades sind ein Modultausch, nie Tiefbau.",
          "**Jenseits von ~120 km** — hörst du auf, Teile zu kaufen, und mietest eine Wellenlänge von einem Carrier; die Demarkation ist eine Ethernet-Übergabe an deinem Switch.",
        ],
      },
      {
        t: "code",
        caption: "Drei Referenzaufbauten, günstigster zuerst.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Schritt 3 · 200G wirklich erreichen", text: "Line-Rate ist eine Konfiguration, kein Kauf" },
      {
        t: "ul",
        items: [
          "Nutze **RDMA (RoCE)** für den Dispatch-Stream, wo verfügbar — Hosts der GB10-Klasse speisen die NIC über geteilte PCIe-Links, und die gemessene volle Geschwindigkeit (~185–190 Gb/s) zeigt sich unter RoCE mit korrekt gemappter Topologie; ein falsch gemappter Pfad deckelt nahe der halben Rate, und ungetuntes reines TCP landet weit darunter.",
          "Aktiviere **Jumbo-Frames (MTU 9000)** Ende-zu-Ende und lasse `TCP_NODELAY` auf den Dispatch-Sockets an (der Hub setzt es bereits).",
          "Rechne mit *Verifizieren*, nicht Annehmen: Führe nach jeder physischen Änderung einen Perftest zwischen den Hubs aus — der Unterschied zwischen 95 und 190 Gb/s ist unsichtbar, bis er gemessen wird.",
          "Behalte das **443-Relay als Fallback-Pfad** — die Wähl-Policy ist direct-first für öffentliche Peers, Relay für NAT. Die Aufgabe des Relays ist Reichweite, die Aufgabe der Direktverbindung ist Geschwindigkeit.",
        ],
      },
      {
        t: "p",
        md: "Warum das für die Architektur wichtig ist: Die Dekodier-Latenz ist durch die Round-Trip-Zeit begrenzt (~5 µs/km in Faser — Physik, von der Bandbreite unberührt), eine fette Leitung kauft also **Prefill-Geschwindigkeit, Durchsatz beim gebündelten Dispatch und nahezu sofortige Verteilung von Experten-Scheiben**, nicht niedrigere Latenz pro Token. Das ist genau die Hub-Tier-Rolle im zweistufigen Design: Kapazität in der Fette-Leitung-Stufe, Reichweite in der Relay-Stufe.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Last-adaptive Skalierung",
    summary: "Kvasirs MoE-Serving-Pfad wächst und schrumpft mit dem Verkehr: Der Koordinator reaktiviert bewährte Worker unter Sättigung, und der Hub rekrutiert brachliegende Knoten, indem er die Experten-Nachfrage erhöht — alles pull-basiert, sodass auch NAT-Geräte beitreten.",
    blocks: [
      {
        t: "p",
        md: "Kvasirs MoE-Serving-Pfad skaliert elastisch mit der Last, in zwei zusammenwirkenden Schichten. Wenn es ruhig ist, bedient der Koordinator alles lokal für den schnellsten Pfad pro Token; wenn er sättigt, lassen die beiden Schichten darunter den Schwarm wachsen — und schrumpfen ihn wieder, wenn der Andrang vorbei ist.",
      },
      { t: "h2", kick: "Schicht 1", text: "Koordinator-Seite: last-adaptiver Dispatch" },
      {
        t: "p",
        md: "Der Backbone-Koordinator (ein `linkcpp-server`, der das vollständige Modell ausführt) bedient geroutete Experten entweder auf seiner eigenen GPU (schnell, lokal) oder indem er sie an entfernte Worker disponiert. Ein Hintergrund-Thread entscheidet alle paar Sekunden, welches von beidem:",
      },
      {
        t: "ul",
        items: [
          "Er pollt seine **eigenen** Inferenz-Slots. Wenn `busy >= saturation threshold` (Standard 2), steht der Koordinator unter Last.",
          "Unter Last, wenn ein **bewährter** Worker aktiv ist — einer mit `last_serve_ms > 0`, d. h. er hat tatsächlich schon Experten berechnet —, disponiert der Koordinator weiter an ihn über den normalen Idle-Timeout hinaus und bevorzugt den Gesamtdurchsatz gegenüber der Latenz pro Token.",
          "Ein Worker, der sich verbunden, aber nie bedient hat (ein Smartphone, das sich ins Relay eingewählt, aber nie gerechnet hat), wird unter Last **nicht** rekrutiert, weil ein Dispatch an ihn den schnellen lokalen Pfad durch einen langsamen Fallback ersetzen würde. Neue Worker bekommen dennoch einen ersten Versuch über ein kurzes Kulanzfenster.",
          "Die Selbstabfrage ist zeitlich begrenzt, sodass ein hängender Poll niemals den Dispatch blockieren kann.",
        ],
      },
      { t: "h2", kick: "Schicht 2", text: "Hub-Seite: last-adaptive Rekrutierung" },
      {
        t: "p",
        md: "Der Steuerungs-Hub beobachtet jeden MoE-Koordinator und vergrößert den Worker-Pool bei Bedarf:",
      },
      {
        t: "ul",
        items: [
          "Eine Hintergrundschleife pollt die Slots jedes Koordinators und erfasst die Sättigung pro Modell.",
          "Solange ein Modell gesättigt ist, wird sein **effektives Experten-Replika-Ziel** angehoben (Basis + Boost). Der Abdeckungsmarkt liest bereits abgedeckte Experten dann wieder als knapp, und ein Modell **ohne** aktive Worker wird aus seinen GGUF-Metadaten (Expertenanzahl) geseedet, sodass die Nachfrage selbst von null an sichtbar ist.",
          "Brachliegende Knoten pollen den Nachfragemarkt (`/api/expert-volunteer`) und bekommen eine `(layer, expert-range)`-Scheibe zum Bedienen zugewiesen. Sie laden die Scheibe herunter, wählen sich ins Relay ein und registrieren Abdeckung; der Hub verdrahtet sie automatisch in die Dispatch-Map des Koordinators.",
          "Wenn die Last abebbt, fällt das Ziel zurück und die Nachfrage verschwindet, sodass die zusätzlichen Worker nicht mehr disponiert werden und herausaltern.",
        ],
      },
      {
        t: "callout",
        md: "Das Design ist **pull-basiert**: Knoten fragen nach Arbeit, statt geschoben zu werden, sodass ein Worker hinter NAT ohne eingehende Konnektivität teilnimmt. Ein in Schicht 2 rekrutierter Knoten, der zu bedienen beginnt, wird zu einem **bewährten** Worker, den Schicht 1 dann unter Last engagiert hält — die beiden Schichten fügen sich zu einer einzigen elastischen Schleife zusammen.",
      },
      {
        t: "p",
        md: "**Beobachtbarkeit:** `GET /api/moe/recruitment` meldet pro Modell busy/saturation und das Basis- gegenüber dem effektiven Ziel; `/api/expert-demand` trägt ein `recruiting`-Flag.",
      },
    ],
  },
};
