/* Español — traducción de las entradas del wiki. La estructura (slug, categoría,
   orden de bloques, código) refleja exactamente entries.ts (fuente en inglés);
   los términos técnicos e identificadores (KVR, p4, bridge, motor de inferencia, GGUF,
   MoE, tok/s, etc.) se mantienen literales. El marco de cumplimiento
   (devnet, token utilitario, no custodial) se conserva. */
import type { WikiTranslation } from "./entries";

export const esWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Red Kvasir",
    summary: "Una red descentralizada de inferencia de IA (DePIN) donde dispositivos cotidianos sirven modelos abiertos y ganan KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** es una red descentralizada de inferencia de IA: los grandes modelos abiertos se reparten entre hardware compartido con el motor **p4**, de modo que ningún nodo tiene que contener el modelo completo. Cualquiera puede aportar una GPU, CPU, NPU — incluso un teléfono — y ganar **KVR** por las capas o expertos que su dispositivo realmente sirve. Los desarrolladores acceden a la red mediante gateways compatibles con OpenAI/Anthropic y pagan por inferencia.",
      },
      {
        t: "ul",
        items: [
          "**Motor de código disponible** — p4 tiene licencia Business Source License 1.1 (se permite el uso interno no monetizado; el uso alojado o que genere ingresos requiere una licencia comercial); el plano de datos de llama.cpp que hay debajo se mantiene cercano a upstream e inspeccionable.",
          "**Wallet de autocustodia** — las claves nunca salen del dispositivo del usuario, y las recompensas se pagan en la propia wallet Solana de cada dueño de nodo. En devnet, el KVR en staking y los créditos prepago quedan en poder de la tesorería del gateway y se registran en su libro contable hasta que se lance un programa de staking en cadena.",
          "**Probado en hardware real** — un modelo de 122B corrió de extremo a extremo en 3 máquinas físicas de nuestra flota de pruebas, con la contribución de cada nodo acreditada de extremo a extremo.",
          "**Nombrado por el mito nórdico** — Kvasir, el ser más sabio, nacido de la esencia común de todos los dioses y propiedad de ninguno.",
        ],
      },
      { t: "h2", kick: "Una petición, muchos dispositivos", text: "Cómo fluye una inferencia" },
      {
        t: "code",
        caption: "Cada salto es HTTP/TCP ordinario; lo distribuido es el modelo en sí.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Los roles se **acumulan**: una misma máquina puede ser nodo de cómputo, host de gateway y host de bridge a la vez, y sus recompensas se suman. El trabajo de la red es que el conjunto parezca una sola máquina — un endpoint delante, miles de dispositivos imperfectos detrás.",
      },
      {
        t: "p",
        md: "Hoy la red corre en **Solana devnet**; KVR es un token utilitario / de contribución, no un activo negociable ni una inversión, y nada en esta página es asesoramiento financiero.",
      },
    ],
  },
  architecture: {
    title: "Arquitectura de Kvasir",
    summary: "Un solo mapa de todo el sistema: las wallets, el gateway que cobra, el bridge que da la cara por el motor, y la red p4 que ejecuta el modelo.",
    blocks: [
      {
        t: "p",
        md: "Kvasir son cuatro capas con una costura cada una. Las **wallets** guardan las claves. El **gateway** cobra el pago y lleva el libro contable. El **bridge** le pone cara HTTP al motor de inferencia. La **red p4** es la que realmente ejecuta el modelo. Todo lo que sigue es consecuencia de dónde caen esas costuras — y el diagrama marca qué corre hoy frente a lo que todavía es un diseño.",
      },
      { t: "h2", kick: "Wallets", text: "Las claves nunca salen del dispositivo" },
      {
        t: "p",
        md: "iOS (Swift), Android (Kotlin) y escritorio (React + Electron) son builds distintas de la misma wallet, y la build de escritorio es además lo que el gateway sirve en `/` como wallet de navegador — una wallet completa, con firma dentro de la página, no una consola de solo lectura. Las recompensas se pagan a la dirección Solana propia de cada dueño; el gateway nunca guarda una clave de usuario.",
      },
      { t: "h2", kick: "Gateway", text: "Un proceso, dos superficies" },
      {
        t: "p",
        md: "`solana/staking-service` es a la vez el **gateway de API** (un `/v1/chat/completions` compatible con OpenAI, más el flujo de pago por petición `/api/pay/quote` → `/api/inference`) y el **gateway de liquidación** (staking, el registro de nodos, las cuentas de crédito, el crédito por contribución). Son un solo proceso porque comparten un solo libro contable: una petición solo se sirve después de verificar su transferencia de KVR on-chain, y ese mismo libro acredita a los nodos que la sirvieron.",
      },
      {
        t: "callout",
        md: "**El pago se liquida antes de que corra la inferencia.** Si después falla el bridge, el gateway reembolsa al pagador desde la tesorería y devuelve un 502 en vez de cobrar por nada. Detrás no hay modelo simulado ni catálogo de relleno: un modelo que la app ofrece es uno que algún bridge está sirviendo, o la lista está vacía.",
      },
      { t: "h2", kick: "Bridge", text: "La cara HTTP del motor" },
      {
        t: "p",
        md: "El bridge (`p4bridge`) es un **OUTER** en términos de p4: instala una sesión a través de los stages, envía al stage de cabeza y recoge el stream de tokens. Para el gateway es un contrato pequeño y fijo — qué modelos están cargados, quién contribuyó cuánto, y completions.",
      },
      {
        t: "table",
        head: ["Ruta", "Qué responde"],
        rows: [
          ["`/api/controllers`", "qué modelos están cargados, y el estado de cada stage"],
          ["`/api/runtime`", "la wallet del operador y las máquinas que hay detrás"],
          ["`/api/contributions`", "filas, unidades, peticiones y throughput por nodo"],
          ["`/c/<model>/v1/chat/completions`", "inferencia"],
        ],
      },
      {
        t: "p",
        md: "Dos trabajos que p4 deja deliberadamente al bridge: **la plantilla de chat** (p4 le entrega al stage server un prompt opaco y no aplica ninguna, así que un modelo instruct continuaría tu texto en vez de responderlo) y **el bloque de razonamiento** (devuelto como `reasoning_content`, separado de `content`, para que una pasada de pensamiento no pueda comerse en silencio el presupuesto de tokens y cobrarle al pagador una respuesta en blanco).",
      },
      {
        t: "callout",
        md: "**El bridge no se publica nunca.** Su única autenticación es un token de servicio compartido, y cualquier cosa que lo alcance puede ejecutar el anillo. Escucha en loopback; la puerta es el túnel.",
      },
      { t: "h2", kick: "Red p4", text: "Los agentes poseen nodos, los stage servers guardan capas" },
      {
        t: "p",
        md: "Un **agente** posee los nodos de un host; un **stage server** es un proceso que guarda una rebanada de las capas del modelo. Un stage entrega su resultado al siguiente pidiéndole a su propio agente que marque al agente de ese stage **en la dirección que ese agente anuncia** — así que la dirección anunciada tiene que ser alcanzable desde los demás hosts, y debería ser la red más rápida que compartan. En el rack MI250 eso es el enlace InfiniBand, no la LAN de la oficina y nunca loopback.",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` y `p4_staged_server` son una sola release.** Un agente compilado desde un árbol más nuevo falla en READY por una capacidad de HELLO ausente — después de cargar el modelo entero.",
          "**La colocación es un artefacto del operador.** Qué capas van en qué GPU, y bajo qué load generation, sale de un plan de colocación; el bridge responde `409` a quien le pida servir, y el watchdog del gateway lo dice una vez y deja de preguntar.",
          "**Un pipeline necesita al menos dos stages.** El comando de sesión rechaza un pipeline de un solo stage.",
        ],
      },
      { t: "h2", kick: "Relay", text: "Una dirección marcable para un portátil" },
      {
        t: "p",
        md: "Los nodos de borde — una app de escritorio, un teléfono — no tienen ninguna dirección que se pueda marcar. El **relay** les da una: el nodo se conecta hacia fuera, demuestra el par de claves de la wallet con un desafío ed25519, y a partir de ahí es alcanzable a través del relay. El relay es la frontera de autenticación y nunca parsea cargas útiles. El instalador de escritorio trae el agente p4 junto a la app, así que unirse no es una segunda instalación.",
      },
      { t: "h2", kick: "Liquidación", text: "El crédito sigue a la participación" },
      {
        t: "p",
        md: "Cada stage reporta las filas de tokens que ejecutó. El bridge las acumula por nodo, y el gateway sondea `/api/contributions` cada 30 segundos y acredita la wallet que el bridge nombra, como `rows / 1000` unidades escaladas por el nivel de rendimiento del nodo. **En un pipeline todos los stages ven las mismas filas**, así que un anillo de cuatro stages paga igual a sus cuatro stages sin importar cuántas capas guarde cada uno — el crédito sigue a la participación, no a la cuota de peso. El sharding de expertos, donde los nodos guardan fracciones distintas de una capa, es el caso que obligará a revisar esto.",
      },
      { t: "h2", kick: "P4 Studio", text: "Lo que el diagrama marca como propuesto" },
      {
        t: "p",
        md: "**P4 Studio** es la consola de operador propia de p4. El feed de observabilidad por petición que quiere de los agentes es una propuesta upstream, no algo que corra aquí — por eso el diagrama lo dibuja discontinuo, junto a los shards de expertos servidos desde nodos de borde, que están diseñados y todavía no en marcha.",
      },
    ],
  },
  bridge: {
    title: "Bridge",
    summary: "La cara HTTP del motor de inferencia: qué está cargado, quién contribuyó y completions — y nada más.",
    blocks: [
      {
        t: "p",
        md: "El **bridge** es lo único con lo que el gateway de liquidación habla para inferir. Es un **OUTER** en términos de p4: instala una sesión a través de los stages del modelo, envía una petición al stage de cabeza, recoge el stream de tokens y reporta lo que aportó cada nodo. No posee colocación, ni planificación, ni más estado que un catálogo de lo que está cargado — deliberadamente pequeño, porque todo lo que no decide es algo que no puede derivar.",
      },
      { t: "h2", kick: "El contrato", text: "Cuatro rutas, un token" },
      {
        t: "table",
        head: ["Ruta", "Qué responde"],
        rows: [
          ["`/api/controllers`", "qué modelos están cargados, y el estado de cada stage"],
          ["`/api/runtime`", "la wallet del operador y las máquinas que hay detrás"],
          ["`/api/contributions`", "filas, unidades, peticiones y throughput por nodo"],
          ["`/c/<model>/v1/chat/completions`", "inferencia"],
        ],
      },
      {
        t: "p",
        md: "Todas las rutas salvo `/api/health` exigen un token de servicio compartido, enviado como `X-Kvasir-Service-Token`. Ese token es lo **único** que hay entre la internet abierta y el uso gratuito del anillo, y por eso el bridge escucha en loopback y se alcanza a través de un túnel en vez de publicarse.",
      },
      { t: "h2", kick: "Lo que p4 le deja", text: "Dos trabajos que el motor no hará" },
      {
        t: "ul",
        items: [
          "**La plantilla de chat.** p4 le entrega al stage server un prompt opaco y no aplica ningún formato de turno propio. El bridge renderiza el del modelo — leído del GGUF y nombrado en el catálogo como `prompt_format`. Sáltatelo y un modelo instruct continúa tu texto en vez de responderlo, nunca emite su token de fin de turno y llega al límite de tokens siempre.",
          "**El bloque de razonamiento.** Un modelo de razonamiento abre su respuesta pensando. El bridge devuelve eso como `reasoning_content`, separado de `content`, y respeta `enable_thinking: false` cerrando el bloque dentro del prompt — si no, una pasada larga de pensamiento puede consumir todo el presupuesto y entregarle a quien llama una respuesta vacía que ya ha pagado.",
        ],
      },
      { t: "h2", kick: "La colocación no es su trabajo", text: "Por qué responde 409" },
      {
        t: "p",
        md: "Pedirle al bridge que sirva un modelo devuelve **409**. Qué capas van en qué GPU, y bajo qué load generation, sale de un plan de colocación que un operador escribió y cargó; no hay ninguna recarga remota que hacer. El watchdog de anillo del gateway lo aprende una vez y deja de preguntar, en vez de reintentar algo que no puede funcionar.",
      },
      {
        t: "callout",
        md: "**Los contadores de contribución viven en memoria.** Un reinicio del bridge pierde lo que el gateway aún no había sondeado — sondea cada 30 segundos — y el gateway rehace la línea base en vez de contar doble cuando un contador va hacia atrás. Un nodo cuyo dueño el bridge no conoce se omite **en silencio**, así que una wallet de operador sin configurar se lee como \"estas máquinas no ganaron nada\".",
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
        md: "El uso se liquida en KVR mediante un flujo de tres pasos — **quote → payment → inference** — de modo que una petición se tarifica antes de ejecutarse y los nodos que la sirvieron se acreditan después. El gateway también agrega un **catálogo de modelos en vivo** de cada bridge alcanzable, así que `/v1/models` refleja lo que la red realmente puede servir ahora mismo.",
      },
      {
        t: "ul",
        items: [
          "Los hosts de gateway ganan una **recompensa por hora de actividad** por mantener el punto de entrada en línea, más un **bono de ×1.5** en cada inferencia que ayudan a servir.",
          "Operar un gateway público requiere staking de **100,000 KVR** (igual que un bridge).",
          "Los despliegues públicos protegen el acceso de operador con **SIWS + 2FA**; un bridge a pelo está diseñado solo para host confiable / LAN / VPN.",
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
          "Los nodos se registran bajo la wallet de su dueño; las recompensas se pagan en esa wallet. Se verificó de extremo a extremo, en la flota de pruebas de un solo operador, que cuatro wallets de dueños separadas ganaban cada una su cuota de capas.",
          "Los datos de capacidades (backend, precisión de acumulación, presupuestos de recursos) deciden qué puede colocar el planner en un nodo — y, en el enjambre, qué rangos puede servir.",
          "Un nodo que no puede proveer monitorización de recursos queda excluido de la carga adaptativa en vez de confiarse a ciegas.",
        ],
      },
    ],
  },
  relay: {
    title: "Relay",
    summary: "Una dirección marcable para una máquina que no tiene ninguna: el nodo marca hacia fuera, demuestra la clave de su wallet y desde entonces es alcanzable como cualquier otra.",
    blocks: [
      {
        t: "p",
        md: "p4 alcanza un nodo **marcando la dirección que anuncia su agente**. Eso funciona para una máquina en un rack y no funciona en absoluto para un portátil tras un router doméstico o un teléfono tras la NAT de una operadora, que pueden abrir conexiones pero no aceptarlas. El **relay** cierra esa brecha: el nodo marca hacia fuera y mantiene la conexión abierta, y a partir de ahí es alcanzable a través del relay exactamente como si tuviera una dirección propia.",
      },
      {
        t: "code",
        caption: "Framing y handshake. El relay nunca mira dentro de una carga útil.",
        code: `frame   magic | u8 type | u32 stream | u32 length | payload

node ──────── connect ───────▶ relay
     ◀─────── CHALLENGE ──────
     ─── HELLO (ed25519 sig) ─▶        signed with the wallet keypair
     ◀─────── WELCOME ────────         the node now has an address`,
      },
      { t: "h2", kick: "Dónde vive la confianza", text: "El relay es la frontera" },
      {
        t: "p",
        md: "p4 en sí no lleva **ninguna autenticación** — asume que las máquinas que pueden alcanzarse entre sí están destinadas a hacerlo. Poner un nodo en un relay público le entregaría esa suposición a internet, así que el relay es donde se prueba la identidad: un nodo solo se admite tras firmar el desafío del relay con el mismo par de claves que posee su wallet, lo que significa que el nodo que se une y la wallet que cobra son la misma parte por construcción.",
      },
      {
        t: "p",
        md: "A partir de ahí el relay es una tubería. Reenvía frames sin parsearlos, así que nunca ve un prompt, un token ni un tensor, y no necesita saber nada del modelo ni del protocolo que va por encima. Eso es lo que lo mantiene lo bastante pequeño como para confiarle la frontera.",
      },
      {
        t: "callout",
        md: "**El instalador de escritorio trae el agente p4 con la app.** Unirse a la red no es una segunda instalación: desbloquea la wallet y el nodo se registra y se conecta con la clave que ya tienes.",
      },
    ],
  },

  p4: {
    title: "p4",
    summary: "El motor detrás de Kvasir: un protocolo direccionado por eventos donde los agentes poseen nodos, los stage servers guardan capas y la colocación es algo que un operador declara, no algo que la red adivina.",
    blocks: [
      {
        t: "p",
        md: "**p4** ejecuta un modelo en varias máquinas cortándolo en **stages** — rebanadas contiguas de sus capas — y dándole a cada stage su propio proceso. Un **agente** posee los nodos de un host: lanza stage servers, enruta eventos entre ellos y responde por su ciclo de vida. No hay ningún scheduler decidiendo dónde va cada cosa; un operador escribe un plan de colocación, lo carga, y la red sirve exactamente eso.",
      },
      {
        t: "code",
        caption: "El camino de una petición por un despliegue p4.",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "Direccionamiento", text: "Un stage marca al agente del siguiente stage" },
      {
        t: "p",
        md: "Cuando un stage termina sus capas, entrega el resultado al siguiente pidiéndole a su propio agente que abra una conexión con **el agente de ese stage, en la dirección que ese agente anuncia**. La dirección anunciada no es por tanto cosmética: tiene que ser alcanzable desde todos los demás hosts del anillo, y debería nombrar la red más rápida que compartan. Anuncia loopback y un anillo de dos hosts se marca a sí mismo en silencio.",
      },
      { t: "h2", kick: "Ciclo de vida", text: "Un número ata toda una carga" },
      {
        t: "ul",
        items: [
          "**La load generation la elige quien carga** y se compara por igualdad exacta en cada sesión, inferencia, liquidación y descarga. No queda registrada en ninguna de las máquinas, así que quien carga la escribe a disco *antes* de que salga el primer comando — sin ella, un modelo cargado ni siquiera se puede bajar.",
          "**La generación de un nodo y la load generation son el mismo número.** El adaptador compara la generación de origen de un recibo de release con la de la carga a la que pertenece y detiene el nodo cuando difieren, así que un anillo cargado con dos números distintos sirve una petición y después pierde la cabeza.",
          "**Hace falta un diario operativo** antes de que un modelo llegue siquiera a cargarse: es el registro de admisión que hace que una carga sea segura ante repeticiones, no una ayuda de depuración.",
        ],
      },
      { t: "h2", kick: "Lo que no hace", text: "Omisiones deliberadas" },
      {
        t: "p",
        md: "p4 no aplica **ninguna plantilla de chat** — reenvía un prompt opaco y espera que quien llama haya renderizado el formato de turno del modelo. No toma **ninguna decisión de colocación**. Y no lleva noción alguna de a quién hay que pagar: los stages reportan las filas de tokens que ejecutaron, y la liquidación es contrato de otro. Cada una de esas es una costura que Kvasir rellena en el [bridge](/wiki/bridge), lo que mantiene el motor lo bastante estrecho como para seguirle el paso a upstream.",
      },
      {
        t: "callout",
        md: "**El agente y el stage server nativo son una sola release.** Un agente compilado desde un árbol más nuevo falla en READY por una capacidad ausente en el HELLO del stage server — después de cargar el modelo entero. Compila ambos desde el mismo checkout.",
      },
    ],
  },
  "in-flight-ring": {
    title: "Anillo en vuelo",
    summary: "Un pipeline que nunca se vacía: varias peticiones ocupan stages distintos a la vez, así que ningún stage espera a que termine la de delante.",
    blocks: [
      {
        t: "p",
        md: "Kvasir sirve un modelo como un **pipeline de stages**, cada uno con una rebanada contigua de sus capas. Un stage ejecuta sus capas y pasa la frontera — un hidden state, no pesos — al siguiente. Ningún stage guarda el modelo completo, y nada se interpone en medio de la ruta de datos: el bridge envía a la cabeza y lee de la cola, mientras los stages se entregan los resultados entre sí a través de sus propios agentes.",
      },
      { t: "h2", kick: "La parte en vuelo", text: "Por qué un pipeline que se vacía desperdicia casi toda la máquina" },
      {
        t: "p",
        md: "Si un pipeline termina una petición antes de admitir la siguiente, en cada instante todos los stages menos uno están ociosos — un anillo de cuatro stages corre a un cuarto de su hardware. El diseño **en vuelo** mantiene varias peticiones moviéndose a la vez: mientras el stage 3 decodifica una petición, el stage 0 ya está haciendo prefill de otra. Los stages reportan cuánto tiempo retuvieron un batch y cuánto estuvieron sin nada que enviar, así que un anillo hambriento se ve distinto de un anillo saturado.",
      },
      {
        t: "code",
        caption: "Cuatro stages, tres peticiones, un instante en el tiempo.",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "Pertenencia", text: "Qué es exactamente un batch" },
      {
        t: "p",
        md: "Las filas de peticiones distintas se empaquetan en un mismo batch físico, y esa pertenencia exacta se reenvía a todos los stages de abajo en vez de volver a decidirse en cada salto. Es lo que permite que un prefill y varias decodificaciones compartan una sola pasada, y es por lo que el tamaño de un batch es una propiedad de la carga: el plan declara de antemano los anchos de fila y de micro-batch, y esos anchos fijan el resultado más grande que un stage podrá devolver jamás.",
      },
      {
        t: "callout",
        md: "**Un pipeline necesita al menos dos stages.** Un pipeline de un solo stage se rechaza de plano — la cabeza y la cola son roles distintos, y un único nodo que colapse ambos es otro motor, no un anillo más pequeño.",
      },
      {
        t: "p",
        md: "El anillo es la ruta de **latencia**, y su granularidad es una capa. El sharding de expertos elimina ese suelo cortando dentro de una capa, y se conecta al mismo tejido de servicio.",
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
        md: "Un modelo **Mixture-of-Experts** sustituye la FFN única de cada capa por un banco de FFN expertas independientes más un **router** que elige unas pocas por token. Step-3.7-Flash, el MoE de 428B que sirve hoy, tiene 288 expertos por capa con enrutamiento top-8. Qwen3.5-122B-A10B es el ejemplo desarrollado de abajo, porque es aquel cuyas cifras se midieron de extremo a extremo:",
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
      {
        t: "callout",
        md: "**Estado del motor.** El sharding al grano de experto se construyó y se demostró sobre el motor anterior de Kvasir, y los resultados de abajo vienen de ese trabajo. El motor actual, [p4](/wiki/p4), sirve hoy al grano de capa; llevar el sharding de expertos hasta él está diseñado y en marcha. Cuando un detalle nombra una herramienta o una ruta, es la que corrió sobre el motor anterior.",
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
        t: "callout",
        md: "**Estado del motor.** El sharding al grano de experto se construyó y se demostró sobre el motor anterior de Kvasir, y los resultados de abajo vienen de ese trabajo. El motor actual, [p4](/wiki/p4), sirve hoy al grano de capa; llevar el sharding de expertos hasta él está diseñado y en marcha. Cuando un detalle nombra una herramienta o una ruta, es la que corrió sobre el motor anterior.",
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
        md: "**GGUF** es el formato de modelo de archivo único del ecosistema motor de inferencia: metadata (arquitectura, número de capas, dimensiones, cuantización) más los tensores como bytes cuantizados crudos (p. ej. Q4_K_M). Un plan de colocación se escribe contra esa metadata — rangos de capas, asignación de dispositivos y estimaciones de tamaño; el lado de servicio rebana los bytes de tensores para producir descargas.",
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
          "**Lado de la ganancia** — unidades de contribución × cuota de capas × nivel de rendimiento para el cómputo; actividad por hora para los roles de bridge/gateway.",
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
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "Los roles se **acumulan** — una máquina puede ser cómputo + gateway + bridge, y sus flujos se suman.",
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
    summary: "Hacer staking de 100,000 KVR habilita a una wallet para operar nodos bridge o gateway.",
    blocks: [
      {
        t: "p",
        md: "El staking bloquea KVR para que una wallet califique para roles de operador y recompensas de nodo. Operar un nodo **bridge** o **gateway** requiere un stake de **100,000 KVR**; los nodos de cómputo normales se unen sin ningún stake y ganan por las capas que corren.",
      },
      {
        t: "ul",
        items: [
          "El staking ocurre en el panel de staking del dashboard de la wallet: introduce una cantidad, **Stake**, y la posición cuenta para la elegibilidad de operador y las recompensas de nodo.",
          "El requisito de 100k es un **filtro de compromiso** para los dos roles de los que depende el tráfico de otros — los puntos de entrada y el plano de control.",
          "En devnet, el KVR en staking se guarda en el vault de staking; la cantidad en staking y las recompensas de nodo se ven en el panel de staking.",
          "El KVR de devnet para staking sale del faucet de distribución; el SOL de devnet para comisiones sale del faucet público.",
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
        md: "La Kvasir Wallet es **no custodial por diseño**: la frase de recuperación de 12 palabras y las claves se guardan solo en el dispositivo del propio usuario, nunca con un operador. Las recompensas se liquidan en Solana directamente a la wallet del dueño de cada nodo — verificado en una flota de pruebas con cuatro wallets de dueños distintos, cada una ganando su propia cuota de capas. En devnet, el staking funciona de otra manera: el KVR en staking queda en la tesorería del gateway y se registra en su libro contable hasta que se lance un programa de staking en cadena.",
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
        md: "Para despliegues públicos, el acceso de operador al gateway se autentica con **Sign-In With Solana**: la wallet del operador firma un nonce emitido por el servidor, probando propiedad sin contraseña ni credencial custodiada. Encima de eso, **2FA TOTP** y códigos de respaldo de un solo uso protegen la sesión.",
      },
      {
        t: "ul",
        items: [
          "**Sin contraseñas en ningún sitio** — la clave de la wallet es la identidad y el nonce previene el replay; no hay nada del lado del servidor que phishear o filtrar.",
          "**El registro TOTP por wallet** se persiste en el libro contable del gateway, así que el 2FA sobrevive a un reinicio.",
          "**Los códigos de respaldo son de un solo uso** — cada uno se consume al iniciar sesión, para recuperación cuando el dispositivo autenticador no está disponible.",
          "**Alcance declarado con honestidad** — el bridge y los puertos del motor asumen un host confiable / LAN / VPN; SIWS + 2FA es la capa que hace seguros de exponer los dominios *públicos*.",
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
        md: "Como la inferencia **debe** pagarse en KVR, el token está ligado al uso real — utilidad, no especulación. El uso financia el KVR que ganan los nodos, lo que mantiene atractivo el contribuir, lo que hace crecer la capacidad, lo que baja el precio y la latencia, lo que atrae más uso. La ventaja más afilada de Kvasir aprieta el bucle todavía más: un participante puede ser **consumidor y proveedor a la vez** (un *prosumer*), así que los dos lados a menudo crecen dentro de las mismas personas.",
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
        md: "Kvasir ya recompensa el **trabajo real** (KVR por tokens servidos × cuota de capa, no la mera presencia) y paga directamente en la propia wallet de cada nodo, que es la parte difícil de hacer honestas las recompensas financiadas por ingresos. El resto — un precio guiado por la utilización y una atenuación de emisión→ingresos — es la hoja de ruta económica que convierte \"más nodos → más barato\" de una intuición en una regla que el protocolo hace cumplir. La entrada **Precio de inferencia** cubre el lado del precio; **Unidades de contribución** cubre cómo el trabajo se convierte en recompensa.",
      },
    ],
  },
  "inference-pricing": {
    title: "Precio de inferencia",
    summary: "Lo que cuesta una inferencia en KVR hoy, por qué una red descentralizada es estructuralmente más barata, y cómo se espera que el precio caiga a medida que crece la oferta.",
    blocks: [
      {
        t: "p",
        md: "El acceso a la red es **de pago por inferencia**: el gateway cotiza un precio en KVR para tu petición, tu wallet lo paga on-chain, y solo entonces el anillo ejecuta el modelo. El precio es una fórmula pequeña y transparente — un suelo por petición más una tarifa por token — cotizada de antemano y liquidada sobre el uso **real** de tokens tras la generación.",
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
      {
        t: "callout",
        md: "**Estado del motor.** El sharding al grano de experto se construyó y se demostró sobre el motor anterior de Kvasir, y los resultados de abajo vienen de ese trabajo. El motor actual, [p4](/wiki/p4), sirve hoy al grano de capa; llevar el sharding de expertos hasta él está diseñado y en marcha. Cuando un detalle nombra una herramienta o una ruta, es la que corrió sobre el motor anterior.",
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
          "**La recompensa es por trabajo.** El trabajo puenteado se acumula en el libro de contribuciones del bridge; el gateway acredita KVR por deltas a tu **propia** wallet. Necesitas una dirección de wallet para cobrar.",
        ],
      },
      {
        t: "callout",
        md: "El worker habla el mismo protocolo de dispatch que una GPU dentro de un datacenter — `(n_used, n_tokens, cur, sel) → experts` sobre un único stream de larga vida. Un worker de shard parcial simplemente pone `n_used = 1`. Esa uniformidad es la razón por la que un teléfono, una caja CPU y una tarjeta Blackwell son miembros intercambiables del mismo enjambre.",
      },
    ],
  },
  "bridge-operations": {
    title: "Operar un bridge",
    summary: "Notas de operador para ejecutar el bridge y el anillo que hay detrás: cargar un plan, sobrevivir a un reinicio, mantener la contribución fluyendo y mantener el motor fuera de internet.",
    blocks: [
      {
        t: "p",
        md: "El **gateway** (punto de entrada público) y el **bridge** (la cara del motor) son los dos servicios de larga vida que un operador mantiene sanos, con los agentes p4 y sus stage servers detrás. El motor no habla ninguna autenticación — asume que las máquinas que pueden alcanzarse entre sí están destinadas a hacerlo — así que todo lo público converge en el gateway, y al bridge se llega por un túnel en vez de publicarlo.",
      },
      { t: "h2", kick: "Cargar", text: "Un plan, y el número bajo el que se carga" },
      {
        t: "ul",
        items: [
          "**La colocación es un plan que escribes**, no una petición que haces: el bridge responde `409` a quien le pida servir. Genera el plan, pásalo en seco, y luego carga con `--confirm`.",
          "**La load generation se escribe a disco antes de que salga el primer comando.** La elige quien carga, se comprueba por igualdad exacta en cada sesión y al descargar, y no queda registrada en ninguna de las máquinas — piérdela y un modelo cargado ni siquiera se puede bajar.",
          "**La generación del nodo es ese mismo número.** Carga un anillo con dos distintas y servirá exactamente una petición antes de que la cabeza se detenga; la siguiente sesión se queda colgada a medio cargar. Usa un valor nuevo en cada carga, o un registro que dejó atrás un intento fallido chocará con él.",
          "**El agente se niega a cargar sin su diario operativo.** Ese es el registro de admisión que necesita una carga segura ante repeticiones, no un flag de depuración.",
        ],
      },
      { t: "h2", kick: "Sobrevivir a un reinicio", text: "Qué vuelve y qué no" },
      {
        t: "ul",
        items: [
          "**Un reinicio del agente tira sus nodos.** Los stage servers son solo de runtime; el modelo hay que volver a cargarlo desde el plan. Eso es el procedimiento de recuperación, no el fallo de uno.",
          "**El gateway no te lo va a recargar.** Su watchdog de anillo nota que un modelo dejó de servir, aprende del `409` del bridge que la colocación es externa, lo dice una vez y deja de preguntar.",
          "**Los contadores de contribución viven en la memoria del bridge.** El gateway sondea cada 30 s y acredita por deltas; un reinicio solo pierde lo que no se había sondeado, y el gateway rehace la línea base en vez de pagar dos veces cuando un contador va hacia atrás.",
        ],
      },
      { t: "h2", kick: "Blindarlo", text: "El motor no mira a internet" },
      {
        t: "ul",
        items: [
          "Ata el bridge a loopback y dale un token de servicio. Sin el token no autentica a nadie, y cualquier cosa que lo alcance puede ejecutar el anillo gratis — lo dice al arrancar en vez de dejar que lo descubras después.",
          "Los agentes anuncian la dirección que marcan los demás agentes. Usa la red más rápida que compartan los hosts, nunca loopback entre hosts, y mantén esa red fuera de la internet pública.",
          "Si algo tiene que estar en una IP pública, recuerda que los puertos publicados de Docker reciben **DNAT antes de la cadena INPUT**, así que una regla sobre `dport` no coincidirá. Filtra en la cadena `DOCKER-USER` usando el puerto de destino original de conntrack (`--ctorigdstport`), y persiste con un oneshot de systemd ordenado `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Trampas", text: "Dos que cuestan tiempo de verdad" },
      {
        t: "ul",
        items: [
          "**Una wallet de operador sin configurar se lee como cero ganancias.** El gateway omite cualquier fila de contribución sin dueño y no registra nada. Los nodos parecen ociosos mientras están sirviendo.",
          "**`pkill` coincide con su propia línea de comandos.** `ssh host 'pkill -f server.js; ...'` mata el shell que lo está ejecutando. Pon el patrón en un archivo de script en vez de en el comando remoto, usa una clase de caracteres (`server[.]js`), y recuerda que un proceso arrancado como un simple `node server.js` no lleva ninguna ruta con la que coincidir — encuéntralo por el puerto en el que escucha.",
        ],
      },
    ],
  },
  "wan-interconnect": {
    title: "Interconexión WAN (óptica 200G)",
    summary: "Cómo se enlazan los sitios de cómputo a 200 Gb/s a través de una sala, un campus o una ciudad: qué óptica a qué distancia, qué se conecta dónde, y qué hace falta para alcanzar de verdad el line rate.",
    blocks: [
      {
        t: "p",
        md: "Cuando dos sitios tienen ambos rutas públicas, el plano de datos de dispatch de expertos debería ser un **enlace directo** — el relay es para bordes sin dirección propia. Esta entrada es la receta concreta para hacer ese enlace directo de clase 200 Gb/s con piezas de catálogo. Una regla lo organiza todo: **la fibra es vidrio neutro a la velocidad; la velocidad vive en el pluggable de cada extremo.**",
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
          "**Lado NIC** — las tarjetas de clase ConnectX-6/7 exponen jaulas QSFP56; DAC/AOC/FR4/LR4/ER4 se asientan todos directamente en la NIC. Un host de clase GB10 ya trae dos puertos QSFP de 200 GbE integrados, así que un enlace de dos sitios necesita exactamente un cable y cero hardware nuevo.",
          "**Lado switch** — la óptica coherente ZR+ es de factor de forma QSFP-DD y pertenece a un switch o router; la NIC del sitio se une entonces a ese switch a 200G sobre un DAC corto. Usa este nivel cuando el sitio lejano está a decenas de kilómetros.",
          "**La fibra en sí** — pares LC dúplex monomodo estándar (G.652), alquilados como fibra oscura por hebra. El mismo vidrio transporta 100G hoy y 400G más tarde; las mejoras son un cambio de módulo, nunca obra civil.",
          "**Más allá de ~120 km** — dejas de comprar piezas y empiezas a alquilar una longitud de onda a un operador; la demarcación es un traspaso Ethernet en tu switch.",
        ],
      },
      {
        t: "code",
        caption: "Tres montajes de referencia, del más barato primero.",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "Paso 3 · alcanzar de verdad los 200G", text: "El line rate es una configuración, no una compra" },
      {
        t: "ul",
        items: [
          "Usa **RDMA (RoCE)** para el stream de dispatch donde esté disponible — los hosts de clase GB10 alimentan la NIC por enlaces PCIe divididos, y la velocidad plena medida (~185–190 Gb/s) aparece bajo RoCE con una topología mapeada correctamente; una ruta mal mapeada topa cerca de la mitad del rate y el TCP plano sin afinar cae mucho más bajo.",
          "Activa **jumbo frames (MTU 9000)** de extremo a extremo y mantén `TCP_NODELAY` en los sockets de dispatch (el bridge ya lo pone).",
          "Cuenta con *verificar*, no asumir: corre un perftest entre sitios tras cada cambio físico — la diferencia entre 95 y 190 Gb/s es invisible hasta que se mide.",
          "Mantén el **relay 443 como camino de reserva** — la política de marcado es directo-primero para pares públicos, relay para NAT. El trabajo del relay es el alcance, el del enlace directo es la velocidad.",
        ],
      },
      {
        t: "p",
        md: "Por qué esto importa a la arquitectura: la latencia de decodificación está acotada por el tiempo de ida y vuelta (~5 µs/km en fibra — física, ajena al ancho de banda), así que una tubería gruesa compra **velocidad de prefill, throughput de dispatch por lotes y distribución casi instantánea de rebanadas de expertos**, no menor latencia por token. Ese es exactamente el rol del nivel de sitio en el diseño de dos niveles: capacidad en el nivel de tubería gruesa, alcance en el nivel de relay.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Escalado adaptativo a la carga",
    summary: "La ruta de servicio MoE de Kvasir crece y se encoge con el tráfico: el coordinador reactiva workers probados bajo saturación, y el bridge recluta nodos ociosos elevando la demanda de expertos — todo basado en pull, así que también se unen dispositivos tras NAT.",
    blocks: [
      {
        t: "p",
        md: "La ruta de servicio MoE de Kvasir escala elásticamente con la carga, en dos capas que cooperan. Cuando hay calma, el coordinador sirve todo localmente para la ruta más rápida por token; cuando se satura, las dos capas de abajo hacen crecer el enjambre — y lo vuelven a encoger cuando pasa el pico.",
      },
      {
        t: "callout",
        md: "**Estado del motor.** El sharding al grano de experto se construyó y se demostró sobre el motor anterior de Kvasir, y los resultados de abajo vienen de ese trabajo. El motor actual, [p4](/wiki/p4), sirve hoy al grano de capa; llevar el sharding de expertos hasta él está diseñado y en marcha. Cuando un detalle nombra una herramienta o una ruta, es la que corrió sobre el motor anterior.",
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
      { t: "h2", kick: "Capa 2", text: "Lado del plano de control: reclutamiento adaptativo a la carga" },
      {
        t: "p",
        md: "El plano de control vigila cada coordinador MoE y hace crecer el pool de workers cuando hace falta:",
      },
      {
        t: "ul",
        items: [
          "Un bucle en segundo plano sondea los slots de cada coordinador y registra la saturación por modelo.",
          "Mientras un modelo está saturado, su **objetivo efectivo de réplicas de expertos** se eleva (base + boost). El mercado de cobertura entonces vuelve a leer como escasos a los expertos ya cubiertos, y un modelo **sin** workers vivos se siembra desde su metadata GGUF (número de expertos) para que la demanda sea visible incluso desde cero.",
          "Los nodos ociosos sondean el mercado de demanda (`/api/expert-volunteer`) y reciben una rebanada `(layer, expert-range)` para servir. Descargan la rebanada, marcan al relay y registran cobertura; el plano de control los cablea automáticamente al mapa de dispatch del coordinador.",
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
