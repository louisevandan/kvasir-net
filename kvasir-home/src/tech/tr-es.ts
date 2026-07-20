/* Español — traducción del blog técnico. La estructura (slug, categoría, orden
   de bloques, código, posiciones de img) refleja exactamente articles.ts
   (fuente en inglés); términos técnicos, identificadores y cifras se mantienen. */
import type { TechTranslation } from "./articles";

export const esTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "Inferencia en enjambre con sharding de expertos: el diseño",
    dek: "El 86% de un MoE de 122B son 12.544 expertos independientes de 5.3 MB. Corta el modelo a ese grano y un teléfono puede cargar una parte real de la inferencia de frontera.",
    blocks: [
      {
        t: "callout",
        md: "**La tesis:** el 86% del peso de Qwen3.5-122B son 12.544 expertos mutuamente independientes de 5.3 MB. Fragmenta al grano del experto y un dispositivo débil carga \"8–64 expertos (42–340 MB)\" en vez de \"una capa de 1.4 GB\" — exactamente la unidad que un teléfono puede sostener de verdad. El MoE es el sustrato natural de un enjambre.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "Sustrato · Qwen3.5-122B-A10B (Q4_K_M)", text: "Los pesos ya vienen empaquetados en unidades del tamaño del enjambre" },
      {
        t: "stats",
        items: [
          { n: "49", l: "capas" },
          { n: "256", l: "expertos / capa" },
          { n: "8", l: "activos / token" },
          { n: "5.3 MB", l: "un experto (Q4)" },
          { n: "12,544", l: "expertos en total" },
          { n: "86%", l: "del peso en expertos" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "dim experto = más externa" },
        ],
      },
      {
        t: "p",
        md: "El índice de experto es la **dimensión más externa** de cada tensor MoE, así que cada experto es una losa contigua alineada a bloques de cuantización. Un mini-GGUF cortado por expertos es una copia limpia de rango de bytes — sin decuantización, sin re-empaquetado.",
      },
      { t: "h2", kick: "Dos roles", text: "Stage de backbone × worker de expertos" },
      {
        t: "ul",
        items: [
          "**Stage de backbone (nodo fuerte):** atención + caché KV, todas las norms, el **router**, el experto compartido y el combine residual — toda la ruta densa. Además mantiene todos los expertos residentes como réplica de respaldo (offload a RAM), lo que da al enjambre tolerancia al churn.",
          "**Worker de expertos (un teléfono):** no es un transformer. Sin atención, sin KV, sin sampler — una función pura `(hidden, local_ids) → out` hecha de tres mat-muls, que solo aloja su propia rebanada de expertos. Cabe en cualquier presupuesto, hasta un teléfono de 4 GB.",
        ],
      },
      {
        t: "code",
        caption: "El punto de corte: el router corre una vez en el backbone, con autoridad.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Como el router corre **exactamente una vez** en el backbone, cada experto seleccionado lo computa exactamente una vez el nodo que lo posee. **No hay aproximación** — el sharding solo mueve dónde ocurren los mat-muls.",
      },
      { t: "h2", kick: "No es un subsistema nuevo", text: "El enjambre es el mercado de recompensas probado, a grano más fino" },
      {
        t: "p",
        md: "Kvasir ya opera un mercado autónomo de escasez para shards de **capas**, verificado en dispositivos reales: un teléfono tras NAT consulta el mapa de demanda, se auto-inscribe en el segmento de **mayor recompensa**, descarga parcialmente solo esa ventana, la carga en su GPU Adreno y completa la inferencia en anillo — ganando recompensas de contribución. El sharding de expertos lo reutiliza todo — mapa de cobertura, auto-inscripción por máxima recompensa, descarga parcial, recompensas por nodo — cambiando solo la unidad de cobertura de *rangos de capas* a *(capa, rango de expertos)*.",
      },
      { t: "h2", kick: "Dos innovaciones ya verificadas en dispositivo", text: "Participación con pesos parciales + el relay 443" },
      {
        t: "ul",
        items: [
          "**Descarga parcial de pesos guiada por recompensa:** los montajes RPC/TP/PP convencionales envían el checkpoint completo a cada rank y un scheduler dicta la colocación. En Kvasir un nodo descarga **solo la rebanada que va a computar**, y elige esa rebanada **él mismo, por recompensa** — un mini-GGUF de stage de 254 MB frente al modelo completo de 77.6 GB. Así es como un teléfono de 4 GB se une a un modelo mucho más grande que él.",
          "**Plano de datos por relay 443:** el edge de Cloudflare limitado a 80/443 más el NAT de operadora impiden la marcación directa en ambos sentidos. Un puente WebSocket por edge con un preámbulo de rol de 1 byte permite que **ambos lados marquen hacia fuera** (el teléfono abre cero puertos entrantes). Aterrizarlo implicó arreglar tres bugs reales — acuerdo de huella de build, autenticación de descarga por node-token, y un bug de longitud de trama con `Int.ushr` de Kotlin que corrompía silenciosamente cada trama ≥ 64 KiB (`ushr` usa solo los 5 bits bajos del desplazamiento; `len ushr 56` se volvió `len ushr 24`) — corregido pasando a desplazamientos `Long`.",
        ],
      },
      { t: "h2", kick: "El quid honesto", text: "Un tejido de throughput, no un decodificador de baja latencia" },
      {
        t: "p",
        md: "La decodificación son 49 capas en serie, y una ida y vuelta por internet por capa cuesta 2.5–10 s por token. Así que la contienda del enjambre es **servir modelos que nadie puede hospedar solo**, medida en throughput agregado: el dispatch por lotes amortiza el RTT, el backbone mantiene una caché de hot-experts y las peticiones se enrutan a réplicas cercanas. La ruta de baja latencia queda en el anillo de pipeline.",
      },
      { t: "h2", kick: "Hoja de ruta", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — offload de expertos del backbone a RAM: correr el 122B en un coordinador, sin cirugía de grafo.",
          "**M1** — prueba expert-parallel en un solo host: mini-GGUF por expertos + runtime del worker + dispatch, logits exactamente iguales al monolítico.",
          "**M2** — workers de teléfono en LAN + NAT computando expertos reales del 122B a través del relay 443.",
          "**M3** — mercado de cobertura a grano de experto con réplicas y respaldo ante churn.",
          "**M4** — throughput: dispatch por lotes + caché de hot-experts, tokens/s escalando con el número de workers.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "Del plano al hardware: lo verificado y la clave de bóveda",
    dek: "Recapitulación de la campaña de verificación — del diseño al núcleo de M2 probado en un 122B real — y la única pieza de integración que desbloquea el resto.",
    blocks: [
      {
        t: "p",
        md: "En las últimas semanas, las piezas difíciles y novedosas del enjambre de expertos se probaron una a una en un **Qwen3.5-122B** real — ni simulado, ni de juguete. He aquí el rastro de verificación hasta ahora, y la única clave de bóveda que quedaba.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "El rastro · todo verificado en el 122B real", text: "Lo que ya ha aterrizado" },
      {
        t: "ul",
        items: [
          "**Diseño (5 revisiones desde el plano)** — arquitectura EP, participación autónoma con pesos parciales, el relay, la especificación del kernel del worker, y la equivalencia numérica entre backends codificada como tecnología central. La autoridad del router quedó escrita como el invariante de coherencia.",
          "**M0 — offload de expertos del backbone a RAM (planner verificado):** el 122B es *feasible* en un solo coordinador de 64 GB — 10 capas de expertos descargadas a RAM, VRAM 62.6 GiB / RAM 14.2 GiB, cableado vía `--override-tensor`.",
          "**M1 — ruta de datos de rebanadas de expertos:** sliceo de mini-GGUF por experto (losas ne[2], copia de bytes sin decuantización) + el endpoint de descarga `/expert-shard`.",
          "**M1 — oráculo numérico:** dispatch + combine == monolítico con **max|Δ| = 3.6e-12** en expertos reales de layer-0 — el sharding es un reagrupamiento exacto de la misma suma ponderada.",
          "**M1 — worker C++ en hardware:** `linkcpp-expert-worker` (ggml/gguf puro) compilado y ejecutado en ROCm, **cosine 0.99995** vs el oráculo; router → dos workers C++ → combine iguala al monolítico con cosine 0.9997–0.9999.",
          "**Núcleo de M2 — un teléfono computa expertos reales del 122B:** cross-build de Android, ejecutado en un SM-S938N, **cosine 0.99992** vs el oráculo.",
          "**Numéricos — matriz de equivalencia de 3 backends:** la misma computación del 122B en ROCm × ARM CPU del teléfono × numpy — ROCm↔teléfono cosine 0.99990, ROCm↔numpy 0.99996, teléfono↔numpy 0.99992. Todo equivalente, nada bit-idéntico.",
        ],
      },
      { t: "h2", kick: "La clave de bóveda", text: "El dispatch del backbone, integrado en la decodificación en vivo" },
      {
        t: "callout",
        md: "Cada **componente** — rebanadas, workers, lógica de dispatch/combine, equivalencia numérica, cómputo en teléfono — estaba verificado en dispositivo. Lo que faltaba era cablearlos **dentro de una decodificación real de motor de inferencia**: un hook en `build_moe_ffn` que despacha expertos a sus nodos dueños a mitad de grafo. Requirió modificar el submódulo fijado de motor de inferencia y varios ciclos de compilar-verificar. Con esta clave de bóveda en pie, **la integración del relay de M2, el mercado de cobertura de M3 y el throughput por lotes de M4** se abren en secuencia — todos dependen de este dispatch.",
      },
      {
        t: "p",
        md: "La clave de bóveda ya aterrizó: las entradas siguientes sobre M2, M3, M4 y la demo en vivo con el teléfono son el resultado de exactamente esta integración.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "Equivalencia numérica entre backends heterogéneos",
    dek: "CUDA, ROCm, Adreno y las CPU nunca coincidirán bit a bit. Que el enjambre aún produzca un modelo coherente es una propiedad diseñada, no suerte.",
    blocks: [
      { t: "h2", kick: "La distinción clave", text: "Exacto vs equivalente — dos propiedades distintas" },
      {
        t: "ul",
        items: [
          "**Dentro de un backend — exacto (3.6e-12):** repartir expertos entre nodos y combinarlos es la misma suma ponderada reagrupada; la única diferencia es el orden de acumulación en coma flotante. Verificado por oráculo.",
          "**Entre backends — equivalente (1e-3…1e-6):** la misma operación en hardware distinto arrastra un error relativo por operación de ~1e-3–1e-6, y nunca es cero. **El enjambre vive en este régimen.**",
        ],
      },
      {
        t: "p",
        md: "\"Exacto\" es lo que la descomposición garantiza dentro de un dispositivo. \"Equivalente\" es lo que da el hardware heterogéneo. El trabajo del enjambre es evitar que la equivalencia se componga en divergencia.",
      },
      { t: "h2", kick: "Medido · 122B real, tres backends", text: "No es teoría — medido en hardware" },
      {
        t: "p",
        md: "El mismo FFN de expertos de layer-0 de Qwen3.5-122B, computado por `linkcpp-expert-worker` en un MI250 (**ROCm**), la **ARM CPU** de un teléfono (SM-S938N) y una referencia **numpy** en x86 — mismas entradas, mismos pesos, distintos juegos de instrucciones y órdenes de reducción:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Par de backends", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU del teléfono vs numpy (x86)", "1.4e-6", "0.99992"],
          ["GPU ROCm vs ARM CPU del teléfono", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Tres juegos de instrucciones, una computación — cada par equivalente (cosine ≈ 0.9999), ningún par bit-idéntico (Δ ≈ 1e-6). Los residuos son pequeños **porque la autoridad del router fijó las entradas y la selección de expertos**.",
      },
      {
        t: "p",
        md: "Una corrida posterior sobre hardware real **NVIDIA GB10 Grace Blackwell** cerró la matriz en el último backend: CUDA ↔ ROCm cayó en **cosine 1.0000000000** (max abs 3.5e-10, efectivamente bit-idéntico, ya que ambos backends de GPU comparten fuentes de kernel), y CUDA ↔ Grace ARM CPU en cosine 0.99975 — el mismo patrón GPU↔CPU visto arriba.",
      },
      { t: "h2", kick: "Por qué difieren los backends", text: "La suma en coma flotante no es asociativa" },
      {
        t: "ul",
        items: [
          "**Orden de reducción del matmul** — tensor cores, tiles MFMA, workgroups OpenCL y lanes SIMD acumulan en órdenes y teselados distintos.",
          "**Fusión FMA** — `a*b+c` redondeado una vez (FMA) o dos, fusionado distinto según backend.",
          "**Precisión de acumulación** — almacenamiento F16/BF16 con acumuladores F32 vs F16 (la mayor palanca de la divergencia).",
          "**Aproximaciones trascendentes** — variantes polinómicas/de tabla de exp (softmax), silu/sigmoid (swiglu), rsqrt (norms).",
          "**Ruta dequant + matmul** — decuantizar-y-multiplicar vs kernels cuantizados fusionados redondean los intermedios de forma distinta.",
          "**Kernels no deterministas** — las reducciones atomic/split-K pueden variar entre ejecuciones en el mismo dispositivo.",
        ],
      },
      { t: "p", md: "Nada de esto es un bug. Es el precio que paga la ruta rápida de cada acelerador." },
      { t: "h2", kick: "Por qué aun así funciona", text: "Una autoridad para las decisiones, precisión suficiente para la acumulación" },
      {
        t: "callout",
        md: "**AUTORIDAD DEL ROUTER — el invariante central.** La única decisión discreta dentro de la red es el enrutamiento MoE (top-8 de 256). Si cada backend re-ejecutara el router, los tokens fronterizos elegirían **expertos distintos** y divergirían de verdad. Kvasir ejecuta el router **una vez, en el backbone**, y envía a los workers solo los ids de expertos seleccionados. Un enjambre heterogéneo puede diferir en la *magnitud* de la salida de cada experto — nunca difiere en *qué expertos corren*. Esto convierte una divergencia discreta catastrófica en un error continuo acotado, y es la regla de coherencia del sharding heterogéneo de expertos.",
      },
      {
        t: "ul",
        items: [
          "**Argmax discreto:** decodificar es un argmax sobre logits. Un temblor de 1e-3 voltea un token solo cuando dos candidatos distan menos de 1e-3 — en la mayoría de posiciones el margen es mucho mayor, así que **los tokens salen idénticos**; los raros volteos ocurren en posiciones tan ambiguas como otra semilla.",
          "**El combine es suma:** los resultados parciales se funden como **suma** ponderada por probabilidad. Errores independientes de ~1e-4 se suman incoherentemente — crecen como √k, no k — y no hay cancelación de valores grandes, así que el residuo queda bien condicionado.",
        ],
      },
      { t: "h2", kick: "Dónde puede romperse · y las reglas que lo impiden", text: "Modos de divergencia y defensas" },
      {
        t: "table",
        head: ["Modo de divergencia", "Mecanismo", "Regla"],
        rows: [
          ["Desajuste de enrutamiento", "Los backends eligen top-8 distintos en tokens fronterizos", "Autoridad del router — decidido una vez en el backbone, ids despachados"],
          ["Bifurcación de trayectoria", "El temblor de logits por token acaba volteando uno; la secuencia se bifurca como otra semilla", "Decodificación/muestreo fijados a un nodo"],
          ["Acumulación en profundidad", "49 capas × ~1e-4 cada una → hasta 1e-2 en los logits finales", "Acumulación F32 en fronteras y combine"],
          ["Auto-no-determinismo", "Los kernels atomic/split-K varían entre ejecuciones", "Kernels de combine deterministas; la verificación usa tolerancias"],
          ["Desajuste de precisión", "Un nodo acumula en F16, otro en F32", "Precisión de acumulación anunciada como capacidad; nodos F32 preferidos para rangos de salida"],
        ],
      },
      { t: "h2", kick: "La equivalencia es un número", text: "El protocolo de medición" },
      {
        t: "ul",
        items: [
          "**Delta por operación** — mismas entradas, error relativo A vs B en matmul, swiglu, softmax, norm.",
          "**Deriva en la frontera de capa** — delta del residual tras una capa, apilada para ver si la profundidad acumula como √L o L.",
          "**Divergencia de logits extremo a extremo** — L∞, L2 y **divergencia KL** sobre el forward completo.",
          "**Acuerdo de decisiones** — acuerdo top-1 de tokens más acuerdo top-8 de enrutamiento (validando por qué la autoridad del router es necesaria).",
          "**Estabilidad de generación** — N tokens greedy; el primer índice donde A y B divergen.",
          "**Nivel de tarea** — deltas de perplexity y puntuaciones de eval: la única métrica que el usuario siente.",
        ],
      },
      {
        t: "p",
        md: "Un aprobado es una **tolerancia** — \"acuerdo top-1 ≥ 99.x%, KL ≤ ε\". Un nodo fuera de tolerancia se marca como no apto para rangos sensibles, no se rechaza de plano.",
      },
      { t: "h2", kick: "Por qué es tecnología central del enjambre", text: "El acuerdo de bits es imposible — e innecesario" },
      {
        t: "p",
        md: "Un clúster homogéneo puede asumir exactitud de bits; un enjambre no — su premisa es *el hardware que aparezca*. Así que Kvasir trata la equivalencia numérica exactamente como la compatibilidad de protocolo: un **contrato de primera clase, medido**. Los backends y la precisión de acumulación se anuncian como capacidades del nodo, la autoridad del router se impone como invariante, y cada verificación usa tolerancias en vez de igualdad de bits. **Equivalencia numérica medida + decisiones discretas de autoridad única** — eso es lo que permite que un modelo corra en cada GPU de la tierra a la vez. Eso es el enjambre.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell se unió al enjambre",
    dek: "Una GB10 Grace Blackwell computó rebanadas reales de FFN de expertos del 122B en CUDA e igualó a AMD ROCm bit a bit (cosine 1.0000000000), y a la Grace ARM CPU dentro de tolerancia. La matriz entre backends está completa.",
    blocks: [
      {
        t: "p",
        md: "La premisa de un enjambre es *el hardware que aparezca*. La equivalencia numérica — la prueba de que los workers CUDA, ROCm, Adreno y CPU emiten todos el mismo token — ya se había medido en ROCm, ARM de teléfono y numpy. NVIDIA es la ruta **por defecto y mejor optimizada** en el ggml/motor de inferencia de serie, pero era el único backend en el que la matriz no se había cerrado. Correr hardware Blackwell real la cierra.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. Sobre la misma rebanada real de expertos de layer-0 del Qwen3.5-122B, los dos backends de GPU son efectivamente bit-idénticos.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Medido · Qwen3.5-122B-A10B real, rebanada de expertos de layer-0", text: "La matriz entre backends" },
      {
        t: "table",
        head: ["Comparación", "Hardware", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "Los dos backends de GPU (CUDA, ROCm) comparten fuentes de kernel, así que caen **efectivamente bit-idénticos** (10⁻¹⁰). GPU↔CPU arrastra una perturbación de ~10⁻³ por operación por un orden de acumulación distinto, pero se mantiene equivalente en **cosine 0.99975** — el mismo patrón que el anterior ROCm↔ARM-de-teléfono 0.99992. El principio de autoridad del router se sostiene de nuevo en NVIDIA: **las decisiones discretas (argmax, selección de expertos) son invariantes sobre esta perturbación continua.**",
      },
      { t: "h2", kick: "Montaje", text: "Qué corrió, sobre qué" },
      {
        t: "ul",
        items: [
          "**Dispositivo** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124.5 GB de memoria unificada.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; el expert-worker de ggml/gguf puro compilado con kernels Blackwell.",
          "**Modelo** — Qwen3.5-122B-A10B-Q4_K_M, todos los expertos de layer-0 (256 expertos, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Método** — la rebanada L0 de 1.58 GB transmitida MI250 → GB10 (comparación sin pérdidas); la misma entrada (h/ids) corrida por CUDA, CPU y ROCm; vectores de salida float32 (36.864) comparados por cosine, L2 relativa y max-abs.",
        ],
      },
      {
        t: "callout",
        md: "**Un tropiezo en hardware real:** la GPU integrada del GB10 se clasifica como tipo de dispositivo ggml `ACCEL`, no `GPU` — así que `init_by_type(GPU)` no encontró nada. Arreglado seleccionando el primer dispositivo no-CPU en vez de codificar el tipo GPU.",
      },
      { t: "h2", kick: "Por qué importa", text: "La matriz está cerrada" },
      {
        t: "p",
        md: "Para que workers heterogéneos sirvan un modelo, una caja CUDA y una caja ROCm deben ser **intercambiables**, y una GPU y una CPU deben ser **numéricamente equivalentes**. Con Blackwell medido, ambas cosas se cumplen en toda la matriz de backends: los workers CUDA↔ROCm pueden sustituirse entre sí, y los workers GPU↔CPU concuerdan dentro de una tolerancia acotada y bien condicionada. El acelerador más común de la tierra es ahora un ciudadano verificado del enjambre.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "Endureciendo la ruta del dinero: seguridad transaccional en la liquidación de KVR",
    dek: "Tres clases reales de vulnerabilidad — replay de firmas de pago, acuñación de recompensas sin autenticación y doble gasto por carrera — encontradas, explotadas en tests y cerradas en el servicio de liquidación del gateway.",
    blocks: [
      {
        t: "p",
        md: "En una DePIN, la ruta del dinero es exactamente tan adversarial como la del cómputo: cada endpoint que acredita KVR será tarde o temprano tanteado por alguien que quiere KVR sin hacer el trabajo. Una pasada de seguridad sobre el servicio de liquidación del gateway — el proceso que verifica pagos on-chain y acredita stakes, recompensas de nodo y cargos de inferencia — encontró y cerró **tres clases reales de vulnerabilidad**. Cada una se demostró con un test estilo exploit antes del arreglo y se re-verificó después.",
      },
      { t: "h2", kick: "El modelo de confianza", text: "Verificar hechos on-chain, no afirmaciones del cliente" },
      {
        t: "p",
        md: "El modelo de custodia de Kvasir deja las claves con los usuarios: las wallets firman transacciones, Solana las registra, y el único trabajo del servicio de liquidación es **verificar qué ocurrió realmente en la cadena** antes de tocar un saldo. Los pagos siguen *quote → payment → inference*, y cada firma de transacción consumida se registra en un registro de un solo uso `usedSignatures`, de modo que nunca puede presentarse dos veces. Eso convierte al servicio de liquidación en el cuello de botella — y la regla que jamás debe romper es: acreditar solo lo que la cadena demuestra, nunca lo que el cliente afirma.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Arreglo #1 · vinculación del remitente", text: "Atar el pago al pagador" },
      {
        t: "p",
        md: "Las firmas de Solana son **públicas**. La ruta de verificación de stake comprobaba que el vault *recibió* el KVR esperado — pero nunca *quién lo envió*. Un atacante podía vigilar en devnet la transferencia KVR→vault de una víctima y luego enviar `{owner: atacante, signature: la de la víctima}`: el chequeo de recepción del vault pasaba, el principal se acreditaba al atacante, y un unstake después los fondos eran suyos. Robo directo, usando nada más que un explorador de bloques.",
      },
      {
        t: "code",
        caption: "El arreglo: el KVR debe haberse debitado de cuentas de token cuyo dueño sea el owner acreditado.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Arreglo #2 · reportero de confianza", text: "Recompensas solo desde fuentes autenticadas" },
      {
        t: "p",
        md: "Los endpoints de recompensa de nodos acuñaban KVR reclamable a partir de **entrada autoinformada**: `POST /api/node/contribution` acreditaba las `units` que el cliente afirmara — con `units: 1e9` y una llamada de claim se podía vaciar el vault — y register/heartbeat aceptaban roles autodeclarados de hub/gateway (recompensas de infraestructura por hora) y puntuaciones de rendimiento (multiplicadores). El arreglo pone cada afirmación que afecta a recompensas tras un **reportero de confianza**: solo el token de servicio M2M usado por el sondeo de contribución del hub, o un admin autenticado, puede afirmar units, roles de infra o niveles de rendimiento — impuesto incluso en modo LAN abierto, porque esto acuña KVR. La comparación del token es de tiempo constante, y el enlace wallet↔nodo sigue libre; solo que ya no puede autoafirmar sus recompensas.",
      },
      { t: "h2", kick: "Arreglo #3 · serialización de la liquidación", text: "Un solo escritor por saldo" },
      {
        t: "p",
        md: "El estado de liquidación era un read-modify-write sin bloqueo, y cada operación de dinero hace *await* de un pago o verificación on-chain a mitad de camino — cediendo el event loop con un saldo obsoleto en la mano. Dos claims concurrentes podían leer el mismo saldo pendiente de 100 KVR y ambos pagarlo. No era teórico: el test de exploit mostró **tres claims concurrentes pagando 300 por un saldo de 100**.",
      },
      {
        t: "code",
        caption: "Serialización asíncrona por clave: las operaciones de dinero con la misma clave corren estrictamente una tras otra.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Defensa en profundidad", text: "Dónde queda cada capa" },
      {
        t: "table",
        head: ["Capa", "Mecanismo"],
        rows: [
          ["Identidad", "Login por firma de wallet SIWS sobre un nonce del servidor + 2FA TOTP + códigos de respaldo de un solo uso"],
          ["Transporte", "Node tokens derivados de la wallet en descargas de shards; acuerdo de huella de build en el relay"],
          ["Pago", "Vinculación del remitente en transferencias de stake; usedSignatures de un solo uso; requestId privado en inferencia"],
          ["Liquidación", "Bloqueos por clave en cada escritura de saldo; pagar primero y debitar después; reenvío idempotente"],
          ["Reportes", "Hechos que afectan recompensas solo del token de servicio M2M o admin, comparados en tiempo constante"],
          ["Custodia", "Wallets no custodiales — el servicio solo puede mover lo que el vault contiene, nunca claves de usuario"],
        ],
      },
      { t: "h2", kick: "Medido, no asumido", text: "Cada arreglo lleva su propio test de exploit" },
      {
        t: "ul",
        items: [
          "Reproducir la firma de transferencia de una víctima bajo la wallet de un atacante ahora se rechaza (\"not sent by owner\"); stakes legítimos, sobre-reclamos y pagos de inferencia se comportan igual.",
          "Tres claims concurrentes contra un saldo pagan **exactamente una vez**; reenviar una inferencia ya pagada devuelve idempotentemente el mismo resultado.",
          "Las `units` autoafirmadas, los roles de hub/gateway y los niveles de rendimiento de clientes sin autenticar ya no mueven ni un lamport de recompensas.",
        ],
      },
      {
        t: "p",
        md: "El hilo conductor de los tres arreglos es un principio aplicado de tres maneras: **la cadena es la fuente de verdad, el servicio es un verificador, y cada saldo tiene exactamente un escritor**. El servicio de liquidación aún corre en Solana devnet — que es exactamente donde quieres encontrar, explotar y arreglar estas clases antes de que mainnet suba las apuestas.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — El motor: linkcpp, un plano de control para motor de inferencia",
    dek: "motor de inferencia trae un plano de datos RPC capaz pero sin plano de control. linkcpp añade la mitad que falta — descubrimiento, planificación, arranque y gateways — alrededor de binarios de serie.",
    blocks: [
      {
        t: "p",
        md: "Todo lo que corre Kvasir empieza aquí. **linkcpp** es un plano de control de código disponible (Business Source License) alrededor del plano de datos RPC de motor de inferencia: ejecuta grandes modelos de IA en múltiples GPU y máquinas usando binarios `ggml-rpc-server` / `llama-server` *de serie*. El plano de datos queda sin bifurcar — todo lo que linkcpp añade es orquestación.",
      },
      { t: "h2", kick: "El hueco", text: "Un plano de datos sin plano de control" },
      {
        t: "p",
        md: "motor de inferencia ya puede repartir un modelo entre máquinas por RPC — pero alguien tiene que descubrir las GPU, decidir qué capas van dónde, lanzar los workers correctos con los presupuestos correctos, comprobar que cada nodo habla el mismo protocolo y exponer una API que los desarrolladores puedan llamar. Hacerlo a mano para un clúster es un fastidio; hacerlo para una red abierta de dispositivos de desconocidos es imposible. Esa capa de coordinación es linkcpp.",
      },
      { t: "h2", kick: "Arquitectura", text: "Un hub, workers de serie, gateways estándar" },
      {
        t: "code",
        caption: "Flujo de la petición — el hub orquesta, los binarios de serie computan.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock motor de inferencia engines below" },
      {
        t: "ul",
        items: [
          "**Tres maneras de unirse:** **slots locales de nodo** fijos con presupuestos VRAM/RAM/CPU editables; **unidades remotas** — registra otro hub e importa sus nodos; y **agentes de nodo gestionados** — servicios solo-worker que se unen por HTTP simple de petición/respuesta, deliberadamente sin stream persistente, para sobrevivir enrutamientos simples de LAN/VPN.",
          "**El gating de compatibilidad es de primera clase:** cada unidad, nodo y agente reporta una identidad de protocolo/runtime-pack más detalles de backend. Los desajustes de unidad, runtime-pack, revisión de motor de inferencia y ABI de RPC se **bloquean duro antes de bind, plan, load o infer** — las diferencias de backend (CUDA/Metal/Vulkan/CPU) se registran como capacidades, no rechazos.",
          "**El planner** lee metadata GGUF y produce colocación contigua de capas por nodo, `--tensor-split`, estimaciones de VRAM de KV-cache/capa/experto, y offload opcional de FFN de expertos a RAM.",
          "**Gateways:** cada controlador expone endpoints compatibles con OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) y con Anthropic (`/anthropic/v1/messages|models`), respaldados por el mismo modelo cargado — los clientes existentes funcionan sin cambios.",
        ],
      },
      {
        t: "p",
        md: "Esta separación deliberada — un plano de datos sin modificar bajo un plano de control abierto — es la base de todo lo posterior: el ring runtime, el mercado de capas y finalmente el enjambre de expertos son evoluciones del plano de control sobre el mismo cómputo de serie.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — El anillo: inferencia en pipeline sin master",
    dek: "Cada dispositivo carga solo su ventana de capas y pasa una pequeña frontera de hidden-state a su vecino. Ningún nodo tiene el modelo; no existe master central.",
    blocks: [
      { t: "h2", kick: "Por qué no una estrella", text: "El master RPC es cuello de botella y portero" },
      {
        t: "p",
        md: "En la topología RPC clásica un master abre el **GGUF entero** y marca hacia cada worker. Esa forma se rompe en una red abierta de tres maneras: el master debe tener y servir el checkpoint completo; cada worker debe ser marcable — los teléfonos tras NAT de operadora no lo son; y el master es un dueño único en una red que no debería tener ninguno.",
      },
      { t: "h2", kick: "El anillo", text: "Ventanas de capas + paso de fronteras" },
      {
        t: "ul",
        items: [
          "Cada dispositivo almacena el mismo modelo pero **carga solo su ventana de capas contigua**, y abre exactamente dos enlaces: uno a su predecesor, otro a su sucesor.",
          "Una petición entra al anillo; cada nodo corre sus capas y pasa solo la **frontera de hidden-state** a su vecino. El último rank muestrea el token y lo envía de vuelta — sin master central, y ningún nodo tiene el modelo completo.",
          "La colocación viene del **rank manifest** del planner — para Qwen3.5-122B, 49 capas repartidas entre la mezcla de GPU, CPU, NPU y teléfono que aparezca.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Convertir dispositivos débiles en miembros reales", text: "Shards parciales, GPU móviles y el relay 443" },
      {
        t: "ul",
        items: [
          "**Descarga de shard parcial:** un stage del anillo no necesita el checkpoint — necesita su ventana. Un mini-GGUF de stage lleva solo esos tensores (**254 MB con 26 tensores** frente al modelo completo de 77.6 GB), así que un teléfono baja ~1.5 GB para una ventana de una capa en vez de todo.",
          "**Ruta de GPU móvil:** la ruta RPC hacia la GPU de un teléfono resultó inviable (el layout de buffers OpenCL de Adreno no sobrevive a la serialización RPC), pero un **stage del anillo corre directamente en la GPU Adreno** — el stage posee su backend localmente, así que solo cruzan el cable las fronteras.",
          "**Travesía de NAT:** los teléfonos no pueden aceptar conexiones entrantes, así que el plano de datos pasa por un **relay 443** — un puente WebSocket por edge con preámbulo de rol de 1 byte que deja a ambos extremos marcar hacia fuera. El teléfono abre cero puertos entrantes.",
          "**Mercado de auto-inscripción:** los stages se reclaman, no se asignan. Un nodo consulta el mapa de cobertura/demanda, elige la ventana sin cubrir de **mayor recompensa**, la descarga y se une — verificado extremo a extremo con un teléfono tras NAT completando inferencia en anillo y ganando su contribución.",
        ],
      },
      { t: "h2", kick: "Dónde encaja el anillo", text: "La ruta de baja latencia" },
      {
        t: "p",
        md: "El anillo es la ruta de **latencia** de Kvasir: las fronteras son pequeñas, los saltos pocos, y la decodificación fluye por el bucle sin reunir nada centralmente. Su límite es la granularidad — la unidad mínima que un nodo puede cargar es una capa (~1.4 GB en el 122B). Quitar ese suelo es lo que hace el enjambre de expertos; el anillo sigue siendo la columna de servicio a la que se enchufa.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — Dentro de un MoE de 122B: por qué los pesos quieren ser fragmentados",
    dek: "Un análisis a nivel de tensores de Qwen3.5-122B: el 86% de los bytes son 12.544 losas de expertos independientes, cada una a una limpia copia de rango de bytes de valerse sola.",
    blocks: [
      {
        t: "p",
        md: "Antes de diseñar nada, desmontamos el 122B en disco. La pregunta: si un enjambre de dispositivos débiles va a cargar este modelo, ¿cuál es la unidad natural de carga? La respuesta cayó del propio layout de tensores del GGUF.",
      },
      { t: "h2", kick: "Anatomía · Qwen3.5-122B-A10B (Q4_K_M)", text: "De qué está hecha realmente una capa MoE" },
      {
        t: "stats",
        items: [
          { n: "49", l: "capas" },
          { n: "256", l: "expertos / capa" },
          { n: "8", l: "activos / token" },
          { n: "12,544", l: "expertos en total" },
          { n: "5.3 MB", l: "un experto (Q4)" },
          { n: "86%", l: "del peso en expertos" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "checkpoint completo" },
        ],
      },
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Cada capa se divide en una **ruta densa** — atención + KV, las norms, el router (`ffn_gate_inp`), un experto compartido — y un **banco de expertos**: 256 FFN independientes almacenadas como tres tensores apilados (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). La ruta densa es la minoría de los bytes; el banco de expertos es el 86% del modelo.",
      },
      { t: "h2", kick: "El regalo del layout", text: "Los expertos son losas contiguas alineadas a bloques" },
      {
        t: "ul",
        items: [
          "El índice de experto es la **dimensión ggml más externa** (`ne[2]`) de cada tensor de expertos — el experto *e* ocupa una losa contigua y alineada a bloques de cuantización de bytes cuantizados crudos.",
          "Eso hace que la extracción por experto sea una **copia de rango de bytes**: `data[a:b]`, sin decuantización, sin re-empaquetado — un mini-GGUF por expertos es barato de producir y fiel al bit.",
          "Por token solo se activan **8 de 256** expertos por capa, elegidos por el router — así que en decodificación el tráfico de expertos de una capa es un puñado de pequeñas multiplicaciones de matrices sobre un vector hidden.",
        ],
      },
      { t: "h2", kick: "La implicación", text: "La unidad de carga baja de 1.4 GB a 5.3 MB" },
      {
        t: "p",
        md: "A grano de capa, lo mínimo que un nodo puede sostener es ~**1.4 GB** — fuera del alcance de la mayoría de los teléfonos una vez que la app, el KV y el SO toman su parte. A grano de experto, la unidad es **5.3 MB**, y una contribución realista son 8–64 expertos (**42–340 MB**) — cómodamente dentro de cualquier dispositivo moderno. Los expertos son mutuamente independientes, así que la propiedad puede dispersarse arbitrariamente y reequilibrarse con libertad. Este análisis es lo que convirtió el sharding a nivel de experto en la apuesta de diseño: los pesos ya venían empaquetados en unidades del tamaño del enjambre — la red solo tenía que honrar el empaquetado.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — Offload de expertos del backbone a RAM (M0)",
    dek: "Transmite las FFN de expertos MoE desde la RAM de la CPU en vez de la VRAM, y un solo coordinador de 64 GB sostiene un 122B — sin cirugía de grafo.",
    blocks: [
      {
        t: "p",
        md: "Las FFN de expertos no tienen que vivir en VRAM. Transmitirlas desde la RAM de la CPU permite que un coordinador sostenga un modelo cuyos expertos exceden su VRAM — la base que permite que los nodos débiles se unan a un gran MoE.",
      },
      { t: "h2", kick: "Planner verificado · GGUF real del 122B", text: "Un 122B cabe en un solo coordinador de 64 GB" },
      {
        t: "p",
        md: "Antes, el anillo colocaba pesos solo en VRAM, así que el 122B (77.6 GB) era **infeasible** en un GCD de 64 GB. Con las reglas de offload de expertos, el dry-run vuelve **feasible**:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "plan de anillo 122B" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (expertos)" },
          { n: "10", l: "capas descargadas" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Salida del planner — formato de reglas -ot de motor de inferencia.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # motor de inferencia -ot format`,
      },
      { t: "h2", kick: "Qué se cableó · Python puro, sin recompilar C++", text: "Llevar las reglas de offload del planner a una carga real" },
      {
        t: "ul",
        items: [
          "**planner** — ya emite `ot` (reglas `-ot` unidas por comas) en cada placement.",
          "**protocol.py** — se añadió el campo `StageStartRequest.ot`.",
          "**runtime.py** — reenvía el `ot` del placement a la petición de stage.",
          "**stage_service.py** — el coordinador arranca con `--override-tensor`.",
          "`linkcpp-server` reenvía argumentos desconocidos al llama-server de serie, así que `-ot` se aplica intacto.",
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
        md: "Verificado con un test de ida y vuelta del protocolo `ot` más la confirmación de que el comando del coordinador emite `--override-tensor`; el hub se redesplegó limpio y sin regresiones. Lo que quedaba entonces: la carga completa de 77 GB en 2 nodos (coordinador con offload + un teléfono con una ventana de una capa de ~1.5 GB), a la espera de servidor. El núcleo de M0 — el offload del backbone que permite a los nodos débiles participar en un MoE grande — quedó completo a nivel de código y planner.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — La ruta de datos de rebanadas de expertos (M1)",
    dek: "Un dispositivo débil descarga unos pocos expertos de 6 MB, no una capa de 1.4 GB — y el cómputo fragmentado iguala al monolítico hasta 3.6e-12.",
    blocks: [
      { t: "h2", kick: "Verificado · Qwen3.5-122B-A10B real", text: "Una rebanada de experto es una copia de bytes — sin decuantización" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "rebanada en dim experto" },
          { n: "~6.1", l: "MB / experto (Q4+Q6)" },
          { n: "206 MB", l: "descarga 2 capas × 16 exp." },
          { n: "200", l: "HTTP, GGUF válido" },
        ],
      },
      {
        t: "p",
        md: "Los tensores de expertos MoE apilan todos los expertos a lo largo de la dimensión ggml más externa, así que el lector expone `(n_expert, rows, row_bytes)` de bytes cuantizados crudos. El experto *e* es una losa contigua alineada a bloques de cuantización — la rebanada es literalmente `data[a:b]`, sin decuantización ni re-empaquetado.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — la ida y vuelta verificada.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "El oráculo numérico", text: "dispatch + combine == monolítico, exactamente" },
      {
        t: "p",
        md: "Con expertos reales de layer-0 del 122B (referencia decuantizada), dividir los expertos en 4 shards, computar cada uno por separado y combinar **iguala a la FFN MoE monolítica**: el sharding es un reagrupamiento exacto de la misma suma ponderada, no una aproximación.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "error relativo" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "expertos tocados" },
        ],
      },
      { t: "h2", kick: "Worker C++, verificado en hardware", text: "linkcpp-expert-worker reproduce el oráculo en ROCm" },
      {
        t: "ul",
        items: [
          "**ggml/gguf puro** (sin libllama): carga la rebanada en un backend de GPU y corre `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`.",
          "**Compilación + ejecución ROCm** en un MI250: 122B layer-0, expertos [0,8), 4 tokens.",
          "**Cosine 0.99995 vs el oráculo**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — este residuo es en sí el primer caso medido de equivalencia entre backends (ROCm vs numpy).",
          "La misma ruta de código cubre CUDA/Metal/Vulkan/CPU (`mul_mat_id`/`swiglu` son ggml de serie; CUDA tiene un kernel MoE dedicado).",
        ],
      },
      {
        t: "p",
        md: "La pieza más difícil y arriesgada — el kernel del worker en dispositivo — quedó verificada aquí. Lo que faltaba era la orquestación backbone↔worker; el worker es una función pura probada que consume estas rebanadas.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — Dispatch distribuido de expertos (M2)",
    dek: "Una decodificación en vivo del 122B entrega el cómputo de expertos de una capa a un proceso worker separado por TCP — y predice exactamente el mismo token.",
    blocks: [
      { t: "h2", kick: "Verificado · 122B real, dos procesos", text: "Decodificación del backbone → TCP → worker → experts → mismo token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "cosine de logits" },
          { n: "0", l: "pérdida de transporte (byte-identical)" },
          { n: "2", l: "procesos (backbone + worker)" },
        ],
      },
      {
        t: "p",
        md: "El worker de expertos sirve la rebanada de layer-0 como **proceso separado** (ROCm), y el callback de dispatch de `build_moe_ffn` del backbone 122B envía `(cur, sel)` por TCP y recibe las salidas de expertos. El cosine de logits es **exactamente el valor in-process** (0.99868775) — el transporte no pierde nada. El cómputo en enjambre expert-parallel funciona a través de una frontera de proceso.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "Una conexión TCP de larga vida — el mismo stream que el anillo/relay 443 puede tunelizar.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Hecho", text: "La cadena de dispatch distribuido" },
      {
        t: "ul",
        items: [
          "Modo `--serve`: cargar la rebanada, escuchar en TCP, responder `(n_used, n_tokens, cur, sel) → experts`.",
          "`--dispatch-port`: el callback del backbone envía/recibe por TCP a un worker separado, reemplazando el cómputo in-process.",
          "Medido en una decodificación en vivo del 122B con layer-0 despachado fuera de proceso → **argmax MATCH**, cosine 0.99869 (= in-process, sin pérdidas).",
          "Núcleo de M2 (antes): la ARM del teléfono computó expertos reales del 122B con cosine 0.99992 (cross-build de Android).",
        ],
      },
      {
        t: "p",
        md: "Lo que sigue desde aquí: tunelizar el mismo stream TCP por el **relay 443** hacia workers en otras máquinas y teléfonos (el transporte ya se probó en el trabajo del anillo), y luego el mercado de cobertura M3 y el throughput por lotes M4.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — El mercado de cobertura de expertos (M3)",
    dek: "Los nodos débiles ven qué (capa, rango de expertos) es más escaso y mejor pagado, y lo llenan ellos mismos — el mercado de capas probado, con grano más fino.",
    blocks: [
      {
        t: "p",
        md: "El mercado de shards de capas de Kvasir — mapa de demanda, auto-inscripción por máxima recompensa, descarga parcial, recompensas por nodo — ya estaba verificado en dispositivo. M3 re-parametriza el mismo mecanismo al grano de **(capa, rango de expertos)**, de modo que la cobertura se auto-repara hacia los rangos de expertos más sub-replicados y mejor pagados.",
      },
      { t: "h2", kick: "Verificado · API", text: "Agregación de escasez → asignación del rango de máxima recompensa" },
      {
        t: "p",
        md: "Tres workers se registran en la capa 0: A = [0,128), B = [128,256), C = [0,128) como segunda réplica, con `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["capa", "expertos", "réplicas", "escasez"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (objetivo cumplido)"],
          ["0", "[128, 256)", "1", "0.5 (bajo objetivo)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → recorta el rango más escaso al presupuesto del nodo.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Hecho · Python puro (hub)", text: "Un mercado de oferta/demanda a grano de experto" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — los workers laten con sus tenencias de (capa, rango de expertos).",
          "`GET /api/expert-demand` — agregación de réplicas por experto → segmentos contiguos de rangos con puntuaciones de escasez.",
          "`POST /api/expert-volunteer` — asigna el rango más escaso recortado al presupuesto del nodo.",
          "El mercado de capas existente (auto-inscripción · descarga parcial · recompensas) re-parametrizado a (capa, rango de expertos).",
        ],
      },
      {
        t: "p",
        md: "Lo que sigue en M4: dispatch por lotes para peticiones concurrentes más una caché de hot-experts — tokens/s proporcional al número de workers — y enrutamiento de réplicas (el worker más cercano/rápido) con respaldo ante churn.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — Throughput del dispatch por lotes (M4)",
    dek: "El enjambre es un tejido de throughput, no una jugada de latencia: agrupar las llamadas de dispatch amortiza el overhead por petición 77× por token.",
    blocks: [
      { t: "h2", kick: "Medido · ROCm, FFN de expertos, n_used = 8", text: "Lotes más grandes, más tok/s por worker" },
      {
        t: "table",
        head: ["lote", "tok/s por worker"],
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
        md: "De **1.45 ms/tok** con lote 1 a **0.019 ms/tok** con lote 512 — una mejora de 77× por token. El tiempo por llamada apenas se mueve (1.45 → 9.6 ms) mientras el lote crece 512× — la GPU procesa el lote casi gratis tras un overhead fijo. Esta es la **propiedad de tejido de throughput** que hace práctico el expert-parallel: el dispatch por lotes amortiza el RTT y el overhead por petición.",
      },
      { t: "h2", kick: "Hecho", text: "Throughput del dispatch por lotes" },
      {
        t: "ul",
        items: [
          "Worker `--bench`: tiempos de compute_dispatch para lotes 1…512 → tok/s.",
          "**53k tok/s por worker** con lote 512 (ROCm) — el loteo amortiza el overhead.",
          "Sobre esto se apilan la caché de hot-experts y el escalado agregado multi-worker (enrutamiento de réplicas).",
        ],
      },
      {
        t: "callout",
        md: "Con M4, **toda la cadena M0 → M4 queda demostrada en un 122B real**: offload del backbone · rebanadas de expertos · workers verificados · dispatch en decodificación en vivo (argmax MATCH) · procesos distribuidos · mercado de cobertura · throughput por lotes.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "Un teléfono se unió a la inferencia de un 122B",
    dek: "Un Galaxy S25 descargó de forma autónoma su rebanada de expertos desde el hub y computó los expertos de una capa en cada paso de una decodificación en vivo del 122B. La salida fue correcta.",
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → generado (con el teléfono en el bucle): **\" Paris.\"** — 8/8 tokens idénticos a la ejecución local.",
      },
      { t: "h2", kick: "Medido · 122B real, el teléfono computa layer-0", text: "Corrección + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens idénticos al local" },
          { n: "4.01", l: "TPS local (línea base)" },
          { n: "3.13", l: "TPS con teléfono" },
          { n: "1.58 GB", l: "descarga autónoma" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Incluso con el teléfono computando los expertos de layer-0 para cada token, **los tokens generados son exactamente los locales** — el correcto \"Paris.\". El TPS baja de 4.01 a 3.13 — la ida y vuelta del dispatch al teléfono (MI250 → túnel → teléfono, ~100 ms/token) cuesta un 22%. El throughput se recupera con lotes y réplicas (M4).",
      },
      { t: "h2", kick: "El flujo de participación autónoma", text: "Descubrir → descarga guiada por recompensa → unirse al cómputo" },
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
      { t: "h2", kick: "Verificado vs pendiente", text: "El mecanismo está completo; el bucle en la app es productización" },
      {
        t: "ul",
        items: [
          "Descarga parcial (el endpoint expert-shard), servicio del worker, dispatch del backbone, generación en vivo del 122B y TPS — todo verificado en el dispositivo real.",
          "Corrección: con el teléfono participando, 8/8 tokens igualan la ejecución local, con la respuesta correcta.",
          "Pendiente: el bucle autónomo en la app (sondear expert-demand → volunteer → descargar → serve → registrar) es cableado Kotlin — esta demo condujo el mecanismo directamente.",
          "Transporte: esta demo usó un túnel SSH; producción usa el relay 443 (ya verificado en el trabajo del anillo).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "La economía Kvasir: un círculo virtuoso de coste y recompensa",
    dek: "Una red descentralizada de inferencia solo funciona si el precio que pagan los consumidores y la recompensa que ganan los nodos se refuerzan mutuamente. Este es el volante de inercia hacia el que construimos, las espirales que lo matan, y las tres invariantes que lo mantienen girando.",
    blocks: [
      {
        t: "callout",
        md: "**Tesis:** Kvasir es un mercado de dos lados liquidado en un solo token — los consumidores pagan KVR por inferir, los nodos ganan KVR por servir. Todo el diseño triunfa o fracasa por una propiedad: esos dos lados deben formar un **círculo virtuoso**, donde cada vuelta hace más fácil la siguiente. Equivócate en eso y cualquier política de precios acabará colapsando; acierta y la red crece *más barata* a medida que crece *más grande*.",
      },
      {
        t: "p",
        md: "Es tentador tratar el coste y la recompensa como un tira y afloja — cada dólar que ahorra un consumidor es un dólar que un nodo no gana. Ese encuadre es una trampa. En una red sana son el **mismo volante de inercia** visto desde dos extremos: los pagos se vuelven recompensas, las recompensas se vuelven oferta, la oferta se vuelve capacidad y precios más bajos, los precios más bajos se vuelven más uso, y más uso se vuelve más pagos. La pregunta no es cómo repartir un pastel fijo; es cómo mantener la rueda girando para que el pastel crezca.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "El volante de inercia", text: "Por qué el uso y la oferta crecen juntos" },
      {
        t: "p",
        md: "El motor del ciclo es una sola regla ya cierta en Kvasir: **la inferencia debe pagarse en KVR**. Eso hace de cada unidad de uso una unidad de demanda real del token — utilidad, no especulación. La demanda del token sostiene el valor del KVR que ganan los nodos; las recompensas atractivas atraen oferta; la oferta expande la capacidad y, mediante la competencia y un sharding de expertos más fino, empuja hacia abajo el coste marginal de servir; un servicio más barato, rápido y capaz atrae más uso. Kvasir aprieta el bucle con una propiedad que ninguna API centralizada puede copiar: un participante puede ser **consumidor y proveedor a la vez**. El lado de la demanda y el lado de la oferta a menudo crecen dentro de las *mismas personas*, lo que amortigua los desequilibrios que arruinan los mercados de un solo lado.",
      },
      { t: "h2", kick: "Los modos de fallo", text: "Cuatro espirales que hacen girar la rueda al revés" },
      {
        t: "p",
        md: "Un volante de inercia puede desacelerar tan fácil como acelerar. Nombrar las espirales de la muerte es cómo se diseña contra ellas:",
      },
      {
        t: "table",
        head: ["Espiral", "Cómo empieza", "Dónde termina"],
        rows: [
          ["Dilución de recompensa", "Más nodos persiguen una demanda plana", "La recompensa por nodo cae, los nodos se van, la capacidad baja"],
          ["Precio demasiado bajo", "Precio barato, recompensas por debajo del coste del nodo", "Servir deja de ser rentable, la oferta y la calidad colapsan"],
          ["Precio demasiado alto", "Buenas recompensas, pero por encima del mercado", "Los usuarios eligen una API más barata, los ingresos se secan"],
          ["Dependencia de la emisión", "Recompensas pagadas acuñando, no con ingresos", "La inflación erosiona el KVR hasta que ambos lados se rinden"],
        ],
      },
      { t: "h2", kick: "Las invariantes", text: "Tres reglas que mantienen virtuoso el ciclo" },
      {
        t: "ul",
        items: [
          "**Las recompensas se financian con ingresos reales.** En estado estacionario, lo que ganan los nodos viene de lo que pagan los consumidores — no de una emisión de tokens sin límite. La emisión es un subsidio de arranque que debe *atenuarse* a medida que crecen los ingresos por tarifas. Kvasir ya ayuda aquí recompensando el **trabajo real** — KVR por tokens realmente servidos × cuota de capa, no la mera presencia — así el subsidio no puede filtrarse a nodos 'mercenarios' ociosos.",
          "**El KVR es el medio obligatorio.** Como no puedes inferir sin pagar KVR, el uso es un sumidero de demanda permanente para el token. Eso ancla el valor del token a la utilidad real en vez de a la especulación — la diferencia entre una moneda y una ficha.",
          "**El precio flota dentro de una banda.** Un suelo mantenido por encima del coste marginal del nodo hace que servir valga la pena; un techo mantenido por debajo de las alternativas centralizadas mantiene competitivo a Kvasir. Entre ellos, el precio se mueve — que es donde el crecimiento de la red por fin se manifiesta como un coste menor.",
        ],
      },
      { t: "h2", kick: "El termostato", text: "Hacer \"más nodos → más barato\" cierto en el código" },
      {
        t: "p",
        md: "Hoy el precio es una constante gobernada — sensato para una devnet, pero significa que añadir nodos aumenta la *capacidad*, no la asequibilidad. La dirección de diseño es un **precio guiado por la utilización**: la oferta ociosa empuja el precio hacia abajo hacia el suelo, la congestión lo empuja hacia arriba hacia el techo. Esa única señal convierte la intuición *\"cuanta más gente comparte cómputo, más barato se vuelve\"* en una regla que el protocolo hace cumplir — mientras el suelo mantiene solventes a los operadores para que la oferta que lo abarató no se evapore. Como el precio es un parámetro económico sensible, cambia solo bajo la **autoridad de la wallet genesis con firma de wallet + 2FA**, nunca una variable de entorno perdida.",
      },
      {
        t: "callout",
        md: "**\"Gratis\" es el neto, no el precio.** Pagas por lo que infieres y ganas por lo que sirves; contribuye aproximadamente tanto como consumes y tu factura se salda a cero. Ninguna API por suscripción — Claude Max, un asiento de Codex — puede ofrecer eso, porque nunca puedes ser su lado de la oferta. Con Kvasir puedes ejecutar modelos que tu propia máquina no puede sostener *y* cobrar por ayudar a otros a ejecutar los suyos.",
      },
      {
        t: "p",
        md: "Nada de esto requiere un diseño de mecanismos exótico. Requiere disciplina en tres cosas: recompensa desde los ingresos, valor desde el uso, equilibrio desde un precio flotante acotado. Kvasir ya entrega las partes difíciles y honestas — liquidación no custodial, recompensas proporcionales al trabajo, un token que de verdad debes gastar para usar la red. El resto es la hoja de ruta económica: la atenuación, el reparto de tarifas que financia un fondo de seguro para inferencias fallidas, y el termostato. Construidos en ese orden, el coste y la recompensa dejan de pelear y empiezan a componerse.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "Una GPU al otro lado de internet se unió a la inferencia de 122B",
    dek: "Una estación de trabajo Blackwell en otra ciudad marcó una única conexión saliente 443 y computó expertos para una decodificación en vivo de 122B — idéntica byte a byte a una corrida local, y pagada en KVR por el trabajo que hizo.",
    blocks: [
      {
        t: "callout",
        md: "**Qué pasó:** una decodificación de 122B corriendo en un backbone AMD en un lugar envió su trabajo de expertos por token a una máquina NVIDIA GB10 (Grace Blackwell) en otra ciudad — por un único WebSocket saliente en el puerto 443 — y recibió de vuelta salidas de expertos que produjeron **exactamente los mismos tokens** que computar localmente. Sin túnel, sin reenvío de puertos, sin agujero entrante en el firewall. La caja remota ganó KVR por los bytes que sirvió.",
      },
      {
        t: "p",
        md: "La premisa de Kvasir es *el hardware que aparezca* — incluido hardware tras NAT de operadora, en internet público, en otra ciudad. Qwen3.5-122B-A10B lleva el **86% de su peso en 12.544 expertos independientes** (48 capas × 256, top-8), cada uno una función pura de 5.3 MB. Ese grano es lo que permite que una máquina distante y ajena sostenga una rebanada y contribuya. La pregunta abierta nunca fue *¿podemos partirlo?* — fue *¿puede un worker al otro lado de internet abierto participar de verdad en una decodificación en vivo, de forma correcta y con rendición de cuentas?*. Ahora lo ha hecho.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "Una sola marcación saliente", text: "Sin túnel, sin puertos entrantes" },
      {
        t: "p",
        md: "El worker remoto abre **una** conexión — `wss://` saliente al gateway público en 443, el único puerto que el NAT de operadora y los edges de CDN dejan pasar de forma fiable. El gateway no parsea el stream; **empalma en crudo** el WebSocket al hub solo-LAN, que lo puentea al listener de dispatch de expertos del backbone. Ambos extremos marcaron hacia fuera y se encontraron en el medio. El worker expone cero puertos entrantes y no necesita dirección pública.",
      },
      {
        t: "code",
        caption: "Dos marcaciones salientes, empalmadas en un único stream de dispatch ordinario.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Idéntico byte a byte a través de internet", text: "El router decide una vez; la matemática reagrupa exactamente" },
      {
        t: "p",
        md: "El backbone corre el router **una vez** y con autoridad; el worker es una función pura `(hidden, ids) → out`. Así que mover esa función a otro continente cambia *dónde* ocurre la multiplicación, no *qué* computa. En una decodificación en vivo del 122B con los expertos de layer-0 servidos remotamente: el stream de tokens greedy fue **8/8 idéntico** (\" Paris.\"), **cosine de logits 0.99773**, argmax coincidente. Es la misma propiedad de autoridad del router que mantiene invariantes las decisiones discretas CUDA↔ROCm↔CPU — los backends heterogéneos quedan acotados por error continuo, nunca una bifurcación catastrófica.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "tokens greedy idénticos" },
          { n: "0.99773", l: "cosine de logits, remoto vs local" },
          { n: "1.2%", l: "sobrecoste de TPS, directo (13 ms RTT)" },
          { n: "0.00895", l: "KVR al worker, primera sesión remota" },
        ],
      },
      { t: "h2", kick: "El coste honesto es el RTT", text: "Por qué el enjambre es un tejido de throughput, no un decodificador de baja latencia" },
      {
        t: "p",
        md: "El dispatch serial por token paga una ida y vuelta por paso. Medido: con un enlace directo (13 ms RTT) el sobrecoste de throughput fue del **1.2%** (4.220 → 4.169 tok/s); enrutado por un edge de CDN en 443 fue del **~28%**. Lo publicamos con honestidad, porque apunta a la verdad de diseño — un enjambre WAN está **limitado por RTT**, así que su fuerza no es la latencia de un stream sino la **capacidad agregada**. El batching amortiza la ida y vuelta: el dispatch de expertos por lotes alcanza **77× el throughput por token** con batch 512. Los bytes son margen; las idas y vueltas son lo que hay que ocultar — que es el tema de la entrada compañera sobre la hoja de ruta.",
      },
      { t: "h2", kick: "Pagado por exactamente el trabajo", text: "Los bytes medidos se vuelven KVR" },
      {
        t: "p",
        md: "La participación no vale nada si no rinde cuentas. El relay **mide los bytes puenteados por sesión** en el registro de contribución del hub; el gateway sondea ese registro y acredita por delta KVR a la wallet **propia** del worker — no custodial, como todo lo demás. La primera sesión a través de internet acumuló de verdad: **1.28 MB de trabajo → 1.277952 units → 0.00895 KVR** en recompensas pendientes. Pequeño, y ese es el punto — es liquidación real por trabajo, no un trofeo de participación.",
      },
      {
        t: "p",
        md: "La misma ruta saliente-443 es exactamente como se une un **teléfono**: un Galaxy S25 ya ha computado expertos del 122B por ella (8/8 tokens idénticos, cosine 0.99992). Un modelo a escala de frontera, servido por un backbone en un lugar, una GPU de datacenter en otra ciudad y un teléfono en el bolsillo de alguien — todos produciendo los mismos tokens, cada uno pagado por su parte. Lo que sigue es abaratar la ida y vuelta WAN; esa hoja de ruta se apoya en números de producción de otros y en nuestras propias mediciones.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "Abaratar el dispatch WAN: una hoja de ruta con fundamento",
    dek: "El dispatch remoto de expertos funciona y es idéntico byte a byte — pero una decodificación WAN está limitada por la ida y vuelta. Este es el plan para recortar el coste, con fundamento en números de producción de DeepSeek, Petals y otros (hoja de ruta, no entregado).",
    blocks: [
      {
        t: "callout",
        md: "**Encuadre:** los números que *medimos* se declaran como medidos; todo lo descrito como un plan es una **hoja de ruta**, no un resultado entregado. La meta es tomar el dispatch remoto de expertos — ya correcto y pagado (ver la entrada compañera) — y hacer la ida y vuelta WAN lo bastante barata como para que una GPU distante o un teléfono sea un miembro de primera clase del enjambre, no uno lento.",
      },
      {
        t: "p",
        md: "Nuestra propia medición, publicada sin rodeos: el dispatch cuesta unos **110 KB por token por capa** — 12.3 KB de ida (dispatch) más 98.3 KB de vuelta (combine). La asimetría de 8× se debe a que cada experto seleccionado devuelve su salida completa *antes* de la suma ponderada. Con enlace directo, eso es un sobrecoste de throughput del **1.2%**; a través de un relay de CDN, **~28%**. Esos son los hechos. El resto de esta entrada es cómo pensamos cerrar la brecha — y por qué los bytes son la parte fácil.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "La ley dominante", text: "Una decodificación WAN está limitada por la ida y vuelta" },
      {
        t: "p",
        md: "El resultado publicado más importante aquí no es nuestro — es de Petals: cuando el RTT pasa de <5 ms a 100 ms, la decodificación cae de **1.24 a 0.57 steps/s**, mientras que un **recorte de 10× en ancho de banda la cambia en ~0**. La latencia domina; el ancho de banda es holgura. Eso replantea todo el problema: rebajar bytes es margen, pero **recortar idas y vueltas es la sustancia**. Cada punto de abajo se ordena por cuánto coste de ida y vuelta elimina.",
      },
      { t: "h2", kick: "Más barato en el cable", text: "Reducción de bytes priorizando la precisión" },
      {
        t: "ul",
        items: [
          "**Devolver sumas parciales ponderadas, no salidas crudas de expertos.** Por linealidad el combine del backbone es exacto en cualquier caso, pero el worker devuelve un vector sumado en vez de 8 — esa es la reducción de ~8× del combine, y es exactamente lo que DeepSeek-V3 / DeepEP hacen en producción.",
          "**Estrella paralela, no cadena serial** entre varios workers: ΣRTT colapsa a max RTT.",
          "**F16 en el cable** — ya aceptamos un cosine entre backends de ~0.998, así que el transporte F16 cae dentro de la tolerancia existente; **INT8/FP8 por bloques más adelante**, después de que nuestra propia puerta de argmax/cosine lo apruebe contra pesos Q4_K_M (Petals mostró INT8 sobre internet real sin pérdida de calidad).",
          "Juntas apuntan a **~110 KB → 9–12 KB por token (~12×)** — real, pero recuerda que es el *margen*, no el cuello de botella.",
        ],
      },
      { t: "h2", kick: "Amortizar la ida y vuelta", text: "La sustancia: menos viajes, viajes ocultos" },
      {
        t: "ul",
        items: [
          "La **decodificación especulativa** convierte muchos tokens en una sola ida y vuelta. A unos 80 ms WAN medidos, el punto de equilibrio es de solo **~1.15–1.2 tokens aceptados/paso** — así que hasta una conjetura n-gram débil gana (el Jacobi vainilla puede salir mal; la elección de técnica importa). Nuestro protocolo de dispatch ya lleva `n_tokens > 1`, así que no hace falta cambio en el cable.",
          "El **batching continuo en el gateway** pliega peticiones concurrentes en un solo viaje; el **caché de prefijos por afinidad de slot** mantiene una sesión en las mismas réplicas.",
          "**Ocultar la latencia**: el experto compartido es un término aditivo independiente, así que el backbone lo computa *localmente* durante la ida y vuelta remota (ScMoE reporta 1.82× sobre PCIe, sin reentrenar). Mantén los **expertos calientes en local**, envía remoto solo los fríos (EPLB replica los ~32 más calientes para un aumento de 2.54× en la decodificación en producción).",
        ],
      },
      { t: "h2", kick: "Política y el futuro de la tubería gruesa", text: "Enrutar por peer, y qué cambia 200 Gb/s" },
      {
        t: "p",
        md: "Política de rutas: los peers enrutables públicamente toman la ruta **directa** (la del 1.2%); el relay es solo para dispositivos atados a NAT. Y cuando lleguen enlaces amplios de 200 Gb/s, los 110 KB se serializan en **~4.4 µs** — el término de ancho de banda se desvanece incluso antes de las reducciones de arriba, y una rebanada de 794 MB se envía en ~32 ms. Pero **el RTT es física; no encoge** — así que la decodificación especulativa y el solapamiento siguen siendo las palancas reales incluso a 200 G. Donde la tubería gruesa importa de verdad es en la federación multi-backbone (varios backbones compartiendo un pool de expertos) y en el trabajo limitado por ancho de banda: prefill de prompts largos y throughput de lotes grandes.",
      },
      {
        t: "callout",
        md: "**Una salvedad, dicha con honestidad:** la propia capa de transporte (WebSocket vs QUIC, sobrecoste de enmascarado, hole-punching de NAT) **no tiene resultado externo que podamos citar** — eso es ingeniería que mediremos nosotros mismos antes de afirmar nada. Todo lo de arriba se apoya en números de producción publicados (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) más nuestras propias mediciones; cuando un punto de la hoja de ruta se entregue, sus números y su tiempo verbal se actualizarán aquí.",
      },
      { t: "h2", kick: "Lo que sigue", text: "Objetivos de incorporación" },
      {
        t: "p",
        md: "Nuestro hook de dispatch se apoya en `build_moe_ffn` — **una única función que 43 arquitecturas MoE comparten** en el motor de inferencia. Tres invariantes son independientes del modelo: la matemática MoE (routed = Σ wᵢ·Eᵢ(x), lineal), la ruta de código compartida, y los tensores de expertos apilados estándar de GGUF (`ne[2]` más externo → sliceo alineado a bloques). Así que incorporar un modelo nuevo no es un rediseño — es una sola pasada por una puerta de verificación de argmax/cosine por modelo.",
      },
      {
        t: "table",
        head: ["modelo", "expertos · enrutamiento", "por experto (Q4≈)", "compartido", "estado"],
        rows: [
          ["Qwen3.5-122B (sirviendo hoy)", "256 · top-8", "5.3 MB (medido)", "sí", "en producción"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "sí", "listo — primer candidato"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "sí", "listo (hook verificado)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "no", "listo (hook verificado)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "sí", "listo (grafo deepseek2)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "sí", "listo (familia deepseek)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "no", "listo"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "no", "listo"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "sí", "listo (MoE en capas alternas)"],
          ["MiniMax M3 428B", "128 · top-4", "por determinar (GGUF)", "sí", "esperando al motor upstream"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "no", "funciona — solo workers de GPU"],
        ],
      },
      {
        t: "p",
        md: "La industria converge hacia MoE de grano fino — expertos más pequeños, más de ellos, mayor dispersión (DeepSeek, Qwen, Kimi, GLM, gpt-oss se movieron todos en esta dirección). Cada paso en esa dirección hace más pequeña la unidad de participación del enjambre y más fino el grano del mercado de escasez. Los modelos de arriba no son una lista de deseos; cada uno ya fluye por el mismo hook de dispatch que corremos en producción — la incorporación es una puerta de verificación, no un proyecto de ingeniería.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "La cuestión de los 200G",
    dek: "Nuestros hubs de enjambre ya pueden enlazarse a 200 Gb/s con piezas de estantería — una viene integrada en el GB10. Esto es lo que una tubería gruesa le compra a un MoE distribuido, y lo único que no puede.",
    blocks: [
      {
        t: "callout",
        md: "**La premisa:** la decodificación WAN está limitada por RTT, no por ancho de banda — nuestra hoja de ruta de comunicaciones mostró que los bytes son la parte fácil. Entonces, ¿qué cambia realmente cuando los hubs consiguen enlaces de 200 Gb/s? Casi todo lo relativo a la *capacidad*, y casi nada lo relativo a la *latencia*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Ya en la caja · ConnectX-7", text: "El hardware no es futurista — uno viene dentro de nuestro worker GB10" },
      {
        t: "p",
        md: "El GB10 Grace Blackwell que computa nuestros expertos del 122B lleva a bordo una **NVIDIA ConnectX-7 con dos puertos QSFP de 200 GbE**. Dos de estas máquinas se conectan directo con un único cable QSFP56 DAC de ~$100 — un clúster de dos hubs a 200G con cero switches. ARM es aquí un ciudadano de primera clase: la misma pila de drivers `mlx5` que hace correr estas NIC en datacenters x86 las hace correr en aarch64, que es exactamente lo que es el GB10.",
      },
      {
        t: "callout",
        md: "**La letra pequeña:** el GB10 alimenta su ConnectX-7 a través de dos enlaces PCIe Gen5 x4 en modo multi-host. La velocidad plena medida (~185–190 Gb/s) requiere **RoCE (RDMA) y una topología mapeada correctamente** — un TCP ingenuo sobre una ruta mal mapeada se queda en ~95 Gb/s o peor. Las tuberías gruesas se compran con configuración, no solo con cables.",
      },
      { t: "h2", kick: "La escalera de distancias", text: "200G es un artículo de catálogo en cada alcance" },
      {
        t: "table",
        head: ["alcance", "pieza", "factor de forma"],
        rows: [
          ["rack (0.5–3 m)", "QSFP56 DAC de cobre", "cable, ~$100"],
          ["sala (~30 m)", "AOC óptico activo", "cable"],
          ["campus (2–10 km)", "óptica 200G FR4 / LR4", "módulo QSFP56"],
          ["metro (~40 km)", "óptica 200G ER4", "módulo QSFP56"],
          ["región (~120 km)", "400G ZR+ coherente, a tasa de línea 200G", "módulo QSFP-DD"],
          ["larga distancia (cientos de km)", "longitud de onda 200G de operadora / sistema de línea DWDM", "servicio arrendado"],
        ],
      },
      {
        t: "p",
        md: "En la WAN, el *cable* es simplemente fibra monomodo estándar — vidrio neutral a la velocidad que ya recorre cada ciudad. La velocidad vive en la óptica enchufable de cada extremo, y **OpenZR+ convirtió los 200G-sobre-120 km en un módulo que enchufas a un switch**, no en un proyecto de telecom. Más allá de eso, arriendas una longitud de onda.",
      },
      { t: "h2", kick: "Qué compra", text: "Todo término de ancho de banda del enjambre se desvanece" },
      {
        t: "ul",
        items: [
          "Una carga útil de dispatch (~110 KB/token/capa hoy, ~10 KB tras la hoja de ruta del cable) se serializa en **microsegundos** — el tamaño de la carga deja por completo de ser una restricción de diseño.",
          "Una **rebanada de experto se envía en ~32 ms** (794 MB, teórico) y un modelo 122B entero se sincroniza en **~3 s** — el reequilibrio del mercado de cobertura y la incorporación de un nuevo hub se vuelven casi instantáneos.",
          "El prefill de contexto largo — la única fase genuinamente pesada en ancho de banda — se mueve a velocidad de cable, así que el tiempo hasta el primer token en prompts de 100K tokens pasa a estar limitado por el cómputo del backbone.",
          "**El dispatch por lotes escala sin techo de cable**: el tráfico del pool de expertos agregado entre muchos streams de usuarios es exactamente la carga pesada en ancho de banda y tolerante a la latencia que una tubería gruesa absorbe. Esto es lo que hace práctica una federación multi-backbone — varios hubs, cada uno guardando el KV de sus propios usuarios, compartiendo un pool de expertos.",
        ],
      },
      { t: "h2", kick: "Qué no puede comprar", text: "La luz no se apura" },
      {
        t: "p",
        md: "La fibra transporta luz a ~5 µs/km, y ninguna cantidad de ancho de banda cambia eso. Una ida y vuelta de 13 ms es 13 ms a 200 Gb/s. La decodificación autorregresiva paga esa ida y vuelta por capa fragmentada, por token — por eso la **decodificación especulativa (k tokens por ida y vuelta) y el solapamiento del experto compartido (computar mientras el dispatch está en vuelo) siguen siendo esenciales** incluso entre hubs unidos por la tubería más gruesa del mercado. El ancho de banda compra throughput; solo la disciplina de la ida y vuelta compra latencia.",
      },
      {
        t: "p",
        md: "Así que la arquitectura se asienta en dos niveles. Un **nivel de hub** — backbones y expertos calientes unidos por enlaces de clase 200G, donde la capacidad es efectivamente ilimitada — y un **nivel de edge** — teléfonos y dispositivos pequeños en el relay 443, que sostienen la larga cola de expertos que el mercado de escasez les asigna. La tubería gruesa hace que el primer nivel se sienta como una sola máquina; el relay mantiene el segundo nivel abierto a cualquiera. Ninguno reemplaza al otro: esa división *es* el diseño.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "El enjambre que crece bajo carga",
    dek: "Un modelo gigante que toma ayuda prestada solo cuando la necesita — el enjambre MoE ahora se escala al tráfico: ajustado y rápido en calma, ancho y paralelo bajo carga.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir sirve modelos mucho más grandes de lo que cualquier máquina sostiene por sí sola — un modelo Mixture-of-Experts de 122B de parámetros corre repartido entre un coordinador más un enjambre de workers: GPU en la LAN, GPU a través de un enlace de 200 Gb/s, incluso teléfonos que marcan por internet. Como un modelo MoE enruta cada token solo a un puñado de sus expertos, la mayor parte de los pesos están ociosos en cualquier momento, y esos expertos ociosos pueden vivir **fuera** del nodo principal — en cualquier hardware que se haya ofrecido a alojarlos.",
      },
      {
        t: "callout",
        md: "Lo nuevo es que el enjambre ahora **se escala a sí mismo según la carga**.",
      },
      { t: "h2", kick: "Cómo se comporta", text: "Ajustado en calma, ancho bajo carga" },
      {
        t: "p",
        md: "Cuando el tráfico es ligero, el coordinador sirve todo en su propia GPU — la ruta más rápida por token, sin saltos de red. Cuando las peticiones empiezan a acumularse y sus slots de inferencia se saturan, ocurren dos cosas automáticamente:",
      },
      {
        t: "ul",
        items: [
          "**Reactiva los workers que ya tiene.** El coordinador vigila su propia cola. Bajo saturación sigue transmitiendo el trabajo de expertos enrutados a workers **probados** — los que realmente han servido antes — cambiando un poco de latencia por token por mucho más throughput total. A un worker que solo se conectó pero nunca computó nunca se le confía carga; un worker completamente nuevo aún recibe un primer intento justo.",
          "**El hub recluta nuevos.** El hub de control advierte esa misma saturación y eleva la \"demanda\" de los expertos de ese modelo. Los nodos ociosos — un teléfono en el bolsillo de alguien, una GPU de sobra al otro lado de la ciudad — ya están sondeando ese mercado de demanda. En cuanto la demanda sube, se les ofrece una rebanada de expertos para servir, la descargan, se conectan y se unen. Cuando pasa el pico, la demanda vuelve a bajar y los workers extra se retiran discretamente.",
        ],
      },
      {
        t: "p",
        md: "Nadie planifica esto. A ningún nodo se le empuja. El enjambre respira con la carga: ajustado y rápido en calma, ancho y paralelo bajo carga — y funciona incluso para nodos tras routers domésticos, porque todo es basado en pull.",
      },
      {
        t: "p",
        md: "Esa es la forma de una red que puede servir modelos de un billón de parámetros sobre hardware que ninguna persona posee: la capacidad ociosa se invita exactamente cuando vale la pena invitarla, y solo entonces.",
      },
    ],
  },
};
