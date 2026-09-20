/* Français — traduction des entrées du wiki. La structure (slug, catégorie,
   ordre des blocs, code) reflète exactement entries.ts (source anglaise) ;
   les termes techniques et identifiants (KVR, p4, bridge, GGUF, MoE,
   stage server, tok/s, etc.) restent tels quels. Le cadrage de conformité
   (devnet, jeton utilitaire, non-dépositaire) est préservé. */
import type { WikiTranslation } from "./entries";

export const frWiki: Record<string, WikiTranslation> = {
  "node-relay": {
    title: "Relais de nœud",
    summary: "Une adresse publique détenue au nom d'une machine qui n'en a pas, afin qu'un nœud derrière un NAT soit joignable sans ouvrir le moindre port.",
    blocks: [
      { t: "p", md: "Un **relais de nœud** donne à la machine d'un contributeur une adresse que le réseau peut appeler. p4 distribue le travail en ouvrant une connexion *vers* un nœud, et une machine domestique derrière une traduction d'adresses n'en possède aucune. Le relais en détient une publiquement, le nœud maintient une unique connexion sortante vers lui, et le travail appelé sur l'adresse publique redescend par la connexion que le nœud tient déjà." },
      { t: "p", md: "Aucune des deux extrémités de p4 n'apprend l'existence du relais. L'appelant voit une adresse ordinaire ; l'agent du nœud reste lié à `127.0.0.1` et n'écoute rien d'autre." },
      { t: "h2", text: "Pourquoi un tunnel plutôt qu'un port redirigé" },
      { t: "p", md: "p4 ne porte aucune authentification : tout hôte capable d'atteindre le port d'un agent peut envoyer `NODE_LOAD`, `NODE_UNLOAD` ou `INSPECT`. Rediriger un port de sa box vers cela exposerait la machine à quiconque la découvre. Derrière un relais, le nœud n'écoute rien et prouve un portefeuille d'opérateur avant que sa connexion ne transporte quoi que ce soit, avec le même schéma de signature que la passerelle de règlement : joignabilité et authentification sont résolues par le même mécanisme." },
      { t: "h2", text: "Ce qu'il ne fait pas" },
      { t: "ul", items: [
        "**Il ne lit pas le trafic.** Les charges passent octet par octet et ne sont jamais analysées ; le relais ne peut donc distinguer une commande d'une autre — et ne le doit pas, car comprendre le trafic le rendrait capable de le modifier.",
        "**Il n'ordonnance rien.** Le placement reste au plan de l'opérateur ; pour le relais, un nœud est une adresse, rien de plus.",
        "**Il ne prouve aucun travail.** Les octets transitant par un relais ne disent rien de l'inférence effectuée et ne comptent jamais comme contribution.",
      ] },
      { t: "h2", text: "Voir aussi" },
      { t: "p", md: "**Agent p4**, le processus qu'exécute la machine d'un contributeur, et **opérateur de nœud**, le portefeuille qu'un relais authentifie avant d'accorder une adresse." },
    ],
  },
  "kvasir-network": {
    title: "Réseau Kvasir",
    summary: "Un réseau d'inférence IA décentralisé (DePIN) où des appareils du quotidien servent des modèles ouverts et gagnent des KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** est un réseau d'inférence IA décentralisé : les grands modèles ouverts sont répartis sur du matériel partagé grâce au moteur **p4**, de sorte qu'aucun nœud n'a besoin de détenir le modèle entier. Chacun peut apporter un GPU, un CPU, un NPU — même un téléphone — et gagner des **KVR** pour les couches ou experts que son appareil sert réellement. Les développeurs atteignent le réseau par des gateways compatibles OpenAI/Anthropic et paient à l'inférence.",
      },
      {
        t: "ul",
        items: [
          "**Moteur à source disponible** — p4 est sous Business Source License 1.1 (l'usage interne non monétisé est autorisé ; l'usage hébergé ou générateur de revenus requiert une licence commerciale) ; le plan de données llama.cpp en dessous reste proche de l'upstream et inspectable.",
          "**Wallet en auto-garde** — les clés ne quittent jamais l'appareil de l'utilisateur, et les récompenses sont versées sur le wallet Solana du propriétaire de chaque nœud. Sur le devnet, les KVR stakés et les crédits prépayés sont détenus par la trésorerie du gateway et suivis dans son registre jusqu'à la mise en service d'un programme de staking on-chain.",
          "**Prouvé sur du matériel réel** — un modèle de 122B a tourné de bout en bout sur 3 machines physiques de notre flotte de test, avec la contribution de chaque nœud créditée de bout en bout.",
          "**Nommé d'après le mythe nordique** — Kvasir, l'être le plus sage, né de l'essence commune de tous les dieux et propriété d'aucun.",
        ],
      },
      { t: "h2", kick: "Une requête, de nombreux appareils", text: "Le parcours d'une inférence" },
      {
        t: "code",
        caption: "Chaque saut est du HTTP/TCP ordinaire ; c'est le modèle lui-même qui est distribué.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Les rôles se **cumulent** : une même machine peut être à la fois nœud de calcul, hôte de gateway et hôte de bridge, et ses récompenses s'additionnent. Le travail du réseau est de faire ressembler l'ensemble à une seule machine — un endpoint devant, des milliers d'appareils imparfaits derrière.",
      },
      {
        t: "p",
        md: "Aujourd'hui le réseau tourne sur **Solana devnet** ; KVR est un jeton utilitaire / de contribution, pas un actif négociable ni un investissement, et rien sur cette page ne constitue un conseil financier.",
      },
    ],
  },
  architecture: {
    title: "Architecture de Kvasir",
    summary: "Une seule carte de tout le système : les wallets, le gateway qui encaisse, le bridge qui donne un visage au moteur, et le réseau p4 qui exécute le modèle.",
    blocks: [
      {
        t: "p",
        md: "Kvasir, ce sont quatre couches et une couture chacune. Les **wallets** détiennent les clés. Le **gateway** encaisse le paiement et tient le registre. Le **bridge** donne un visage HTTP au moteur d'inférence. Le **réseau p4** exécute réellement le modèle. Tout ce qui suit découle de l'endroit où tombent ces coutures — et le schéma distingue ce qui tourne aujourd'hui de ce qui n'est encore qu'une conception.",
      },
      { t: "h2", kick: "Wallets", text: "Les clés ne quittent jamais l'appareil" },
      {
        t: "p",
        md: "iOS (Swift), Android (Kotlin) et desktop (React + Electron) sont des builds distincts du même wallet, et c'est le build desktop que le gateway sert aussi sur `/` comme wallet de navigateur — un wallet complet, avec signature dans la page, pas une console en lecture seule. Les récompenses sont versées sur l'adresse Solana propre à chaque propriétaire ; le gateway ne détient jamais une clé d'utilisateur.",
      },
      { t: "h2", kick: "Gateway", text: "Un seul processus, deux surfaces" },
      {
        t: "p",
        md: "`solana/staking-service` est à la fois le **gateway d'API** (un `/v1/chat/completions` compatible OpenAI, plus le flux de paiement à la requête `/api/pay/quote` → `/api/inference`) et le **gateway de règlement** (staking, registre des nœuds, comptes de crédit, crédit de contribution). Les deux ne font qu'un seul processus parce qu'ils partagent un seul registre : une requête n'est servie qu'une fois son transfert de KVR vérifié on-chain, et c'est ce même registre qui crédite les nœuds qui l'ont servie.",
      },
      {
        t: "callout",
        md: "**Le paiement se règle avant que l'inférence ne s'exécute.** Si le bridge échoue ensuite, le gateway rembourse le payeur depuis la trésorerie et renvoie un 502 plutôt que de facturer du vide. Il n'y a derrière ni modèle factice ni catalogue de remplissage : un modèle que l'app propose est un modèle qu'un bridge sert réellement, sinon la liste est vide.",
      },
      { t: "h2", kick: "Bridge", text: "Le visage HTTP du moteur" },
      {
        t: "p",
        md: "Le bridge (`p4bridge`) est un **OUTER** au sens de p4 : il installe une session à travers les stages, soumet au stage de tête et recueille le flux de tokens. Pour le gateway, c'est un contrat petit et figé — quels modèles sont chargés, qui a contribué combien, et les complétions.",
      },
      {
        t: "table",
        head: ["Route", "Ce qu'elle répond"],
        rows: [
          ["`/api/controllers`", "quels modèles sont chargés, et l'état de chaque stage"],
          ["`/api/runtime`", "le wallet de l'opérateur et les machines derrière lui"],
          ["`/api/contributions`", "les lignes par nœud, unités, requêtes, débit"],
          ["`/c/<model>/v1/chat/completions`", "l'inférence"],
        ],
      },
      {
        t: "p",
        md: "Deux tâches que p4 laisse délibérément au bridge : **le template de conversation** (p4 transmet au stage server un prompt opaque et n'en applique aucun, si bien qu'un modèle instruct poursuivrait votre texte au lieu d'y répondre) et **le bloc de raisonnement** (renvoyé comme `reasoning_content`, séparé de `content`, pour qu'une passe de réflexion ne puisse pas dévorer silencieusement le budget de tokens et facturer au payeur une réponse vide).",
      },
      {
        t: "callout",
        md: "**Le bridge n'est jamais publié.** Sa seule authentification est un service token partagé, et tout ce qui l'atteint peut faire tourner l'anneau. Il écoute sur la loopback ; le tunnel est la porte.",
      },
      { t: "h2", kick: "Réseau p4", text: "Les agents possèdent les nœuds, les stage servers détiennent les couches" },
      {
        t: "p",
        md: "Un **agent** possède les nœuds d'un hôte ; un **stage server** est un processus détenant une tranche des couches du modèle. Un stage passe son résultat au suivant en demandant à son propre agent de composer vers l'agent de ce stage **à l'adresse que cet agent annonce** — l'adresse annoncée doit donc être joignable depuis les autres hôtes, et devrait désigner le réseau le plus rapide qu'ils partagent. Sur le rack MI250, c'est le lien InfiniBand, pas le LAN du bureau, et jamais la loopback.",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` et `p4_staged_server` forment une seule release.** Un agent compilé depuis un arbre plus récent échoue au READY sur une capacité absente du HELLO — après avoir chargé le modèle entier.",
          "**Le placement est un artefact d'opérateur.** Quelles couches se posent sur quel GPU, sous quelle load generation, vient d'un plan de placement ; le bridge répond `409` à qui lui demande de servir, et le watchdog du gateway le signale une fois puis cesse de demander.",
          "**Un pipeline exige au moins deux stages.** La commande de session refuse un pipeline à un seul stage.",
        ],
      },
      { t: "h2", kick: "Relais", text: "Une adresse composable pour un portable" },
      {
        t: "p",
        md: "Les nœuds de bordure — une app desktop, un téléphone — n'ont aucune adresse que l'on puisse composer. Le **relais** leur en donne une : le nœud se connecte vers l'extérieur, prouve la paire de clés de son wallet sur un challenge ed25519, et devient dès lors joignable à travers le relais. Le relais est la frontière d'authentification et n'analyse jamais les payloads. L'installeur desktop embarque l'agent p4 avec l'app : rejoindre le réseau n'est pas une seconde installation.",
      },
      { t: "h2", kick: "Règlement", text: "Le crédit suit la participation" },
      {
        t: "p",
        md: "Chaque stage rapporte les lignes de tokens qu'il a exécutées. Le bridge les accumule par nœud, et le gateway interroge `/api/contributions` toutes les 30 secondes et crédite le wallet que le bridge désigne, à raison de `rows / 1000` unités mises à l'échelle par le niveau de performance du nœud. **Dans un pipeline, tous les stages voient les mêmes lignes** : un anneau à quatre stages paie donc ses quatre stages à égalité, quel que soit le nombre de couches détenues par chacun — le crédit suit la participation, pas la part de poids. Le sharding d'experts, où les nœuds détiennent des fractions différentes d'une même couche, est le cas qui obligera à revoir cela.",
      },
      { t: "h2", kick: "P4 Studio", text: "Ce que le schéma marque comme proposé" },
      {
        t: "p",
        md: "**P4 Studio** est la console d'opérateur propre à p4. Le flux d'observabilité par requête qu'elle attend des agents est une proposition en amont, pas quelque chose qui tourne ici — c'est pour cela que le schéma le dessine en pointillés, aux côtés des shards d'experts servis depuis des nœuds de bordure, conçus mais pas encore en service.",
      },
    ],
  },
  bridge: {
    title: "Bridge",
    summary: "Le visage HTTP du moteur d'inférence : ce qui est chargé, qui a contribué, et les complétions — et rien d'autre.",
    blocks: [
      {
        t: "p",
        md: "Le **bridge** est la seule chose à laquelle le gateway de règlement s'adresse pour l'inférence. C'est un **OUTER** au sens de p4 : il installe une session à travers les stages du modèle, soumet une requête au stage de tête, recueille le flux de tokens et rapporte ce que chaque nœud a contribué. Il ne possède ni placement, ni ordonnancement, ni autre état qu'un catalogue de ce qui est chargé — délibérément petit, car tout ce qu'il ne décide pas est autant qui ne peut pas dériver.",
      },
      { t: "h2", kick: "Le contrat", text: "Quatre routes, un seul jeton" },
      {
        t: "table",
        head: ["Route", "Ce qu'elle répond"],
        rows: [
          ["`/api/controllers`", "quels modèles sont chargés, et l'état de chaque stage"],
          ["`/api/runtime`", "le wallet de l'opérateur et les machines derrière lui"],
          ["`/api/contributions`", "les lignes par nœud, unités, requêtes, débit"],
          ["`/c/<model>/v1/chat/completions`", "l'inférence"],
        ],
      },
      {
        t: "p",
        md: "Toutes les routes sauf `/api/health` exigent un service token partagé, envoyé dans `X-Kvasir-Service-Token`. Ce jeton est la **seule** chose qui sépare l'Internet ouvert d'un usage gratuit de l'anneau : c'est pourquoi le bridge écoute sur la loopback et s'atteint par un tunnel plutôt qu'en étant publié.",
      },
      { t: "h2", kick: "Ce que p4 lui laisse", text: "Deux tâches que le moteur ne fera pas" },
      {
        t: "ul",
        items: [
          "**Le template de conversation.** p4 transmet au stage server un prompt opaque et n'applique aucun format de tour qui lui soit propre. C'est le bridge qui rend celui du modèle — lu dans le GGUF et nommé `prompt_format` dans le catalogue. Sautez-le et un modèle instruct poursuit votre texte au lieu d'y répondre, n'émet jamais son token de fin de tour, et court jusqu'à la limite de tokens à chaque fois.",
          "**Le bloc de raisonnement.** Un modèle de raisonnement ouvre sa réponse en réfléchissant. Le bridge renvoie cela comme `reasoning_content`, séparé de `content`, et honore `enable_thinking: false` en fermant le bloc dans le prompt — sans quoi une longue passe de réflexion peut consommer tout le budget et rendre à l'appelant une réponse vide qu'il a déjà payée.",
        ],
      },
      { t: "h2", kick: "Le placement n'est pas son affaire", text: "Pourquoi il répond 409" },
      {
        t: "p",
        md: "Demander au bridge de servir un modèle renvoie **409**. Quelles couches se posent sur quel GPU, sous quelle load generation, vient d'un plan de placement qu'un opérateur a écrit et chargé ; il n'y a aucun rechargement à distance à effectuer. Le watchdog d'anneau du gateway l'apprend une fois et cesse de demander, plutôt que de réessayer une chose qui ne peut pas marcher.",
      },
      {
        t: "callout",
        md: "**Les compteurs de contribution vivent en mémoire.** Un redémarrage du bridge perd tout ce que le gateway n'avait pas encore relevé — il interroge toutes les 30 secondes — et le gateway rétablit une nouvelle base plutôt que de compter deux fois quand un compteur recule. Un nœud dont le bridge ignore le propriétaire est sauté **silencieusement** : un wallet d'opérateur non renseigné se lit donc « ces machines n'ont rien gagné ».",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Le point d'entrée public : APIs compatibles OpenAI/Anthropic et règlement du paiement à l'inférence en KVR.",
    blocks: [
      {
        t: "p",
        md: "Le **gateway** est l'endroit où les développeurs rencontrent le réseau. Chaque contrôleur expose des endpoints compatibles OpenAI (`/v1/chat/completions`, `/v1/models`) et Anthropic (`/anthropic/v1/messages`, `/anthropic/v1/models`), tous adossés au même modèle chargé — un client existant fonctionne en ne changeant que la base URL et la clé.",
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
        md: "L'usage se règle en KVR par un flux en trois étapes — **quote → payment → inference** — ainsi une requête est tarifée avant de s'exécuter et les nœuds qui l'ont servie sont crédités ensuite. Le gateway agrège aussi un **catalogue de modèles en direct** depuis chaque bridge joignable, de sorte que `/v1/models` reflète ce que le réseau peut réellement servir à l'instant.",
      },
      {
        t: "ul",
        items: [
          "Les hôtes de gateway gagnent une **récompense horaire de disponibilité** pour maintenir le point d'entrée en ligne, plus un **bonus ×1.5** sur chaque inférence qu'ils aident à servir.",
          "Exploiter un gateway public exige un staking de **100 000 KVR** (comme un bridge).",
          "Les déploiements publics protègent l'accès opérateur avec **SIWS + 2FA** ; un bridge nu est conçu pour hôte de confiance / LAN / VPN uniquement.",
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
          "Les nœuds s'enregistrent sous le wallet de leur propriétaire ; les récompenses y sont versées. Quatre wallets de propriétaires distincts, chacun gagnant sa part de couches, ont été vérifiés de bout en bout sur la flotte de test d'un seul opérateur.",
          "Les données de capacités (backend, précision d'accumulation, budgets de ressources) déterminent ce que le planner peut placer sur un nœud — et, dans l'essaim, quels rangs il peut servir.",
          "Un nœud qui ne peut fournir de supervision de ressources est exclu du chargement adaptatif plutôt que d'être cru sur parole.",
        ],
      },
    ],
  },
  p4: {
    title: "p4",
    summary: "Le moteur derrière Kvasir : un protocole adressé par événements où les agents possèdent les nœuds, les stage servers détiennent les couches, et le placement est ce qu'un opérateur énonce plutôt que ce que le réseau devine.",
    blocks: [
      {
        t: "p",
        md: "**p4** exécute un même modèle sur plusieurs machines en le découpant en **stages** — des tranches contiguës de ses couches — et en donnant à chaque stage son propre processus. Un **agent** possède les nœuds d'un hôte : il lance les stage servers, route les événements entre eux et répond de leur cycle de vie. Aucun ordonnanceur ne décide où vont les choses ; un opérateur écrit un plan de placement, le charge, et le réseau sert alors exactement cela.",
      },
      {
        t: "code",
        caption: "Le chemin d'une requête à travers un déploiement p4.",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "Adressage", text: "Un stage compose vers l'agent du stage suivant" },
      {
        t: "p",
        md: "Quand un stage termine ses couches, il passe le résultat au stage suivant en demandant à son propre agent d'ouvrir une connexion vers **l'agent de ce stage, à l'adresse que cet agent annonce**. L'adresse annoncée n'a donc rien de cosmétique : elle doit être joignable depuis tous les autres hôtes de l'anneau, et devrait désigner le réseau le plus rapide qu'ils partagent. Annoncez la loopback et un anneau à deux hôtes se compose silencieusement lui-même.",
      },
      { t: "h2", kick: "Cycle de vie", text: "Un seul nombre relie tout un chargement" },
      {
        t: "ul",
        items: [
          "**La load generation est choisie par celui qui charge** et comparée pour égalité exacte à chaque session, inférence, règlement et déchargement. Elle n'est consignée nulle part sur les machines : le chargeur l'écrit donc sur disque *avant* que la première commande ne parte — sans elle, un modèle chargé ne peut même pas être démonté.",
          "**La generation d'un nœud et la load generation sont le même nombre.** L'adaptateur compare la generation source d'un reçu de libération à celle du chargement auquel il appartient et arrête le nœud quand elles diffèrent : un anneau chargé avec deux nombres différents sert une requête, puis perd sa tête.",
          "**Un journal opérationnel est exigé** avant qu'un modèle ne se charge tout court : c'est le registre d'admission qui rend un chargement rejouable sans danger, pas une aide au débogage.",
        ],
      },
      { t: "h2", kick: "Ce qu'il ne fait pas", text: "Des omissions délibérées" },
      {
        t: "p",
        md: "p4 n'applique **aucun template de conversation** — il transmet un prompt opaque et attend de l'appelant qu'il ait rendu le format de tour du modèle. Il ne prend **aucune décision de placement**. Et il ne porte aucune notion de qui doit être payé : les stages rapportent les lignes de tokens qu'ils ont exécutées, et le règlement est le contrat de quelqu'un d'autre. Chacune de ces coutures, Kvasir la comble dans le [bridge](/wiki/bridge), ce qui garde le moteur assez étroit pour suivre l'upstream.",
      },
      {
        t: "callout",
        md: "**L'agent et le stage server natif forment une seule release.** Un agent compilé depuis un arbre plus récent échoue au READY sur une capacité absente du HELLO du stage server — après avoir chargé le modèle entier. Compilez les deux depuis le même checkout.",
      },
    ],
  },
  "in-flight-ring": {
    title: "Anneau in-flight",
    summary: "Un pipeline qui ne se vide jamais : plusieurs requêtes occupent des stages différents au même instant, si bien qu'aucun stage n'attend la fin de celle qui le précède.",
    blocks: [
      {
        t: "p",
        md: "Kvasir sert un modèle comme un **pipeline de stages**, chacun détenant une tranche contiguë de ses couches. Un stage exécute ses couches et passe la frontière — un hidden state, pas des poids — au suivant. Aucun stage ne détient le modèle entier, et rien ne se tient au milieu du chemin de données : le bridge soumet à la tête et lit à la queue, tandis que les stages se passent les résultats entre eux via leurs propres agents.",
      },
      { t: "h2", kick: "La partie « in-flight »", text: "Pourquoi un pipeline qui se vide gaspille l'essentiel de la machine" },
      {
        t: "p",
        md: "Si un pipeline termine une requête avant d'admettre la suivante, tous les stages sauf un sont inoccupés à chaque instant — un anneau à quatre stages tourne au quart de son matériel. La conception **in-flight** garde plusieurs requêtes en mouvement à la fois : pendant que le stage 3 décode une requête, le stage 0 en préremplit déjà une autre. Les stages rapportent combien de temps ils ont retenu un batch et combien de temps ils n'ont rien eu à soumettre : un anneau affamé ne ressemble donc pas à un anneau saturé.",
      },
      {
        t: "code",
        caption: "Quatre stages, trois requêtes, un seul instant.",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "Appartenance", text: "Ce qu'est exactement un batch" },
      {
        t: "p",
        md: "Des lignes issues de requêtes différentes sont empaquetées dans un même batch physique, et cette appartenance exacte est transmise à tous les stages en aval plutôt que redécidée à chaque saut. C'est ce qui permet à un prefill et à plusieurs décodages de partager une seule passe, et c'est pourquoi la taille d'un batch est une propriété du chargement : le plan énonce d'avance les largeurs de lignes et de micro-batch, et ces largeurs fixent le plus gros résultat qu'un stage puisse jamais renvoyer.",
      },
      {
        t: "callout",
        md: "**Un pipeline exige au moins deux stages.** Un pipeline à un seul stage est refusé net — la tête et la queue sont des rôles distincts, et un nœud unique qui les confond est un autre moteur, pas un anneau plus petit.",
      },
      {
        t: "p",
        md: "L'anneau est la voie de la **latence**, et sa granularité est la couche. Le sharding d'experts supprime ce plancher en coupant à l'intérieur d'une couche, et se branche sur le même tissu de service.",
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
        md: "Un modèle **Mixture-of-Experts** remplace la FFN unique de chaque couche par une banque de FFN expertes indépendantes plus un **routeur** qui en choisit quelques-unes par token. Step-3.7-Flash, le MoE de 428B en service aujourd'hui, compte 288 experts par couche avec un routage top-8. Qwen3.5-122B-A10B est l'exemple détaillé ci-dessous, car c'est celui dont les chiffres ont été mesurés de bout en bout :",
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
      {
        t: "callout",
        md: "**État du moteur.** Le sharding au grain de l'expert a été construit et démontré sur le moteur précédent de Kvasir, et les résultats ci-dessous proviennent de ce travail. Le moteur actuel, [p4](/wiki/p4), sert aujourd'hui au grain de la couche ; le portage du sharding d'experts vers lui est conçu et en cours. Quand un détail nomme un outil ou une route, c'est celui qui tournait sur le moteur précédent.",
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
        t: "callout",
        md: "**État du moteur.** Le sharding au grain de l'expert a été construit et démontré sur le moteur précédent de Kvasir, et les résultats ci-dessous proviennent de ce travail. Le moteur actuel, [p4](/wiki/p4), sert aujourd'hui au grain de la couche ; le portage du sharding d'experts vers lui est conçu et en cours. Quand un détail nomme un outil ou une route, c'est celui qui tournait sur le moteur précédent.",
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
        md: "**GGUF** est le format de modèle en fichier unique de l'écosystème moteur d'inférence : des métadonnées (architecture, nombre de couches, dimensions, quantification) plus les tenseurs en octets quantifiés bruts (p. ex. Q4_K_M). Un plan de placement s'écrit contre ces métadonnées — plages de couches, affectation aux appareils et estimations de taille ; le côté service découpe les octets de tenseurs pour produire les téléchargements.",
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
          "**Côté gain** — unités de contribution × part de couches × niveau de performance pour le calcul ; disponibilité horaire pour les rôles bridge/gateway.",
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
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "Les rôles se **cumulent** — une machine peut être calcul + gateway + bridge, et ses flux s'additionnent.",
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
    summary: "Staker 100 000 KVR qualifie un wallet pour opérer des nœuds bridge ou gateway.",
    blocks: [
      {
        t: "p",
        md: "Le staking verrouille des KVR pour qualifier un wallet aux rôles d'opérateur et aux récompenses de nœud. Exploiter un nœud **bridge** ou **gateway** exige un stake de **100 000 KVR** ; les nœuds de calcul ordinaires rejoignent sans aucun stake et gagnent pour les couches qu'ils exécutent.",
      },
      {
        t: "ul",
        items: [
          "Le staking se fait dans le panneau de staking du tableau de bord du wallet : saisissez un montant, **Stake**, et la position compte pour l'éligibilité opérateur et les récompenses de nœud.",
          "L'exigence de 100k est un **filtre d'engagement** pour les deux rôles dont dépend le trafic des autres — les points d'entrée et le plan de contrôle.",
          "Sur le devnet, les KVR stakés sont conservés dans le vault de staking ; le montant staké et les récompenses de nœud sont visibles dans le panneau de staking.",
          "Le KVR de devnet pour staker vient du faucet de distribution ; le SOL de devnet pour les frais vient du faucet public.",
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
        md: "Le Kvasir Wallet est **non-dépositaire par conception** : la phrase de récupération de 12 mots et les clés ne sont stockées que sur l'appareil de l'utilisateur, jamais chez un opérateur. Les récompenses se règlent sur Solana directement vers le wallet du propriétaire de chaque nœud — vérifié sur une flotte de test avec quatre wallets de propriétaires distincts, chacun gagnant sa propre part de couches. Le staking fonctionne différemment sur le devnet : les KVR stakés sont détenus dans la trésorerie du gateway et suivis dans son registre jusqu'à la mise en service d'un programme de staking on-chain.",
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
        md: "Pour les déploiements publics, l'accès opérateur au gateway s'authentifie par **Sign-In With Solana** : le wallet de l'opérateur signe un nonce émis par le serveur, prouvant la propriété sans mot de passe ni identifiant en dépôt. Par-dessus, la **2FA TOTP** et des codes de secours à usage unique protègent la session.",
      },
      {
        t: "ul",
        items: [
          "**Aucun mot de passe nulle part** — la clé du wallet est l'identité et le nonce empêche le rejeu ; il n'y a rien côté serveur à hameçonner ou à divulguer.",
          "**L'enrôlement TOTP par wallet** est persisté dans le registre du gateway : la 2FA survit donc à un redémarrage.",
          "**Les codes de secours sont à usage unique** — chacun se consomme à la connexion, pour récupérer quand l'appareil d'authentification est indisponible.",
          "**Périmètre annoncé honnêtement** — le bridge et les ports du moteur supposent un hôte de confiance / LAN / VPN ; SIWS + 2FA est la couche qui rend les domaines *publics* sûrs à exposer.",
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
        md: "Parce que l'inférence **doit** être payée en KVR, le jeton est lié à un usage réel — de l'utilité, pas de la spéculation. L'usage finance les KVR que les nœuds gagnent, ce qui garde la contribution attractive, ce qui accroît la capacité, ce qui abaisse le prix et la latence, ce qui attire plus d'usage. L'avantage le plus tranchant de Kvasir resserre encore la boucle : un participant peut être **consommateur et fournisseur à la fois** (un *prosommateur*), de sorte que les deux côtés grandissent souvent chez les mêmes personnes.",
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
        md: "Kvasir récompense déjà le **travail réel** (KVR par tokens servis × part de couches, pas la simple présence) et verse les KVR directement sur le wallet de chaque nœud, ce qui est la partie difficile pour rendre honnêtes des récompenses financées par les revenus. Le reste — un prix piloté par l'utilisation et une transition émission→revenus — est la feuille de route économique qui transforme « plus de nœuds → moins cher » d'une intuition en une règle imposée par le protocole. L'entrée **Tarification de l'inférence** couvre le côté prix ; **Unités de contribution** couvre comment le travail devient récompense.",
      },
    ],
  },
  "inference-pricing": {
    title: "Tarification de l'inférence",
    summary: "Ce qu'une inférence coûte en KVR aujourd'hui, pourquoi un réseau décentralisé est structurellement moins cher, et comment le prix est censé baisser à mesure que l'offre croît.",
    blocks: [
      {
        t: "p",
        md: "L'accès au réseau se fait en **paiement à l'inférence** : le gateway cote un prix en KVR pour votre requête, votre wallet le paie on-chain, et alors seulement l'anneau exécute le modèle. La tarification est une formule petite et transparente — un plancher par requête plus un tarif par token — cotée d'avance et réglée sur l'usage **réel** de tokens après génération.",
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
      {
        t: "callout",
        md: "**État du moteur.** Le sharding au grain de l'expert a été construit et démontré sur le moteur précédent de Kvasir, et les résultats ci-dessous proviennent de ce travail. Le moteur actuel, [p4](/wiki/p4), sert aujourd'hui au grain de la couche ; le portage du sharding d'experts vers lui est conçu et en cours. Quand un détail nomme un outil ou une route, c'est celui qui tournait sur le moteur précédent.",
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
          "**La récompense va au travail.** Le travail passé par le bridge s'accumule dans son registre de contributions ; le gateway crédite en delta des KVR vers votre **propre** wallet. Il vous faut une adresse de wallet pour être payé.",
        ],
      },
      {
        t: "callout",
        md: "Le worker parle le même protocole de dispatch qu'un GPU en datacenter — `(n_used, n_tokens, cur, sel) → experts` sur un unique flux de longue durée. Un worker à shard partiel se contente de fixer `n_used = 1`. C'est cette uniformité qui fait qu'un téléphone, une machine CPU et une carte Blackwell sont des membres interchangeables du même essaim.",
      },
    ],
  },
  "bridge-operations": {
    title: "Exploiter un bridge",
    summary: "Notes d'opérateur pour faire tourner le bridge et l'anneau derrière lui : charger un plan, survivre à un redémarrage, garder la contribution en mouvement, et tenir le moteur hors d'Internet.",
    blocks: [
      {
        t: "p",
        md: "Le **gateway** (point d'entrée public) et le **bridge** (visage du moteur) sont les deux services de longue durée qu'un opérateur maintient en bonne santé, avec derrière eux les agents p4 et leurs stage servers. Le moteur ne parle aucune authentification — il suppose que les machines capables de s'atteindre sont censées le faire — si bien que tout ce qui est public converge vers le gateway, et que le bridge s'atteint par un tunnel plutôt qu'en étant publié.",
      },
      { t: "h2", kick: "Chargement", text: "Un plan, et le nombre sous lequel il est chargé" },
      {
        t: "ul",
        items: [
          "**Le placement est un plan que vous écrivez**, pas une requête que vous formulez : le bridge répond `409` à qui lui demande de servir. Générez le plan, passez-le à blanc, puis chargez avec `--confirm`.",
          "**La load generation est écrite sur disque avant que la première commande ne parte.** Elle est choisie par le chargeur, vérifiée pour égalité exacte à chaque session et au déchargement, et consignée nulle part sur les machines — perdez-la et un modèle chargé ne peut même pas être démonté.",
          "**La generation de nœud est ce même nombre.** Chargez un anneau avec deux valeurs différentes et il sert exactement une requête avant que la tête ne s'arrête ; la session suivante reste bloquée à demi chargée. Utilisez une valeur neuve à chaque chargement, sinon un enregistrement laissé par une tentative ratée entre en collision avec elle.",
          "**L'agent refuse de charger sans son journal opérationnel.** C'est le registre d'admission qu'exige un chargement rejouable sans danger, pas un drapeau de débogage.",
        ],
      },
      { t: "h2", kick: "Survivre à un redémarrage", text: "Ce qui revient et ce qui ne revient pas" },
      {
        t: "ul",
        items: [
          "**Un redémarrage d'agent fait tomber ses nœuds.** Les stage servers n'existent qu'à l'exécution ; le modèle doit être rechargé depuis le plan. C'est la procédure de reprise, pas l'échec d'une procédure.",
          "**Le gateway ne le rechargera pas pour vous.** Son watchdog d'anneau remarque un modèle qui a cessé de servir, apprend du `409` du bridge que le placement est externe, le signale une fois, puis cesse de demander.",
          "**Les compteurs de contribution vivent dans la mémoire du bridge.** Le gateway interroge toutes les 30 s et crédite en delta ; un redémarrage ne perd que ce qui n'avait pas été relevé, et le gateway rétablit une nouvelle base plutôt que de payer deux fois quand un compteur recule.",
        ],
      },
      { t: "h2", kick: "Verrouillez-le", text: "Le moteur n'est pas exposé à Internet" },
      {
        t: "ul",
        items: [
          "Faites écouter le bridge sur la loopback et donnez-lui un service token. Sans ce jeton il n'authentifie personne, et tout ce qui l'atteint peut faire tourner l'anneau gratuitement — il le dit au démarrage plutôt que de vous le laisser découvrir plus tard.",
          "Les agents annoncent l'adresse que les autres agents composent. Utilisez le réseau le plus rapide que les hôtes partagent, jamais la loopback entre hôtes, et gardez ce réseau hors de l'Internet public.",
          "Si quelque chose doit se trouver sur une IP publique, rappelez-vous que les ports publiés par Docker sont **DNAT'd avant la chaîne INPUT**, donc une règle sur `dport` ne correspondra pas. Filtrez dans la chaîne `DOCKER-USER` sur le port de destination d'origine de conntrack (`--ctorigdstport`), et persistez avec un oneshot systemd ordonné `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Pièges", text: "Deux qui coûtent du temps réel" },
      {
        t: "ul",
        items: [
          "**Un wallet d'opérateur non renseigné se lit comme zéro gain.** Le gateway saute toute ligne de contribution sans propriétaire et ne journalise rien. Les nœuds ont l'air inoccupés alors qu'ils servent.",
          "**`pkill` correspond à sa propre ligne de commande.** `ssh host 'pkill -f server.js; ...'` tue le shell qui l'exécute. Mettez le motif dans un fichier de script plutôt que dans la commande distante, utilisez une classe de caractères (`server[.]js`), et souvenez-vous qu'un processus lancé en simple `node server.js` ne porte aucun chemin sur lequel filtrer — trouvez-le plutôt par son port d'écoute.",
        ],
      },
    ],
  },
  "wan-interconnect": {
    title: "Interconnexion WAN (optiques 200G)",
    summary: "Comment des sites de calcul se relient à 200 Gb/s à travers une salle, un campus ou une ville : quelle optique à quelle distance, quoi se branche où, et ce qu'il faut pour vraiment atteindre le débit nominal.",
    blocks: [
      {
        t: "p",
        md: "Quand deux sites ont tous deux des routes publiques, le plan de données de dispatch d'experts devrait être un **lien direct** — le relais est pour les bordures sans adresse à elles. Cette entrée est la recette concrète pour faire de ce lien direct un lien de classe 200 Gb/s avec des pièces de catalogue. Une règle organise tout : **la fibre est un verre neutre en vitesse ; la vitesse réside dans le module enfichable à chaque extrémité.**",
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
          "**Côté NIC** — les cartes de classe ConnectX-6/7 exposent des cages QSFP56 ; DAC/AOC/FR4/LR4/ER4 se logent tous directement dans la NIC. Un hôte de classe GB10 possède déjà deux ports QSFP 200 GbE embarqués, si bien qu'un lien entre deux sites ne requiert exactement qu'un seul câble et aucun nouveau matériel.",
          "**Côté switch** — les optiques cohérentes ZR+ sont au format QSFP-DD et ont leur place dans un switch ou un routeur ; la NIC du site rejoint alors ce switch à 200G via un court DAC. Utilisez ce palier quand le site distant est à des dizaines de kilomètres.",
          "**La fibre elle-même** — des paires LC duplex monomode standard (G.652), louées comme fibre noire au brin. Le même verre porte 100G aujourd'hui et 400G demain ; les mises à niveau sont un échange de module, jamais des travaux de génie civil.",
          "**Au-delà de ~120 km** — vous cessez d'acheter des pièces et commencez à louer une longueur d'onde à un opérateur ; la démarcation est une remise Ethernet sur votre switch.",
        ],
      },
      {
        t: "code",
        caption: "Trois montages de référence, du moins cher au plus cher.",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "Étape 3 · atteindre réellement 200G", text: "Le débit ligne est une configuration, pas un achat" },
      {
        t: "ul",
        items: [
          "Utilisez **RDMA (RoCE)** pour le flux de dispatch là où c'est disponible — les hôtes de classe GB10 alimentent la NIC via des liens PCIe scindés, et la pleine vitesse mesurée (~185–190 Gb/s) apparaît sous RoCE avec une topologie correctement mappée ; un chemin mal mappé plafonne près de la moitié du débit et un TCP simple non optimisé atterrit bien plus bas.",
          "Activez les **jumbo frames (MTU 9000)** de bout en bout et gardez `TCP_NODELAY` sur les sockets de dispatch (le bridge le règle déjà).",
          "Attendez-vous à *vérifier*, pas à supposer : lancez un perftest entre sites après chaque changement physique — la différence entre 95 et 190 Gb/s est invisible tant qu'elle n'est pas mesurée.",
          "Gardez le **relais 443 comme voie de repli** — la politique de composition est direct d'abord pour les pairs publics, relais pour le NAT. Le rôle du relais est la portée, le rôle du lien direct est la vitesse.",
        ],
      },
      {
        t: "p",
        md: "Pourquoi cela compte pour l'architecture : la latence de décodage est bornée par le temps d'aller-retour (~5 µs/km dans la fibre — de la physique, insensible à la bande passante), si bien qu'un gros tuyau achète **de la vitesse de prefill, du débit de dispatch par lots et une distribution quasi instantanée des tranches d'experts**, pas une latence par token plus basse. C'est exactement le rôle du palier site dans la conception à deux paliers : la capacité dans le palier gros tuyau, la portée dans le palier relais.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Mise à l'échelle adaptative à la charge",
    summary: "Le chemin de service MoE de Kvasir grandit et se contracte avec le trafic : le coordinateur remobilise les workers éprouvés en saturation, et le bridge recrute des nœuds inactifs en augmentant la demande d'experts — le tout basé sur le pull, si bien que les appareils derrière NAT participent aussi.",
    blocks: [
      {
        t: "p",
        md: "Le chemin de service MoE de Kvasir se met à l'échelle de façon élastique avec la charge, en deux couches coopérantes. Au repos, le coordinateur sert tout localement pour le chemin le plus rapide par token ; en saturation, les deux couches ci-dessous font grandir l'essaim — et le contractent de nouveau quand la pointe passe.",
      },
      {
        t: "callout",
        md: "**État du moteur.** Le sharding au grain de l'expert a été construit et démontré sur le moteur précédent de Kvasir, et les résultats ci-dessous proviennent de ce travail. Le moteur actuel, [p4](/wiki/p4), sert aujourd'hui au grain de la couche ; le portage du sharding d'experts vers lui est conçu et en cours. Quand un détail nomme un outil ou une route, c'est celui qui tournait sur le moteur précédent.",
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
      { t: "h2", kick: "Couche 2", text: "Côté plan de contrôle : recrutement adaptatif à la charge" },
      {
        t: "p",
        md: "Le plan de contrôle surveille chaque coordinateur MoE et fait grandir le pool de workers lorsque nécessaire :",
      },
      {
        t: "ul",
        items: [
          "Une boucle d'arrière-plan interroge les slots de chaque coordinateur et enregistre la saturation par modèle.",
          "Tant qu'un modèle est saturé, sa **cible effective de répliques d'experts** est relevée (base + boost). Le marché de couverture relit alors les experts déjà couverts comme de nouveau rares, et un modèle **sans** worker actif est amorcé à partir de ses métadonnées GGUF (nombre d'experts), afin que la demande soit visible même depuis zéro.",
          "Les nœuds inactifs interrogent le marché de la demande (`/api/expert-volunteer`) et reçoivent une tranche `(layer, expert-range)` à servir. Ils téléchargent la tranche, appellent le relay et enregistrent leur couverture ; le plan de contrôle les câble automatiquement dans la carte de dispatch du coordinateur.",
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
