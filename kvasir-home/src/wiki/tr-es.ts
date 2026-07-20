/* Español — traducción de las entradas del wiki. La estructura (slug, categoría,
   orden de bloques, código) refleja exactamente entries.ts (fuente en inglés);
   los términos técnicos e identificadores (KVR, linkcpp, motor de inferencia, GGUF, MoE,
   ring runtime, tok/s, etc.) se mantienen literales. El marco de cumplimiento
   (devnet, token utilitario, no custodial) se conserva. */
import type { WikiTranslation } from "./entries";

export const esWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Red Kvasir",
    summary: "Una red descentralizada de inferencia de IA (DePIN) donde dispositivos cotidianos sirven modelos abiertos y ganan KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** es una red descentralizada de inferencia de IA: los grandes modelos abiertos se reparten entre hardware compartido con el motor **linkcpp**, de modo que ningún nodo posee el modelo completo. Cualquiera puede aportar una GPU, CPU, NPU — incluso un teléfono — y ganar **KVR** por las capas o expertos que su dispositivo realmente sirve. Los desarrolladores acceden a la red mediante gateways compatibles con OpenAI/Anthropic y pagan por inferencia.",
      },
      {
        t: "ul",
        items: [
          "**Motor de código disponible** — linkcpp tiene licencia BSL (gratis para desarrollo y pruebas, el uso en producción requiere una licencia); el plano de datos de motor de inferencia debajo permanece intacto e inspeccionable.",
          "**No custodial** — las recompensas se liquidan en la propia wallet Solana de cada dueño de nodo; las claves nunca abandonan al usuario.",
          "**Probado en hardware real** — un modelo de 122B corrió repartido en 4 GPU AMD MI250, sobre una flota heterogénea de nodos GPU/CPU/NPU/móviles, con la contribución de cada nodo acreditada de extremo a extremo.",
          "**Nombrado por el mito nórdico** — Kvasir, el ser más sabio, nacido de la esencia común de todos los dioses y propiedad de ninguno.",
        ],
      },
      { t: "h2", kick: "Una petición, muchos dispositivos", text: "Cómo fluye una inferencia" },
      {
        t: "code",
        caption: "Cada salto es HTTP/TCP ordinario; lo distribuido es el modelo en sí.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Los roles se **acumulan**: una misma máquina puede ser nodo de cómputo, host de gateway y host de hub a la vez, y sus recompensas se suman. El trabajo de la red es que el conjunto parezca una sola máquina — un endpoint delante, miles de dispositivos imperfectos detrás.",
      },
      {
        t: "p",
        md: "Hoy la red corre en **Solana devnet**; KVR es un token utilitario / de contribución, no un activo negociable ni una inversión, y nada en esta página es asesoramiento financiero.",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "El plano de control: descubre dispositivos, planifica la colocación de capas, lanza workers y orquesta el anillo.",
    blocks: [
      {
        t: "p",
        md: "El **hub** es el plano de control de la red, servido por linkcpp como una única imagen Docker (`controller.hub:app`, un servicio FastAPI en el puerto **19000**). Descubre dispositivos, verifica compatibilidad de runtime, planifica la colocación con el planner, lanza workers de motor de inferencia de serie y expone los gateways por controlador. Es infraestructura deliberadamente aburrida: HTTP de petición/respuesta, estado a prueba de reinicios, sin transportes exóticos.",
      },
      { t: "h2", kick: "Tres puertas de entrada", text: "Cómo se unen las máquinas a un hub" },
      {
        t: "ul",
        items: [
          "**Slots locales de nodo** — cinco slots fijos por hub, mapeados a los puertos RPC **50052–50056**. Los slots siempre existen; se editan los presupuestos GPU + VRAM/RAM/CPU de un slot en vez de crear nodos arbitrarios, y los recursos solo son editables **mientras el slot está sin enlazar**, lo que protege el contrato de capacidad bajo un controlador en marcha.",
          "**Unidades remotas** — registra otro hub linkcpp en marcha e importa sus nodos visibles. El endpoint del plano de datos siempre se deriva de la URL de la *unidad* registrada más el puerto de worker que la unidad expone — nunca del host de nodo que anuncie el sistema remoto.",
          "**Agentes de nodo gestionados** — servicios solo-worker (`nodeagent.py`) que se unen por HTTP simple de petición/respuesta (`/control/join|status|download|load|unload`) y reportan vía `POST /api/node-reports`. Deliberadamente **no** son un stream persistente, para sobrevivir a enrutamientos simples de LAN/VPN.",
        ],
      },
      { t: "h2", kick: "Nada se carga sin verificar", text: "Compuerta de compatibilidad" },
      {
        t: "p",
        md: "Cada unidad, nodo y agente reporta una identidad de protocolo / runtime-pack más detalles de backend. Los desajustes de unidad, runtime-pack, revisión de motor de inferencia y ABI de RPC se **bloquean duramente antes de bind, plan, load o infer**; las diferencias de backend (CUDA/Metal/Vulkan/CPU) se registran como capacidades del nodo, no como rechazos. La carga adaptativa también se bloquea cuando un nodo no puede proveer la monitorización de recursos que un plan seguro necesita.",
      },
      {
        t: "code",
        caption: "Qué sobrevive a un reinicio y qué no.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Al ser el rol más crítico, los hosts de hub ganan la **recompensa por hora de actividad más alta**. Operar un hub público requiere hacer staking de **100,000 KVR**.",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "El punto de entrada público: APIs compatibles con OpenAI/Anthropic y liquidación de pago por inferencia en KVR.",
    blocks: [
      {
        t: "p",
        md: "El **gateway** es donde los desarrolladores se encuentran con la red. Cada controlador expone endpoints compatibles con OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) y con Anthropic (`/anthropic/v1/messages`, `/anthropic/v1/models`), todos respaldados por el mismo modelo cargado — un cliente existente funciona cambiando solo la base URL y la clave.",
      },
      {
        t: "code",
        caption: "Una llamada estilo OpenAI de serie contra el gateway de Kvasir.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Medición", text: "Pago por inferencia en KVR" },
      {
        t: "p",
        md: "El uso se liquida en KVR mediante un flujo de tres pasos — **quote → payment → inference** — de modo que una petición se tarifica antes de ejecutarse y los nodos que la sirvieron se acreditan después. El gateway también agrega un **catálogo de modelos en vivo** de cada hub alcanzable, así que `/v1/models` refleja lo que la red realmente puede servir ahora mismo.",
      },
      {
        t: "ul",
        items: [
          "Los hosts de gateway ganan una **recompensa por hora de actividad** por mantener el punto de entrada en línea, más un **bono de ×1.5** en cada inferencia que ayudan a servir.",
          "Operar un gateway público requiere staking de **100,000 KVR** (igual que un hub).",
          "Los despliegues públicos protegen el acceso de operador con **SIWS + 2FA**; los hubs a pelo están diseñados solo para host confiable / LAN / VPN.",
        ],
      },
    ],
  },
  node: {
    title: "Nodo",
    summary: "Cualquier dispositivo que sirva una parte de un modelo — GPU, CPU, NPU o teléfono — ganando KVR por el trabajo que hace.",
    blocks: [
      {
        t: "p",
        md: "Un **nodo** es cualquier dispositivo que sirve parte de un modelo: una caja con GPU, una máquina CPU, un dispositivo NPU o un teléfono. Un nodo posee solo su parte — una ventana de capas en el anillo, o una rebanada de expertos en el enjambre — y gana KVR ponderado por exactamente el trabajo realizado. La flota en vivo mezcla AMD MI250, NVIDIA GB10 y RTX Pro 6000, un MacBook, máquinas CPU x86 con Windows y nodos móviles en una sola red.",
      },
      { t: "h2", kick: "De la descarga al cobro", text: "El ciclo de vida de un nodo" },
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
          "**Los nodos de cómputo** ganan por unidad de contribución, ponderada por su cuota de capas y escalada por su nivel de rendimiento — sin necesidad de staking.",
          "Los nodos se registran bajo la wallet de su dueño; las recompensas se liquidan a esa wallet, de forma no custodial. Cuatro wallets de dueños distintos ganando cada una su cuota de capas fue verificado de extremo a extremo.",
          "Los datos de capacidades (backend, precisión de acumulación, presupuestos de recursos) deciden qué puede colocar el planner en un nodo — y, en el enjambre, qué rangos puede servir.",
          "Un nodo que no puede proveer monitorización de recursos queda excluido de la carga adaptativa en vez de confiarse a ciegas.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "Relay 443",
    summary: "El plano de datos para dispositivos tras NAT: ambos extremos marcan hacia fuera a través de un puente WebSocket en el puerto 443.",
    blocks: [
      {
        t: "p",
        md: "Los teléfonos tras NAT de operadora no pueden aceptar conexiones entrantes, y bordes como Cloudflare solo dejan pasar los puertos 80/443. El **relay 443** resuelve ambas cosas: un puente WebSocket por borde con un **preámbulo de rol de 1 byte** permite que ambos lados marquen **hacia fuera**, de modo que un teléfono participa en el plano de datos abriendo **cero puertos entrantes**.",
      },
      {
        t: "code",
        caption: "Dos conexiones salientes se encuentran en el medio; el preámbulo dice quién es quién.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "Endurecido en producción", text: "Tres bugs reales, tres arreglos" },
      {
        t: "ul",
        items: [
          "**Acuerdo de huella de build** — ambos extremos deben demostrar que ejecutan el mismo runtime pack antes de que fluya cualquier byte de tensor.",
          "**Autenticación de descarga por node-token** — las descargas de shards parciales se autentican con el mismo node token derivado de la wallet que la app ya posee.",
          "**El atasco de frames de `Int.ushr`** — el `ushr` de Kotlin usa solo los 5 bits bajos del desplazamiento, así que `len ushr 56` se convirtió en `len ushr 24` y corrompió silenciosamente cada frame ≥ 64 KiB (un `result_output` de 593 KB fue la primera víctima). Arreglado moviendo el empaquetado de longitud a desplazamientos `Long` — y es un arreglo estructural para el dispatch de expertos por lotes, que supera 64 KiB de forma rutinaria.",
        ],
      },
      {
        t: "p",
        md: "El relay transporta lo que la topología necesite — fronteras de capas del anillo o streams de dispatch de expertos — y el mismo mecanismo verificado para el anillo es el que usan los workers de teléfono en producción dentro del enjambre.",
      },
      {
        t: "p",
        md: "Tanto los upgrades de `/api/expert-relay` como los de `/api/ring-relay` se **empalman en crudo**: el gateway reenvía los frames WebSocket byte a byte sin parsearlos, así que el relay sigue siendo una tubería delgada y agnóstica al modelo. Aun así **mide los bytes que puentea por sesión**, y ese trabajo medido fluye al libro de contribuciones del hub y se liquida a la propia wallet del worker en **KVR** — hacer de relay para un teléfono tras NAT gana exactamente igual que un nodo conectado directamente.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "El plano de control de código disponible (BSL) que convierte hardware cotidiano en un motor de inferencia distribuida.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** es el motor detrás de Kvasir: un plano de control alrededor del plano de datos RPC de motor de inferencia que ejecuta grandes modelos de IA en múltiples GPU y máquinas usando binarios `ggml-rpc-server` / `llama-server` *de serie*. Todo lo que añade es orquestación — descubrimiento de GPU, slots de nodo, planificación de colocación de capas, lanzamiento de workers y los gateways OpenAI/Anthropic.",
      },
      { t: "h2", kick: "Arquitectura", text: "Un hub, workers de serie" },
      {
        t: "code",
        caption: "El camino de una petición por un despliegue linkcpp.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Código disponible bajo la BSL** — léelo, ejecútalo y construye sobre él gratis en desarrollo y pruebas; el uso en producción requiere una licencia.",
          "El plano de datos de motor de inferencia permanece **sin bifurcar** (salvo un parche móvil de GPU-sobre-RPC fijado), así que las mejoras de rendimiento del upstream siguen entrando.",
          "Se distribuye como **una sola imagen Docker**: el hub FastAPI más los dos binarios de motor de inferencia horneados dentro; los nodos worker nativos se compilan fuera de Docker para CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "El planner", text: "Entra metadata GGUF, sale la colocación" },
      {
        t: "p",
        md: "El planner lee la metadata GGUF y produce ventanas de capas contiguas por nodo, el `--tensor-split` correspondiente y estimaciones de VRAM de KV-cache / capa / experto por nodo — más el offload opcional de FFN de expertos MoE a la RAM del nodo, emitido como reglas `-ot` de motor de inferencia (p. ej. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) y llevado al lanzamiento vía `--override-tensor`. Un plan que no cabe se reporta **infeasible** antes de cargar nada, no se descubre como un OOM en runtime.",
      },
      {
        t: "p",
        md: "La compatibilidad de runtime es un concepto de primera clase: protocolo, runtime-pack, revisión de motor de inferencia y ABI de RPC se verifican, y los desajustes se bloquean duramente antes de cualquier bind, plan, load o inferencia.",
      },
    ],
  },
  "ring-runtime": {
    title: "Ring runtime",
    summary: "Inferencia en pipeline sin master: cada dispositivo ejecuta su ventana de capas y pasa solo fronteras a su vecino.",
    blocks: [
      {
        t: "p",
        md: "El **ring runtime** es la topología de servicio de baja latencia de Kvasir. Cada dispositivo carga solo su **ventana de capas** contigua y abre exactamente dos enlaces — predecesor y sucesor. Las fronteras de hidden-state circulan por el anillo; el último rank muestrea el token y lo devuelve. **Sin master central, y ningún nodo posee el modelo completo.**",
      },
      { t: "h2", kick: "Por qué no una estrella", text: "El problema del master RPC" },
      {
        t: "p",
        md: "En la topología RPC clásica un master abre el **GGUF completo** y marca hacia cada worker. Eso se rompe en una red abierta de tres maneras: el master debe poseer y servir el checkpoint entero; cada worker debe ser marcable — los teléfonos tras NAT de operadora no lo son; y el master es un dueño único en una red que no debería tener ninguno. El anillo elimina las tres: cada stage posee su ventana, las conexiones son vecino a vecino y el relay hace alcanzables a los dispositivos tras NAT.",
      },
      {
        t: "code",
        caption: "Un paso de decodificación alrededor de un anillo de 4 stages.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "La colocación viene del **rank manifest** del planner — p. ej. las 49 capas de Qwen3.5-122B repartidas entre una GPU, CPU, NPU y un teléfono.",
          "Las fronteras son pequeñas (un vector de hidden-state por token), así que los saltos son baratos incluso con enlaces débiles.",
          "Las GPU móviles ejecutan stages del anillo **directamente** (Adreno vía OpenCL) — la ruta RPC hacia la GPU de un teléfono resultó inviable porque el layout de buffers de Adreno no sobrevive a la serialización RPC, pero un stage local posee su backend, así que solo las fronteras cruzan el cable.",
        ],
      },
      {
        t: "p",
        md: "El anillo es la ruta de **latencia**; su suelo es la granularidad de capa (~1.4 GB en el 122B). El enjambre de expertos elimina ese suelo y se conecta al mismo tejido de servicio.",
      },
    ],
  },
  "layer-window": {
    title: "Ventana de capas y shards parciales",
    summary: "La rebanada contigua del modelo que toca a un nodo — descargable como mini-GGUF en vez del checkpoint completo.",
    blocks: [
      {
        t: "p",
        md: "Una **ventana de capas** es el rango contiguo de capas transformer que un nodo del anillo sirve. Un nodo no necesita el checkpoint completo para servirla — un **mini-GGUF de stage** lleva solo los tensores de la ventana: para el 122B, **254 MB con 26 tensores** (de 338 en total) frente al modelo completo de 77.6 GB, o ~1.5 GB para una ventana de una capa en un teléfono.",
      },
      {
        t: "code",
        caption: "Una fila del rank manifest: quién sirve qué, dentro de qué presupuesto.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Reclamado, no asignado", text: "Auto-inscripción" },
      {
        t: "ul",
        items: [
          "Un nodo consulta el **mapa de cobertura/demanda** para ver qué ventanas están mal servidas y cuánto paga cada una.",
          "Elige la ventana sin cubrir de **mayor recompensa** que quepa en su presupuesto, descarga exactamente eso y se une.",
          "La cobertura se auto-repara: cuando un nodo se marcha, su ventana vuelve a ser escasa — y por tanto lucrativa — otra vez.",
          "Verificado de extremo a extremo con un teléfono tras NAT: poll → auto-inscripción → descarga parcial → carga en GPU Adreno → inferencia de anillo completada, contribución acreditada.",
        ],
      },
      {
        t: "p",
        md: "El enjambre de expertos reutiliza exactamente este mercado en el grano más fino de **(capa, rango de expertos)** — el mismo mapa, la misma auto-inscripción, las mismas recompensas, unidades más pequeñas.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "Un modelo cuyas FFN son cientos de expertos independientes, de los que solo unos pocos se activan por token.",
    blocks: [
      {
        t: "p",
        md: "Un modelo **Mixture-of-Experts** sustituye la FFN única de cada capa por un banco de FFN expertas independientes más un **router** que elige unas pocas por token. Qwen3.5-122B-A10B es el ejemplo insignia de la red:",
      },
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
      {
        t: "p",
        md: "Cada capa se divide en una **ruta densa** — atención + KV, norms, el router (`ffn_gate_inp`), un experto compartido — y un **banco de expertos** almacenado como tres tensores apilados (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). La ruta densa es la minoría de los bytes; el banco de expertos es el 86% del modelo.",
      },
      {
        t: "ul",
        items: [
          "El índice de experto es la **dimensión más externa del GGUF**, así que cada experto es una losa contigua alineada a bloques de cuantización — extraerlo es una copia de rango de bytes, sin decuantización.",
          "Por token solo se activan **8 de 256** expertos por capa, así que el tráfico de expertos de una capa en decodificación son unas pocas multiplicaciones de matrices pequeñas sobre un vector hidden (~6 KB de dispatch).",
          "Los expertos son mutuamente independientes — la propiedad puede dispersarse entre dispositivos y reequilibrarse libremente.",
        ],
      },
      {
        t: "p",
        md: "Por eso MoE es el sustrato natural del enjambre: los pesos vienen pre-empaquetados en unidades del tamaño de un dispositivo y de propiedad independiente.",
      },
    ],
  },
  "expert-sharding": {
    title: "Sharding de expertos",
    summary: "Partir un MoE al grano de experto, para que un teléfono cargue 42–340 MB de expertos en vez de una capa de 1.4 GB.",
    blocks: [
      {
        t: "p",
        md: "El **sharding de expertos** baja la unidad de carga del enjambre de una capa (~1.4 GB en el 122B) a un experto (**5.3 MB**). Un dispositivo débil descarga una rebanada de 8–64 expertos (**42–340 MB**), la carga como worker de función pura — sin atención, sin KV, sin sampler — y computa sus expertos cada vez que el router del backbone los selecciona.",
      },
      { t: "h2", kick: "Dos roles", text: "Backbone × worker" },
      {
        t: "code",
        caption: "El punto de corte dentro de una capa MoE (el router corre una vez, en el backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "El **backbone** conserva la ruta densa (atención, norms, router, experto compartido, combine) y mantiene todos los expertos como réplica de respaldo descargada a RAM, para tolerar el churn.",
          "**Los workers** (`linkcpp-expert-worker --serve`) responden `(n_used, n_tokens, cur, sel) → experts` sobre un stream TCP de larga vida — el mismo stream que el relay 443 tuneliza para los teléfonos.",
          "La cobertura se auto-repara mediante el **mercado de cobertura de expertos**: `POST /api/expert-coverage` late con las tenencias, `GET /api/expert-demand` agrega la escasez y `POST /api/expert-volunteer` asigna el rango más escaso recortado al presupuesto del nodo.",
        ],
      },
      { t: "h2", kick: "Medido, no prometido", text: "Verificado en hardware real" },
      {
        t: "ul",
        items: [
          "Cómputo sharded == monolítico hasta **max|Δ| = 3.6e-12** (un reagrupamiento exacto, no una aproximación).",
          "Dispatch entre procesos en una decodificación 122B en vivo: **argmax MATCH**, cosine de logits 0.99869 — byte a byte idéntico al in-process.",
          "Un Galaxy S25 descargó de forma autónoma su rebanada de 1.58 GB y computó los expertos de layer-0 en cada token: **8/8 tokens idénticos** a la ejecución local.",
          "Una GPU remota a través de la internet pública — un viaje de ida y vuelta WAN por token — se mantuvo **8/8 idéntico en greedy** (cosine 0.99773): **1.2% de sobrecarga de throughput** en un enlace directo, ~28% a través de un borde CDN. El coste honesto del dispatch serial por token, y por qué la palanca del tejido es el procesamiento por lotes, no una menor latencia.",
          "El dispatch por lotes alcanza **53k tok/s por worker** con batch 512 (ROCm) — la propiedad de tejido de throughput que hace práctico el enjambre.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "Autoridad del router",
    summary: "El invariante de coherencia del enjambre: el enrutamiento se decide una vez, en el backbone — los workers solo reciben ids de expertos.",
    blocks: [
      {
        t: "callout",
        md: "**El invariante:** la única decisión discreta dentro de la red es el enrutamiento MoE (top-8 de 256). Kvasir ejecuta el router **exactamente una vez, en el backbone**, y despacha a los workers solo los ids de expertos seleccionados. Un enjambre heterogéneo puede diferir levemente en la *magnitud* de la salida de cada experto — nunca difiere en *qué expertos corren*.",
      },
      {
        t: "p",
        md: "Sin esta regla, cada backend re-ejecutaría el router y elegiría **expertos distintos** en tokens fronterizos — divergencia genuina y catastrófica, porque desde ese token el cómputo se bifurca como con otra semilla aleatoria. Con ella, las diferencias de hardware se reducen a un error continuo acotado que el combine ponderado por probabilidad absorbe.",
      },
      { t: "h2", kick: "Qué previene", text: "Modos de divergencia cerrados por un solo punto de decisión" },
      {
        t: "table",
        head: ["Modo de divergencia", "Sin autoridad", "Con autoridad"],
        rows: [
          ["Desajuste de enrutamiento", "Los backends eligen top-8 distintos en las fronteras", "Los ids se deciden una vez y se despachan a sus dueños"],
          ["Bifurcación de trayectoria", "Un token volteado bifurca toda la secuencia", "Decodificación/muestreo fijados a un nodo"],
          ["Verificación", "Comparación bit a bit entre backends (imposible)", "Chequeos de tolerancia sobre residuos bien definidos"],
        ],
      },
      {
        t: "p",
        md: "El coste es despreciable: el backbone ya computaba `ffn_norm` y los logits del router; lo que cruza el cable son solo las filas hidden más los ids seleccionados — unos **6 KB por paso de decodificación**.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "Equivalencia numérica",
    summary: "Los backends distintos nunca coinciden bit a bit; el enjambre trata la tolerancia medida como un contrato de primera clase.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno y las CPU computan la misma operación con distintos órdenes de reducción, fusión FMA, acumuladores y aproximaciones de funciones trascendentes — los resultados difieren ~1e-6…1e-3 por operación, **por diseño, nunca de forma bit-idéntica**. Un enjambre hecho del hardware que aparezca no puede exigir exactitud de bits, así que Kvasir mide la equivalencia en su lugar.",
      },
      {
        t: "table",
        head: ["Par de backends (122B real, expertos de layer-0)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU de teléfono vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "La matriz completa de backends está cerrada: los dos backends de GPU (CUDA, ROCm) comparten fuentes de kernel y caen **efectivamente bit-idénticos** (cosine 1.0000000000), mientras que los pares GPU↔CPU se mantienen equivalentes en ~0.9997. Un worker CUDA y un worker ROCm son intercambiables; un worker GPU y un worker CPU son numéricamente equivalentes.",
      },
      { t: "h2", kick: "Por qué difieren", text: "La suma en coma flotante no es asociativa" },
      {
        t: "ul",
        items: [
          "**Orden de reducción del matmul** — tensor cores, tiles MFMA, workgroups OpenCL y lanes SIMD acumulan en órdenes distintos.",
          "**Precisión de acumulación** — almacenamiento F16/BF16 con acumuladores F32 o F16: la palanca más grande de la divergencia.",
          "**Aproximaciones trascendentes** — exp (softmax), silu (swiglu) y rsqrt (norms) usan variantes polinómicas/de tabla distintas por backend.",
        ],
      },
      { t: "h2", kick: "El contrato", text: "Tolerancias, capacidades, autoridad única" },
      {
        t: "ul",
        items: [
          "La verificación es una **tolerancia** — \"acuerdo top-1 ≥ 99.x%, KL ≤ ε\" — nunca igualdad de bits.",
          "Los backends y la precisión de acumulación se anuncian como **capacidades** del nodo; los nodos que acumulan en F32 se prefieren para rangos sensibles a la salida.",
          "Los nodos fuera de tolerancia se marcan como no aptos para rangos sensibles, no se rechazan de plano.",
          "Las decisiones discretas (enrutamiento, muestreo) se fijan a autoridades únicas para que el error continuo nunca pueda volverse divergencia discreta.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "El formato de archivo de modelo cuantizado que usa motor de inferencia — y el layout que abarata el sliceo parcial y de expertos.",
    blocks: [
      {
        t: "p",
        md: "**GGUF** es el formato de modelo de archivo único del ecosistema motor de inferencia: metadata (arquitectura, número de capas, dimensiones, cuantización) más los tensores como bytes cuantizados crudos (p. ej. Q4_K_M). El planner de linkcpp lee la metadata para computar colocaciones y estimaciones de tamaño; el lado de servicio rebana los bytes de tensores para producir descargas.",
      },
      {
        t: "ul",
        items: [
          "**Los mini-GGUF de stage** llevan los tensores de una ventana de capas — 254 MB en vez de 77.6 GB para un stage de anillo del 122B.",
          "**Los GGUF de shard de expertos** llevan una rebanada (capa, rango de expertos), servida por `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` con autenticación por node-token.",
          "Ambos son **archivos GGUF válidos**: el lector en el nodo los carga con las herramientas de serie, sin formato propio.",
        ],
      },
      {
        t: "code",
        caption: "Por qué rebanar expertos es una copia de bytes: el índice de experto es la dimensión más externa.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "El router (`ffn_gate_inp`) y el experto compartido quedan **excluidos** de los shards de expertos — pertenecen al backbone, que es exactamente lo que la autoridad del router exige.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "El token utilitario de la red: los desarrolladores lo gastan en inferencia, los contribuidores lo ganan por cómputo.",
    blocks: [
      {
        t: "p",
        md: "**KVR** (nombre on-chain \"Kvasir\", 6 decimales, Solana) es un mismo token fluyendo en ambas direcciones: los desarrolladores **gastan** KVR para ejecutar inferencia a través del gateway, los contribuidores **ganan** KVR por el cómputo que proveen sus nodos. Las recompensas se computan del trabajo real — capas y expertos realmente servidos — no de la participación.",
      },
      {
        t: "ul",
        items: [
          "**Lado del gasto** — pago por inferencia a través del gateway: quote → payment → inference.",
          "**Lado de la ganancia** — unidades de contribución × cuota de capas × nivel de rendimiento para el cómputo; actividad por hora para los roles de hub/gateway.",
          "**Liquidación** — en Solana, a la propia wallet de cada dueño de nodo; el servicio de liquidación acredita a cada nodo que tocó una petición.",
          "**Nombrado por el mito** — el Hidromiel de la Poesía, destilado de Kvasir, que daba sabiduría a todo el que lo bebía: acceso abierto, y recompensas para todos los que aportan.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, token utilitario.** KVR corre actualmente en Solana devnet y es un token utilitario / de contribución — no un activo negociable, un precio ni una inversión. Nada de esto es asesoramiento financiero ni una promesa de retorno.",
      },
    ],
  },
  "contribution-units": {
    title: "Unidades de contribución",
    summary: "La fórmula de recompensa: las unidades siguen a los tokens servidos ponderados por cuota de capas, y escalan por nivel de rendimiento.",
    blocks: [
      {
        t: "code",
        caption: "Cómo se computan las recompensas de cómputo.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "Una **unidad** ≈ 1k tokens servidos, ponderada por la **cuota de capas** del nodo en cada inferencia — un nodo que corre 12 de 49 capas gana 12/49 de las unidades de cada inferencia. El multiplicador de nivel premia la velocidad medida, y los roles de infraestructura acumulan actividad por hora encima.",
      },
      { t: "h2", kick: "Ejemplo resuelto", text: "Una inferencia, cuatro nodos" },
      {
        t: "table",
        head: ["Nodo", "Capas", "Cuota", "Nivel", "Unidades efectivas / 1k tokens"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Teléfono", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Las recompensas siguen al **trabajo real**: un nodo que no sirvió nada no gana nada, sin importar su tiempo en línea (en roles de cómputo).",
          "Los roles se **acumulan** — una máquina puede ser cómputo + gateway + hub, y sus flujos se suman.",
          "Todo se liquida en KVR a la wallet del propio dueño del nodo; el panel muestra bruto × nivel = efectivo y un saldo reclamable.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "Niveles de rendimiento",
    summary: "El throughput medido fija un multiplicador: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["Nivel", "Throughput medido", "Multiplicador"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "La velocidad de decodificación medida de un nodo fija su nivel, y el nivel multiplica sus unidades ganadas — el hardware más rápido gana proporcionalmente más por el mismo trabajo. La vista de estado de nodos de la wallet muestra el nivel de cada nodo junto a su contribución.",
      },
      {
        t: "ul",
        items: [
          "Los niveles se **miden, no se autodeclaran** — el throughput sale del rendimiento real de servicio del nodo y se vuelve a medir con el tiempo.",
          "Un teléfono de nivel C también gana — ×0.7 de su cuota de capas — y ese es el punto: el suelo es para toda participación, no solo para datacenters.",
          "El nivel multiplica las unidades *efectivas*, así que se compone con la cuota de capas y el bono de gateway en vez de reemplazarlos.",
        ],
      },
    ],
  },
  staking: {
    title: "Staking",
    summary: "Haz staking de KVR para ganar interés APR; 100,000 KVR en staking habilitan a una wallet para operar nodos hub o gateway.",
    blocks: [
      {
        t: "p",
        md: "El staking bloquea KVR en tu propia wallet para ganar **interés APR** y calificar para recompensas de nodo. Operar un nodo **hub** o **gateway** requiere un stake de **100,000 KVR**; los nodos de cómputo normales se unen sin ningún stake y ganan por las capas que corren.",
      },
      {
        t: "ul",
        items: [
          "El staking ocurre en el panel de staking del dashboard de la wallet: introduce una cantidad, **Stake**, y la posición empieza a acumular APR más elegibilidad para recompensas de nodo.",
          "El requisito de 100k es un **filtro de compromiso** para los dos roles de los que depende el tráfico de otros — los puntos de entrada y el plano de control.",
          "El staking es no custodial como todo lo demás: la posición vive en tu propia wallet, y el principal, el interés acumulado y las recompensas de nodo se ven en el panel de staking.",
          "El KVR de devnet para staking llega por distribución o swap (swap SOL/ETH ↔ KVR: próximamente); el SOL de devnet para comisiones sale del faucet público.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "Wallet no custodial",
    summary: "Las claves viven solo en el dispositivo del usuario — web, escritorio, iOS y Android — y las recompensas se liquidan directamente allí.",
    blocks: [
      {
        t: "p",
        md: "La Kvasir Wallet es **no custodial por diseño**: la frase de recuperación de 12 palabras y las claves se guardan solo en el dispositivo del propio usuario, nunca con un operador. Las recompensas se liquidan en Solana directamente a la wallet del dueño de cada nodo — verificado con cuatro wallets de dueños distintos, cada una ganando su propia cuota de capas.",
      },
      {
        t: "ul",
        items: [
          "**Plataformas** — web, escritorio (macOS/Windows/Linux, wallet + nodo en una sola app Electron), iOS y Android.",
          "**Una frase, todos los dispositivos** — la misma frase de 12 palabras restaura la misma cuenta en escritorio, teléfono y web; una passphrase local desbloquea cada instalación.",
          "**Wallet = identidad del nodo** — la wallet firma la identidad de red del nodo, así que \"quién gana por este dispositivo\" es criptográfico, no una fila de cuenta en el servidor de alguien.",
          "**Pierde la frase, pierde la cuenta** — la no custodia corta en ambos sentidos; no existe operador que pueda restablecerla.",
        ],
      },
      {
        t: "p",
        md: "Las apps móviles son también apps de nodo: la misma wallet que guarda tu KVR configura el backend de cómputo y el modo de nodo del teléfono, hace staking y reclama recompensas.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "El login de operador es una firma de wallet (Sign-In With Solana) sobre un nonce del servidor, más 2FA TOTP opcional.",
    blocks: [
      {
        t: "p",
        md: "Para despliegues públicos, el acceso de operador al hub y al gateway se autentica con **Sign-In With Solana**: la wallet del operador firma un nonce emitido por el servidor, probando propiedad sin contraseña ni credencial custodiada. Encima, **2FA TOTP** y códigos de respaldo de un solo uso protegen la sesión — tanto en el hub como en el gateway.",
      },
      {
        t: "ul",
        items: [
          "**Sin contraseñas en ningún sitio** — la clave de la wallet es la identidad y el nonce previene el replay; no hay nada del lado del servidor que phishear o filtrar.",
          "**El registro TOTP por wallet** se persiste en el estado del hub, así que el 2FA sobrevive a los reinicios junto con slots y bindings.",
          "**Los códigos de respaldo son de un solo uso** — cada uno se consume al iniciar sesión, para recuperación cuando el dispositivo autenticador no está disponible.",
          "**Alcance declarado con honestidad** — el hub a pelo y los puertos RPC están diseñados para host confiable / LAN / VPN; SIWS + 2FA es la capa que hace seguros de exponer los dominios *públicos*.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "La economía KVR",
    summary: "Cómo el coste del consumidor y la recompensa del nodo forman un solo bucle que se refuerza a sí mismo — el círculo virtuoso que deja a la red abaratarse a medida que crece.",
    blocks: [
      {
        t: "p",
        md: "Kvasir es un **mercado de dos lados** liquidado en un solo token. Los consumidores pagan **KVR** por inferencia al tesoro; los nodos ganan **KVR** por el trabajo exacto que sirvieron, pagado de vuelta a sus propias wallets. El objetivo de diseño es que estos dos lados no compitan — sino que se **potencien**: más oferta abarata y mejora la red, lo que atrae más demanda, cuyos pagos financian recompensas más ricas, lo que atrae más oferta.",
      },
      { t: "h2", kick: "El volante de inercia", text: "Uso y oferta crecen juntos" },
      {
        t: "p",
        md: "Como la inferencia **debe** pagarse en KVR, cada unidad de uso es demanda real del token — utilidad, no especulación. Esa demanda sostiene el valor del KVR que ganan los nodos, lo que mantiene atractivo el contribuir, lo que hace crecer la capacidad, lo que baja el precio y la latencia, lo que atrae más uso. La ventaja más afilada de Kvasir aprieta el bucle todavía más: un participante puede ser **consumidor y proveedor a la vez** (un *prosumer*), así que los dos lados a menudo crecen dentro de las mismas personas.",
      },
      {
        t: "callout",
        md: "**\"Gratis cuando contribuyes\" es neto-gratis, no coste-cero.** Pagas por lo que infieres y ganas por lo que sirves; contribuye aproximadamente tanto como consumes y ambos se cancelan. La red no es gratis — *tu* factura lo es.",
      },
      { t: "h2", kick: "Mantenerlo virtuoso", text: "Tres invariantes, y las espirales que evitan" },
      {
        t: "table",
        head: ["Invariante", "Espiral que evita"],
        rows: [
          ["Recompensas financiadas por ingresos reales (emisión solo para arrancar, luego se atenúa)", "La inflación erosiona el KVR hasta que ambos lados colapsan"],
          ["El KVR es el medio obligatorio para la inferencia", "El valor del token se desacopla del uso y cae en pura especulación"],
          ["El precio flota entre un suelo de coste y un techo por debajo del mercado", "Demasiado bajo mata de hambre a los nodos; demasiado alto pierde usuarios ante APIs centralizadas"],
        ],
      },
      {
        t: "p",
        md: "Kvasir ya recompensa el **trabajo real** (KVR por tokens servidos × cuota de capa, no la mera presencia) y liquida de forma no custodial, que es la parte difícil de hacer honestas las recompensas financiadas por ingresos. El resto — un precio guiado por la utilización y una atenuación de emisión→ingresos — es la hoja de ruta económica que convierte \"más nodos → más barato\" de una intuición en una regla que el protocolo hace cumplir. La entrada **Precio de inferencia** cubre el lado del precio; **Unidades de contribución** cubre cómo el trabajo se convierte en recompensa.",
      },
    ],
  },
  "inference-pricing": {
    title: "Precio de inferencia",
    summary: "Lo que cuesta una inferencia en KVR hoy, por qué una red descentralizada es estructuralmente más barata, y cómo se espera que el precio caiga a medida que crece la oferta.",
    blocks: [
      {
        t: "p",
        md: "El acceso a la red es **de pago por inferencia**: el gateway cotiza un precio en KVR para tu petición, tu wallet lo paga on-chain, y solo entonces el hub ejecuta el modelo. El precio es una fórmula pequeña y transparente — un suelo por petición más una tarifa por token — cotizada de antemano y liquidada sobre el uso **real** de tokens tras la generación.",
      },
      {
        t: "code",
        caption: "La fórmula de liquidación — cotizada antes, cobrada sobre el uso real después.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Por qué puede ser más barato", text: "Sin margen central que pagar" },
      {
        t: "p",
        md: "Una API centralizada pone precio al coste **más** un gran margen y la recuperación de capital. Una red descentralizada pone precio cerca del **coste marginal** de sus contribuyentes — electricidad y amortización de hardware — más una tarifa de protocolo delgada. Esa brecha estructural existe sin importar el tamaño. El crecimiento la ensancha: el **sharding de expertos** significa que más nodos sostienen cada uno una rebanada más pequeña, así que dispositivos más baratos pueden servir, bajando el coste marginal de participar y profundizando la oferta.",
      },
      {
        t: "callout",
        md: "**El precio se gobierna, no es una barra libre.** Las tarifas son un parámetro económico sensible, cambiado solo por la wallet genesis bajo firma de wallet + 2FA — nunca por una variable de entorno. Esto mantiene la economía del token estable y auditable.",
      },
      { t: "h2", kick: "Hacia dónde va", text: "Precio guiado por la utilización" },
      {
        t: "p",
        md: "La dirección de diseño es un precio que **flota con la utilización de la red** entre un suelo (mantenido por encima del coste marginal del nodo, para que servir siga valiendo la pena) y un techo (mantenido por debajo de las alternativas centralizadas, para que siga siendo competitivo). La oferta ociosa empuja el precio hacia abajo; la congestión lo empuja hacia arriba. Ese es el mecanismo que por fin hace **\"más nodos compartidos → menor precio\"** cierto en el código — el termostato natural de la **economía KVR**.",
      },
    ],
  },
  "run-expert-worker": {
    title: "Ejecutar un worker de expertos",
    summary: "Convierte una GPU, CPU o teléfono de sobra en un worker de expertos: constrúyelo, ofrécete voluntario para la rebanada más escasa, descárgala, sírvela y marca hacia fuera por 443 para ganar KVR.",
    blocks: [
      {
        t: "p",
        md: "Un **worker de expertos** es una función pura `(hidden, ids) → out` — sin atención, sin caché KV, sin sampler — que computa una rebanada de los expertos de un modelo MoE cada vez que el router del backbone los selecciona. No eliges qué servir; el **mercado de cobertura** te entrega el rango más escaso y de mayor recompensa recortado a tu presupuesto, así que tanto un teléfono de 4 GB como una GPU de datacenter encuentran un hueco.",
      },
      { t: "h2", kick: "Siete pasos", text: "Construir → ofrecerse → servir → marcar → ganar" },
      {
        t: "code",
        caption: "El camino completo — el script de marcado espera al backbone con un bucle de reintentos.",
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
          "**La rebanada es diminuta.** Una rebanada de layer-0 con 128 expertos son **794 MB** frente al modelo completo de 72 GB — el grano que deja participar a dispositivos débiles. Solo descargas el rango que el mercado te asignó.",
          "**Marcar hacia fuera, nunca hacia dentro.** El paso 5 abre un WebSocket saliente en 443, así que la NAT de operadora y los bordes CDN lo dejan pasar y expones cero puertos entrantes — el mismo camino que usa un teléfono.",
          "**El latido es estructural.** Sin `POST /api/expert-coverage` no sirves nada que el mapa de demanda conozca, y nada de lo que hagas se acredita.",
          "**La recompensa es por trabajo.** El trabajo puenteado se acumula en el libro de contribuciones del hub; el gateway acredita KVR por deltas a tu **propia** wallet (no custodial). Necesitas una dirección de wallet para cobrar.",
        ],
      },
      {
        t: "callout",
        md: "El worker habla el mismo protocolo de dispatch que una GPU dentro de un datacenter — `(n_used, n_tokens, cur, sel) → experts` sobre un único stream de larga vida. Un worker de shard parcial simplemente pone `n_used = 1`. Esa uniformidad es la razón por la que un teléfono, una caja CPU y una tarjeta Blackwell son miembros intercambiables del mismo enjambre.",
      },
    ],
  },
  "hub-operations": {
    title: "Operar un hub",
    summary: "Notas de operador para ejecutar un hub y un gateway: parchear en caliente sin recompilar, sobrevivir a reinicios, mantener el catálogo registrado y blindar la superficie a 443.",
    blocks: [
      {
        t: "p",
        md: "El hub (plano de control) y el gateway (punto de entrada público) son los dos servicios de larga vida que un operador mantiene sanos. El hub y sus puertos RPC están **sin autenticar por diseño** — solo host confiable / LAN / VPN — y todo el tráfico público converge en la única superficie 443 del gateway. Estas son las notas de operación que mantienen ese arreglo estable a través de cambios de código, reinicios y rearranques del sistema.",
      },
      { t: "h2", kick: "Desplegar y parchear en caliente", text: "Cambiar código sin recompilar" },
      {
        t: "ul",
        items: [
          "**Camino rápido:** actualiza el código del hub/gateway con `docker cp <file> <container>:/app/...` + `docker restart` — sin recompilar la imagen. Pero **añadir una variable de entorno no puede hacerse así** (requiere recrear el contenedor); prefiere en su lugar una API de configuración en runtime que persista al hub-state.",
          "**Deriva de compose:** un contenedor de larga ejecución puede divergir de su archivo compose (modo de red, entrypoint, env). Haz siempre `docker inspect` de la configuración real antes de recrear con `docker compose up -d` — si ha derivado, recrear borra los ajustes de producción. Usa cp + restart.",
          "**Haz diff antes de parchear:** saca con `docker cp` el archivo de dentro del contenedor y compáralo con el HEAD del repo antes de reemplazarlo, para que un parche en caliente de una sesión anterior no se pierda en silencio.",
        ],
      },
      { t: "h2", kick: "Sobrevivir a un reinicio", text: "El estado persiste; los modelos cargados no" },
      {
        t: "ul",
        items: [
          "Un reinicio del hub **detiene el servicio.** Los slots, controladores y bindings se restauran desde `hub-state.json`, pero un modelo cargado es solo de runtime. Tras un reinicio, lee el `last_load` de cada controlador y vuelve a disparar `POST /api/controllers/{cid}/serve` — incluso un modelo grande vuelve en ~1 minuto gracias a la caché de página.",
          "**Watchdog del gateway:** sondea cada modelo servido con una petición de 1 token cada 30 s y recarga automáticamente desde `last_load` ante un fallo (con un cooldown). **Sondea *todos* los modelos, no `catalog[0]`** — en cuanto un modelo sano de otro hub se ordena al frente, un sondeo del-primero-solo se pierde la caída de un modelo grande (un bug real, ya arreglado).",
          "**TTL del catálogo:** `POST /api/pay/hub/register` tiene un TTL de 90 s, así que mantén viva la registración con un bucle de latido de ~60 s, hecho duradero a través de reinicios con un cron `@reboot` o una unidad systemd.",
        ],
      },
      { t: "h2", kick: "Blindarlo", text: "Todo lo público pasa por 443" },
      {
        t: "ul",
        items: [
          "El hub (:19000) y los puertos RPC asumen una red confiable; lo único que debería mirar a internet es el gateway en 443 (incluido su passthrough de relay WebSocket).",
          "Si un hub debe estar en una IP pública, ponle firewall a IPs confiables — pero los puertos publicados de Docker reciben **DNAT antes de la cadena INPUT**, así que una regla sobre `dport` no coincidirá. Filtra en su lugar en la cadena `DOCKER-USER` usando el puerto de destino original de conntrack (`--ctorigdstport`), y persiste las reglas con un oneshot de systemd ordenado `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Liquidación y trampas", text: "Pull, no push — y una trampa de shell" },
      {
        t: "ul",
        items: [
          "**La liquidación es pull, no push:** el hub acumula contribuciones; el gateway sondea `GET /api/contributions` y acredita KVR por deltas. Si un reinicio del hub reinicia sus contadores, el gateway rehace la línea base para que nada se pague dos veces. La tarifa del trabajo de expertos la fija `LINKCPP_EXPERT_UNITS_PER_MB`.",
          "**La trampa de `pkill`:** `ssh host 'pkill -f X; ...'` coincide con su *propia* línea de comandos y se mata a sí mismo. Usa una clase de caracteres en el patrón (`X[x]`), y nunca pongas el spawn y el pkill en el mismo comando remoto.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Interconexión WAN de hubs (óptica 200G)",
    summary: "Cómo se enlazan los hubs a 200 Gb/s a través de una sala, un campus o una ciudad: qué óptica a qué distancia, qué se conecta dónde, y qué hace falta para alcanzar de verdad el line rate.",
    blocks: [
      {
        t: "p",
        md: "Cuando dos hubs tienen ambos rutas públicas, el plano de datos de dispatch de expertos debería ser un **enlace directo** — el relay 443 es para bordes tras NAT. Esta entrada es la receta concreta para hacer ese enlace directo de clase 200 Gb/s con piezas de catálogo. Una regla lo organiza todo: **la fibra es vidrio neutro a la velocidad; la velocidad vive en el pluggable de cada extremo.**",
      },
      { t: "h2", kick: "Paso 1 · elige por distancia", text: "La escalera de alcance" },
      {
        t: "table",
        head: ["distancia", "pieza", "se conecta a"],
        rows: [
          ["mismo rack, 0.5–3 m", "QSFP56 DAC (cobre pasivo)", "NIC ↔ NIC, sin switch"],
          ["misma sala, ≤30 m", "QSFP56 AOC (óptico activo)", "NIC ↔ NIC / switch"],
          ["campus, 2–10 km", "módulo 200G FR4 (2 km) / LR4 (10 km) + LC dúplex, fibra monomodo", "jaula QSFP56 de NIC o switch"],
          ["metro, ≤40 km", "módulo 200G ER4, fibra monomodo", "jaula QSFP56 de NIC o switch"],
          ["región, ≤120 km", "módulo coherente 400G ZR+ configurado a un line rate de 200G", "jaula QSFP-DD de switch/router (no la NIC)"],
          ["larga distancia, cientos de km", "longitud de onda 200G (o 2×100G) alquilada al operador sobre DWDM", "tu switch traspasa al operador"],
        ],
      },
      { t: "h2", kick: "Paso 2 · qué se conecta dónde", text: "Lado NIC vs lado switch" },
      {
        t: "ul",
        items: [
          "**Lado NIC** — las tarjetas de clase ConnectX-6/7 exponen jaulas QSFP56; DAC/AOC/FR4/LR4/ER4 se asientan todos directamente en la NIC. Un hub de clase GB10 ya trae dos puertos QSFP de 200 GbE integrados, así que un enlace de dos hubs necesita exactamente un cable y cero hardware nuevo.",
          "**Lado switch** — la óptica coherente ZR+ es de factor de forma QSFP-DD y pertenece a un switch o router; la NIC del hub se une entonces a ese switch a 200G sobre un DAC corto. Usa este nivel cuando el hub lejano está a decenas de kilómetros.",
          "**La fibra en sí** — pares LC dúplex monomodo estándar (G.652), alquilados como fibra oscura por hebra. El mismo vidrio transporta 100G hoy y 400G más tarde; las mejoras son un cambio de módulo, nunca obra civil.",
          "**Más allá de ~120 km** — dejas de comprar piezas y empiezas a alquilar una longitud de onda a un operador; la demarcación es un traspaso Ethernet en tu switch.",
        ],
      },
      {
        t: "code",
        caption: "Tres montajes de referencia, del más barato primero.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Paso 3 · alcanzar de verdad los 200G", text: "El line rate es una configuración, no una compra" },
      {
        t: "ul",
        items: [
          "Usa **RDMA (RoCE)** para el stream de dispatch donde esté disponible — los hosts de clase GB10 alimentan la NIC por enlaces PCIe divididos, y la velocidad plena medida (~185–190 Gb/s) aparece bajo RoCE con una topología mapeada correctamente; una ruta mal mapeada topa cerca de la mitad del rate y el TCP plano sin afinar cae mucho más bajo.",
          "Activa **jumbo frames (MTU 9000)** de extremo a extremo y mantén `TCP_NODELAY` en los sockets de dispatch (el hub ya lo pone).",
          "Cuenta con *verificar*, no asumir: corre un perftest entre hubs tras cada cambio físico — la diferencia entre 95 y 190 Gb/s es invisible hasta que se mide.",
          "Mantén el **relay 443 como camino de reserva** — la política de marcado es directo-primero para pares públicos, relay para NAT. El trabajo del relay es el alcance, el del enlace directo es la velocidad.",
        ],
      },
      {
        t: "p",
        md: "Por qué esto importa a la arquitectura: la latencia de decodificación está acotada por el tiempo de ida y vuelta (~5 µs/km en fibra — física, ajena al ancho de banda), así que una tubería gruesa compra **velocidad de prefill, throughput de dispatch por lotes y distribución casi instantánea de rebanadas de expertos**, no menor latencia por token. Ese es exactamente el rol del nivel-hub en el diseño de dos niveles: capacidad en el nivel de tubería gruesa, alcance en el nivel de relay.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Escalado adaptativo a la carga",
    summary: "La ruta de servicio MoE de Kvasir crece y se encoge con el tráfico: el coordinador reactiva workers probados bajo saturación, y el hub recluta nodos ociosos elevando la demanda de expertos — todo basado en pull, así que también se unen dispositivos tras NAT.",
    blocks: [
      {
        t: "p",
        md: "La ruta de servicio MoE de Kvasir escala elásticamente con la carga, en dos capas que cooperan. Cuando hay calma, el coordinador sirve todo localmente para la ruta más rápida por token; cuando se satura, las dos capas de abajo hacen crecer el enjambre — y lo vuelven a encoger cuando pasa el pico.",
      },
      { t: "h2", kick: "Capa 1", text: "Lado del coordinador: dispatch adaptativo a la carga" },
      {
        t: "p",
        md: "El coordinador de backbone (un `linkcpp-server` que corre el modelo completo) sirve los expertos enrutados o bien en su propia GPU (rápido, local) o bien despachándolos a workers remotos. Un hilo en segundo plano decide cuál, cada pocos segundos:",
      },
      {
        t: "ul",
        items: [
          "Sondea sus **propios** slots de inferencia. Cuando `busy >= saturation threshold` (por defecto 2), el coordinador está bajo carga.",
          "Bajo carga, si hay un worker **probado** vivo — uno cuyo `last_serve_ms > 0`, es decir, que realmente ha computado expertos antes — el coordinador le sigue despachando más allá del timeout de inactividad normal, favoreciendo el throughput agregado sobre la latencia por token.",
          "Un worker que se conectó pero nunca sirvió (un teléfono que marcó al relay pero nunca computó) **no** se recluta bajo carga, porque despacharle reemplazaría la ruta local rápida por un fallback lento. Los workers nuevos aún reciben un primer intento a través de una breve ventana de gracia.",
          "La autoconsulta está acotada en el tiempo, así que un sondeo atascado nunca puede bloquear el dispatch.",
        ],
      },
      { t: "h2", kick: "Capa 2", text: "Lado del hub: reclutamiento adaptativo a la carga" },
      {
        t: "p",
        md: "El hub de control vigila cada coordinador MoE y hace crecer el pool de workers cuando hace falta:",
      },
      {
        t: "ul",
        items: [
          "Un bucle en segundo plano sondea los slots de cada coordinador y registra la saturación por modelo.",
          "Mientras un modelo está saturado, su **objetivo efectivo de réplicas de expertos** se eleva (base + boost). El mercado de cobertura entonces vuelve a leer como escasos a los expertos ya cubiertos, y un modelo **sin** workers vivos se siembra desde su metadata GGUF (número de expertos) para que la demanda sea visible incluso desde cero.",
          "Los nodos ociosos sondean el mercado de demanda (`/api/expert-volunteer`) y reciben una rebanada `(layer, expert-range)` para servir. Descargan la rebanada, marcan al relay y registran cobertura; el hub los cablea automáticamente al mapa de dispatch del coordinador.",
          "Cuando la carga se drena, el objetivo vuelve a bajar y la demanda desaparece, así que a los workers extra ya no se les despacha y expiran.",
        ],
      },
      {
        t: "callout",
        md: "El diseño es **basado en pull**: los nodos piden trabajo en vez de que se les empuje, así que un worker tras NAT participa sin conectividad entrante. Un nodo reclutado en la Capa 2 que empieza a servir se convierte en un worker **probado** que la Capa 1 mantiene entonces activo bajo carga — las dos capas se componen en un solo bucle elástico.",
      },
      {
        t: "p",
        md: "**Observabilidad:** `GET /api/moe/recruitment` reporta busy/saturación por modelo y el objetivo base vs efectivo; `/api/expert-demand` lleva un flag `recruiting`.",
      },
    ],
  },
};
