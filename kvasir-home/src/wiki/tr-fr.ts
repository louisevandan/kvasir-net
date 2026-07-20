/* Français — traduction des entrées du wiki. La structure (slug, catégorie,
   ordre des blocs, code) reflète exactement entries.ts (source anglaise) ;
   les termes techniques et identifiants (KVR, linkcpp, moteur d'inférence, GGUF, MoE,
   ring runtime, tok/s, etc.) restent tels quels. Le cadrage de conformité
   (devnet, jeton utilitaire, non-dépositaire) est préservé. */
import type { WikiTranslation } from "./entries";

export const frWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Réseau Kvasir",
    summary: "Un réseau d'inférence IA décentralisé (DePIN) où des appareils du quotidien servent des modèles ouverts et gagnent des KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** est un réseau d'inférence IA décentralisé : les grands modèles ouverts sont répartis sur du matériel partagé grâce au moteur **linkcpp**, de sorte qu'aucun nœud ne détient le modèle entier. Chacun peut apporter un GPU, un CPU, un NPU — même un téléphone — et gagner des **KVR** pour les couches ou experts que son appareil sert réellement. Les développeurs atteignent le réseau par des gateways compatibles OpenAI/Anthropic et paient à l'inférence.",
      },
      {
        t: "ul",
        items: [
          "**Moteur à source disponible** — linkcpp est sous licence BSL (gratuit pour le développement et les tests, l'usage en production requiert une licence) ; le plan de données moteur d'inférence en dessous reste d'origine et inspectable.",
          "**Non-dépositaire** — les récompenses se règlent sur le wallet Solana du propriétaire de chaque nœud ; les clés ne quittent jamais l'utilisateur.",
          "**Prouvé sur du matériel réel** — un modèle de 122B a tourné réparti sur 4 GPU AMD MI250, au sein d'une flotte hétérogène de nœuds GPU/CPU/NPU/mobiles, avec la contribution de chaque nœud créditée de bout en bout.",
          "**Nommé d'après le mythe nordique** — Kvasir, l'être le plus sage, né de l'essence commune de tous les dieux et propriété d'aucun.",
        ],
      },
      { t: "h2", kick: "Une requête, de nombreux appareils", text: "Le parcours d'une inférence" },
      {
        t: "code",
        caption: "Chaque saut est du HTTP/TCP ordinaire ; c'est le modèle lui-même qui est distribué.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Les rôles se **cumulent** : une même machine peut être à la fois nœud de calcul, hôte de gateway et hôte de hub, et ses récompenses s'additionnent. Le travail du réseau est de faire ressembler l'ensemble à une seule machine — un endpoint devant, des milliers d'appareils imparfaits derrière.",
      },
      {
        t: "p",
        md: "Aujourd'hui le réseau tourne sur **Solana devnet** ; KVR est un jeton utilitaire / de contribution, pas un actif négociable ni un investissement, et rien sur cette page ne constitue un conseil financier.",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "Le plan de contrôle : découvre les appareils, planifie le placement des couches, lance les workers, orchestre l'anneau.",
    blocks: [
      {
        t: "p",
        md: "Le **hub** est le plan de contrôle du réseau, servi par linkcpp comme une seule image Docker (`controller.hub:app`, un service FastAPI sur le port **19000**). Il découvre les appareils, vérifie la compatibilité runtime, planifie le placement avec le planner, lance les workers moteur d'inférence d'origine et expose les gateways par contrôleur. C'est une infrastructure volontairement ennuyeuse : HTTP requête/réponse, état résistant aux redémarrages, aucun transport exotique.",
      },
      { t: "h2", kick: "Trois portes d'entrée", text: "Comment les machines rejoignent un hub" },
      {
        t: "ul",
        items: [
          "**Slots de nœud locaux** — cinq slots fixes par hub, mappés sur les ports RPC **50052–50056**. Les slots existent toujours ; on édite les budgets GPU + VRAM/RAM/CPU d'un slot plutôt que de créer des nœuds arbitraires, et les ressources ne sont modifiables **que lorsque le slot n'est pas lié**, ce qui protège le contrat de capacité sous un contrôleur en marche.",
          "**Unités distantes** — enregistrez un autre hub linkcpp en fonctionnement et importez ses nœuds visibles. L'endpoint du plan de données dérive toujours de l'URL de l'*unité* enregistrée plus le port worker exposé par l'unité — jamais d'un hôte de nœud annoncé par le système distant.",
          "**Agents de nœud managés** — des services worker-only (`nodeagent.py`) qui rejoignent par simple HTTP requête/réponse (`/control/join|status|download|load|unload`) et rendent compte via `POST /api/node-reports`. Volontairement **pas** un flux persistant, pour survivre aux routages LAN/VPN simples.",
        ],
      },
      { t: "h2", kick: "Rien ne se charge sans vérification", text: "Le portail de compatibilité" },
      {
        t: "p",
        md: "Chaque unité, nœud et agent rapporte une identité de protocole / runtime-pack plus les détails de backend. Les désaccords d'unité, de runtime-pack, de révision moteur d'inférence et d'ABI RPC sont **bloqués en dur avant bind, plan, load ou infer** ; les différences de backend (CUDA/Metal/Vulkan/CPU) sont suivies comme des capacités du nœud, pas comme des rejets. Le chargement adaptatif est aussi bloqué quand un nœud ne peut pas fournir la supervision de ressources qu'un plan sûr exige.",
      },
      {
        t: "code",
        caption: "Ce qui survit à un redémarrage, et ce qui n'y survit pas.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Le hub étant le rôle le plus critique, ses hôtes gagnent la **plus haute récompense horaire de disponibilité**. Exploiter un hub public exige un staking de **100 000 KVR**.",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Le point d'entrée public : APIs compatibles OpenAI/Anthropic et règlement du paiement à l'inférence en KVR.",
    blocks: [
      {
        t: "p",
        md: "Le **gateway** est l'endroit où les développeurs rencontrent le réseau. Chaque contrôleur expose des endpoints compatibles OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) et Anthropic (`/anthropic/v1/messages`, `/anthropic/v1/models`), tous adossés au même modèle chargé — un client existant fonctionne en ne changeant que la base URL et la clé.",
      },
      {
        t: "code",
        caption: "Un appel OpenAI standard contre le gateway Kvasir.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Facturation", text: "Paiement à l'inférence en KVR" },
      {
        t: "p",
        md: "L'usage se règle en KVR par un flux en trois étapes — **quote → payment → inference** — ainsi une requête est tarifée avant de s'exécuter et les nœuds qui l'ont servie sont crédités ensuite. Le gateway agrège aussi un **catalogue de modèles en direct** depuis chaque hub joignable, de sorte que `/v1/models` reflète ce que le réseau peut réellement servir à l'instant.",
      },
      {
        t: "ul",
        items: [
          "Les hôtes de gateway gagnent une **récompense horaire de disponibilité** pour maintenir le point d'entrée en ligne, plus un **bonus ×1.5** sur chaque inférence qu'ils aident à servir.",
          "Exploiter un gateway public exige un staking de **100 000 KVR** (comme un hub).",
          "Les déploiements publics protègent l'accès opérateur avec **SIWS + 2FA** ; les hubs nus sont conçus pour hôte de confiance / LAN / VPN uniquement.",
        ],
      },
    ],
  },
  node: {
    title: "Nœud",
    summary: "Tout appareil servant une part d'un modèle — GPU, CPU, NPU ou téléphone — gagnant des KVR pour le travail accompli.",
    blocks: [
      {
        t: "p",
        md: "Un **nœud** est tout appareil qui sert une partie d'un modèle : une machine GPU, une machine CPU, un appareil NPU ou un téléphone. Un nœud ne détient que sa part — une fenêtre de couches sur l'anneau, ou une tranche d'experts dans l'essaim — et gagne des KVR pondérés par exactement le travail accompli. La flotte en service mêle des AMD MI250, des NVIDIA GB10 et RTX Pro 6000, un MacBook, des machines CPU x86 sous Windows et des nœuds mobiles dans un seul réseau.",
      },
      { t: "h2", kick: "Du téléchargement au versement", text: "Le cycle de vie d'un nœud" },
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
          "**Les nœuds de calcul** gagnent par unité de contribution, pondérée par la part de couches et mise à l'échelle par le niveau de performance — aucun staking requis.",
          "Les nœuds s'enregistrent sous le wallet de leur propriétaire ; les récompenses s'y règlent, de façon non-dépositaire. Quatre wallets de propriétaires distincts gagnant chacun leur part de couches ont été vérifiés de bout en bout.",
          "Les données de capacités (backend, précision d'accumulation, budgets de ressources) déterminent ce que le planner peut placer sur un nœud — et, dans l'essaim, quels rangs il peut servir.",
          "Un nœud qui ne peut fournir de supervision de ressources est exclu du chargement adaptatif plutôt que d'être cru sur parole.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "Relais 443",
    summary: "Le plan de données pour les appareils derrière NAT : les deux extrémités composent vers l'extérieur via un pont WebSocket sur le port 443.",
    blocks: [
      {
        t: "p",
        md: "Les téléphones derrière le NAT des opérateurs ne peuvent pas accepter de connexions entrantes, et des bordures comme Cloudflare ne laissent passer que les ports 80/443. Le **relais 443** résout les deux : un pont WebSocket par bordure avec un **préambule de rôle d'1 octet** permet aux deux côtés de composer **vers l'extérieur**, de sorte qu'un téléphone participe au plan de données en n'ouvrant **aucun port entrant**.",
      },
      {
        t: "code",
        caption: "Deux connexions sortantes se rejoignent au milieu ; le préambule dit qui est qui.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "Endurci en production", text: "Trois vrais bugs, trois correctifs" },
      {
        t: "ul",
        items: [
          "**Accord d'empreinte de build** — les deux extrémités doivent prouver qu'elles exécutent le même runtime pack avant qu'un seul octet de tenseur ne circule.",
          "**Authentification de téléchargement par node-token** — les téléchargements de shards partiels s'authentifient avec le node token dérivé du wallet que l'app détient déjà.",
          "**Le blocage de trames `Int.ushr`** — le `ushr` de Kotlin n'utilise que les 5 bits bas du décalage : `len ushr 56` est devenu `len ushr 24`, corrompant silencieusement toute trame ≥ 64 KiB (un `result_output` de 593 KB fut la première victime). Corrigé en passant l'empaquetage des longueurs en décalages `Long` — un correctif porteur pour le dispatch d'experts par lots, qui dépasse régulièrement 64 KiB.",
        ],
      },
      {
        t: "p",
        md: "Le relais transporte tout ce dont la topologie a besoin — frontières de couches de l'anneau ou flux de dispatch d'experts — et le mécanisme vérifié pour l'anneau est exactement celui qu'utilisent les workers téléphone en production dans l'essaim.",
      },
      {
        t: "p",
        md: "Les mises à niveau `/api/expert-relay` comme `/api/ring-relay` sont **raccordées à cru** : le gateway transmet les trames WebSocket octet pour octet sans les analyser, de sorte que le relais reste un tuyau mince, indépendant du modèle. Il **compte toujours les octets qu'il fait transiter par session**, et ce travail mesuré alimente le registre de contributions du hub et se règle sur le propre wallet du worker en **KVR** — relayer pour un téléphone derrière NAT rapporte exactement comme un nœud connecté en direct.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "Le plan de contrôle à source disponible (BSL) qui transforme le matériel du quotidien en moteur d'inférence distribué.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** est le moteur derrière Kvasir : un plan de contrôle autour du plan de données RPC de moteur d'inférence, qui exécute de grands modèles d'IA sur plusieurs GPU et machines avec des binaires `ggml-rpc-server` / `llama-server` *d'origine*. Tout ce qu'il ajoute est de l'orchestration — découverte des GPU, slots de nœud, planification du placement des couches, lancement des workers, et les gateways OpenAI/Anthropic.",
      },
      { t: "h2", kick: "Architecture", text: "Un hub, des workers d'origine" },
      {
        t: "code",
        caption: "Le chemin d'une requête à travers un déploiement linkcpp.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Source disponible sous la BSL** — lisez-la, exécutez-la et construisez dessus gratuitement en développement et en test ; l'usage en production requiert une licence.",
          "Le plan de données moteur d'inférence reste **non forké** (hormis un patch mobile GPU-sur-RPC épinglé), si bien que les gains de performance de l'upstream continuent d'affluer.",
          "Livré comme **une seule image Docker** : le hub FastAPI plus les deux binaires moteur d'inférence intégrés ; les nœuds workers natifs se compilent hors Docker pour CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "Le planner", text: "Métadonnées GGUF en entrée, placement en sortie" },
      {
        t: "p",
        md: "Le planner lit les métadonnées GGUF et produit des fenêtres de couches contiguës par nœud, le `--tensor-split` correspondant et des estimations de VRAM KV-cache / couche / expert par nœud — plus l'offload optionnel des FFN d'experts MoE vers la RAM du nœud, émis sous forme de règles `-ot` de moteur d'inférence (p. ex. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) et transmis au lancement via `--override-tensor`. Un plan qui ne tient pas est signalé **infeasible** avant tout chargement, au lieu d'être découvert comme un OOM à l'exécution.",
      },
      {
        t: "p",
        md: "La compatibilité runtime est un concept de premier ordre : protocole, runtime-pack, révision moteur d'inférence et ABI RPC sont vérifiés, et tout désaccord est bloqué en dur avant le moindre bind, plan, load ou inférence.",
      },
    ],
  },
  "ring-runtime": {
    title: "Ring runtime",
    summary: "Inférence en pipeline sans master : chaque appareil exécute sa fenêtre de couches et ne passe que des frontières à son voisin.",
    blocks: [
      {
        t: "p",
        md: "Le **ring runtime** est la topologie de service à faible latence de Kvasir. Chaque appareil ne charge que sa **fenêtre de couches** contiguë, puis ouvre exactement deux liens — prédécesseur et successeur. Les frontières de hidden-state circulent autour de l'anneau ; le dernier rang échantillonne le token et le renvoie. **Pas de master central, et aucun nœud ne détient le modèle entier.**",
      },
      { t: "h2", kick: "Pourquoi pas une étoile", text: "Le problème du master RPC" },
      {
        t: "p",
        md: "Dans la topologie RPC classique, un master ouvre le **GGUF entier** et compose vers chaque worker. Cela casse dans un réseau ouvert de trois façons : le master doit détenir et servir tout le checkpoint ; chaque worker doit être joignable — les téléphones derrière NAT d'opérateur ne le sont pas ; et le master est un propriétaire unique dans un réseau qui ne devrait en avoir aucun. L'anneau supprime les trois : chaque stage possède sa fenêtre, les connexions sont de voisin à voisin, et le relais rend joignables les appareils derrière NAT.",
      },
      {
        t: "code",
        caption: "Un pas de décodage autour d'un anneau à 4 stages.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "Le placement vient du **rank manifest** du planner — p. ex. les 49 couches de Qwen3.5-122B réparties entre un GPU, un CPU, un NPU et un téléphone.",
          "Les frontières sont petites (un vecteur de hidden-state par token), donc les sauts restent bon marché même sur des liens faibles.",
          "Les GPU mobiles exécutent des stages de l'anneau **directement** (Adreno via OpenCL) — la voie RPC vers le GPU d'un téléphone s'est révélée impraticable car la disposition des buffers d'Adreno ne survit pas à la sérialisation RPC ; un stage local possède son backend, donc seules les frontières traversent le réseau.",
        ],
      },
      {
        t: "p",
        md: "L'anneau est la voie de la **latence** ; son plancher est la granularité de couche (~1.4 GB sur le 122B). L'essaim d'experts supprime ce plancher et se branche sur le même tissu de service.",
      },
    ],
  },
  "layer-window": {
    title: "Fenêtre de couches et shards partiels",
    summary: "La tranche contiguë du modèle dévolue à un nœud — téléchargeable en mini-GGUF au lieu du checkpoint complet.",
    blocks: [
      {
        t: "p",
        md: "Une **fenêtre de couches** est la plage contiguë de couches transformer qu'un nœud de l'anneau sert. Un nœud n'a pas besoin du checkpoint complet pour la servir — un **mini-GGUF de stage** ne transporte que les tenseurs de la fenêtre : pour le 122B, **254 MB pour 26 tenseurs** (sur 338 au total) contre 77.6 GB pour le modèle complet, ou ~1.5 GB pour une fenêtre d'une couche sur un téléphone.",
      },
      {
        t: "code",
        caption: "Une ligne du rank manifest : qui sert quoi, dans quel budget.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Revendiquée, pas assignée", text: "Auto-inscription" },
      {
        t: "ul",
        items: [
          "Un nœud interroge la **carte de couverture/demande** pour voir quelles fenêtres sont mal servies et combien chacune paie.",
          "Il choisit la fenêtre non couverte au **rendement maximal** qui tient dans son budget, télécharge exactement cela, et rejoint.",
          "La couverture s'auto-répare : quand un nœud disparaît, sa fenêtre redevient rare — donc lucrative — à nouveau.",
          "Vérifié de bout en bout avec un téléphone derrière NAT : poll → auto-inscription → téléchargement partiel → chargement GPU Adreno → inférence en anneau achevée, contribution créditée.",
        ],
      },
      {
        t: "p",
        md: "L'essaim d'experts réutilise exactement ce marché au grain plus fin de **(couche, plage d'experts)** — même carte, même auto-inscription, mêmes récompenses, unités plus petites.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "Un modèle dont les FFN sont des centaines d'experts indépendants, dont seuls quelques-uns s'activent par token.",
    blocks: [
      {
        t: "p",
        md: "Un modèle **Mixture-of-Experts** remplace la FFN unique de chaque couche par une banque de FFN expertes indépendantes plus un **routeur** qui en choisit quelques-unes par token. Qwen3.5-122B-A10B est l'exemple phare du réseau :",
      },
      {
        t: "stats",
        items: [
          { n: "49", l: "couches" },
          { n: "256", l: "experts / couche" },
          { n: "8", l: "actifs / token" },
          { n: "12,544", l: "experts au total" },
          { n: "5.3 MB", l: "un expert (Q4)" },
          { n: "86%", l: "du poids dans les experts" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "checkpoint complet" },
        ],
      },
      {
        t: "p",
        md: "Chaque couche se scinde en une **voie dense** — attention + KV, norms, le routeur (`ffn_gate_inp`), un expert partagé — et une **banque d'experts** stockée en trois tenseurs empilés (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). La voie dense représente une minorité des octets ; la banque d'experts fait 86 % du modèle.",
      },
      {
        t: "ul",
        items: [
          "L'indice d'expert est la **dimension la plus externe du GGUF** : chaque expert est une dalle contiguë alignée sur les blocs de quantification — l'extraction est une copie de plage d'octets, sans déquantification.",
          "Par token, seuls **8 experts sur 256** s'activent par couche : le trafic d'experts d'une couche au décodage se résume à quelques petites multiplications matricielles sur un vecteur hidden (~6 KB de dispatch).",
          "Les experts sont mutuellement indépendants — leur propriété peut être dispersée entre appareils et rééquilibrée librement.",
        ],
      },
      {
        t: "p",
        md: "Voilà pourquoi le MoE est le substrat naturel de l'essaim : les poids sont déjà conditionnés en unités à la taille d'un appareil, possédables indépendamment.",
      },
    ],
  },
  "expert-sharding": {
    title: "Sharding d'experts",
    summary: "Découper un MoE au grain de l'expert, pour qu'un téléphone porte 42–340 MB d'experts au lieu d'une couche de 1.4 GB.",
    blocks: [
      {
        t: "p",
        md: "Le **sharding d'experts** abaisse l'unité de portage de l'essaim d'une couche (~1.4 GB sur le 122B) à un expert (**5.3 MB**). Un appareil faible télécharge une tranche de 8–64 experts (**42–340 MB**), la charge comme worker à fonction pure — sans attention, sans KV, sans sampler — et calcule ses experts chaque fois que le routeur du backbone les sélectionne.",
      },
      { t: "h2", kick: "Deux rôles", text: "Backbone × worker" },
      {
        t: "code",
        caption: "Le point de coupe dans une couche MoE (le routeur tourne une fois, sur le backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "Le **backbone** garde la voie dense (attention, norms, routeur, expert partagé, combine) et conserve tous les experts en réplique de secours déchargée en RAM, pour tolérer le churn.",
          "**Les workers** (`linkcpp-expert-worker --serve`) répondent `(n_used, n_tokens, cur, sel) → experts` sur un flux TCP de longue durée — le même flux que le relais 443 tunnelise pour les téléphones.",
          "La couverture s'auto-répare via le **marché de couverture d'experts** : `POST /api/expert-coverage` bat le rappel des détentions, `GET /api/expert-demand` agrège la rareté, et `POST /api/expert-volunteer` attribue la plage la plus rare, taillée au budget du nœud.",
        ],
      },
      { t: "h2", kick: "Mesuré, pas promis", text: "Vérifié sur du matériel réel" },
      {
        t: "ul",
        items: [
          "Calcul shardé == monolithique à **max|Δ| = 3.6e-12** près (un regroupement exact, pas une approximation).",
          "Dispatch inter-processus sur un décodage 122B réel : **argmax MATCH**, cosine des logits 0.99869 — identique octet pour octet à l'in-process.",
          "Un Galaxy S25 a téléchargé de façon autonome sa tranche de 1.58 GB et calculé les experts de la layer-0 à chaque token : **8/8 tokens identiques** à l'exécution locale.",
          "Un GPU distant à travers l'Internet public — un aller-retour WAN par token — est resté **greedy 8/8 identique** (cosine 0.99773) : **1.2% de surcoût de débit** sur un lien direct, ~28% via une bordure CDN. Le coût honnête d'un dispatch sériel, token par token, et pourquoi le levier du tissu est le traitement par lots, non une latence plus basse.",
          "Le dispatch par lots atteint **53k tok/s par worker** à batch 512 (ROCm) — la propriété de tissu de débit qui rend l'essaim praticable.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "Autorité du routeur",
    summary: "L'invariant de cohérence de l'essaim : le routage se décide une fois, sur le backbone — les workers ne reçoivent que des ids d'experts.",
    blocks: [
      {
        t: "callout",
        md: "**L'invariant :** la seule décision discrète du réseau est le routage MoE (top-8 sur 256). Kvasir exécute le routeur **exactement une fois, sur le backbone**, et n'expédie aux workers que les ids des experts choisis. Un essaim hétérogène peut différer légèrement dans la *grandeur* de la sortie de chaque expert — jamais dans *quels experts tournent*.",
      },
      {
        t: "p",
        md: "Sans cette règle, chaque backend rejouerait le routeur et choisirait **des experts différents** pour les tokens limites — une divergence authentiquement catastrophique, car dès ce token le calcul bifurque comme avec une autre graine aléatoire. Avec elle, les différences matérielles se réduisent à une erreur continue bornée que le combine pondéré par probabilité absorbe.",
      },
      { t: "h2", kick: "Ce qu'elle empêche", text: "Modes de divergence fermés par un seul point de décision" },
      {
        t: "table",
        head: ["Mode de divergence", "Sans autorité", "Avec autorité"],
        rows: [
          ["Désaccord de routage", "Les backends choisissent des top-8 différents aux frontières", "Ids décidés une fois, expédiés aux détenteurs"],
          ["Bifurcation de trajectoire", "Un token basculé bifurque toute la séquence", "Décodage/échantillonnage épinglés à un seul nœud"],
          ["Vérification", "Comparaison bit à bit entre backends (impossible)", "Contrôles de tolérance sur des résidus bien définis"],
        ],
      },
      {
        t: "p",
        md: "Le coût est négligeable : le backbone calculait déjà `ffn_norm` et les logits du routeur ; ce qui traverse le réseau n'est que les lignes hidden plus les ids sélectionnés — environ **6 KB par pas de décodage**.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "Équivalence numérique",
    summary: "Des backends différents ne s'accordent jamais bit à bit ; l'essaim traite la tolérance mesurée comme un contrat de premier ordre.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno et les CPU calculent la même opération avec des ordres de réduction, des fusions FMA, des accumulateurs et des approximations de fonctions transcendantes différents — les résultats diffèrent de ~1e-6…1e-3 par opération, **par conception, jamais à l'identique au bit près**. Un essaim fait du matériel qui se présente ne peut exiger l'exactitude binaire, alors Kvasir mesure l'équivalence à la place.",
      },
      {
        t: "table",
        head: ["Paire de backends (122B réel, experts layer-0)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU de téléphone vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "La matrice complète de backends est close : les deux backends GPU (CUDA, ROCm) partagent leurs sources de kernel et atterrissent **effectivement bit-identiques** (cosine 1.0000000000), tandis que les paires GPU↔CPU restent équivalentes à ~0.9997. Un worker CUDA et un worker ROCm sont interchangeables ; un worker GPU et un worker CPU sont numériquement équivalents.",
      },
      { t: "h2", kick: "Pourquoi elles diffèrent", text: "L'addition en virgule flottante n'est pas associative" },
      {
        t: "ul",
        items: [
          "**Ordre de réduction du matmul** — tensor cores, tuiles MFMA, workgroups OpenCL et lanes SIMD accumulent dans des ordres différents.",
          "**Précision d'accumulation** — stockage F16/BF16 avec accumulateurs F32 ou F16 : le plus grand levier de la divergence.",
          "**Approximations transcendantes** — exp (softmax), silu (swiglu) et rsqrt (norms) utilisent des variantes polynomiales/tabulées différentes selon le backend.",
        ],
      },
      { t: "h2", kick: "Le contrat", text: "Tolérances, capacités, autorité unique" },
      {
        t: "ul",
        items: [
          "La vérification est une **tolérance** — « accord top-1 ≥ 99.x %, KL ≤ ε » — jamais une égalité binaire.",
          "Les backends et la précision d'accumulation sont annoncés comme des **capacités** du nœud ; les nœuds accumulant en F32 sont préférés pour les rangs sensibles à la sortie.",
          "Les nœuds hors tolérance sont marqués inaptes pour les rangs sensibles, pas rejetés en bloc.",
          "Les décisions discrètes (routage, échantillonnage) sont épinglées à des autorités uniques pour que l'erreur continue ne puisse jamais devenir divergence discrète.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "Le format de fichier de modèle quantifié de moteur d'inférence — et la disposition qui rend le découpage partiel et par expert bon marché.",
    blocks: [
      {
        t: "p",
        md: "**GGUF** est le format de modèle en fichier unique de l'écosystème moteur d'inférence : des métadonnées (architecture, nombre de couches, dimensions, quantification) plus les tenseurs en octets quantifiés bruts (p. ex. Q4_K_M). Le planner de linkcpp lit les métadonnées pour calculer placements et estimations de taille ; le côté service découpe les octets de tenseurs pour produire les téléchargements.",
      },
      {
        t: "ul",
        items: [
          "**Les mini-GGUF de stage** transportent les tenseurs d'une seule fenêtre de couches — 254 MB au lieu de 77.6 GB pour un stage d'anneau du 122B.",
          "**Les GGUF de shard d'experts** transportent une tranche (couche, plage d'experts), servie par `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` avec authentification par node-token.",
          "Les deux sont des **fichiers GGUF valides** : le lecteur du nœud les charge avec l'outillage d'origine, sans format maison.",
        ],
      },
      {
        t: "code",
        caption: "Pourquoi le découpage d'expert est une copie d'octets : l'indice d'expert est la dimension la plus externe.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "Le routeur (`ffn_gate_inp`) et l'expert partagé sont **exclus** des shards d'experts — ils appartiennent au backbone, ce qui est exactement ce que l'autorité du routeur exige.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "Le jeton utilitaire du réseau : les développeurs le dépensent en inférence, les contributeurs le gagnent pour leur calcul.",
    blocks: [
      {
        t: "p",
        md: "**KVR** (nom on-chain « Kvasir », 6 décimales, Solana) est un même jeton circulant dans les deux sens : les développeurs **dépensent** des KVR pour exécuter de l'inférence via le gateway, les contributeurs **gagnent** des KVR pour le calcul fourni par leurs nœuds. Les récompenses se calculent sur le travail réel — couches et experts effectivement servis — pas sur la participation.",
      },
      {
        t: "ul",
        items: [
          "**Côté dépense** — paiement à l'inférence via le gateway : quote → payment → inference.",
          "**Côté gain** — unités de contribution × part de couches × niveau de performance pour le calcul ; disponibilité horaire pour les rôles hub/gateway.",
          "**Règlement** — sur Solana, vers le wallet du propriétaire de chaque nœud ; le service de règlement crédite chaque nœud ayant touché une requête.",
          "**Nommé d'après le mythe** — l'Hydromel de Poésie, brassé à partir de Kvasir, donnant la sagesse à quiconque le boit : accès ouvert, et récompenses pour tous ceux qui versent au pot.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, jeton utilitaire.** KVR tourne actuellement sur Solana devnet et est un jeton utilitaire / de contribution — ni actif négociable, ni prix, ni investissement. Rien ici n'est un conseil financier ou une promesse de rendement.",
      },
    ],
  },
  "contribution-units": {
    title: "Unités de contribution",
    summary: "La formule de récompense : les unités suivent les tokens servis pondérés par la part de couches, puis s'échelonnent par niveau de performance.",
    blocks: [
      {
        t: "code",
        caption: "Comment les récompenses de calcul sont calculées.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "Une **unité** ≈ 1k tokens servis, pondérée par la **part de couches** du nœud dans chaque inférence — un nœud qui exécute 12 couches sur 49 gagne 12/49 des unités de chaque inférence. Le multiplicateur de niveau récompense ensuite la vitesse mesurée, et les rôles d'infrastructure accumulent en plus la disponibilité horaire.",
      },
      { t: "h2", kick: "Exemple chiffré", text: "Une inférence, quatre nœuds" },
      {
        t: "table",
        head: ["Nœud", "Couches", "Part", "Niveau", "Unités effectives / 1k tokens"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Téléphone", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Les récompenses suivent le **travail réel** : un nœud qui n'a rien servi ne gagne rien, quel que soit son temps en ligne (rôles de calcul).",
          "Les rôles se **cumulent** — une machine peut être calcul + gateway + hub, et ses flux s'additionnent.",
          "Tout se règle en KVR vers le wallet du propriétaire du nœud ; le tableau de bord affiche brut × niveau = effectif et un solde réclamable.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "Niveaux de performance",
    summary: "Le débit mesuré fixe un multiplicateur : S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["Niveau", "Débit mesuré", "Multiplicateur"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "La vitesse de décodage mesurée d'un nœud fixe son niveau, et le niveau multiplie ses unités gagnées — un matériel plus rapide gagne proportionnellement plus pour le même travail. La vue d'état des nœuds du wallet affiche le niveau de chaque nœud à côté de sa contribution.",
      },
      {
        t: "ul",
        items: [
          "Les niveaux sont **mesurés, pas auto-déclarés** — le débit provient de la performance de service réelle du nœud et se remesure au fil du temps.",
          "Un téléphone de niveau C gagne quand même — ×0.7 de sa part de couches — et c'est là le but : le plancher vaut pour toute participation, pas seulement pour les datacenters.",
          "Le niveau multiplie les unités *effectives* : il se compose avec la part de couches et le bonus gateway au lieu de les remplacer.",
        ],
      },
    ],
  },
  staking: {
    title: "Staking",
    summary: "Stakez des KVR pour gagner un intérêt APR ; 100 000 KVR stakés qualifient un wallet pour opérer des nœuds hub ou gateway.",
    blocks: [
      {
        t: "p",
        md: "Le staking verrouille des KVR dans votre propre wallet pour gagner un **intérêt APR** et se qualifier aux récompenses de nœud. Exploiter un nœud **hub** ou **gateway** exige un stake de **100 000 KVR** ; les nœuds de calcul ordinaires rejoignent sans aucun stake et gagnent pour les couches qu'ils exécutent.",
      },
      {
        t: "ul",
        items: [
          "Le staking se fait dans le panneau de staking du tableau de bord du wallet : saisissez un montant, **Stake**, et la position commence à accumuler l'APR plus l'éligibilité aux récompenses de nœud.",
          "L'exigence de 100k est un **filtre d'engagement** pour les deux rôles dont dépend le trafic des autres — les points d'entrée et le plan de contrôle.",
          "Le staking est non-dépositaire comme tout le reste : la position vit dans votre propre wallet, et principal, intérêts accumulés et récompenses de nœud sont tous visibles dans le panneau de staking.",
          "Le KVR de devnet pour staker vient par distribution ou swap (swap SOL/ETH ↔ KVR : bientôt) ; le SOL de devnet pour les frais vient du faucet public.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "Wallet non-dépositaire",
    summary: "Les clés ne vivent que sur l'appareil de l'utilisateur — web, desktop, iOS et Android — et les récompenses s'y règlent directement.",
    blocks: [
      {
        t: "p",
        md: "Le Kvasir Wallet est **non-dépositaire par conception** : la phrase de récupération de 12 mots et les clés ne sont stockées que sur l'appareil de l'utilisateur, jamais chez un opérateur. Les récompenses se règlent sur Solana directement vers le wallet du propriétaire de chaque nœud — vérifié avec quatre wallets de propriétaires distincts, chacun gagnant sa propre part de couches.",
      },
      {
        t: "ul",
        items: [
          "**Plateformes** — web, desktop (macOS/Windows/Linux, wallet + nœud dans une seule app Electron), iOS et Android.",
          "**Une phrase, tous les appareils** — la même phrase de 12 mots restaure le même compte sur desktop, téléphone et web ; une passphrase locale déverrouille chaque installation.",
          "**Wallet = identité du nœud** — le wallet signe l'identité réseau du nœud : « qui gagne pour cet appareil » est cryptographique, pas une ligne de compte sur le serveur de quelqu'un.",
          "**Perdez la phrase, perdez le compte** — la non-custodie coupe dans les deux sens ; aucun opérateur ne peut la réinitialiser.",
        ],
      },
      {
        t: "p",
        md: "Les apps mobiles font aussi office d'apps de nœud : le même wallet qui détient vos KVR configure le backend de calcul et le mode nœud du téléphone, stake et réclame les récompenses.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "La connexion opérateur est une signature de wallet (Sign-In With Solana) sur un nonce serveur, plus une 2FA TOTP optionnelle.",
    blocks: [
      {
        t: "p",
        md: "Pour les déploiements publics, l'accès opérateur au hub et au gateway s'authentifie par **Sign-In With Solana** : le wallet de l'opérateur signe un nonce émis par le serveur, prouvant la propriété sans mot de passe ni identifiant en dépôt. Par-dessus, la **2FA TOTP** et des codes de secours à usage unique protègent la session — sur le hub comme sur le gateway.",
      },
      {
        t: "ul",
        items: [
          "**Aucun mot de passe nulle part** — la clé du wallet est l'identité et le nonce empêche le rejeu ; il n'y a rien côté serveur à hameçonner ou à divulguer.",
          "**L'enrôlement TOTP par wallet** est persisté dans l'état du hub : la 2FA survit aux redémarrages avec les slots et les liaisons.",
          "**Les codes de secours sont à usage unique** — chacun se consomme à la connexion, pour récupérer quand l'appareil d'authentification est indisponible.",
          "**Périmètre annoncé honnêtement** — le hub nu et les ports RPC sont conçus pour hôte de confiance / LAN / VPN ; SIWS + 2FA est la couche qui rend les domaines *publics* sûrs à exposer.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "L'économie KVR",
    summary: "Comment le coût pour le consommateur et la récompense du nœud forment une seule boucle qui se renforce elle-même — le cercle vertueux qui laisse le réseau devenir moins cher à mesure qu'il grandit.",
    blocks: [
      {
        t: "p",
        md: "Kvasir est un **marché biface** réglé en un seul jeton. Les consommateurs paient des **KVR** par inférence dans le trésor ; les nœuds gagnent des **KVR** pour le travail exact qu'ils ont servi, reversés sur leurs propres wallets. L'objectif de conception est que ces deux côtés ne se concurrencent pas — ils se **composent** : plus d'offre rend le réseau moins cher et meilleur, ce qui attire plus de demande, dont les paiements financent des récompenses plus riches, ce qui attire plus d'offre.",
      },
      { t: "h2", kick: "Le volant d'inertie", text: "L'usage et l'offre croissent ensemble" },
      {
        t: "p",
        md: "Parce que l'inférence **doit** être payée en KVR, chaque unité d'usage est une demande réelle pour le jeton — de l'utilité, pas de la spéculation. Cette demande soutient la valeur des KVR que les nœuds gagnent, ce qui garde la contribution attractive, ce qui accroît la capacité, ce qui abaisse le prix et la latence, ce qui attire plus d'usage. L'avantage le plus tranchant de Kvasir resserre encore la boucle : un participant peut être **consommateur et fournisseur à la fois** (un *prosommateur*), de sorte que les deux côtés grandissent souvent chez les mêmes personnes.",
      },
      {
        t: "callout",
        md: "**« Gratuit quand vous contribuez » est net-gratuit, pas sans coût.** Vous payez ce que vous inférez et gagnez pour ce que vous servez ; contribuez à peu près autant que vous consommez et les deux s'annulent. Le réseau n'est pas gratuit — c'est *votre* facture qui l'est.",
      },
      { t: "h2", kick: "Le garder vertueux", text: "Trois invariants, et les spirales qu'ils préviennent" },
      {
        t: "table",
        head: ["Invariant", "Spirale qu'il prévient"],
        rows: [
          ["Récompenses financées par des revenus réels (émission seulement pour amorcer, puis dégressive)", "L'inflation érode le KVR jusqu'à l'effondrement des deux côtés"],
          ["Le KVR est le médium obligatoire de l'inférence", "La valeur du jeton se découple de l'usage pour devenir pure spéculation"],
          ["Le prix flotte entre un plancher de coût et un plafond sous le marché", "Trop bas affame les nœuds ; trop haut perd les utilisateurs au profit des API centralisées"],
        ],
      },
      {
        t: "p",
        md: "Kvasir récompense déjà le **travail réel** (KVR par tokens servis × part de couches, pas la simple présence) et règle de façon non-dépositaire, ce qui est la partie difficile pour rendre honnêtes des récompenses financées par les revenus. Le reste — un prix piloté par l'utilisation et une transition émission→revenus — est la feuille de route économique qui transforme « plus de nœuds → moins cher » d'une intuition en une règle imposée par le protocole. L'entrée **Tarification de l'inférence** couvre le côté prix ; **Unités de contribution** couvre comment le travail devient récompense.",
      },
    ],
  },
  "inference-pricing": {
    title: "Tarification de l'inférence",
    summary: "Ce qu'une inférence coûte en KVR aujourd'hui, pourquoi un réseau décentralisé est structurellement moins cher, et comment le prix est censé baisser à mesure que l'offre croît.",
    blocks: [
      {
        t: "p",
        md: "L'accès au réseau se fait en **paiement à l'inférence** : le gateway cote un prix en KVR pour votre requête, votre wallet le paie on-chain, et alors seulement le hub exécute le modèle. La tarification est une formule petite et transparente — un plancher par requête plus un tarif par token — cotée d'avance et réglée sur l'usage **réel** de tokens après génération.",
      },
      {
        t: "code",
        caption: "La formule de règlement — cotée avant, facturée sur l'usage réel après.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Pourquoi il peut être moins cher", text: "Aucune marge centrale à payer" },
      {
        t: "p",
        md: "Une API centralisée tarife au coût **plus** une large marge et le recouvrement du capital. Un réseau décentralisé tarife près du **coût marginal** de ses contributeurs — électricité et amortissement du matériel — plus une mince commission de protocole. Cet écart structurel existe quelle que soit la taille. La croissance le creuse : le **sharding d'experts** fait que plus de nœuds détiennent chacun une tranche plus petite, si bien que des appareils moins chers peuvent servir, abaissant le coût marginal de participation et approfondissant l'offre.",
      },
      {
        t: "callout",
        md: "**Le prix est gouverné, pas un libre-service.** Les tarifs sont un paramètre économique sensible, modifiés seulement par le wallet genesis sous signature de wallet + 2FA — jamais par une variable d'environnement. Cela garde l'économie du jeton stable et auditable.",
      },
      { t: "h2", kick: "Vers où cela se dirige", text: "Un prix piloté par l'utilisation" },
      {
        t: "p",
        md: "La direction de conception est un prix qui **flotte avec l'utilisation du réseau** entre un plancher (gardé au-dessus du coût marginal des nœuds, pour que servir reste rentable) et un plafond (gardé sous les alternatives centralisées, pour rester compétitif). L'offre inoccupée pousse le prix vers le bas ; la congestion le pousse vers le haut. C'est le mécanisme qui rend enfin **« plus de nœuds partagés → prix plus bas »** vrai dans le code — le thermostat naturel de l'**économie KVR**.",
      },
    ],
  },
  "run-expert-worker": {
    title: "Exploiter un worker d'experts",
    summary: "Transformez un GPU, un CPU ou un téléphone de rechange en worker d'experts : compilez-le, portez-vous volontaire pour la tranche la plus rare, téléchargez-la, servez, et composez vers l'extérieur sur le 443 pour gagner des KVR.",
    blocks: [
      {
        t: "p",
        md: "Un **worker d'experts** est une fonction pure `(hidden, ids) → out` — sans attention, sans cache KV, sans sampler — qui calcule une tranche des experts d'un modèle MoE chaque fois que le routeur du backbone les sélectionne. Vous ne choisissez pas ce que vous servez ; le **marché de couverture** vous confie la plage la plus rare et la mieux rémunérée, taillée à votre budget, de sorte qu'un téléphone de 4 GB comme un GPU de datacenter y trouvent une place.",
      },
      { t: "h2", kick: "Sept étapes", text: "Compiler → se porter volontaire → servir → composer → gagner" },
      {
        t: "code",
        caption: "Tout le parcours — le script de composition attend le backbone avec une boucle de réessai.",
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
          "**La tranche est minuscule.** Une tranche layer-0 de 128 experts fait **794 MB** face au modèle complet de 72 GB — le grain qui permet aux appareils faibles de participer. Vous ne téléchargez que la plage que le marché a attribuée.",
          "**Composition sortante, jamais entrante.** L'étape 5 ouvre un seul WebSocket sortant sur le 443, si bien que le NAT des opérateurs et les bordures CDN le laissent passer et que vous n'exposez aucun port entrant — le même chemin qu'utilise un téléphone.",
          "**Le heartbeat est porteur.** Sans `POST /api/expert-coverage`, vous ne servez rien que la carte de demande connaisse, et rien de ce que vous faites n'est crédité.",
          "**La récompense va au travail.** Le travail relayé s'accumule dans le registre de contributions du hub ; le gateway crédite en delta des KVR vers votre **propre** wallet (non-dépositaire). Il vous faut une adresse de wallet pour être payé.",
        ],
      },
      {
        t: "callout",
        md: "Le worker parle le même protocole de dispatch qu'un GPU en datacenter — `(n_used, n_tokens, cur, sel) → experts` sur un unique flux de longue durée. Un worker à shard partiel se contente de fixer `n_used = 1`. C'est cette uniformité qui fait qu'un téléphone, une machine CPU et une carte Blackwell sont des membres interchangeables du même essaim.",
      },
    ],
  },
  "hub-operations": {
    title: "Exploiter un hub",
    summary: "Notes d'opérateur pour faire tourner un hub et un gateway : corriger à chaud sans reconstruction, survivre aux redémarrages, garder le catalogue enregistré, et verrouiller la surface au seul 443.",
    blocks: [
      {
        t: "p",
        md: "Le hub (plan de contrôle) et le gateway (point d'entrée public) sont les deux services de longue durée qu'un opérateur maintient en bonne santé. Le hub et ses ports RPC sont **non authentifiés par conception** — hôte de confiance / LAN / VPN uniquement — et tout le trafic public converge vers l'unique surface 443 du gateway. Voici les notes d'exploitation qui gardent cet arrangement stable à travers les changements de code, les redémarrages et les reboots.",
      },
      { t: "h2", kick: "Déploiement et correction à chaud", text: "Changer le code sans reconstruire" },
      {
        t: "ul",
        items: [
          "**Voie rapide :** mettez à jour le code du hub/gateway avec `docker cp <file> <container>:/app/...` + `docker restart` — sans reconstruction d'image. Mais **ajouter une variable d'environnement ne peut se faire ainsi** (cela nécessite une recréation du conteneur) ; préférez plutôt une API de config runtime qui persiste dans l'état du hub.",
          "**Dérive de compose :** un conteneur de longue durée peut diverger de son fichier compose (mode réseau, entrypoint, env). Faites toujours `docker inspect` sur la config réelle avant une recréation par `docker compose up -d` — si elle a dérivé, la recréation efface les réglages de production. Utilisez cp + restart.",
          "**Diffez avant de patcher :** extrayez le fichier du conteneur avec `docker cp` et comparez-le au HEAD du dépôt avant de le remplacer, pour qu'un hot-patch d'une session antérieure ne soit pas silencieusement perdu.",
        ],
      },
      { t: "h2", kick: "Survivre à un redémarrage", text: "L'état persiste ; les modèles chargés non" },
      {
        t: "ul",
        items: [
          "Un redémarrage du hub **arrête le service.** Les slots, contrôleurs et liaisons se restaurent depuis `hub-state.json`, mais un modèle chargé est runtime-only. Après un redémarrage, lisez le `last_load` de chaque contrôleur et relancez `POST /api/controllers/{cid}/serve` — même un grand modèle revient en ~1 minute grâce au cache de pages.",
          "**Watchdog du gateway :** sondez chaque modèle servi avec une requête d'1 token toutes les 30 s et rechargez automatiquement depuis `last_load` en cas d'échec (avec un cooldown). **Sondez *tous* les modèles, pas `catalog[0]`** — dès qu'un modèle sain d'un autre hub remonte en tête, une sonde limitée au premier manque un gros modèle qui tombe (un vrai bug, depuis corrigé).",
          "**TTL du catalogue :** `POST /api/pay/hub/register` a un TTL de 90 s, alors gardez l'enregistrement vivant avec une boucle de heartbeat d'environ 60 s, rendue durable au fil des reboots par un cron `@reboot` ou une unité systemd.",
        ],
      },
      { t: "h2", kick: "Verrouillez-le", text: "Tout ce qui est public passe par le 443" },
      {
        t: "ul",
        items: [
          "Le hub (:19000) et les ports RPC supposent un réseau de confiance ; la seule chose qui devrait faire face à Internet est le gateway sur le 443 (y compris son passage de relais WebSocket).",
          "Si un hub doit se trouver sur une IP publique, filtrez-le au pare-feu vers des IP de confiance — mais les ports publiés par Docker sont **DNAT'd avant la chaîne INPUT**, donc une règle sur `dport` ne correspondra pas. Filtrez plutôt dans la chaîne `DOCKER-USER` en utilisant le port de destination d'origine de conntrack (`--ctorigdstport`), et persistez les règles avec un oneshot systemd ordonné `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Règlement et pièges", text: "Pull, pas push — et un piège shell" },
      {
        t: "ul",
        items: [
          "**Le règlement est pull, pas push :** le hub accumule les contributions ; le gateway interroge `GET /api/contributions` et crédite en delta des KVR. Si un redémarrage du hub remet ses compteurs à zéro, le gateway rétablit une nouvelle base pour que rien ne soit payé deux fois. Le taux de travail d'experts est fixé par `LINKCPP_EXPERT_UNITS_PER_MB`.",
          "**Le piège `pkill` :** `ssh host 'pkill -f X; ...'` correspond à sa *propre* ligne de commande et se tue lui-même. Utilisez une classe de caractères dans le motif (`X[x]`), et ne mettez jamais le lancement du processus et le pkill dans la même commande distante.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Interconnexion WAN entre hubs (optiques 200G)",
    summary: "Comment des hubs se relient à 200 Gb/s à travers une salle, un campus ou une ville : quelle optique à quelle distance, quoi se branche où, et ce qu'il faut pour vraiment atteindre le débit nominal.",
    blocks: [
      {
        t: "p",
        md: "Quand deux hubs ont tous deux des routes publiques, le plan de données de dispatch d'experts devrait être un **lien direct** — le relais 443 est pour les bordures derrière NAT. Cette entrée est la recette concrète pour faire de ce lien direct un lien de classe 200 Gb/s avec des pièces de catalogue. Une règle organise tout : **la fibre est un verre neutre en vitesse ; la vitesse réside dans le module enfichable à chaque extrémité.**",
      },
      { t: "h2", kick: "Étape 1 · choisir par distance", text: "L'échelle de portée" },
      {
        t: "table",
        head: ["distance", "pièce", "se branche sur"],
        rows: [
          ["même rack, 0.5–3 m", "QSFP56 DAC (cuivre passif)", "NIC ↔ NIC, sans switch"],
          ["même salle, ≤30 m", "QSFP56 AOC (optique active)", "NIC ↔ NIC / switch"],
          ["campus, 2–10 km", "module 200G FR4 (2 km) / LR4 (10 km) + LC duplex, fibre monomode", "cage QSFP56 de NIC ou de switch"],
          ["métro, ≤40 km", "module 200G ER4, fibre monomode", "cage QSFP56 de NIC ou de switch"],
          ["région, ≤120 km", "module cohérent 400G ZR+ réglé sur un débit ligne 200G", "cage QSFP-DD de switch/routeur (pas la NIC)"],
          ["longue distance, des centaines de km", "longueur d'onde 200G louée à l'opérateur (ou 2×100G) sur DWDM", "votre switch passe la main à l'opérateur"],
        ],
      },
      { t: "h2", kick: "Étape 2 · quoi se branche où", text: "Côté NIC vs côté switch" },
      {
        t: "ul",
        items: [
          "**Côté NIC** — les cartes de classe ConnectX-6/7 exposent des cages QSFP56 ; DAC/AOC/FR4/LR4/ER4 se logent tous directement dans la NIC. Un hub de classe GB10 possède déjà deux ports QSFP 200 GbE embarqués, si bien qu'un lien entre deux hubs ne requiert exactement qu'un seul câble et aucun nouveau matériel.",
          "**Côté switch** — les optiques cohérentes ZR+ sont au format QSFP-DD et ont leur place dans un switch ou un routeur ; la NIC du hub rejoint alors ce switch à 200G via un court DAC. Utilisez ce palier quand le hub distant est à des dizaines de kilomètres.",
          "**La fibre elle-même** — des paires LC duplex monomode standard (G.652), louées comme fibre noire au brin. Le même verre porte 100G aujourd'hui et 400G demain ; les mises à niveau sont un échange de module, jamais des travaux de génie civil.",
          "**Au-delà de ~120 km** — vous cessez d'acheter des pièces et commencez à louer une longueur d'onde à un opérateur ; la démarcation est une remise Ethernet sur votre switch.",
        ],
      },
      {
        t: "code",
        caption: "Trois montages de référence, du moins cher au plus cher.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Étape 3 · atteindre réellement 200G", text: "Le débit ligne est une configuration, pas un achat" },
      {
        t: "ul",
        items: [
          "Utilisez **RDMA (RoCE)** pour le flux de dispatch là où c'est disponible — les hôtes de classe GB10 alimentent la NIC via des liens PCIe scindés, et la pleine vitesse mesurée (~185–190 Gb/s) apparaît sous RoCE avec une topologie correctement mappée ; un chemin mal mappé plafonne près de la moitié du débit et un TCP simple non optimisé atterrit bien plus bas.",
          "Activez les **jumbo frames (MTU 9000)** de bout en bout et gardez `TCP_NODELAY` sur les sockets de dispatch (le hub le règle déjà).",
          "Attendez-vous à *vérifier*, pas à supposer : lancez un perftest entre hubs après chaque changement physique — la différence entre 95 et 190 Gb/s est invisible tant qu'elle n'est pas mesurée.",
          "Gardez le **relais 443 comme voie de repli** — la politique de composition est direct d'abord pour les pairs publics, relais pour le NAT. Le rôle du relais est la portée, le rôle du lien direct est la vitesse.",
        ],
      },
      {
        t: "p",
        md: "Pourquoi cela compte pour l'architecture : la latence de décodage est bornée par le temps d'aller-retour (~5 µs/km dans la fibre — de la physique, insensible à la bande passante), si bien qu'un gros tuyau achète **de la vitesse de prefill, du débit de dispatch par lots et une distribution quasi instantanée des tranches d'experts**, pas une latence par token plus basse. C'est exactement le rôle du palier hub dans la conception à deux paliers : la capacité dans le palier gros tuyau, la portée dans le palier relais.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Mise à l'échelle adaptative à la charge",
    summary: "Le chemin de service MoE de Kvasir grandit et se contracte avec le trafic : le coordinateur remobilise les workers éprouvés en saturation, et le hub recrute des nœuds inactifs en augmentant la demande d'experts — le tout basé sur le pull, si bien que les appareils derrière NAT participent aussi.",
    blocks: [
      {
        t: "p",
        md: "Le chemin de service MoE de Kvasir se met à l'échelle de façon élastique avec la charge, en deux couches coopérantes. Au repos, le coordinateur sert tout localement pour le chemin le plus rapide par token ; en saturation, les deux couches ci-dessous font grandir l'essaim — et le contractent de nouveau quand la pointe passe.",
      },
      { t: "h2", kick: "Couche 1", text: "Côté coordinateur : dispatch adaptatif à la charge" },
      {
        t: "p",
        md: "Le coordinateur backbone (un `linkcpp-server` exécutant le modèle complet) sert les experts routés soit sur son propre GPU (rapide, local), soit en les dispatchant vers des workers distants. Un thread d'arrière-plan décide lequel, toutes les quelques secondes :",
      },
      {
        t: "ul",
        items: [
          "Il interroge ses **propres** slots d'inférence. Quand `busy >= saturation threshold` (défaut 2), le coordinateur est sous charge.",
          "Sous charge, si un worker **éprouvé** est actif — un worker dont `last_serve_ms > 0`, c'est-à-dire qui a déjà réellement calculé des experts — le coordinateur continue de lui dispatcher du travail au-delà du délai d'inactivité normal, privilégiant le débit agrégé sur la latence par token.",
          "Un worker qui s'est connecté sans jamais servir (un téléphone qui a appelé le relay sans jamais calculer) n'est **pas** recruté sous charge, car lui dispatcher du travail remplacerait le chemin local rapide par un repli lent. Les nouveaux workers ont tout de même un premier essai via une courte fenêtre de grâce.",
          "L'auto-interrogation est bornée dans le temps, de sorte qu'un sondage bloqué ne peut jamais bloquer le dispatch.",
        ],
      },
      { t: "h2", kick: "Couche 2", text: "Côté hub : recrutement adaptatif à la charge" },
      {
        t: "p",
        md: "Le hub de contrôle surveille chaque coordinateur MoE et fait grandir le pool de workers lorsque nécessaire :",
      },
      {
        t: "ul",
        items: [
          "Une boucle d'arrière-plan interroge les slots de chaque coordinateur et enregistre la saturation par modèle.",
          "Tant qu'un modèle est saturé, sa **cible effective de répliques d'experts** est relevée (base + boost). Le marché de couverture relit alors les experts déjà couverts comme de nouveau rares, et un modèle **sans** worker actif est amorcé à partir de ses métadonnées GGUF (nombre d'experts), afin que la demande soit visible même depuis zéro.",
          "Les nœuds inactifs interrogent le marché de la demande (`/api/expert-volunteer`) et reçoivent une tranche `(layer, expert-range)` à servir. Ils téléchargent la tranche, appellent le relay et enregistrent leur couverture ; le hub les câble automatiquement dans la carte de dispatch du coordinateur.",
          "Quand la charge s'épuise, la cible retombe et la demande disparaît, si bien que les workers supplémentaires ne reçoivent plus de dispatch et s'éteignent avec le temps.",
        ],
      },
      {
        t: "callout",
        md: "La conception est **basée sur le pull** : les nœuds demandent du travail au lieu de se le voir imposer, si bien qu'un worker derrière NAT participe sans aucune connectivité entrante. Un nœud recruté en Couche 2 qui se met à servir devient un worker **éprouvé** que la Couche 1 maintient ensuite engagé sous charge — les deux couches se composent en une seule boucle élastique.",
      },
      {
        t: "p",
        md: "**Observabilité :** `GET /api/moe/recruitment` rapporte le busy/saturation par modèle et la cible base vs effective ; `/api/expert-demand` porte un drapeau `recruiting`.",
      },
    ],
  },
};
