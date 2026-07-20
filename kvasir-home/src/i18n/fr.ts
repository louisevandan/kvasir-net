/* Français — traduit à partir du dictionnaire source anglais (en.ts).
   Termes techniques et identifiants conservés tels quels (KVR, linkcpp,
   moteur d'inférence, GPU, CPU, NPU, OpenAI, Anthropic, Solana, GGUF, MoE, SIWS, 2FA,
   TOTP, ring runtime, MIT, tok/s, layer, Qwen3.5-122B, etc.). Le cadrage
   d'honnêteté est préservé (devnet, jeton utilitaire, pas un investissement,
   non-dépositaire). */
import type { Dict } from "./types";

export const fr: Dict = {
  nav: {
    why: "Pourquoi",
    how: "Comment ça marche",
    contributors: "Contributeurs",
    developers: "Développeurs",
    token: "KVR",
    rewards: "Récompenses",
    tech: "Technologie",
    roadmap: "Feuille de route",
    careers: "Recrutement",
    technology: "Technologie",
    blog: "Blog",
    wiki: "Wiki",
  },

  actions: {
    runNode: "Faire tourner un nœud",
    joinNode: "Rejoindre",
    useApi: "Utiliser l’API",
    getApiKey: "Obtenir une clé API",
    getApiAccess: "Obtenir un accès API",
    runNodeGuide: "Guide de mise en place d’un nœud",
    readDocs: "Lire la documentation",
    viewGithub: "Voir sur GitHub",
    github: "GitHub",
    menu: "Menu",
    copy: "copier",
    copied: "copié ✓",
    language: "Langue",
  },

  hero: {
    eyebrow: "DePIN · IA décentralisée — au-delà du monopole",
    headline1: "Apportez de la puissance de calcul.",
    headline2: "Gagnez des KVR.",
    sub: "Kvasir répartit de grands modèles ouverts sur du matériel partagé grâce à linkcpp, de sorte qu’aucun nœud ne détient le modèle entier. Contribuez avec un GPU, un CPU, un NPU — même un téléphone — et gagnez des KVR pour les layers que vous exécutez.",
    badges: [
      "Fonctionne sur GPU · CPU · NPU · téléphone",
      "Compatible OpenAI + Anthropic",
      "Source disponible (BSL)",
      "Solana devnet",
    ],
    ringCenter: "un seul anneau · aucun maître",
    topologyCaption:
      "Un anneau d’appareils — un GPU, un CPU, un NPU et un téléphone — chacun détenant quelques-uns des 49 layers. Chaque nœud exécute sa tranche et ne transmet à son voisin que la frontière de l’état caché ; le dernier renvoie le token autour de l’anneau. Aucun nœud ne détient le modèle entier, et il n’y a pas de maître central — à titre illustratif.",
  },

  thesis: {
    eyebrow: "Pourquoi une IA décentralisée",
    title: "L’IA ne devrait pas appartenir à une poignée d’entreprises",
    lede: "L’inférence de pointe se concentre derrière quelques centres de données fermés — poids fermés, accès facturé à l’usage, une seule facture payée à un seul propriétaire. Kvasir prend le chemin inverse : des modèles ouverts servis sur un réseau sans permission d’appareils du quotidien, possédé et gagné par ceux qui le font tourner.",
    centralizedLabel: "IA centralisée",
    centralizedPoints: [
      "Quelques hyperscalers possèdent les GPU",
      "Modèles et infrastructure derrière des API fermées",
      "Vous louez l’accès ; la valeur remonte vers le haut",
      "Opaque — vous faites confiance à l’opérateur",
    ],
    kvasirLabel: "Kvasir",
    kvasirPoints: [
      "N’importe quel appareil rejoint un anneau pair-à-pair — aucun maître central",
      "Moteur linkcpp à source disponible — sous licence BSL et entièrement inspectable",
      "Les contributeurs gagnent des KVR pour le calcul réel qu’ils fournissent",
      "Non-dépositaire — vos clés, votre nœud, vos récompenses",
    ],
  },

  origin: {
    eyebrow: "Le nom",
    title: "Kvasir — une sagesse née de tous, partagée avec tous",
    mythLabel: "Mythologie nordique",
    myth: "Dans la mythologie nordique, Kvasir naquit lorsque les Ases et les Vanes mirent fin à leur guerre et firent la paix : chacun cracha dans un même vase, et de leur essence réunie s’éleva l’être le plus sage qui ait jamais vécu — celui qui pouvait répondre à n’importe quelle question. Lorsqu’il fut tué, son sang fut brassé en l’Hydromel de la poésie, un breuvage qui accordait la sagesse à quiconque en buvait.",
    whyLabel: "Pourquoi nous l’avons choisi",
    mappings: [
      {
        from: "Rassemblé de chaque dieu, possédé par aucun",
        to: "Une intelligence assemblée à partir des appareils de nombreux contributeurs — aucun propriétaire unique.",
      },
      {
        from: "L’être le plus sage, répondant à toute question",
        to: "Un réseau d’inférence ouvert que chacun peut interroger.",
      },
      {
        from: "Un hydromel qui partageait la sagesse avec tous",
        to: "Un accès ouvert, et des récompenses KVR pour chaque contributeur qui y verse sa part.",
      },
    ],
    footnote: "Sur la chaîne, le jeton porte le nom de Kvasir (KVR) — l’hydromel, distribué.",
  },

  how: {
    eyebrow: "Comment ça marche",
    title: "Un modèle, de nombreux appareils, payé au layer",
    lede: "Aucun nœud ne détient le modèle entier. Une requête parcourt le chemin des layers et chaque nœud est récompensé pour exactement le travail qu’il a fourni.",
    steps: [
      {
        title: "Répartir",
        body: "Le modèle est divisé en fenêtres de layers contiguës. Chaque appareil stocke le même modèle mais ne charge que sa fenêtre — aucun nœud ne détient l’ensemble.",
        note: "Qwen3.5-122B · 49 layers · manifeste de rangs",
      },
      {
        title: "Servir",
        body: "La requête entre dans l’anneau. Chaque nœud exécute ses layers et ne transmet à son voisin que la frontière de l’état caché ; le dernier nœud échantillonne le token et le renvoie autour de l’anneau — aucun maître central.",
        note: "ring runtime · compatible OpenAI/Anthropic",
      },
      {
        title: "Récompenser",
        body: "Chaque nœud gagne des KVR pondérés par les layers qu’il a exécutés — sa part des tokens produits — réglés sur Solana vers le propre portefeuille de ce nœud.",
        note: "units += (out_tokens / 1000) × layer_share",
      },
    ],
  },

  contributors: {
    pill: "Pour les contributeurs",
    title: "Transformez le calcul inutilisé en KVR",
    lede: "Pointez un appareil compatible vers le réseau et il commence à servir des layers. Vous gagnez des KVR proportionnellement aux layers que votre nœud exécute — vos clés restent dans votre propre portefeuille.",
    nonCustodial:
      "Non-dépositaire par conception — la connexion de l’opérateur est une signature de portefeuille (Sign-In With Solana) avec 2FA optionnelle.",
    points: [
      {
        title: "N’importe quel appareil peut rejoindre",
        body: "Les GPU, CPU, NPU et téléphones exécutent déjà des layers aujourd’hui. Un ring runtime pair-à-pair permet à chaque appareil de ne détenir que quelques layers et de ne transmettre qu’un petit état de frontière à son voisin — donc aucun gardien, et aucun propriétaire unique.",
      },
      {
        title: "Récompenses au prorata des layers",
        body: "Les récompenses sont proportionnelles aux layers que votre nœud exécute, pas à une vague participation. 1 unité ≈ 1k tokens × votre part de layers de chaque inférence.",
      },
      {
        title: "Paliers de performance",
        body: "Le débit mesuré fixe un palier — S (×1.5), A (×1.25), B (×1.0), C (×0.7) — qui multiplie les unités que vous gagnez. Matériel plus rapide, multiplicateur plus élevé.",
      },
      {
        title: "Disponibilité pour les rôles d’infrastructure",
        body: "Les nœuds occupant des rôles d’infrastructure accumulent aussi des récompenses horaires de disponibilité pour maintenir le réseau accessible.",
      },
    ],
    devicesLabel: "Appareils pris en charge",
    statusLive: "Actif",
    statusComing: "À venir",
    deviceDetails: [
      "CUDA · ROCm · Metal · Vulkan",
      "x86-64 · ARM",
      "accélérateurs embarqués",
      "téléphones & edge · ring runtime",
    ],
  },

  developers: {
    pill: "Pour les développeurs",
    title: "Un seul endpoint, adossé à de nombreux appareils",
    lede: "Conservez votre client OpenAI ou Anthropic existant. Pointez-le vers la passerelle Kvasir et payez à l’inférence en KVR — sans réécriture.",
    points: [
      "Compatible OpenAI : prêt à l’emploi pour /v1/chat/completions, /v1/responses, /v1/models",
      "Compatible Anthropic : /anthropic/v1/messages et /anthropic/v1/models",
      "Paiement à l’inférence en KVR : devis → paiement → inférence",
      "Catalogue de modèles en direct agrégé depuis les hubs accessibles",
    ],
    codeHeader: "POST /v1/chat/completions",
  },

  token: {
    eyebrow: "Jeton & récompenses",
    title: "Le KVR paie le calcul — et le récompense",
    lede: "Le KVR est l’unité que les développeurs dépensent pour l’inférence et l’unité que les contributeurs gagnent pour les layers qu’ils exécutent. Les récompenses sont calculées à partir du travail réel, pas de la participation.",
    facts: [
      { k: "Symbole", v: "KVR", note: "nom on-chain « Kvasir », 6 decimals" },
      { k: "Chaîne", v: "Solana", note: "devnet aujourd’hui" },
      { k: "Paie pour", v: "L’inférence", note: "paiement à la requête via la passerelle" },
      { k: "Récompense", v: "Le calcul", note: "part de layers × palier de performance" },
    ],
    whatForTitle: "À quoi sert le KVR",
    whatForBody:
      "Un seul jeton, dans les deux sens : les développeurs dépensent des KVR pour lancer des inférences via la passerelle, et les contributeurs gagnent des KVR pour le calcul que fournissent leurs nœuds. C’est l’unité de compte du réseau pour le travail réel — voyez ci-dessous comment les récompenses se répartissent par rôle.",
    whatForChips: ["payer à l’inférence", "récompenser au layer", "régler sur Solana"],
    custodyTitle: "Portefeuille non-dépositaire",
    custodyBody:
      "Les récompenses sont réglées vers le propre portefeuille du propriétaire de chaque nœud. Les clés résident dans le portefeuille de l’utilisateur — navigateur, ordinateur ou mobile — jamais chez un opérateur. Vérifié sur quatre portefeuilles propriétaires distincts, chacun gagnant sa part de layers.",
    custodyChips: ["web", "desktop", "iOS", "Android"],
    devnetStrong: "Devnet, jeton utilitaire.",
    devnetBody:
      "Le KVR fonctionne actuellement sur Solana devnet et est un jeton utilitaire / de contribution — pas un actif négociable, un prix ou un investissement. Rien ici ne constitue un conseil financier ni une promesse de rendement.",
  },

  network: {
    eyebrow: "Réseau & récompenses",
    title: "Chaque rôle du réseau gagne des KVR",
    lede: "L’anneau de nœuds de calcul est coordonné par les rôles de hub et de passerelle. Chacun est payé en KVR pour ce qu’il fait réellement — du calcul pour les layers qu’il exécute, de l’infrastructure pour la disponibilité qu’il assure.",
    roles: [
      {
        role: "Nœud de calcul",
        tagline: "Exécute les layers du modèle",
        body: "Détient quelques layers contiguës dans l’anneau et les exécute pour chaque requête. Gagne par unité de contribution (≈1k tokens servis), pondérée par sa part de layers et mise à l’échelle par son palier de performance.",
        earns: "par unité × part de layers × palier",
      },
      {
        role: "Hôte de passerelle",
        tagline: "Point d’entrée public + règlement",
        body: "Sert la passerelle OpenAI/Anthropic et règle les paiements en KVR. Gagne une récompense horaire de disponibilité pour maintenir le point d’entrée en ligne, plus un bonus ×1.5 sur chaque inférence qu’il aide à servir.",
        earns: "disponibilité horaire + bonus d’inférence ×1.5",
      },
      {
        role: "Hôte de hub",
        tagline: "Le plan de contrôle",
        body: "Découvre les appareils, planifie le placement des layers et orchestre l’anneau. Le rôle le plus critique — il gagne donc la récompense horaire de disponibilité la plus élevée pour maintenir la coordination du réseau.",
        earns: "disponibilité horaire la plus élevée",
      },
    ],
    rolesNote:
      "Les rôles se cumulent : une même machine peut être à la fois calcul, passerelle et hub, et ses récompenses s’additionnent. Tout est réglé en KVR vers le propre portefeuille de ce nœud.",
    formulaTitle: "Comment les récompenses sont calculées",
    formulaLabels: ["Unités de calcul", "Effectif", "Disponibilité infra"],
    tiersTitle: "Paliers de performance",
    tiersBody:
      "La vitesse de décodage mesurée d’un nœud fixe son multiplicateur — un matériel plus rapide gagne proportionnellement plus pour le même travail.",
  },

  tech: {
    eyebrow: "Sous le capot",
    title: "linkcpp — le moteur derrière le réseau",
    lede: "linkcpp est le hub de contrôle ouvert qui transforme du matériel du quotidien en un moteur d’inférence distribué. Son ring runtime permet à chaque appareil de ne détenir que quelques layers et de transmettre l’état caché à son voisin — aucun maître central — tandis que le data plane standard de moteur d'inférence reste non forké.",
    taglineCaption: "— linkcpp, en ses propres mots",
    points: [
      {
        title: "Ring runtime",
        body: "Chaque appareil stocke le même modèle et ne charge que sa fenêtre de layers, puis ouvre un lien vers son prédécesseur et un vers son successeur. Les frontières de l’état caché circulent autour de l’anneau et le dernier rang renvoie le token — aucun maître central, aucun nœud ne détient tout.",
      },
      {
        title: "Hub de contrôle linkcpp",
        body: "Un unique hub dockerisé — le plan de contrôle qui manquait au data plane RPC de moteur d'inférence. Il découvre les appareils, planifie le placement des layers, lance les workers standards et expose les passerelles. Source disponible sous la Business Source License (BSL).",
      },
      {
        title: "Placement distribué des layers",
        body: "linkcpp lit les métadonnées GGUF et calcule des fenêtres de layers contiguës par nœud via un manifeste de rangs, avec un déchargement optionnel des FFN d’experts MoE vers la RAM du nœud.",
      },
      {
        title: "Sécurité SIWS + 2FA",
        body: "Pour les déploiements publics, l’accès de l’opérateur est une signature Sign-In With Solana sur un nonce serveur, plus une 2FA TOTP et des codes de secours à usage unique — sur le hub comme sur la passerelle.",
      },
    ],
    openText:
      "La source est disponible sous la Business Source License (BSL) — lisez-la, exécutez-la et construisez dessus gratuitement en développement et en test. L'usage en production (commercial) requiert une licence achetée.",
  },

  roadmap: {
    eyebrow: "Feuille de route",
    title: "Actif aujourd’hui, et vers où cela va",
    lede: "Une ligne claire entre ce qui fonctionne déjà et ce qui est prévu. Nous ne présentons pas la feuille de route comme livrée.",
    items: [
      {
        phase: "Maintenant",
        title: "Inférence sur n’importe quel appareil, en direct",
        body: "Les GPU, CPU, NPU et téléphones servent des layers à travers le ring runtime. Le 122B a tourné réparti sur 4 GPU ; la contribution est créditée de bout en bout ; les portefeuilles non-dépositaires sont livrés sur web/desktop/iOS/Android ; l’accès est sécurisé sur des domaines publics.",
      },
      {
        phase: "À venir",
        title: "Mainnet & règlement on-chain",
        body: "Tout fonctionne aujourd’hui sur Solana devnet avec un service de règlement hors chaîne. Un programme de récompenses on-chain et le mainnet sont prévus.",
      },
      {
        phase: "À venir",
        title: "Un réseau mondial sans permission",
        body: "Les démos actuelles ont tourné sur le matériel d’un seul opérateur. Ouvrir le réseau pour que n’importe qui, où qu’il soit, puisse brancher un appareil et gagner — sans gardien — est la prochaine étape.",
      },
    ],
  },

  proof: {
    pill: "Prouvé sur cette build",
    title: "Une véritable inférence distribuée, tournant sur des domaines publics",
    items: [
      "paramètres servis répartis sur 4 GPU AMD MI250",
      "portefeuilles propriétaires distincts gagnant chacun leur part de layers",
      "surfaces d’API — compatibles OpenAI + Anthropic",
      "plateformes de portefeuille — web · desktop · iOS · Android",
    ],
    strip:
      "122B servi sur 4 GPU · compatible OpenAI + Anthropic · portefeuilles sur web / desktop / iOS / Android · en direct sur des domaines publics",
  },

  footer: {
    ctaTitle: "Mettez votre GPU sur le réseau.",
    ctaBody:
      "Faites tourner un nœud et gagnez des KVR pour les layers que vous servez, ou branchez la passerelle sur votre application avec un endpoint compatible OpenAI/Anthropic.",
    tagline:
      "La marque réseau de l’inférence IA décentralisée, propulsée par le hub de contrôle linkcpp — un moteur à source disponible (BSL) qui répartit de grands modèles sur des appareils du quotidien (sur un data plane standard moteur d'inférence).",
    disclaimerStrong: "Avertissement.",
    disclaimer:
      "Le KVR est un jeton utilitaire / de contribution utilisé pour payer l’inférence et récompenser le calcul. Il fonctionne aujourd’hui sur Solana devnet — ce n’est pas un actif mainnet négociable et rien ici ne constitue une offre, un prix ou une promesse de rendement financier. Les récompenses reflètent le calcul réel apporté, pas la participation.",
    rights: "© 2026 Kvasir · linkcpp. Moteur sous la Business Source License (BSL) — gratuit pour le développement et les tests ; l'usage en production requiert une licence.",
  },

  guide: {
    home: "Accueil",
    eyebrow: "Guide de l'opérateur de nœud",
    headline1: "Apportez de la puissance de calcul,",
    headline2: "exécutez un nœud.",
    sub: "Créez un portefeuille, mettez des KVR en jeu (stake), puis connectez votre appareil au réseau Kvasir et gagnez des KVR pour la puissance de calcul que vous apportez. Choisissez votre plateforme ci-dessous pour les étapes de téléchargement, d'installation et d'exécution.",
    badgeCustody: "Non dépositaire — vos clés",
    badgeDevices: "GPU · CPU · NPU",
    badgeToken: "Solana devnet · KVR",
    devnetNote: "KVR est un token utilitaire Solana devnet — pas un actif mainnet négociable ni un rendement financier.",
    reqTitle: "Exigence pour opérateur de hub · gateway",
    reqBody: "Pour exécuter un nœud hub ou un nœud gateway, vous devez mettre en jeu 100 000 KVR dans votre portefeuille. Les nœuds de calcul classiques rejoignent le réseau sans cette exigence et gagnent des récompenses pour les couches qu'ils exécutent.",
    tabDesktop: "Ordinateur",
    tabMobile: "Mobile",
    soon: "Bientôt disponible",
    download: "Télécharger",
    desktopTitle: "Kvasir Wallet · Application de bureau",
    desktopSub: "macOS · Windows · Linux — portefeuille et nœud dans une seule application.",
    desktop: [
      { title: "Téléchargez l'application", body: "Téléchargez l'installateur de Kvasir Wallet pour votre système d'exploitation ci-dessus. Un GPU (NVIDIA / AMD / Apple Silicon) est recommandé, mais le CPU fonctionne aussi.", body2: "" },
      { title: "Installez et ouvrez", body: "Exécutez l'installateur, puis ouvrez Kvasir Wallet. Sur macOS, si vous voyez un avertissement « développeur non identifié », autorisez-le dans Réglages Système → Confidentialité et sécurité.", body2: "" },
      { title: "Créez votre portefeuille", body: "Choisissez Créer un nouveau portefeuille. Notez votre phrase de récupération de 12 mots et conservez-la en lieu sûr — elle ne peut pas être récupérée si elle est perdue. Définissez ensuite une phrase de passe pour déverrouiller l'application. Les clés sont non dépositaires et stockées uniquement sur cet appareil.", body2: "" },
      { title: "Alimentez et mettez en jeu des KVR", body: "Recevez des SOL devnet (pour les frais) et des KVR (à mettre en jeu) à l'adresse de réception de votre portefeuille. Dans le panneau de staking du tableau de bord, saisissez un montant et cliquez sur Mettre en jeu pour gagner des intérêts APR et devenir éligible aux récompenses de nœud.", body2: "" },
      { title: "Configurez le nœud", body: "Dans Paramètres du nœud, choisissez le backend de calcul de cette machine (CUDA / ROCm / Metal / CPU) et sélectionnez Fragment local (recommandé) — il exécute le fragment de couches localement et ne relaie que le petit état de frontière, le mode le plus rapide.", body2: "" },
      { title: "Exécutez le nœud", body: "Activez Exécuter le nœud (en direct) pour enregistrer cette machine sur le réseau sous votre portefeuille (propriétaire) et la mettre en ligne.", body2: "Pour un véritable nœud de calcul GPU, exécutez également l'agent natif ci-dessous. Le planificateur du hub place les couches du modèle sur votre machine, et votre nœud gagne une part de KVR par couche, créditée au portefeuille propriétaire." },
      { title: "Suivez la contribution et les récompenses", body: "Dans Statut des nœuds, surveillez nœuds / en ligne / contribution effective / réclamable. Les nœuds sont classés par niveau selon leur débit (S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7) ; brut × niveau = effectif. Utilisez Réclamer les récompenses pour transférer les KVR accumulés vers votre portefeuille.", body2: "" },
    ],
    faucetTitle: "Obtenir des SOL devnet (faucet gratuit)",
    faucetIntro: "Vous avez besoin d'un peu de SOL devnet pour les frais de transaction (utilisez l'adresse de réception de votre portefeuille) :",
    faucetWeb: "Web : faucet.solana.com — collez votre adresse et sélectionnez le réseau Devnet",
    faucetCli: "CLI : solana airdrop 2 <your address> --url devnet",
    faucetAlt: "Alternatives : QuickNode · SolFaucet devnet",
    faucetKvr: "Obtenez des KVR à mettre en jeu via la distribution ou l'échange (échange KVR : SOL/ETH ↔ KVR — bientôt disponible).",
    mobileTitle: "Kvasir Wallet · Application {0}",
    mobileSub: "Créez un portefeuille et connectez votre appareil au réseau.",
    mobile: [
      { title: "Installez l'application", body: "Installez Kvasir Wallet depuis {0}. Utilisez le bouton ci-dessus pour ouvrir la page du store. Un appareil récent doté d'un GPU/NPU est recommandé.", note: "" },
      { title: "Créez / restaurez un portefeuille", body: "Ouvrez l'application et choisissez Créer un nouveau portefeuille ou Restaurer depuis une phrase de récupération. Conservez votre phrase de 12 mots en lieu sûr et définissez une phrase de passe — le même compte peut être restauré sur ordinateur et sur d'autres appareils à partir de cette phrase. Les clés sont non dépositaires, stockées uniquement sur l'appareil.", note: "" },
      { title: "Configurez le nœud", body: "Dans Paramètres du nœud mobile, choisissez un backend de calcul (GPU · OpenCL/Vulkan · CPU) et Fragment local (recommandé). Le débit attendu (tok/s) ainsi que l'impact sur la mémoire / la thermique / les performances sont affichés.", note: "" },
      { title: "Mise en jeu et récompenses", body: "Dans Mise en jeu et récompenses de nœud, mettez des KVR en jeu et consultez / réclamez les récompenses réclamables accumulées par votre nœud. Le statut du nœud affiche votre niveau de performance et votre contribution.", note: "La participation à l'inférence par fragment local sur mobile est en cours de déploiement ; aujourd'hui, les principaux nœuds de calcul sont des machines GPU/CPU exécutant l'agent." },
    ],
    viewGithub: "Voir sur GitHub",
    capWelcome: "Bienvenue — créez ou restaurez un portefeuille",
    capRecovery: "Enregistrez votre phrase de récupération de 12 mots (mots flous)",
    capPassphrase: "Définissez une phrase de passe → Commencer",
    capReceive: "Recevoir — adresse et QR (adresse partiellement masquée)",
    capBalances: "Solde du portefeuille — KVR · SOL",
    capStaking: "Staking — APR · capital · intérêts · récompenses de nœud",
    capBackend: "Backend de calcul (CUDA · ROCm · Metal · CPU)",
    capMode: "Mode du nœud — Fragment local (recommandé)",
    capRunlive: "Exécuter le nœud (en direct) — jauges en direct · ID du nœud · OS",
    capNodes: "Statut des nœuds — totaux · niveaux · contribution par nœud",
    capClaim: "Réclamer les récompenses — KVR réclamables",
    capWallet: "Accueil du portefeuille — solde KVR (adresse masquée)",
    capNodeset: "Paramètres du nœud — backend / mode / ressources attendues",
    capStakingM: "Mise en jeu et récompenses d'opérateur de nœud",
  },

  techBlog: {
    docTitle: "Kvasir — Blog technique",
    pill: "Blog technique",
    title: "L’ingénierie de l’essaim",
    lede: "Notes de conception et jalons vérifiés sur du matériel réel, issus de la construction de l’inférence en essaim à experts fragmentés sur linkcpp — comment un modèle de 122B tourne à travers GPU, CPU et téléphones.",
    langNote: "",
    sidebarTitle: "Parcourir les articles",
    allArticles: "Tous les articles",
    read: "Lire",
    notFound: "Cet article n’existe pas.",
    categories: {
      overview: "Vision et architecture",
      core: "Technologie de cœur",
      milestones: "Jalons",
      demos: "Démos sur appareil réel",
    },
  },

  wiki: {
    docTitle: "Kvasir — Wiki",
    pill: "Wiki",
    title: "La base de connaissances Kvasir",
    lede: "Des entrées courtes et précises sur chaque concept du réseau — du ring runtime et du sharding d’experts aux récompenses KVR.",
    langNote: "",
    sidebarTitle: "Parcourir les entrées",
    allEntries: "Toutes les entrées",
    notFound: "Cette entrée n’existe pas.",
    categories: {
      network: "Réseau et rôles",
      inference: "Inférence et moteur",
      token: "Jeton et récompenses",
    },
  },

  apiDocs: {
    docTitle: "API Kvasir — inférence au paiement à l'usage avec KVR",
    pill: "Pour les développeurs · devnet",
    title: "Appelez l'inférence Kvasir, payée en KVR",
    lede: "Les modèles de l'essaim Kvasir ne sont pas exposés à l'internet ouvert. Le seul point d'entrée public est la passerelle de paiement à l'usage en KVR : chaque inférence est déverrouillée par un paiement KVR on-chain signé par votre portefeuille. Voici tout le flux, dans le langage que vous utilisez.",
    devnetNote: "Fonctionne sur le devnet de Solana — KVR n'est pas un actif réel. Avant de commencer, approvisionnez un portefeuille devnet en KVR et un peu de SOL pour les frais.",
    baseLabel: "URL de base",
    flowTitle: "Quatre étapes",
    flowSteps: [
      { n: "1", title: "Découvrir un modèle", body: "Demandez à la passerelle quels modèles l'essaim sert en ce moment. La liste est en direct : ne codez rien en dur." },
      { n: "2", title: "Obtenir un devis", body: "Envoyez l'id du modèle et votre prompt. Vous recevez un requestId lié à ce prompt et un prix en KVR." },
      { n: "3", title: "Payer on-chain", body: "Transférez les KVR devisés vers le compte de jetons du destinataire et signez avec votre portefeuille. Conservez la signature." },
      { n: "4", title: "Échanger", body: "Renvoyez le requestId et la signature. La passerelle vérifie le paiement, exécute l'inférence et renvoie le résultat." },
    ],
    refTitle: "Référence de l'API",
    requestLabel: "Requête",
    responseLabel: "Réponse",
    apiModels: "Liste les modèles que l'essaim sert en ce moment — un tableau vide quand il n'y en a aucun, donc ne codez jamais un id en dur.",
    apiQuote: "Obtenez un devis de prix et un requestId lié à votre prompt. priceToken est la quantité de KVR à payer ; la facturation finale se base sur l'usage réel de jetons.",
    apiPay: "Transférez les KVR devisés vers le compte de jetons associé du destinataire (le vault) et signez avec votre portefeuille. La signature est à usage unique.",
    apiInfer: "La passerelle interroge la chaîne pour vérifier le paiement, exécute l'inférence sur le hub, puis renvoie le résultat ainsi que l'usage et le coût réels.",
    codeTitle: "Exemple de bout en bout",
    codeLede: "Chargez la clé secrète de votre portefeuille depuis l'environnement, devisez, payez et échangez — un extrait autonome. Les étapes 1, 2 et 4 sont du HTTP pur ; seule l'étape 3 (le transfert SPL) diffère selon le SDK.",
    adapterTitle: "Adaptateur compatible OpenAI",
    adapterLede: "Vous avez déjà un client OpenAI (ou un outil qui ne parle qu'OpenAI) ? Lancez cet adaptateur prêt à l'emploi à côté de votre application. Il expose /v1/chat/completions et paie chaque appel depuis votre propre portefeuille — devis, signature, échange — en coulisses. Pointez la base URL de votre client vers l'adaptateur et utilisez n'importe quelle clé d'API factice.",
    adapterNote: "Non custodial : le secret du portefeuille (KVR_SECRET_KEY) reste dans ce processus et n'atteint jamais Kvasir. Il n'existe pas de clé d'API Kvasir — l'authentification est le paiement KVR on-chain que votre adaptateur signe. Chaque appel est un aller-retour devis/paiement/échange ; mettez en cache ou groupez selon votre débit.",
    prereqTitle: "Avant de commencer",
    prereqs: [
      "Un portefeuille devnet Solana qui détient des KVR (pour payer) et un peu de SOL (pour les frais).",
      "Le mint KVR a 6 décimales — le montant on-chain est round(priceToken × 1 000 000).",
      "La destination est le compte de jetons associé du destinataire ; s'il n'existe pas encore, votre transfert doit le créer (cela coûte un peu de SOL).",
    ],
    securityTitle: "Règles de sécurité appliquées par la passerelle",
    security: [
      "Chaque signature de transaction est à usage unique — la rejouer renvoie 409.",
      "L'inférence est protégée par le requestId et la signature à usage unique, alors gardez le requestId privé — seul son émetteur doit l'échanger.",
      "Le vault doit recevoir au moins priceToken KVR, sinon la requête est rejetée.",
      "Cas d'échec : requestId inconnu (404), signature déjà utilisée (409), transaction pas encore confirmée (400).",
    ],
    walletTitle: "Créer un portefeuille de test",
    walletLede: "Pas encore de portefeuille devnet ? Générez une paire de clés, affichez son secret pour votre environnement et approvisionnez-le en SOL pour les frais — puis demandez des KVR au faucet ci-dessous.",
    walletNote: "Gardez le secret hors du contrôle de version et chargez-le depuis une variable d'environnement. Devnet uniquement — ne réutilisez jamais une clé de test sur le mainnet. Vous pouvez aussi créer un portefeuille dans l'app Kvasir et copier son adresse.",
    faucetTitle: "Obtenir des KVR de test",
    faucetLede: "Collez une adresse devnet Solana pour recevoir 100 KVR — de quoi essayer le flux ci-dessus. Une demande par adresse et par jour.",
    faucetPlaceholder: "Votre adresse devnet Solana",
    faucetButton: "Demander 100 KVR",
    faucetSending: "Envoi…",
    faucetSuccess: "{0} KVR envoyés vers votre portefeuille",
    faucetViewTx: "Voir la transaction",
    faucetError: "Impossible d'envoyer les KVR",
    ctaTitle: "Construire sur Kvasir",
    ctaBody: "Le même contrat /api/pay alimente les portefeuilles de bureau, iOS et Android de Kvasir. Lisez l'implémentation de référence et le code source de la passerelle sur GitHub.",
    ctaButton: "Voir sur GitHub",
    catRunNode: "Lancez un nœud → inférence gratuite",
    catUseApi: "Utiliser l'API",
    selfHostTitle: "Lancez un nœud, obtenez de l'inférence gratuite",
    selfHostPitch: "Envie d'utiliser des modèles d'IA gratuitement ? Demandez à votre agent de code de relier votre machine au réseau en tant que nœud — et de vous rendre un endpoint d'inférence.",
    selfHostBody: "Un script démarre le hub (et, en option, la passerelle KVR) avec Docker. Ajoutez votre GPU et chargez un modèle ouvert dans l'UI du hub, puis appelez un endpoint standard compatible OpenAI — /c/<id>/v1/chat/completions — qui tourne sur votre propre matériel. Pointez-y n'importe quel outil qui parle OpenAI.",
    selfHostNote: "Cela sert gratuitement les modèles ouverts que votre machine peut héberger — c'est votre calcul. Pour les modèles de pointe trop gros pour une seule machine, rejoignez l'essaim : c'est à cela que sert l'API KVR au paiement à l'usage ci-dessous.",
    inferenceApiTitle: "API d'inférence (crédits)",
    inferenceApiLede: "Le chemin le plus simple : un endpoint OpenAI natif avec une clé d'API. Le streaming (SSE) et les appels d'outils natifs fonctionnent d'emblée, et chaque appel est déduit d'un solde KVR prépayé — pas de signature du portefeuille à chaque appel. L'accès est contrôlé par une liste blanche de portefeuilles.",
    inferenceApiSteps: [
      { title: "Approvisionner des crédits", body: "Obtenez du crédit KVR via une allocation de l'opérateur, ou déposez vous-même : transférez des KVR vers la trésorerie et soumettez la signature à POST /api/credits/deposit." },
      { title: "Obtenir une clé d'API", body: "Prouvez une fois la propriété du portefeuille (SIWS) : demandez un challenge, signez le message, échangez-le contre une clé. L'apiKey n'est renvoyée qu'une seule fois — conservez-la." },
      { title: "L'appeler comme OpenAI", body: "Pointez n'importe quel client OpenAI vers l'URL de base avec votre clé. Le streaming et tool_calls fonctionnent sans changement ; le solde est débité à chaque appel." },
    ],
    inferenceApiKeyLede: "Émettez une clé d'API (une seule signature du portefeuille)",
    inferenceApiCallLede: "Ensuite, appelez-la avec le SDK OpenAI standard — seuls base_url et la clé changent",
    inferenceApiRefTitle: "Référence",
    inferenceApiRef: {
      base: "URL de base", auth: "Authentification", endpoints: "Endpoints", balance: "Solde",
      pricing: "Tarification", errors: "Erreurs", context: "Contexte max", model: "Modèle",
    },
    inferenceApiThinkNote: "Le modèle servi raisonne par défaut. Pour des réponses courtes ou des appels d'outils, définissez chat_template_kwargs.enable_thinking = false ; laissez le raisonnement activé (préférable pour le codage) avec un max_tokens généreux. Les appels d'outils fonctionnent toujours. Le tarif actuel est fixé par la gouvernance et peut changer.",
    keyIssueTitle: "Émettez une clé ici",
    keyIssueLede: "Vous préférez ne pas le scripter ? Exécutez tout le flux directement — saisissez votre portefeuille, signez chaque challenge et obtenez une clé. Les mêmes endpoints que ci-dessus ; votre clé ne quitte jamais votre navigateur.",
    keyIssueWalletPh: "Votre adresse de portefeuille Solana",
    keyIssueLabelPh: "Libellé de la clé (par ex. my-app)",
    keyIssueStart: "Démarrer",
    keyIssueRegisterNote: "Ce portefeuille n'est pas encore enregistré — signez une fois pour vous auto-enregistrer, puis à nouveau pour obtenir la clé.",
    keyIssueSignPrompt: "Signez ce message exact avec votre portefeuille (ed25519), puis collez la signature base64 :",
    keyIssueSigPh: "signature base64",
    keyIssueSubmit: "Soumettre la signature",
    keyIssuePending: "Ce portefeuille n'est pas sur la liste blanche et l'auto-enregistrement est fermé. Un opérateur doit l'approuver — merci de nous contacter.",
    keyIssueKeyReady: "Votre API key — affichée une seule fois. Copiez-la maintenant.",
    keyIssueBalanceLabel: "Solde de crédit",
    keyIssueTopUp: "Le solde est de 0 — approvisionnez du crédit KVR (transférez des KVR vers la treasury, puis POST /api/credits/deposit) avant de passer des appels.",
    keyIssueError: "Échec de la requête",
    keyIssueUseNote: "Utilisez-la maintenant : c'est une clé Bearer pour https://gate.kvasir-ai.net/v1. Lancez la commande prête à l'emploi ci-dessous, ou pointez n'importe quel SDK OpenAI sur cette base URL (exemples complets plus bas).",
    selfIssueTitle: "Auto-émission d'une clé (agents)",
    selfIssueLede: "Un agent de code peut tout faire sans interface à partir d'un secret de portefeuille : signer les challenges SIWS, s'auto-enregistrer, obtenir une clé, puis appeler l'endpoint OpenAI. Sans navigateur, sans clic.",
    gateTitle: "Mettez une étoile pour débloquer les docs",
    gateBody: "Les docs restent ouverts — une étoile GitHub vous tient simplement informé et aide le projet à grandir. Connectez-vous avec GitHub et mettez une étoile au dépôt pour continuer.",
    gateSignIn: "Se connecter avec GitHub",
    gateStarBody: "Connecté en tant que {0}. Mettez une étoile au dépôt sur GitHub, puis revérifiez pour débloquer.",
    gateStarLink: "Étoiler louisevandan/kvasir-net ↗",
    gateRecheck: "C'est fait — revérifier",
  },

  careers: {
    docTitle: "Marketing & Croissance — Kvasir",
    pill: "Nous recrutons",
    headline1: "Marketing & Croissance",
    headline2: "faites grandir le réseau",
    sub: "Kvasir est un réseau décentralisé d’inférence IA (DePIN) sur Solana. Le moteur open source linkcpp répartit de grands modèles ouverts sur de nombreux GPU et machines contribués, et chaque nœud gagne des KVR pour les couches qu’il a réellement servies. La technique fonctionne déjà — il nous faut la personne qui le fera savoir au monde.",
    factRole: "Rôle",
    factRoleV: "Marketing & croissance — temps plein",
    factLocation: "Lieu",
    factLocationV: "À distance · fuseau États-Unis/Europe ou Asie du Sud-Est · ≥3–4 h de chevauchement quotidien avec KST",
    factComp: "Rémunération",
    factCompV:
      "Equity early-stage (vesting 4 ans / cliff 1 an) + allocation de tokens conditionnée au TGE · essai rémunéré avant tout engagement",
    factEngine: "Moteur",
    liveTitle: "Ce qui tourne déjà",
    liveLede: "Vous ne rejoignez pas un whitepaper. Vérifié et en production aujourd’hui :",
    liveProof: [
      "Modèles testés sur le réseau : Qwen3.5 122B, Qwen3.5 35B et Gemma4 12B — chacun découpé couche par couche sur plusieurs machines, aucun nœud ne détient le modèle entier.",
      "Une flotte hétérogène en production de 21 nœuds : 4× AMD MI250 (hôte ARM), 4× NVIDIA GB10, 4× NVIDIA RTX Pro 6000, 1 MacBook Pro, 6 machines x86 Windows CPU et 2 nœuds mobiles (iOS + Android).",
      "Comptabilisation de la contribution par nœud : chaque nœud gagne des KVR pondérés par sa part de couches sur chaque inférence servie, réglés sur son propre portefeuille.",
      "Passerelle de paiement à l’inférence compatible OpenAI et Anthropic, en production sur notre propre domaine.",
      "Portefeuilles non dépositaires livrés sur web, desktop, iOS et Android, avec connexion par signature de portefeuille (Sign-In With Solana) + 2FA.",
    ],
    devnetNote:
      "KVR fonctionne actuellement sur le devnet Solana. C’est un jeton d’utilité/de contribution — rien sur cette page ne constitue une offre de titres ni une promesse de valeur du jeton.",
    ownsTitle: "Ce dont vous serez responsable",
    ownsLede:
      "Un réseau biface exige une croissance biface : opérateurs de nœuds côté offre, développeurs côté demande. Les deux partent de zéro — c’est le poste.",
    owns: [
      {
        title: "Communauté & réseaux sociaux",
        body: "Construire X et Discord à partir de zéro. Développer les relations avec les KOL de la niche DePIN / crypto-IA et tenir un rythme de contenu régulier.",
      },
      {
        title: "Croissance des opérateurs de nœuds",
        body: "Acquisition côté offre : toucher les possesseurs de GPU et les communautés home-lab, et mener des campagnes qui les convertissent en nœuds Kvasir actifs.",
      },
      {
        title: "Demande développeurs",
        body: "Marketing côté demande auprès des développeurs et startups IA qui ont besoin d’endpoints d’inférence compatibles OpenAI/Anthropic — contenus proches de la doc, billets de lancement, vitrines d’intégrations.",
      },
      {
        title: "Campagnes & analytics",
        body: "Concevoir des expériences de croissance, les mesurer honnêtement et doubler la mise sur les canaux qui font vraiment bouger le nombre de nœuds et l’usage de l’API.",
      },
      {
        title: "Soutien lancement & partenariats",
        body: "Soutenir le marketing du lancement du token quand le réseau quittera le devnet, et appuyer la prospection de partenariats (flottes de GPU, portefeuilles, fournisseurs de modèles).",
      },
    ],
    profileTitle: "Qui nous cherchons",
    profile: [
      "Marketeur crypto-natif : vous avez fait grandir une communauté ou un produit web3 à partir de zéro — vérifiable sur X, Discord ou on-chain.",
      "Une bonne connaissance de DePIN ou de la crypto-IA est un vrai plus ; vous savez expliquer à un possesseur de GPU pourquoi faire tourner un nœud.",
      "Anglais natif ou courant ; fuseau États-Unis/Europe ou Asie du Sud-Est avec ≥3–4 h de chevauchement quotidien avec KST (UTC+9).",
      "À l’aise avec une rémunération early-stage : un equity significatif + un potentiel token plutôt qu’un gros salaire.",
      "Exécutant de terrain — vous publiez vous-même posts, campagnes et expériences.",
    ],
    processTitle: "Notre processus",
    processLede:
      "Chaque candidat passe par un essai rémunéré avant toute discussion d’equity — cela protège les deux parties.",
    process: [
      {
        title: "Premier appel",
        body: "Nous vous présentons le réseau en production et la feuille de route ; vous nous présentez une communauté ou une campagne que vous avez réellement construite.",
      },
      {
        title: "Essai rémunéré (2–4 semaines)",
        body: "Un vrai travail rémunéré — p. ex. un plan d’acquisition d’opérateurs de nœuds avec calculs par canal, ou une expérience de croissance en direct sur X/Discord. Nous jugeons le rendu, la vitesse et l’autonomie.",
      },
      {
        title: "Offre",
        body: "Marketing & Croissance : equity avec vesting standard de 4 ans (cliff d’1 an) plus une allocation de tokens conditionnée au TGE ; une base en cash dès que le financement arrive.",
      },
      {
        title: "Construire ensemble",
        body: "Premier jalon en équipe : co-construire notre prochaine participation à un hackathon et faire grandir la première cohorte d’opérateurs de nœuds.",
      },
    ],
    applyTitle: "Comment postuler",
    applyBody:
      "Envoyez par e-mail une courte présentation avec des liens qui prouvent le profil ci-dessus — la communauté ou la campagne que vous avez construite, votre handle X, tout élément on-chain. Le CV est optionnel ; les preuves ne le sont pas.",
    applyCta: "Postuler",
    readCode: "Lire le code d’abord",
    seeProduct: "Voir le produit",
  },
};
