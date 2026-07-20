/* Français — traduction du blog technique. La structure (slug, catégorie,
   ordre des blocs, code, positions des img) reflète exactement articles.ts
   (source anglaise) ; termes techniques, identifiants et chiffres conservés. */
import type { TechTranslation } from "./articles";

export const frTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "Inférence en essaim par sharding d'experts : la conception",
    dek: "86 % d'un MoE de 122B, ce sont 12 544 experts indépendants de 5.3 MB. Découpez le modèle à ce grain et un téléphone peut porter une part réelle de l'inférence de frontière.",
    blocks: [
      {
        t: "callout",
        md: "**La thèse :** 86 % du poids de Qwen3.5-122B, ce sont 12 544 experts de 5.3 MB mutuellement indépendants. Shardez au grain de l'expert et un appareil faible porte « 8–64 experts (42–340 MB) » au lieu d'« une couche de 1.4 GB » — exactement l'unité qu'un téléphone peut réellement tenir. Le MoE est le substrat naturel d'un essaim.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "Substrat · Qwen3.5-122B-A10B (Q4_K_M)", text: "Les poids sont déjà conditionnés en unités à la taille de l'essaim" },
      {
        t: "stats",
        items: [
          { n: "49", l: "couches" },
          { n: "256", l: "experts / couche" },
          { n: "8", l: "actifs / token" },
          { n: "5.3 MB", l: "un expert (Q4)" },
          { n: "12,544", l: "experts au total" },
          { n: "86%", l: "du poids dans les experts" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "dim expert = la plus externe" },
        ],
      },
      {
        t: "p",
        md: "L'indice d'expert est la **dimension la plus externe** de chaque tenseur MoE : chaque expert est donc une dalle contiguë alignée sur les blocs de quantification. Un mini-GGUF découpé par experts est une copie propre de plage d'octets — sans déquantification, sans ré-empaquetage.",
      },
      { t: "h2", kick: "Deux rôles", text: "Stage backbone × worker d'experts" },
      {
        t: "ul",
        items: [
          "**Stage backbone (nœud fort) :** attention + cache KV, toutes les norms, le **routeur**, l'expert partagé et le combine résiduel — toute la voie dense. Il garde aussi tous les experts résidents comme réplique de secours (offload en RAM), ce qui donne à l'essaim sa tolérance au churn.",
          "**Worker d'experts (un téléphone) :** pas un transformer. Ni attention, ni KV, ni sampler — une fonction pure `(hidden, local_ids) → out` faite de trois mat-muls, ne logeant que sa propre tranche d'experts. Elle tient dans n'importe quel budget, jusqu'au téléphone de 4 GB.",
        ],
      },
      {
        t: "code",
        caption: "Le point de coupe : le routeur tourne une fois sur le backbone, avec autorité.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Parce que le routeur tourne **exactement une fois** sur le backbone, chaque expert sélectionné est calculé exactement une fois par le nœud qui le possède. **Aucune approximation** — le sharding ne fait que déplacer l'endroit où se produisent les mat-muls.",
      },
      { t: "h2", kick: "Pas un nouveau sous-système", text: "L'essaim, c'est le marché de récompenses éprouvé, à grain plus fin" },
      {
        t: "p",
        md: "Kvasir opère déjà un marché autonome de rareté pour les shards de **couches**, vérifié sur appareils réels : un téléphone derrière NAT interroge la carte de demande, s'auto-inscrit sur le segment **le mieux récompensé**, ne télécharge partiellement que cette fenêtre, la charge sur son GPU Adreno et achève l'inférence en anneau — en gagnant des récompenses de contribution. Le sharding d'experts réutilise tout — carte de couverture, auto-inscription à récompense maximale, téléchargement partiel, récompenses par nœud — en ne changeant que l'unité de couverture : des *plages de couches* aux *(couche, plage d'experts)*.",
      },
      { t: "h2", kick: "Deux innovations déjà vérifiées sur appareil", text: "Participation à poids partiels + le relais 443" },
      {
        t: "ul",
        items: [
          "**Téléchargement partiel des poids guidé par la récompense :** les montages RPC/TP/PP classiques expédient le checkpoint complet à chaque rang, et un scheduler dicte le placement. Chez Kvasir, un nœud ne télécharge **que la tranche qu'il va calculer**, et choisit cette tranche **lui-même, à la récompense** — un mini-GGUF de stage de 254 MB contre le modèle complet de 77.6 GB. C'est ainsi qu'un téléphone de 4 GB rejoint un modèle bien plus grand que lui.",
          "**Plan de données par relais 443 :** la bordure Cloudflare limitée à 80/443 plus le NAT d'opérateur interdisent l'appel direct dans les deux sens. Un pont WebSocket par bordure avec un préambule de rôle d'1 octet permet aux **deux côtés d'appeler vers l'extérieur** (le téléphone n'ouvre aucun port entrant). L'atterrissage a exigé de corriger trois vrais bugs — accord d'empreinte de build, authentification de téléchargement par node-token, et un bug de longueur de trame `Int.ushr` de Kotlin qui corrompait silencieusement chaque trame ≥ 64 KiB (`ushr` n'utilise que les 5 bits bas du décalage ; `len ushr 56` devenait `len ushr 24`) — corrigé en passant aux décalages `Long`.",
        ],
      },
      { t: "h2", kick: "Le nœud honnête du problème", text: "Un tissu de débit, pas un décodeur basse latence" },
      {
        t: "p",
        md: "Le décodage, ce sont 49 couches en série, et un aller-retour internet par couche coûte 2.5–10 s par token. L'enjeu de l'essaim est donc de **servir des modèles que personne ne peut héberger seul**, mesuré en débit agrégé : le dispatch par lots amortit le RTT, le backbone tient un cache de hot-experts, et les requêtes sont routées vers les répliques proches. La voie basse latence reste à l'anneau en pipeline.",
      },
      { t: "h2", kick: "Feuille de route", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — offload des experts du backbone en RAM : faire tourner le 122B sur un coordinateur, sans chirurgie de graphe.",
          "**M1** — preuve expert-parallel sur un seul hôte : mini-GGUF par experts + runtime du worker + dispatch, logits exactement égaux au monolithique.",
          "**M2** — workers téléphone LAN + NAT calculant de vrais experts du 122B à travers le relais 443.",
          "**M3** — marché de couverture au grain de l'expert avec répliques et repli anti-churn.",
          "**M4** — débit : dispatch par lots + cache de hot-experts, tokens/s croissant avec le nombre de workers.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "Du plan au matériel : le vérifié, et la clé de voûte",
    dek: "Récapitulatif de la campagne de vérification — de la conception au cœur de M2, prouvés sur un vrai 122B — et l'unique pièce d'intégration qui déverrouille le reste.",
    blocks: [
      {
        t: "p",
        md: "Ces dernières semaines, les pièces difficiles et inédites de l'essaim d'experts ont été prouvées une à une sur un **Qwen3.5-122B** réel — ni simulé, ni taille jouet. Voici la piste de vérification jusqu'ici, et l'unique clé de voûte qui restait.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "La piste · tout vérifié sur le vrai 122B", text: "Ce qui a déjà atterri" },
      {
        t: "ul",
        items: [
          "**Conception (5 révisions depuis le plan)** — architecture EP, participation autonome à poids partiels, le relais, la spec du kernel worker, et l'équivalence numérique inter-backends codifiée comme technologie de cœur. L'autorité du routeur consignée comme invariant de cohérence.",
          "**M0 — offload des experts du backbone en RAM (planner vérifié) :** le 122B est *feasible* sur un seul coordinateur de 64 GB — 10 couches d'experts déchargées en RAM, VRAM 62.6 GiB / RAM 14.2 GiB, câblé via `--override-tensor`.",
          "**M1 — voie de données des tranches d'experts :** découpe mini-GGUF par expert (dalles ne[2], copie d'octets sans déquantification) + l'endpoint de téléchargement `/expert-shard`.",
          "**M1 — oracle numérique :** dispatch + combine == monolithique avec **max|Δ| = 3.6e-12** sur de vrais experts de layer-0 — le sharding est un regroupement exact de la même somme pondérée.",
          "**M1 — worker C++ sur matériel :** `linkcpp-expert-worker` (ggml/gguf pur) compilé et exécuté sur ROCm, **cosine 0.99995** vs l'oracle ; routeur → deux workers C++ → combine égale le monolithique à cosine 0.9997–0.9999.",
          "**Cœur de M2 — un téléphone calcule de vrais experts du 122B :** cross-build Android, exécuté sur un SM-S938N, **cosine 0.99992** vs l'oracle.",
          "**Numérique — matrice d'équivalence à 3 backends :** le même calcul 122B sur ROCm × ARM CPU du téléphone × numpy — ROCm↔téléphone cosine 0.99990, ROCm↔numpy 0.99996, téléphone↔numpy 0.99992. Tout équivalent, rien de bit-identique.",
        ],
      },
      { t: "h2", kick: "La clé de voûte", text: "Le dispatch du backbone, intégré au décodage en direct" },
      {
        t: "callout",
        md: "Chaque **composant** — tranches, workers, logique dispatch/combine, équivalence numérique, calcul sur téléphone — était vérifié sur appareil. Restait à les câbler **dans un vrai décodage moteur d'inférence** : un hook `build_moe_ffn` qui dispatch les experts vers leurs nœuds propriétaires en plein graphe. Il a fallu modifier le sous-module moteur d'inférence épinglé et plusieurs cycles compiler-vérifier. Une fois cette clé de voûte posée, **l'intégration du relais M2, le marché de couverture M3 et le débit par lots M4** s'ouvrent en séquence — tous dépendent de ce dispatch.",
      },
      {
        t: "p",
        md: "La clé de voûte a depuis atterri : les billets suivants sur M2, M3, M4 et la démo téléphone en direct sont les fruits de cette intégration précisément.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "Équivalence numérique entre backends hétérogènes",
    dek: "CUDA, ROCm, Adreno et les CPU ne s'accorderont jamais bit à bit. Que l'essaim produise malgré tout un modèle cohérent est une propriété conçue, pas de la chance.",
    blocks: [
      { t: "h2", kick: "La distinction clé", text: "Exact vs équivalent — deux propriétés différentes" },
      {
        t: "ul",
        items: [
          "**Au sein d'un backend — exact (3.6e-12) :** répartir les experts entre nœuds puis combiner, c'est la même somme pondérée regroupée ; la seule différence est l'ordre d'accumulation flottante. Vérifié par oracle.",
          "**Entre backends — équivalent (1e-3…1e-6) :** la même opération sur des matériels différents porte une erreur relative par op d'environ 1e-3–1e-6, jamais nulle. **L'essaim vit dans ce régime.**",
        ],
      },
      {
        t: "p",
        md: "« Exact », c'est ce que la décomposition garantit dans un seul appareil. « Équivalent », c'est ce que donne le matériel hétérogène. Le travail de l'essaim est d'empêcher l'équivalence de se composer en divergence.",
      },
      { t: "h2", kick: "Mesuré · vrai 122B, trois backends", text: "Pas une théorie — mesuré sur le matériel" },
      {
        t: "p",
        md: "Le même FFN d'experts de layer-0 de Qwen3.5-122B, calculé par `linkcpp-expert-worker` sur un MI250 (**ROCm**), l'**ARM CPU** d'un téléphone (SM-S938N) et une référence **numpy** x86 — mêmes entrées, mêmes poids, jeux d'instructions et ordres de réduction différents :",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Paire de backends", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU téléphone vs numpy (x86)", "1.4e-6", "0.99992"],
          ["GPU ROCm vs ARM CPU téléphone", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Trois jeux d'instructions, un seul calcul — chaque paire équivalente (cosine ≈ 0.9999), aucune paire bit-identique (Δ ≈ 1e-6). Les résidus sont petits **parce que l'autorité du routeur a épinglé les entrées et la sélection d'experts**.",
      },
      {
        t: "p",
        md: "Une exécution ultérieure sur du vrai matériel **NVIDIA GB10 Grace Blackwell** a clos la matrice sur le dernier backend : CUDA ↔ ROCm a atteint **cosine 1.0000000000** (max abs 3.5e-10, effectivement bit-identique, puisque les deux backends GPU partagent leurs sources de kernel), et CUDA ↔ Grace ARM CPU cosine 0.99975 — le même motif GPU↔CPU vu ci-dessus.",
      },
      { t: "h2", kick: "Pourquoi les backends diffèrent", text: "L'addition flottante n'est pas associative" },
      {
        t: "ul",
        items: [
          "**Ordre de réduction du matmul** — tensor cores, tuiles MFMA, workgroups OpenCL et lanes SIMD accumulent chacun dans des ordres et pavages différents.",
          "**Fusion FMA** — `a*b+c` arrondi une fois (FMA) ou deux, fusionné différemment selon le backend.",
          "**Précision d'accumulation** — stockage F16/BF16 avec accumulateurs F32 vs F16 (le plus grand levier de divergence).",
          "**Approximations transcendantes** — variantes polynomiales/tabulées d'exp (softmax), silu/sigmoid (swiglu), rsqrt (norms).",
          "**Voie dequant + matmul** — déquantifier-puis-multiplier vs kernels quantifiés fusionnés arrondissent les intermédiaires différemment.",
          "**Kernels non déterministes** — les réductions atomic/split-K peuvent varier d'une exécution à l'autre sur le même appareil.",
        ],
      },
      { t: "p", md: "Rien de tout cela n'est un bug. C'est le prix que paie la voie rapide de chaque accélérateur." },
      { t: "h2", kick: "Pourquoi ça marche quand même", text: "Une autorité pour les décisions, assez de précision pour l'accumulation" },
      {
        t: "callout",
        md: "**AUTORITÉ DU ROUTEUR — l'invariant central.** La seule décision discrète du réseau est le routage MoE (top-8 sur 256). Si chaque backend rejouait le routeur, les tokens limites choisiraient **des experts différents** et divergeraient pour de bon. Kvasir exécute le routeur **une fois, sur le backbone**, et n'envoie aux workers que les ids des experts choisis. Un essaim hétérogène peut différer dans la *grandeur* de la sortie de chaque expert — jamais dans *quels experts tournent*. Cela convertit une divergence discrète catastrophique en erreur continue bornée : c'est la règle de cohérence du sharding hétérogène d'experts.",
      },
      {
        t: "ul",
        items: [
          "**Argmax discret :** décoder, c'est un argmax sur les logits. Un frémissement de 1e-3 ne bascule un token que lorsque deux candidats sont à moins de 1e-3 — à la plupart des positions la marge est bien plus grande, donc **les tokens sortent identiques** ; les rares bascules sont des positions aussi ambiguës qu'une autre graine.",
          "**Le combine est une addition :** les résultats partiels fusionnent en **somme** pondérée par probabilité. Des erreurs indépendantes d'environ 1e-4 s'ajoutent de façon incohérente — elles croissent en √k, pas en k — et sans annulation de grandes valeurs, le résidu reste bien conditionné.",
        ],
      },
      { t: "h2", kick: "Où ça peut casser · et les règles qui l'empêchent", text: "Modes de divergence et défenses" },
      {
        t: "table",
        head: ["Mode de divergence", "Mécanisme", "Règle"],
        rows: [
          ["Désaccord de routage", "Les backends choisissent des top-8 différents pour les tokens limites", "Autorité du routeur — décidé une fois sur le backbone, ids expédiés"],
          ["Bifurcation de trajectoire", "Le frémissement des logits finit par basculer un token ; la séquence bifurque comme une nouvelle graine", "Décodage/échantillonnage épinglés à un nœud"],
          ["Accumulation en profondeur", "49 couches × ~1e-4 chacune → jusqu'à 1e-2 sur les logits finaux", "Accumulation F32 aux frontières et au combine"],
          ["Auto-non-déterminisme", "Les kernels atomic/split-K varient d'une exécution à l'autre", "Kernels de combine déterministes ; la vérification utilise des tolérances"],
          ["Désaccord de précision", "Un nœud accumule en F16, un autre en F32", "Précision d'accumulation annoncée comme capacité ; nœuds F32 préférés pour les rangs de sortie"],
        ],
      },
      { t: "h2", kick: "L'équivalence est un nombre", text: "Le protocole de mesure" },
      {
        t: "ul",
        items: [
          "**Delta par op** — mêmes entrées, erreur relative A vs B sur matmul, swiglu, softmax, norm.",
          "**Dérive à la frontière de couche** — delta du résiduel après une couche, empilé pour voir si la profondeur accumule en √L ou en L.",
          "**Divergence des logits de bout en bout** — L∞, L2 et **divergence KL** sur le forward complet.",
          "**Accord des décisions** — accord top-1 des tokens plus accord top-8 du routage (validant la nécessité de l'autorité du routeur).",
          "**Stabilité de génération** — N tokens greedy ; le premier indice où A et B divergent.",
          "**Niveau tâche** — deltas de perplexité et de scores d'éval : la seule métrique que l'utilisateur ressent.",
        ],
      },
      {
        t: "p",
        md: "Une réussite est une **tolérance** — « accord top-1 ≥ 99.x %, KL ≤ ε ». Un nœud hors tolérance est marqué inapte aux rangs sensibles, pas rejeté d'office.",
      },
      { t: "h2", kick: "Pourquoi c'est une technologie de cœur de l'essaim", text: "L'accord au bit près est impossible — et inutile" },
      {
        t: "p",
        md: "Un cluster homogène peut supposer l'exactitude au bit ; un essaim non — sa prémisse est *le matériel qui se présente*. Kvasir traite donc l'équivalence numérique exactement comme la compatibilité de protocole : un **contrat de premier ordre, mesuré**. Backends et précision d'accumulation sont annoncés comme capacités de nœud, l'autorité du routeur est imposée comme invariant, et chaque vérification utilise des tolérances plutôt que l'égalité de bits. **Équivalence numérique mesurée + décisions discrètes à autorité unique** — voilà ce qui permet à un modèle de tourner sur chaque GPU de la planète à la fois. Voilà l'essaim.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell a rejoint l'essaim",
    dek: "Un GB10 Grace Blackwell a calculé de vraies tranches de FFN d'experts du 122B en CUDA et a égalé AMD ROCm au bit près (cosine 1.0000000000), et le Grace ARM CPU dans la tolérance. La matrice inter-backends est complète.",
    blocks: [
      {
        t: "p",
        md: "La prémisse d'un essaim est *le matériel qui se présente*. L'équivalence numérique — la preuve que les workers CUDA, ROCm, Adreno et CPU émettent tous le même token — avait déjà été mesurée sur ROCm, ARM de téléphone et numpy. NVIDIA est la voie **par défaut et la mieux optimisée** dans le ggml/moteur d'inférence d'origine, mais c'était le seul backend sur lequel la matrice n'avait pas été close. Faire tourner du vrai matériel Blackwell la clôt.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a : cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. Sur la même vraie tranche d'experts de layer-0 du Qwen3.5-122B, les deux backends GPU sont effectivement bit-identiques.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Mesuré · vrai Qwen3.5-122B-A10B, tranche d'experts de layer-0", text: "La matrice inter-backends" },
      {
        t: "table",
        head: ["Comparaison", "Matériel", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "Les deux backends GPU (CUDA, ROCm) partagent leurs sources de kernel, ils atterrissent donc **effectivement bit-identiques** (10⁻¹⁰). GPU↔CPU porte une perturbation d'environ 10⁻³ par op due à un ordre d'accumulation différent, mais reste équivalent à **cosine 0.99975** — le même motif que le précédent ROCm↔ARM-de-téléphone 0.99992. Le principe d'autorité du routeur tient de nouveau sur NVIDIA : **les décisions discrètes (argmax, sélection d'experts) sont invariantes sur cette perturbation continue.**",
      },
      { t: "h2", kick: "Configuration", text: "Ce qui a tourné, sur quoi" },
      {
        t: "ul",
        items: [
          "**Appareil** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124,5 Go de mémoire unifiée.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3 ; l'expert-worker ggml/gguf pur compilé avec les kernels Blackwell.",
          "**Modèle** — Qwen3.5-122B-A10B-Q4_K_M, tous les experts de layer-0 (256 experts, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Méthode** — la tranche L0 de 1,58 Go transmise MI250 → GB10 (comparaison sans perte) ; la même entrée (h/ids) passée par CUDA, CPU et ROCm ; vecteurs de sortie float32 (36 864) comparés par cosine, L2 relative et max-abs.",
        ],
      },
      {
        t: "callout",
        md: "**Un piège matériel réel :** le GPU intégré du GB10 est classé par ggml comme type d'appareil `ACCEL`, pas `GPU` — si bien que `init_by_type(GPU)` ne trouvait rien. Corrigé en sélectionnant le premier appareil non-CPU au lieu de coder en dur le type GPU.",
      },
      { t: "h2", kick: "Pourquoi c'est important", text: "La matrice est close" },
      {
        t: "p",
        md: "Pour que des workers hétérogènes servent un même modèle, une machine CUDA et une machine ROCm doivent être **interchangeables**, et un GPU et un CPU doivent être **numériquement équivalents**. Blackwell mesuré, les deux tiennent sur toute la matrice de backends : les workers CUDA↔ROCm peuvent se remplacer, et les workers GPU↔CPU concordent dans une tolérance bornée et bien conditionnée. L'accélérateur le plus répandu sur terre est désormais un citoyen vérifié de l'essaim.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "Durcir le chemin de l'argent : sécurité transactionnelle du règlement KVR",
    dek: "Trois classes réelles de vulnérabilités — rejeu de signature de paiement, frappe de récompenses sans authentification, double dépense par course — trouvées, exploitées en test et fermées dans le service de règlement du gateway.",
    blocks: [
      {
        t: "p",
        md: "Dans une DePIN, le chemin de l'argent est exactement aussi adverse que celui du calcul : chaque endpoint qui crédite du KVR sera tôt ou tard sondé par quelqu'un qui veut du KVR sans faire le travail. Une passe de sécurité sur le service de règlement du gateway — le processus qui vérifie les paiements on-chain et crédite stakes, récompenses de nœud et frais d'inférence — a trouvé et fermé **trois classes réelles de vulnérabilités**. Chacune a été démontrée par un test façon exploit avant correction, puis re-vérifiée après.",
      },
      { t: "h2", kick: "Le modèle de confiance", text: "Vérifier les faits on-chain, pas les affirmations du client" },
      {
        t: "p",
        md: "Le modèle de garde de Kvasir laisse les clés aux utilisateurs : les wallets signent les transactions, Solana les enregistre, et l'unique travail du service de règlement est de **vérifier ce qui s'est réellement passé sur la chaîne** avant de toucher un solde. Les paiements suivent *quote → payment → inference*, chaque signature de transaction consommée étant inscrite dans un registre à usage unique `usedSignatures`, impossible à présenter deux fois. Cela fait du service de règlement le goulot — et la règle qu'il ne doit jamais briser : ne créditer que ce que la chaîne prouve, jamais ce que le client affirme.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Correctif n°1 · liaison de l'expéditeur", text: "Lier le paiement au payeur" },
      {
        t: "p",
        md: "Les signatures Solana sont **publiques**. La vérification de stake contrôlait que le vault avait *reçu* le KVR attendu — jamais *qui l'avait envoyé*. Un attaquant pouvait guetter sur devnet le transfert KVR→vault d'une victime, puis soumettre `{owner: attaquant, signature: celle de la victime}` : le contrôle de réception du vault passait, le principal était crédité à l'attaquant, et un unstake plus tard les fonds étaient à lui. Un vol direct, avec pour seul outil un explorateur de blocs.",
      },
      {
        t: "code",
        caption: "Le correctif : le KVR doit avoir été débité de comptes de jetons appartenant à l'owner crédité.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Correctif n°2 · rapporteur de confiance", text: "Des récompenses uniquement de sources authentifiées" },
      {
        t: "p",
        md: "Les endpoints de récompense de nœuds frappaient du KVR réclamable à partir d'**entrées auto-déclarées** : `POST /api/node/contribution` créditait les `units` que le client affirmait — `units: 1e9` et un appel de claim pouvaient vider le vault — et register/heartbeat honoraient des rôles hub/gateway auto-déclarés (récompenses d'infra horaires) et des scores de performance (multiplicateurs). Le correctif place chaque assertion affectant les récompenses derrière un **rapporteur de confiance** : seuls le jeton de service M2M utilisé par la sonde de contribution du hub, ou un admin authentifié, peuvent affirmer units, rôles d'infra ou niveaux de perf — imposé même en mode LAN ouvert, car cela frappe du KVR. La comparaison du jeton est à temps constant, et la liaison wallet↔nœud reste libre ; elle ne peut simplement plus auto-affirmer ses récompenses.",
      },
      { t: "h2", kick: "Correctif n°3 · sérialisation du règlement", text: "Un seul écrivain par solde" },
      {
        t: "p",
        md: "L'état de règlement était un read-modify-write sans verrou, et chaque opération d'argent fait un *await* de paiement ou de vérification on-chain en plein milieu — cédant la boucle d'événements avec un solde périmé en main. Deux claims concurrents pouvaient lire le même solde en attente de 100 KVR et le payer tous deux. Rien de théorique : le test d'exploit a montré **trois claims concurrents payant 300 pour un solde de 100**.",
      },
      {
        t: "code",
        caption: "Sérialisation asynchrone par clé : les opérations d'argent de même clé s'exécutent strictement l'une après l'autre.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Défense en profondeur", text: "Où en est chaque couche" },
      {
        t: "table",
        head: ["Couche", "Mécanisme"],
        rows: [
          ["Identité", "Connexion par signature de wallet SIWS sur un nonce serveur + 2FA TOTP + codes de secours à usage unique"],
          ["Transport", "Node tokens dérivés du wallet sur les téléchargements de shards ; accord d'empreinte de build sur le relais"],
          ["Paiement", "Liaison de l'expéditeur sur les transferts de stake ; usedSignatures à usage unique ; requestId privé pour l'inférence"],
          ["Règlement", "Verrous par clé autour de chaque écriture de solde ; payer d'abord puis débiter ; re-soumission idempotente"],
          ["Rapports", "Faits affectant les récompenses réservés au jeton de service M2M ou à l'admin, comparés à temps constant"],
          ["Garde", "Wallets non-dépositaires — le service ne peut déplacer que ce que contient le vault, jamais les clés des utilisateurs"],
        ],
      },
      { t: "h2", kick: "Mesuré, pas supposé", text: "Chaque correctif embarque son propre test d'exploit" },
      {
        t: "ul",
        items: [
          "Rejouer la signature de transfert d'une victime sous le wallet d'un attaquant est désormais rejeté (« not sent by owner ») ; stakes légitimes, sur-réclamations et paiements d'inférence se comportent inchangés.",
          "Trois claims concurrents sur un même solde paient **exactement une fois** ; re-soumettre une inférence déjà payée renvoie idempotemment le même résultat.",
          "Les `units` auto-affirmées, rôles hub/gateway et niveaux de perf de clients non authentifiés ne déplacent plus un seul lamport de récompenses.",
        ],
      },
      {
        t: "p",
        md: "Le fil conducteur des trois correctifs est un principe appliqué de trois façons : **la chaîne est la source de vérité, le service est un vérificateur, et chaque solde a exactement un écrivain**. Le service de règlement tourne encore sur Solana devnet — précisément là où l'on veut trouver, exploiter et corriger ces classes avant que le mainnet ne relève la mise.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — Le moteur : linkcpp, un plan de contrôle pour moteur d'inférence",
    dek: "moteur d'inférence livre un plan de données RPC capable mais aucun plan de contrôle. linkcpp ajoute la moitié manquante — découverte, planification, lancement et gateways — autour de binaires d'origine.",
    blocks: [
      {
        t: "p",
        md: "Tout ce que Kvasir fait tourner commence ici. **linkcpp** est un plan de contrôle à source disponible (Business Source License) autour du plan de données RPC de moteur d'inférence : il exécute de grands modèles d'IA sur plusieurs GPU et machines avec des binaires `ggml-rpc-server` / `llama-server` *d'origine*. Le plan de données reste non forké — tout ce que linkcpp ajoute est de l'orchestration.",
      },
      { t: "h2", kick: "Le manque", text: "Un plan de données sans plan de contrôle" },
      {
        t: "p",
        md: "moteur d'inférence sait déjà répartir un modèle entre machines via RPC — mais quelqu'un doit découvrir les GPU, décider quelles couches vont où, lancer les bons workers avec les bons budgets, vérifier que chaque nœud parle le même protocole, et exposer une API que les développeurs peuvent réellement appeler. Le faire à la main pour un cluster est une corvée ; le faire pour un réseau ouvert d'appareils d'inconnus est impossible. Cette couche de coordination, c'est linkcpp.",
      },
      { t: "h2", kick: "Architecture", text: "Un hub, des workers d'origine, des gateways standard" },
      {
        t: "code",
        caption: "Flux de requête — le hub orchestre, les binaires d'origine calculent.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock moteur d'inférence engines below" },
      {
        t: "ul",
        items: [
          "**Trois façons de rejoindre :** des **slots de nœud locaux** fixes aux budgets VRAM/RAM/CPU éditables ; des **unités distantes** — enregistrer un autre hub et importer ses nœuds ; et des **agents de nœud managés** — services worker-only rejoignant par simple HTTP requête/réponse, volontairement sans flux persistant, pour survivre aux routages LAN/VPN simples.",
          "**Le gating de compatibilité est de premier ordre :** chaque unité, nœud et agent rapporte une identité protocole/runtime-pack plus des détails de backend. Les désaccords d'unité, de runtime-pack, de révision moteur d'inférence et d'ABI RPC sont **bloqués en dur avant bind, plan, load ou infer** — les différences de backend (CUDA/Metal/Vulkan/CPU) sont suivies comme capacités, pas comme rejets.",
          "**Le planner** lit les métadonnées GGUF et produit un placement contigu de couches par nœud, `--tensor-split`, des estimations de VRAM KV-cache/couche/expert, et un offload optionnel des FFN d'experts en RAM.",
          "**Gateways :** chaque contrôleur expose des endpoints compatibles OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) et Anthropic (`/anthropic/v1/messages|models`), adossés au même modèle chargé — les clients existants fonctionnent tels quels.",
        ],
      },
      {
        t: "p",
        md: "Cette séparation délibérée — un plan de données non modifié sous un plan de contrôle ouvert — est ce sur quoi tout le reste se construit : le ring runtime, le marché de couches, et finalement l'essaim d'experts sont autant d'évolutions du plan de contrôle sur le même calcul d'origine.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — L'anneau : inférence en pipeline sans master",
    dek: "Chaque appareil ne charge que sa fenêtre de couches et passe une petite frontière de hidden-state à son voisin. Aucun nœud ne détient le modèle ; aucun master central n'existe.",
    blocks: [
      { t: "h2", kick: "Pourquoi pas une étoile", text: "Le master RPC est un goulot et un portier" },
      {
        t: "p",
        md: "Dans la topologie RPC classique, un master ouvre le **GGUF entier** et appelle chaque worker. Cette forme casse dans un réseau ouvert de trois façons : le master doit détenir et servir tout le checkpoint ; chaque worker doit être joignable — les téléphones derrière NAT d'opérateur ne le sont pas ; et le master est un propriétaire unique dans un réseau qui ne devrait en avoir aucun.",
      },
      { t: "h2", kick: "L'anneau", text: "Fenêtres de couches + passage de frontières" },
      {
        t: "ul",
        items: [
          "Chaque appareil stocke le même modèle mais **ne charge que sa fenêtre de couches contiguë**, puis ouvre exactement deux liens : un vers son prédécesseur, un vers son successeur.",
          "Une requête entre dans l'anneau ; chaque nœud exécute ses couches et ne passe que la **frontière de hidden-state** à son voisin. Le dernier rang échantillonne le token et le renvoie — pas de master central, et aucun nœud ne détient le modèle entier.",
          "Le placement vient du **rank manifest** du planner — pour Qwen3.5-122B, 49 couches réparties sur n'importe quel mélange de GPU, CPU, NPU et téléphone qui se présente.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Faire des appareils faibles de vrais membres", text: "Shards partiels, GPU mobiles et le relais 443" },
      {
        t: "ul",
        items: [
          "**Téléchargement de shard partiel :** un stage de l'anneau n'a pas besoin du checkpoint — il a besoin de sa fenêtre. Un mini-GGUF de stage ne porte que ces tenseurs (**254 MB pour 26 tenseurs** contre le modèle complet de 77.6 GB), un téléphone ne tire donc que ~1.5 GB pour une fenêtre d'une couche.",
          "**Voie GPU mobile :** la route RPC vers le GPU d'un téléphone s'est révélée impraticable (la disposition des buffers OpenCL d'Adreno ne survit pas à la sérialisation RPC), mais un **stage de l'anneau tourne directement sur le GPU Adreno** — le stage possède son backend localement, donc rien ne traverse le fil sauf les frontières.",
          "**Traversée de NAT :** les téléphones ne peuvent accepter de connexions entrantes, le plan de données passe donc par un **relais 443** — un pont WebSocket par bordure avec un préambule de rôle d'1 octet qui laisse les deux extrémités appeler vers l'extérieur. Le téléphone n'ouvre aucun port entrant.",
          "**Marché d'auto-inscription :** les stages se revendiquent, ne s'assignent pas. Un nœud interroge la carte de couverture/demande, choisit la fenêtre non couverte **la mieux récompensée**, la télécharge et rejoint — vérifié de bout en bout avec un téléphone derrière NAT achevant l'inférence en anneau et gagnant sa contribution.",
        ],
      },
      { t: "h2", kick: "La place de l'anneau", text: "La voie basse latence" },
      {
        t: "p",
        md: "L'anneau est la voie de **latence** de Kvasir : les frontières sont petites, les sauts peu nombreux, et le décodage circule sans rien rassembler au centre. Sa limite est la granularité — la plus petite unité qu'un nœud peut porter est une couche (~1.4 GB sur le 122B). Supprimer ce plancher, c'est le rôle de l'essaim d'experts ; l'anneau reste la colonne de service à laquelle il se branche.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — À l'intérieur d'un MoE de 122B : pourquoi les poids veulent être shardés",
    dek: "Une analyse au niveau des tenseurs de Qwen3.5-122B : 86 % des octets sont 12 544 dalles d'experts indépendantes, chacune à une propre copie de plage d'octets de l'autonomie.",
    blocks: [
      {
        t: "p",
        md: "Avant de concevoir quoi que ce soit, nous avons démonté le 122B sur disque. La question : si un essaim d'appareils faibles doit porter ce modèle, quelle est l'unité naturelle de portage ? La réponse est tombée du layout des tenseurs GGUF lui-même.",
      },
      { t: "h2", kick: "Anatomie · Qwen3.5-122B-A10B (Q4_K_M)", text: "De quoi une couche MoE est réellement faite" },
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
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Chaque couche se scinde en une **voie dense** — attention + KV, les norms, le routeur (`ffn_gate_inp`), un expert partagé — et une **banque d'experts** : 256 FFN indépendantes stockées en trois tenseurs empilés (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). La voie dense est la minorité des octets ; la banque d'experts fait 86 % du modèle.",
      },
      { t: "h2", kick: "Le cadeau du layout", text: "Les experts sont des dalles contiguës alignées sur les blocs" },
      {
        t: "ul",
        items: [
          "L'indice d'expert est la **dimension ggml la plus externe** (`ne[2]`) de chaque tenseur d'experts — l'expert *e* occupe une dalle contiguë, alignée sur les blocs de quantification, d'octets quantifiés bruts.",
          "L'extraction par expert devient donc une **copie de plage d'octets** : `data[a:b]`, sans déquantification, sans ré-empaquetage — un mini-GGUF par experts est bon marché à produire et fidèle au bit.",
          "Par token, seuls **8 experts sur 256** s'activent par couche, choisis par le routeur — au décodage, le trafic d'experts d'une couche se résume à quelques petites multiplications matricielles sur un vecteur hidden.",
        ],
      },
      { t: "h2", kick: "L'implication", text: "L'unité de portage passe de 1.4 GB à 5.3 MB" },
      {
        t: "p",
        md: "Au grain de la couche, le minimum qu'un nœud peut tenir est ~**1.4 GB** — hors de portée de la plupart des téléphones une fois que l'app, le KV et l'OS ont pris leur part. Au grain de l'expert, l'unité est **5.3 MB**, et une contribution réaliste est de 8–64 experts (**42–340 MB**) — confortablement dans n'importe quel appareil moderne. Les experts étant mutuellement indépendants, la propriété peut être dispersée arbitrairement et rééquilibrée librement. C'est cette analyse qui a fait du sharding au niveau expert le pari de conception : les poids étaient déjà conditionnés en unités à la taille de l'essaim — le réseau n'avait qu'à honorer le conditionnement.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — Offload des experts du backbone en RAM (M0)",
    dek: "Diffusez les FFN d'experts MoE depuis la RAM CPU plutôt que la VRAM, et un seul coordinateur de 64 GB tient un 122B — sans chirurgie de graphe.",
    blocks: [
      {
        t: "p",
        md: "Les FFN d'experts n'ont pas à vivre en VRAM. Les diffuser depuis la RAM CPU permet à un coordinateur de tenir un modèle dont les experts dépassent sa VRAM — la fondation qui permet aux nœuds faibles de rejoindre un grand MoE.",
      },
      { t: "h2", kick: "Planner vérifié · vrai GGUF 122B", text: "Un 122B tient sur un seul coordinateur de 64 GB" },
      {
        t: "p",
        md: "Auparavant, l'anneau plaçait les poids en VRAM seulement, le 122B (77.6 GB) était donc **infeasible** sur un GCD de 64 GB. Avec les règles d'offload d'experts, le dry-run revient **feasible** :",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "plan d'anneau 122B" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (experts)" },
          { n: "10", l: "couches déchargées" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Sortie du planner — format de règles -ot de moteur d'inférence.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # moteur d'inférence -ot format`,
      },
      { t: "h2", kick: "Ce qui a été câblé · Python pur, sans recompiler le C++", text: "Porter les règles d'offload du planner jusqu'au vrai chargement" },
      {
        t: "ul",
        items: [
          "**planner** — émet déjà `ot` (règles `-ot` jointes par virgules) dans chaque placement.",
          "**protocol.py** — champ `StageStartRequest.ot` ajouté.",
          "**runtime.py** — transmet l'`ot` du placement à la requête de stage.",
          "**stage_service.py** — le coordinateur se lance avec `--override-tensor`.",
          "`linkcpp-server` transmet les arguments inconnus au llama-server d'origine, `-ot` s'applique donc tel quel.",
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
        md: "Vérifié par un test aller-retour du protocole `ot` et la confirmation que la commande du coordinateur émet `--override-tensor` ; le hub s'est redéployé proprement, sans régression. Restait alors : le chargement complet de 77 GB sur 2 nœuds (coordinateur avec offload + un téléphone tenant une fenêtre d'une couche de ~1.5 GB), en attente de serveur. Le cœur de M0 — l'offload du backbone qui permet aux nœuds faibles de participer à un grand MoE — était achevé au niveau du code et du planner.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — La voie de données des tranches d'experts (M1)",
    dek: "Un appareil faible télécharge quelques experts de 6 MB, pas une couche de 1.4 GB — et le calcul shardé égale le monolithique à 3.6e-12 près.",
    blocks: [
      { t: "h2", kick: "Vérifié · vrai Qwen3.5-122B-A10B", text: "Une tranche d'expert est une copie d'octets — sans déquantification" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "tranche en dim expert" },
          { n: "~6.1", l: "MB / expert (Q4+Q6)" },
          { n: "206 MB", l: "téléch. 2 couches × 16 exp." },
          { n: "200", l: "HTTP, GGUF valide" },
        ],
      },
      {
        t: "p",
        md: "Les tenseurs d'experts MoE empilent tous les experts le long de la dimension ggml la plus externe, le lecteur expose donc `(n_expert, rows, row_bytes)` d'octets quantifiés bruts. L'expert *e* est une dalle contiguë alignée sur les blocs de quantification — la tranche est littéralement `data[a:b]`, sans déquantification ni ré-empaquetage.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — l'aller-retour vérifié.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "L'oracle numérique", text: "dispatch + combine == monolithique, exactement" },
      {
        t: "p",
        md: "Avec de vrais experts de layer-0 du 122B (référence déquantifiée), scinder les experts en 4 shards, calculer chacun séparément puis combiner **égale la FFN MoE monolithique** : le sharding est un regroupement exact de la même somme pondérée, pas une approximation.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "erreur relative" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "experts touchés" },
        ],
      },
      { t: "h2", kick: "Worker C++, vérifié sur matériel", text: "linkcpp-expert-worker reproduit l'oracle sur ROCm" },
      {
        t: "ul",
        items: [
          "**ggml/gguf pur** (sans libllama) : charge la tranche dans un backend GPU et exécute `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`.",
          "**Compilation + exécution ROCm** sur un MI250 : 122B layer-0, experts [0,8), 4 tokens.",
          "**Cosine 0.99995 vs l'oracle**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — ce résidu est en soi le premier cas mesuré d'équivalence inter-backends (ROCm vs numpy).",
          "La même voie de code couvre CUDA/Metal/Vulkan/CPU (`mul_mat_id`/`swiglu` sont du ggml d'origine ; CUDA a un kernel MoE dédié).",
        ],
      },
      {
        t: "p",
        md: "La pièce la plus difficile et risquée — le kernel du worker sur l'appareil — a été vérifiée ici. Restait l'orchestration backbone↔worker ; le worker est une fonction pure prouvée qui consomme ces tranches.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — Dispatch distribué d'experts (M2)",
    dek: "Un décodage 122B en direct confie le calcul d'experts d'une couche à un processus worker séparé via TCP — et prédit exactement le même token.",
    blocks: [
      { t: "h2", kick: "Vérifié · vrai 122B, deux processus", text: "Décodage backbone → TCP → worker → experts → même token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "cosine des logits" },
          { n: "0", l: "perte de transport (byte-identical)" },
          { n: "2", l: "processus (backbone + worker)" },
        ],
      },
      {
        t: "p",
        md: "Le worker d'experts sert la tranche de layer-0 en **processus séparé** (ROCm), et le callback de dispatch `build_moe_ffn` du backbone 122B expédie `(cur, sel)` via TCP et reçoit les sorties d'experts. Le cosine des logits est **exactement la valeur in-process** (0.99868775) — le transport est sans perte. Le calcul en essaim expert-parallel fonctionne à travers une frontière de processus.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "Une connexion TCP de longue durée — le même flux que l'anneau/le relais 443 peut tunneliser.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Fait", text: "La chaîne de dispatch distribué" },
      {
        t: "ul",
        items: [
          "Mode `--serve` : charger la tranche, écouter en TCP, répondre `(n_used, n_tokens, cur, sel) → experts`.",
          "`--dispatch-port` : le callback du backbone envoie/reçoit via TCP vers un worker séparé, remplaçant le calcul in-process.",
          "Mesuré sur un décodage 122B en direct avec layer-0 dispatché hors processus → **argmax MATCH**, cosine 0.99869 (= in-process, sans perte).",
          "Cœur de M2 (plus tôt) : l'ARM du téléphone a calculé de vrais experts du 122B à cosine 0.99992 (cross-build Android).",
        ],
      },
      {
        t: "p",
        md: "La suite : tunneliser le même flux TCP par le **relais 443** vers des workers sur d'autres machines et téléphones (le transport a déjà été prouvé dans le travail de l'anneau), puis le marché de couverture M3 et le débit par lots M4.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — Le marché de couverture d'experts (M3)",
    dek: "Les nœuds faibles voient quelle (couche, plage d'experts) est la plus rare et la mieux payée, et la comblent eux-mêmes — le marché de couches éprouvé, au grain plus fin.",
    blocks: [
      {
        t: "p",
        md: "Le marché de shards de couches de Kvasir — carte de demande, auto-inscription à récompense maximale, téléchargement partiel, récompenses par nœud — était déjà vérifié sur appareil. M3 re-paramètre le même mécanisme au grain de la **(couche, plage d'experts)**, de sorte que la couverture s'auto-répare vers les plages d'experts les plus sous-répliquées et les mieux payées.",
      },
      { t: "h2", kick: "Vérifié · API", text: "Agrégation de rareté → attribution de la plage la mieux récompensée" },
      {
        t: "p",
        md: "Trois workers s'enregistrent sur la couche 0 : A = [0,128), B = [128,256), C = [0,128) en seconde réplique, avec `target_replicas = 2` :",
      },
      {
        t: "table",
        head: ["couche", "experts", "répliques", "rareté"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (objectif atteint)"],
          ["0", "[128, 256)", "1", "0.5 (sous l'objectif)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → taille la plage la plus rare au budget du nœud.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Fait · Python pur (hub)", text: "Un marché offre/demande au grain de l'expert" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — les workers signalent en heartbeat leurs détentions (couche, plage d'experts).",
          "`GET /api/expert-demand` — agrégation des répliques par expert → segments contigus de plages avec scores de rareté.",
          "`POST /api/expert-volunteer` — attribue la plage la plus rare taillée au budget du nœud.",
          "Le marché de couches existant (auto-inscription · téléchargement partiel · récompenses) re-paramétré en (couche, plage d'experts).",
        ],
      },
      {
        t: "p",
        md: "La suite en M4 : dispatch par lots des requêtes concurrentes plus un cache de hot-experts — tokens/s proportionnel au nombre de workers — et routage de répliques (worker le plus proche/rapide) avec repli anti-churn.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — Débit du dispatch par lots (M4)",
    dek: "L'essaim est un tissu de débit, pas un jeu de latence : grouper les appels de dispatch amortit l'overhead par requête de 77× par token.",
    blocks: [
      { t: "h2", kick: "Mesuré · ROCm, FFN d'experts, n_used = 8", text: "Plus gros lots, plus de tok/s par worker" },
      {
        t: "table",
        head: ["lot", "tok/s par worker"],
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
        md: "De **1.45 ms/tok** au lot 1 à **0.019 ms/tok** au lot 512 — une amélioration de 77× par token. Le temps par appel bouge à peine (1.45 → 9.6 ms) tandis que le lot grandit de 512× — le GPU traite le lot presque gratuitement derrière un overhead fixe. C'est la **propriété de tissu de débit** qui rend l'expert-parallel praticable : le dispatch par lots amortit le RTT et l'overhead par requête.",
      },
      { t: "h2", kick: "Fait", text: "Débit du dispatch par lots" },
      {
        t: "ul",
        items: [
          "Worker `--bench` : chronométrages de compute_dispatch pour les lots 1…512 → tok/s.",
          "**53k tok/s par worker** au lot 512 (ROCm) — le lotissement amortit l'overhead.",
          "Là-dessus s'empilent le cache de hot-experts et la mise à l'échelle agrégée multi-workers (routage de répliques).",
        ],
      },
      {
        t: "callout",
        md: "Avec M4, **toute la chaîne M0 → M4 est démontrée sur un vrai 122B** : offload du backbone · tranches d'experts · workers vérifiés · dispatch en décodage direct (argmax MATCH) · processus distribués · marché de couverture · débit par lots.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "Un téléphone a rejoint l'inférence d'un 122B",
    dek: "Un Galaxy S25 a téléchargé de façon autonome sa tranche d'experts depuis le hub et calculé les experts d'une couche à chaque pas d'un décodage 122B en direct. La sortie était correcte.",
    blocks: [
      {
        t: "callout",
        md: "prompt : **\"The capital of France is\"** → généré (avec le téléphone dans la boucle) : **\" Paris.\"** — 8/8 tokens identiques à l'exécution locale.",
      },
      { t: "h2", kick: "Mesuré · vrai 122B, le téléphone calcule layer-0", text: "Exactitude + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens identiques au local" },
          { n: "4.01", l: "TPS local (référence)" },
          { n: "3.13", l: "TPS avec téléphone" },
          { n: "1.58 GB", l: "téléchargement autonome" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Même avec le téléphone calculant les experts de layer-0 pour chaque token, **les tokens générés sont exactement les tokens locaux** — le juste \"Paris.\". Le TPS passe de 4.01 à 3.13 — l'aller-retour du dispatch téléphone (MI250 → tunnel → téléphone, ~100 ms/token) coûte 22 %. Le débit se récupère avec lots et répliques (M4).",
      },
      { t: "h2", kick: "Le flux de participation autonome", text: "Découvrir → téléchargement guidé par la récompense → rejoindre le calcul" },
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
      { t: "h2", kick: "Vérifié vs restant", text: "Le mécanisme est complet ; la boucle in-app relève de la productisation" },
      {
        t: "ul",
        items: [
          "Téléchargement partiel (l'endpoint expert-shard), service du worker, dispatch du backbone, génération 122B en direct et TPS — tout vérifié sur l'appareil réel.",
          "Exactitude : avec le téléphone participant, 8/8 tokens égalent l'exécution locale, avec la bonne réponse.",
          "Restant : la boucle autonome in-app (sonder expert-demand → volunteer → télécharger → serve → enregistrer) est du câblage Kotlin — cette démo a piloté le mécanisme directement.",
          "Transport : cette démo a utilisé un tunnel SSH ; la production utilise le relais 443 (déjà vérifié dans le travail de l'anneau).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "L'économie Kvasir : un cercle vertueux de coût et de récompense",
    dek: "Un réseau d'inférence décentralisé ne fonctionne que si le prix que paient les consommateurs et la récompense que gagnent les nœuds se renforcent mutuellement. Voici le volant d'inertie vers lequel nous construisons, les spirales qui le tuent, et les trois invariants qui le maintiennent en rotation.",
    blocks: [
      {
        t: "callout",
        md: "**Thèse :** Kvasir est un marché biface réglé en un seul jeton — les consommateurs paient des KVR pour inférer, les nœuds gagnent des KVR pour servir. Toute la conception réussit ou échoue sur une seule propriété : ces deux côtés doivent former un **cercle vertueux**, où chaque tour rend le tour suivant plus facile. Ratez cela et toute politique de prix finit par s'effondrer ; réussissez-le et le réseau devient *moins cher* à mesure qu'il devient *plus grand*.",
      },
      {
        t: "p",
        md: "Il est tentant de traiter le coût et la récompense comme un bras de fer — chaque dollar qu'un consommateur économise est un dollar qu'un nœud ne gagne pas. Ce cadrage est un piège. Dans un réseau sain, ce sont le **même volant d'inertie** vu par deux bouts : les paiements deviennent des récompenses, les récompenses deviennent de l'offre, l'offre devient de la capacité et des prix plus bas, les prix plus bas deviennent plus d'usage, et plus d'usage devient plus de paiements. La question n'est pas comment partager un gâteau de taille fixe ; c'est comment garder la roue en rotation pour que le gâteau grandisse.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "Le volant d'inertie", text: "Pourquoi l'usage et l'offre croissent ensemble" },
      {
        t: "p",
        md: "Le moteur du cycle est une règle unique déjà vraie chez Kvasir : **l'inférence doit être payée en KVR**. Cela fait de chaque unité d'usage une unité de demande réelle pour le jeton — de l'utilité, pas de la spéculation. La demande de jeton soutient la valeur des KVR que les nœuds gagnent ; des récompenses attractives attirent l'offre ; l'offre étend la capacité et, par la concurrence et un sharding d'experts plus fin, tire vers le bas le coût marginal du service ; un service moins cher, plus rapide et plus capable attire plus d'usage. Kvasir resserre la boucle avec une propriété qu'aucune API centralisée ne peut copier : un participant peut être **consommateur et fournisseur à la fois**. Le côté demande et le côté offre grandissent souvent chez les *mêmes personnes*, ce qui amortit les déséquilibres qui ruinent les marchés unilatéraux.",
      },
      { t: "h2", kick: "Les modes d'échec", text: "Quatre spirales qui font tourner la roue à l'envers" },
      {
        t: "p",
        md: "Un volant d'inertie peut ralentir aussi facilement qu'accélérer. Nommer les spirales mortelles, c'est ainsi qu'on conçoit contre elles :",
      },
      {
        t: "table",
        head: ["Spirale", "Comment elle commence", "Où elle finit"],
        rows: [
          ["Dilution des récompenses", "Plus de nœuds courent après une demande stagnante", "La récompense par nœud chute, les nœuds partent, la capacité baisse"],
          ["Prix trop bas", "Prix bon marché, récompenses sous le coût du nœud", "Servir cesse de payer, l'offre et la qualité s'effondrent"],
          ["Prix trop haut", "Bonnes récompenses, mais au-dessus du marché", "Les utilisateurs choisissent une API moins chère, les revenus se tarissent"],
          ["Dépendance à l'émission", "Récompenses payées par frappe, pas par revenus", "L'inflation érode le KVR jusqu'à ce que les deux côtés abandonnent"],
        ],
      },
      { t: "h2", kick: "Les invariants", text: "Trois règles qui gardent le cycle vertueux" },
      {
        t: "ul",
        items: [
          "**Les récompenses sont financées par des revenus réels.** En régime permanent, ce que les nœuds gagnent vient de ce que les consommateurs paient — pas d'une émission de jeton sans limite. L'émission est une subvention d'amorçage qui doit *décroître* à mesure que les revenus de commissions augmentent. Kvasir aide déjà ici en récompensant le **travail réel** — KVR par tokens réellement servis × part de couches, pas la simple présence — de sorte que la subvention ne peut pas fuir vers des nœuds « mercenaires » inactifs.",
          "**Le KVR est le médium obligatoire.** Parce que vous ne pouvez pas inférer sans payer des KVR, l'usage est un puits de demande permanent pour le jeton. Cela ancre la valeur du jeton à une utilité réelle plutôt qu'à la spéculation — la différence entre une monnaie et un jeton de casino.",
          "**Le prix flotte dans une bande.** Un plancher gardé au-dessus du coût marginal des nœuds garde le service rentable ; un plafond gardé sous les alternatives centralisées garde Kvasir compétitif. Entre les deux, le prix bouge — et c'est là que la croissance du réseau se traduit enfin en coût plus bas.",
        ],
      },
      { t: "h2", kick: "Le thermostat", text: "Rendre « plus de nœuds → moins cher » vrai dans le code" },
      {
        t: "p",
        md: "Aujourd'hui le prix est une constante gouvernée — sensé pour un devnet, mais cela signifie qu'ajouter des nœuds augmente la *capacité*, pas l'accessibilité tarifaire. La direction de conception est un **prix piloté par l'utilisation** : l'offre inoccupée pousse le prix vers le bas, vers le plancher, la congestion le pousse vers le haut, vers le plafond. Ce seul signal transforme l'intuition *« plus les gens partagent du calcul, moins cher cela devient »* en une règle imposée par le protocole — tandis que le plancher garde les opérateurs solvables pour que l'offre qui l'a rendu bon marché ne s'évapore pas. Parce que le prix est un paramètre économique sensible, il ne change que sous **l'autorité du wallet genesis avec signature de wallet + 2FA**, jamais une variable d'environnement égarée.",
      },
      {
        t: "callout",
        md: "**« Gratuit », c'est le net, pas le prix.** Vous payez ce que vous inférez et gagnez pour ce que vous servez ; contribuez à peu près autant que vous consommez et votre facture se solde à zéro. Aucune API par abonnement — Claude Max, un siège Codex — ne peut offrir cela, parce que vous ne pouvez jamais être leur côté offre. Avec Kvasir, vous pouvez exécuter des modèles que votre propre machine ne peut pas contenir *et* être payé pour aider les autres à exécuter les leurs.",
      },
      {
        t: "p",
        md: "Rien de tout cela n'exige un design de mécanisme exotique. Cela exige de la discipline sur trois choses : la récompense issue des revenus, la valeur issue de l'usage, l'équilibre issu d'un prix flottant borné. Kvasir livre déjà les parties difficiles et honnêtes — règlement non-dépositaire, récompenses proportionnelles au travail, un jeton que vous devez réellement dépenser pour utiliser le réseau. Le reste est la feuille de route économique : la transition dégressive, le partage de commissions qui finance un fonds d'assurance pour les inférences échouées, et le thermostat. Construits dans cet ordre, le coût et la récompense cessent de s'affronter et commencent à se composer.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "Un GPU à travers l'internet a rejoint l'inférence d'un 122B",
    dek: "Une station de travail Blackwell dans une autre ville a ouvert une seule connexion 443 sortante et a calculé des experts pour un décodage 122B en direct — identique octet pour octet à une exécution locale, et payée en KVR pour le travail accompli.",
    blocks: [
      {
        t: "callout",
        md: "**Ce qui s'est passé :** un décodage 122B tournant sur un backbone AMD à un endroit a envoyé son travail d'experts par token à une machine NVIDIA GB10 (Grace Blackwell) dans une autre ville — via un unique WebSocket sortant sur le port 443 — et a récupéré des sorties d'experts qui ont produit **exactement les mêmes tokens** qu'un calcul local. Pas de tunnel, pas de redirection de port, aucun trou entrant dans le pare-feu. La machine distante a gagné des KVR pour les octets qu'elle a servis.",
      },
      {
        t: "p",
        md: "La prémisse de Kvasir est *le matériel qui se présente* — y compris du matériel derrière un NAT d'opérateur, sur l'internet public, dans une autre ville. Qwen3.5-122B-A10B porte **86 % de son poids dans 12 544 experts indépendants** (48 couches × 256, top-8), chacun une fonction pure de 5.3 MB. C'est ce grain qui permet à une machine distante et sans lien de tenir une tranche et de contribuer. La question ouverte n'a jamais été *pouvons-nous le découper* — c'était *un worker à travers l'internet ouvert peut-il réellement participer à un décodage en direct, correctement et de façon comptabilisable*. C'est désormais le cas.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "Un seul appel sortant", text: "Pas de tunnel, aucun port entrant" },
      {
        t: "p",
        md: "Le worker distant ouvre **une seule** connexion — un `wss://` sortant vers le gateway public sur 443, le seul port que le NAT d'opérateur et les bordures CDN laissent passer de façon fiable. Le gateway n'analyse pas le flux ; il **épissure en brut** le WebSocket vers le hub réservé au LAN, qui le relie à l'écouteur de dispatch d'experts du backbone. Les deux extrémités ont appelé vers l'extérieur et se sont rejointes au milieu. Le worker n'expose aucun port entrant et n'a besoin d'aucune adresse publique.",
      },
      {
        t: "code",
        caption: "Deux appels sortants, épissurés en un flux de dispatch ordinaire.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Identique octet pour octet à travers l'internet", text: "Le routeur décide une fois ; le calcul se regroupe exactement" },
      {
        t: "p",
        md: "Le backbone exécute le routeur **une fois** et avec autorité ; le worker est une fonction pure `(hidden, ids) → out`. Déplacer cette fonction à travers un continent change donc *où* se produit la multiplication, pas *ce* qu'elle calcule. Sur un décodage 122B en direct avec les experts de layer-0 servis à distance : le flux de tokens greedy était **8/8 identique** (\" Paris.\"), **cosine des logits 0.99773**, argmax concordant. C'est la même propriété d'autorité du routeur qui maintient invariantes les décisions discrètes CUDA↔ROCm↔CPU — les backends hétérogènes restent bornés par une erreur continue, jamais une bifurcation catastrophique.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens greedy identiques" },
          { n: "0.99773", l: "cosine des logits, distant vs local" },
          { n: "1.2%", l: "surcoût de TPS, direct (13 ms RTT)" },
          { n: "0.00895", l: "KVR au worker, première session distante" },
        ],
      },
      { t: "h2", kick: "Le coût honnête, c'est le RTT", text: "Pourquoi l'essaim est un tissu de débit, pas un décodeur basse latence" },
      {
        t: "p",
        md: "Le dispatch série par token paie un aller-retour par pas. Mesuré : avec un lien direct (13 ms RTT), le surcoût de débit était de **1.2 %** (4.220 → 4.169 tok/s) ; routé par une bordure CDN sur 443, il était de **~28 %**. Nous le publions honnêtement, car cela pointe la vérité de conception — un essaim WAN est **borné par le RTT**, sa force n'est donc pas la latence d'un flux mais la **capacité agrégée**. Le lotissement amortit l'aller-retour : le dispatch d'experts par lots atteint **77× le débit par token** au lot 512. Les octets sont de la marge ; les allers-retours sont la chose à masquer — ce qui fait l'objet du billet de feuille de route compagnon.",
      },
      { t: "h2", kick: "Payé pour exactement le travail", text: "Des octets mesurés deviennent des KVR" },
      {
        t: "p",
        md: "La participation ne vaut rien si elle n'est pas comptabilisable. Le relais **mesure les octets relayés par session** dans le registre de contribution du hub ; le gateway interroge ce registre et crédite en delta des KVR sur le wallet **propre** du worker — non-dépositaire, comme tout le reste. La première session à travers l'internet a réellement accumulé : **1.28 MB de travail → 1.277952 units → 0.00895 KVR** en récompenses en attente. Petit, et c'est le but — c'est un règlement réel, par travail, pas un trophée de participation.",
      },
      {
        t: "p",
        md: "Ce même chemin 443-sortant est exactement la façon dont un **téléphone** rejoint : un Galaxy S25 a déjà calculé des experts du 122B par ce biais (8/8 tokens identiques, cosine 0.99992). Un modèle à l'échelle de la frontière, servi par un backbone à un endroit, un GPU de datacenter dans une autre ville, et un téléphone dans la poche de quelqu'un — tous produisant les mêmes tokens, chacun payé pour sa part. La suite, c'est rendre l'aller-retour WAN bon marché ; cette feuille de route est étayée par les chiffres de production d'autrui et par nos propres mesures.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "Rendre le dispatch WAN bon marché : une feuille de route étayée",
    dek: "Le dispatch distant d'experts fonctionne et est identique octet pour octet — mais un décodage WAN est borné par les allers-retours. Voici le plan pour en réduire le coût, étayé par des chiffres de production de DeepSeek, Petals et d'autres (feuille de route, pas livré).",
    blocks: [
      {
        t: "callout",
        md: "**Cadrage :** les chiffres *que nous avons mesurés* sont présentés comme mesurés ; tout ce qui est décrit comme un plan est une **feuille de route**, pas un résultat livré. Le but est de prendre le dispatch distant d'experts — déjà correct et payé (voir le billet compagnon) — et de rendre l'aller-retour WAN assez bon marché pour qu'un GPU distant ou un téléphone soit un membre de l'essaim de premier ordre, pas un membre lent.",
      },
      {
        t: "p",
        md: "Notre propre mesure, publiée sans détour : le dispatch coûte environ **110 KB par token par couche** — 12.3 KB à l'aller (dispatch) plus 98.3 KB au retour (combine). L'asymétrie de 8× vient de ce que chaque expert sélectionné renvoie sa sortie complète *avant* la somme pondérée. En lien direct, c'est un surcoût de débit de **1.2 %** ; via un relais CDN, **~28 %**. Voilà les faits. Le reste de ce billet, c'est comment nous entendons combler l'écart — et pourquoi les octets sont la partie facile.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "La loi dominante", text: "Un décodage WAN est borné par les allers-retours" },
      {
        t: "p",
        md: "Le résultat publié le plus important ici n'est pas le nôtre — c'est celui de Petals : quand le RTT passe de <5 ms à 100 ms, le décodage chute de **1.24 à 0.57 steps/s**, tandis qu'une **coupe de bande passante de 10× ne le change que de ~0**. La latence domine ; la bande passante a du mou. Cela recadre tout le problème : rogner des octets, c'est de la marge, mais **couper les allers-retours, c'est la substance**. Chaque point ci-dessous est classé selon la quantité de coût d'aller-retour qu'il retire.",
      },
      { t: "h2", kick: "Moins cher sur le fil", text: "Réduction d'octets, l'exactitude d'abord" },
      {
        t: "ul",
        items: [
          "**Renvoyer des sommes partielles pondérées, pas les sorties brutes des experts.** Par linéarité, le combine du backbone est exact dans les deux cas, mais le worker renvoie un seul vecteur sommé au lieu de 8 — c'est la réduction ~8× du combine, et c'est précisément ce que font DeepSeek-V3 / DeepEP en production.",
          "**Étoile parallèle, pas chaîne série** sur plusieurs workers : ΣRTT s'effondre à max RTT.",
          "**F16 sur le fil** — nous acceptons déjà un cosine inter-backends de ~0.998, le transport F16 est donc dans la tolérance existante ; **INT8/FP8 par blocs plus tard**, une fois que notre propre porte argmax/cosine l'aura validé face aux poids Q4_K_M (Petals a montré de l'INT8 sur l'internet réel sans perte de qualité).",
          "Ensemble, ceux-ci visent **~110 KB → 9–12 KB par token (~12×)** — réel, mais rappelez-vous que c'est la *marge*, pas le goulot.",
        ],
      },
      { t: "h2", kick: "Amortir l'aller-retour", text: "La substance : moins de trajets, des trajets masqués" },
      {
        t: "ul",
        items: [
          "**Le décodage spéculatif** transforme plusieurs tokens en un seul aller-retour. À un WAN mesuré de 80 ms, le seuil de rentabilité n'est que de **~1.15–1.2 tokens acceptés par pas** — même une faible supposition n-gram l'emporte donc (le Jacobi vanilla peut se retourner contre soi ; le choix de la technique compte). Notre protocole de dispatch porte déjà `n_tokens > 1`, aucun changement sur le fil n'est donc nécessaire.",
          "**Le lotissement continu au gateway** replie les requêtes concurrentes en un seul trajet ; **le cache de préfixes à affinité de slot** garde une session sur les mêmes répliques.",
          "**Masquage de latence** : l'expert partagé est un terme additif indépendant, le backbone le calcule donc *localement* pendant l'aller-retour distant (ScMoE rapporte 1.82× sur PCIe, sans réentraînement). Gardez les **hot experts en local**, n'envoyez à distance que les froids (EPLB réplique les ~32 plus chauds pour une accélération de décodage de 2.54× en production).",
        ],
      },
      { t: "h2", kick: "Politique et l'avenir des gros tuyaux", text: "Router par pair, et ce que change 200 Gb/s" },
      {
        t: "p",
        md: "Politique de chemin : les pairs routables publiquement prennent la voie **directe** (la route à 1.2 %) ; le relais est réservé aux seuls appareils derrière NAT. Et quand arriveront de larges liens 200 Gb/s, les 110 KB se sérialisent en **~4.4 µs** — le terme de bande passante disparaît avant même les réductions ci-dessus, et une tranche de 794 MB s'expédie en ~32 ms. Mais **le RTT, c'est de la physique ; il ne rétrécit pas** — le décodage spéculatif et le chevauchement restent donc les vrais leviers, même à 200 G. Là où le gros tuyau compte réellement, c'est la fédération multi-backbone (plusieurs backbones partageant un pool d'experts) et le travail borné par la bande passante : prefill de prompts longs et débit à gros lots.",
      },
      {
        t: "callout",
        md: "**Une réserve, dite honnêtement :** la couche de transport elle-même (WebSocket vs QUIC, surcoût de masquage, hole-punching NAT) n'a **aucun résultat externe que nous puissions citer** — c'est de l'ingénierie que nous mesurerons nous-mêmes avant d'affirmer quoi que ce soit. Tout ce qui précède repose sur des chiffres de production publiés (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) plus nos propres mesures ; quand un point de la feuille de route est livré, ses chiffres et son temps sont mis à jour ici.",
      },
      { t: "h2", kick: "La suite", text: "Cibles d'intégration" },
      {
        t: "p",
        md: "Notre hook de dispatch se pose sur `build_moe_ffn` — **une seule fonction que partagent 43 architectures MoE** dans le moteur d'inférence. Trois invariants sont indépendants du modèle : le calcul MoE (routé = Σ wᵢ·Eᵢ(x), linéaire), la voie de code partagée, et les tenseurs d'experts empilés standard de GGUF (`ne[2]` le plus externe → découpe alignée sur les blocs). Intégrer un nouveau modèle n'est donc pas une refonte — c'est un seul passage par une porte de vérification argmax/cosine propre au modèle.",
      },
      {
        t: "table",
        head: ["modèle", "experts · routage", "par expert (Q4≈)", "partagé", "statut"],
        rows: [
          ["Qwen3.5-122B (servi aujourd'hui)", "256 · top-8", "5.3 MB (mesuré)", "oui", "en production"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "oui", "prêt — premier candidat"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "oui", "prêt (hook vérifié)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "non", "prêt (hook vérifié)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "oui", "prêt (graphe deepseek2)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "oui", "prêt (famille deepseek)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "non", "prêt"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "non", "prêt"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "oui", "prêt (MoE une couche sur deux)"],
          ["MiniMax M3 428B", "128 · top-4", "à déterminer (GGUF)", "oui", "en attente du moteur amont"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "non", "fonctionne — workers GPU uniquement"],
        ],
      },
      {
        t: "p",
        md: "L'industrie converge vers le MoE à grain fin — des experts plus petits, plus nombreux, une sparsité plus élevée (DeepSeek, Qwen, Kimi, GLM, gpt-oss ont tous pris cette voie). Chaque pas dans cette direction rend l'unité de participation de l'essaim plus petite et le grain du marché de rareté plus fin. Les modèles ci-dessus ne sont pas une liste de souhaits ; chacun passe déjà par le même hook de dispatch que nous exécutons en production — l'intégration est une porte de vérification, pas un projet d'ingénierie.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "La question du 200G",
    dek: "Nos hubs d'essaim peuvent déjà se relier à 200 Gb/s avec des pièces sur étagère — l'une est intégrée au GB10. Voici ce qu'un gros tuyau achète à un MoE distribué, et la seule chose qu'il ne peut pas.",
    blocks: [
      {
        t: "callout",
        md: "**La prémisse :** le décodage WAN est borné par le RTT, pas par la bande passante — notre feuille de route de communication a montré que les octets sont la partie facile. Alors qu'est-ce qui change vraiment quand les hubs obtiennent des liens à 200 Gb/s ? Presque tout sur la *capacité*, et presque rien sur la *latence*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Déjà dans la boîte · ConnectX-7", text: "Le matériel n'a rien de futuriste — l'un est livré dans notre worker GB10" },
      {
        t: "p",
        md: "Le GB10 Grace Blackwell qui calcule nos experts du 122B embarque une **NVIDIA ConnectX-7 avec deux ports QSFP 200 GbE**. Deux de ces machines se connectent en direct avec un seul câble QSFP56 DAC à ~$100 — un cluster 200G à deux hubs avec zéro switch. ARM est ici un citoyen de premier ordre : la même pile de pilotes `mlx5` qui fait tourner ces cartes réseau dans les datacenters x86 les fait tourner sur aarch64, ce qu'est exactement le GB10.",
      },
      {
        t: "callout",
        md: "**Les petits caractères :** le GB10 alimente sa ConnectX-7 par deux liens PCIe Gen5 x4 en mode multi-hôte. La pleine vitesse mesurée (~185–190 Gb/s) exige **RoCE (RDMA) et une topologie correctement mappée** — un TCP naïf sur un chemin mal mappé atterrit à ~95 Gb/s ou pire. Les gros tuyaux s'achètent avec de la configuration, pas seulement des câbles.",
      },
      { t: "h2", kick: "L'échelle des distances", text: "Le 200G est un article de catalogue à toutes les portées" },
      {
        t: "table",
        head: ["portée", "pièce", "format"],
        rows: [
          ["rack (0.5–3 m)", "QSFP56 DAC cuivre", "câble, ~$100"],
          ["salle (~30 m)", "AOC optique active", "câble"],
          ["campus (2–10 km)", "optiques 200G FR4 / LR4", "module QSFP56"],
          ["métro (~40 km)", "optiques 200G ER4", "module QSFP56"],
          ["région (~120 km)", "400G ZR+ cohérent, exploité au débit ligne 200G", "module QSFP-DD"],
          ["longue distance (centaines de km)", "longueur d'onde opérateur 200G / système de ligne DWDM", "service loué"],
        ],
      },
      {
        t: "p",
        md: "Dans le WAN, le *câble* n'est que de la fibre monomode standard — du verre neutre en vitesse qui relie déjà chaque ville. La vitesse réside dans les optiques enfichables à chaque extrémité, et **OpenZR+ a fait du 200G-sur-120 km un module qu'on enfiche dans un switch**, pas un projet télécom. Au-delà, on loue une longueur d'onde.",
      },
      { t: "h2", kick: "Ce qu'il achète", text: "Chaque terme de bande passante de l'essaim disparaît" },
      {
        t: "ul",
        items: [
          "Une charge utile de dispatch (~110 KB/token/couche aujourd'hui, ~10 KB après la feuille de route sur le fil) se sérialise en **microsecondes** — la taille de la charge utile cesse totalement d'être une contrainte de conception.",
          "Une **tranche d'experts s'expédie en ~32 ms** (794 MB, théorique) et un modèle 122B entier se synchronise en **~3 s** — le rééquilibrage du marché de couverture et l'intégration de nouveaux hubs deviennent quasi instantanés.",
          "Le prefill à long contexte — la seule phase réellement gourmande en bande passante — avance à la vitesse du fil, de sorte que le temps du premier token sur des prompts de 100K tokens devient borné par le calcul du backbone.",
          "**Le dispatch par lots monte en charge sans plafond de fil** : le trafic du pool d'experts agrégé sur de nombreux flux utilisateurs est exactement la charge gourmande en bande passante et tolérante à la latence qu'un gros tuyau absorbe. C'est ce qui rend praticable une fédération multi-backbone — plusieurs hubs, chacun gardant le KV de ses propres utilisateurs, partageant un pool d'experts.",
        ],
      },
      { t: "h2", kick: "Ce qu'il ne peut pas acheter", text: "La lumière ne se presse pas" },
      {
        t: "p",
        md: "La fibre porte la lumière à ~5 µs/km, et aucune quantité de bande passante n'y change rien. Un aller-retour de 13 ms reste 13 ms à 200 Gb/s. Le décodage autorégressif paie cet aller-retour par couche shardée, par token — c'est pourquoi **le décodage spéculatif (k tokens par aller-retour) et le chevauchement de l'expert partagé (calculer pendant que le dispatch est en vol) restent essentiels**, même entre des hubs reliés par le plus gros tuyau du marché. La bande passante achète du débit ; seule la discipline des allers-retours achète de la latence.",
      },
      {
        t: "p",
        md: "L'architecture se stabilise donc en deux paliers. Un **palier hub** — backbones et hot experts reliés par des liens de classe 200G, où la capacité est effectivement illimitée — et un **palier de bordure** — téléphones et petits appareils sur le relais 443, détenant la longue traîne d'experts que le marché de rareté leur attribue. Le gros tuyau donne au premier palier l'allure d'une seule machine ; le relais garde le second palier ouvert à tous. Aucun ne remplace l'autre : cette séparation *est* la conception.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "L'essaim qui grandit sous la charge",
    dek: "Un modèle géant qui n'emprunte de l'aide que lorsqu'il en a besoin — l'essaim MoE se met désormais à l'échelle du trafic : compact et rapide au repos, large et parallèle en pleine charge.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir sert des modèles bien plus grands que ce que peut contenir une seule machine — un modèle Mixture-of-Experts de 122B de paramètres tourne réparti sur un coordinateur plus un essaim de workers : des GPU sur le LAN, des GPU au bout d'un lien à 200 Gb/s, et même des téléphones qui se connectent par internet. Parce qu'un modèle MoE route chaque token vers une poignée seulement de ses experts, la plupart des poids restent inactifs à tout instant, et ces experts inactifs peuvent vivre **hors** du nœud principal — sur n'importe quel matériel qui s'est proposé pour les héberger.",
      },
      {
        t: "callout",
        md: "La nouveauté, c'est que l'essaim se **met désormais lui-même à l'échelle de la charge**.",
      },
      { t: "h2", kick: "Comment il se comporte", text: "Compact au repos, large en pleine charge" },
      {
        t: "p",
        md: "Quand le trafic est faible, le coordinateur sert tout sur son propre GPU — le chemin le plus rapide par token, sans saut réseau. Quand les requêtes commencent à s'accumuler et que ses slots d'inférence saturent, deux choses se produisent automatiquement :",
      },
      {
        t: "ul",
        items: [
          "**Il remobilise les workers qu'il a déjà.** Le coordinateur surveille sa propre file. En saturation, il continue de diffuser le travail des experts routés vers des workers **éprouvés** — ceux qui ont réellement servi auparavant — échangeant un peu de latence par token contre beaucoup plus de débit total. Un worker qui s'est simplement connecté sans jamais calculer ne se voit jamais confier de charge ; un worker tout neuf a tout de même droit à un premier essai équitable.",
          "**Le hub en recrute de nouveaux.** Le hub de contrôle remarque la même saturation et augmente la « demande » pour les experts de ce modèle. Les nœuds inactifs — un téléphone dans une poche, un GPU disponible à l'autre bout de la ville — interrogent déjà ce marché de la demande. Dès que la demande monte, on leur propose une tranche d'experts à servir ; ils la téléchargent, se connectent et rejoignent l'essaim. Quand la pointe passe, la demande retombe et les workers supplémentaires se retirent discrètement.",
        ],
      },
      {
        t: "p",
        md: "Personne n'ordonnance cela. Aucun nœud n'est poussé. L'essaim respire au rythme de la charge : compact et rapide au repos, large et parallèle en pleine charge — et cela fonctionne même pour des nœuds derrière des routeurs domestiques, parce que tout repose sur le pull.",
      },
      {
        t: "p",
        md: "Voilà la forme d'un réseau capable de servir des modèles à mille milliards de paramètres sur du matériel que personne ne possède seul : la capacité inactive est invitée exactement quand cela en vaut la peine, et seulement alors.",
      },
    ],
  },
};
