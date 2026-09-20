/* 中文 — 维基条目翻译。结构（slug、分类、区块顺序、代码）严格镜像 entries.ts
   （英文源）；技术术语与标识符（KVR, p4, bridge, GGUF, MoE, tok/s 等）保持原文。
   合规表述（devnet、实用型代币、非托管）保持不变。 */
import type { WikiTranslation } from "./entries";

export const zhWiki: Record<string, WikiTranslation> = {
  "node-relay": {
    title: "节点中继",
    summary: "代没有地址的机器持有一个公网地址，使 NAT 之后的节点无需开放任何端口即可被拨通。",
    blocks: [
      { t: "p", md: "**节点中继**为贡献者的机器提供一个网络可以拨通的地址。p4 通过*向*节点打开连接来派发工作，而 NAT 之后的家用机器没有可供打开连接的地址。中继代为持有一个公网地址，节点只维持一条通往中继的向外连接，拨到该公网地址的工作便沿着节点早已持有的连接流下来。" },
      { t: "p", md: "p4 的两端都不知道中继的存在。拨号方看到的是一个普通地址；节点的代理仍然只绑定 `127.0.0.1`，此外什么都不监听。" },
      { t: "h2", text: "为什么用隧道而非端口转发" },
      { t: "p", md: "p4 没有任何认证 —— 任何能触及代理端口的主机都可以发送 `NODE_LOAD`、`NODE_UNLOAD` 或 `INSPECT`。把家用路由器的端口转发进去，等于把机器暴露给任何找到它的人。在中继之后，节点什么都不监听，并在连接承载任何内容之前，用结算网关同样的签名方式证明运营者钱包。可达性与认证由同一套机制一并解决。" },
      { t: "h2", text: "它不做的事" },
      { t: "ul", items: [
        "**不读取流量。** 载荷逐字节通过、从不解析，因此中继无法区分命令 —— 也不应当能够区分，因为理解流量就意味着有能力改动它。",
        "**不做调度。** 编排仍归运营者的计划所有；对中继而言，节点只是一个地址。",
        "**不是工作的凭证。** 经由中继传输的字节不说明任何已完成的推理，也从不计入贡献度。",
      ] },
      { t: "h2", text: "相关条目" },
      { t: "p", md: "另见 **p4 代理**（贡献者机器所运行的进程）与 **节点运营者**（中继在发放地址前所认证的钱包）。" },
    ],
  },
  "kvasir-network": {
    title: "Kvasir 网络",
    summary: "一个去中心化 AI 推理网络（DePIN）：日常设备共同服务开源模型并赚取 KVR。",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** 是一个去中心化 AI 推理网络：大型开源模型通过 **p4** 引擎分布在共享硬件上，任何单一节点都无需持有完整模型。任何人都可以贡献 GPU、CPU、NPU——甚至手机——并按设备实际服务的层或专家赚取 **KVR**。开发者通过 OpenAI/Anthropic 兼容网关访问网络，按推理付费。",
      },
      {
        t: "ul",
        items: [
          "**源码可得的引擎** — p4 采用 Business Source License 1.1 许可（允许不产生收益的内部使用；托管服务或产生收入的用途需要商业许可证）；其下的 llama.cpp 数据平面保持贴近上游、可审查。",
          "**自托管钱包** — 密钥从不离开用户的设备，奖励支付到每个节点所有者自己的 Solana 钱包。在 devnet 上，在链上质押程序上线之前，已质押的 KVR 和预付额度由网关的 treasury 持有，并记录在其账本中。",
          "**已在真实硬件上验证** — 一个 122B 模型已在我们测试机队的 3 台物理机器上端到端运行，并端到端记账每个节点的贡献。",
          "**名字源自北欧神话** — Kvasir：由众神的精华汇聚而生、却不属于任何一位神的最智慧存在。",
        ],
      },
      { t: "h2", kick: "一个请求，众多设备", text: "一次推理如何流动" },
      {
        t: "code",
        caption: "每一跳都是普通的 HTTP/TCP；被分布的是模型本身。",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "角色可以**叠加**：一台机器可以同时是计算节点、网关主机和 bridge 主机，奖励相加。网络的职责是让这个集合看起来像一台机器——前面是一个端点，后面是成千上万台并不完美的设备。",
      },
      {
        t: "p",
        md: "目前网络运行在 **Solana devnet** 上；KVR 是实用型/贡献型代币，不是可交易资产或投资品，本页内容均不构成任何投资建议。",
      },
    ],
  },
  architecture: {
    title: "Kvasir 架构",
    summary: "整个系统的一张图：钱包、收款的网关、为引擎装上门面的 bridge，以及真正运行模型的 p4 网络。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 是四个层次，每两层之间只有一道接缝。**钱包**持有密钥。**网关**收取付款并保管账本。**bridge** 为推理引擎装上 HTTP 门面。**p4 网络**真正运行模型。下面的一切都是这些接缝落在何处的结果——示意图也标出了哪些今天已在运行、哪些仍只是设计。",
      },
      { t: "h2", kick: "钱包", text: "密钥从不离开设备" },
      {
        t: "p",
        md: "iOS（Swift）、Android（Kotlin）和桌面（React + Electron）是同一个钱包的三种构建，而桌面构建同时也是网关在 `/` 上提供的浏览器钱包——一个支持页内签名的完整钱包，不是只读控制台。奖励支付到每位所有者自己的 Solana 地址；网关从不持有用户的密钥。",
      },
      { t: "h2", kick: "网关", text: "一个进程，两张面孔" },
      {
        t: "p",
        md: "`solana/staking-service` 既是 **API 网关**（OpenAI 兼容的 `/v1/chat/completions`，以及 `/api/pay/quote` → `/api/inference` 的按次付费流程），也是**结算网关**（质押、节点注册表、额度账户、贡献记账）。它们是同一个进程，因为共享同一本账：一个请求只有在其 KVR 转账在链上被验证之后才会被服务，而同一本账随后又为服务了它的节点记账。",
      },
      {
        t: "callout",
        md: "**付款在推理运行之前就已结算。**如果随后 bridge 失败，网关会从 treasury 向付款方退款并返回 502，而不是收了钱什么都不给。它背后没有模拟模型，也没有占位目录：应用列出的模型一定是某个 bridge 正在服务的，否则列表就是空的。",
      },
      { t: "h2", kick: "Bridge", text: "引擎的 HTTP 门面" },
      {
        t: "p",
        md: "bridge（`p4bridge`）在 p4 的术语里是一个 **OUTER**：它在各 stage 上装配会话、向头部提交请求、汇集 token 流。对网关而言，它只是一份小而固定的契约——哪些模型已加载、谁贡献了多少、以及补全。",
      },
      {
        t: "table",
        head: ["路由", "它回答什么"],
        rows: [
          ["`/api/controllers`", "哪些模型已加载，以及每个 stage 的状态"],
          ["`/api/runtime`", "运营者钱包，以及它背后的机器"],
          ["`/api/contributions`", "每节点的 rows、units、请求数、吞吐"],
          ["`/c/<model>/v1/chat/completions`", "推理"],
        ],
      },
      {
        t: "p",
        md: "p4 刻意留给 bridge 的两件事：**对话模板**（p4 把不透明的提示词原样交给 stage server，自己不套用任何格式，因此 instruct 模型会续写你的文本而不是回答它）和**思考块**（作为 `reasoning_content` 返回，与 `content` 分开，这样一次思考过程就不会悄悄吃光 token 预算、让付款方为一份空白回复买单）。",
      },
      {
        t: "callout",
        md: "**bridge 从不对外发布。**它唯一的认证是一个共享服务令牌，任何能触达它的东西都能驱动整个环。它绑定 loopback；隧道才是那扇门。",
      },
      { t: "h2", kick: "p4 网络", text: "agent 拥有节点，stage server 持有层" },
      {
        t: "p",
        md: "**agent** 拥有一台主机上的节点；**stage server** 是持有模型一段层切片的单个进程。一个 stage 把结果交给下一个的方式，是请自己的 agent 去拨通那个 stage 的 agent——**按那个 agent 所公示的地址**。因此公示地址必须能被其他主机访问，并且应当是它们共享的最快的那张网。在 MI250 机架上那是 InfiniBand 链路，而不是办公室 LAN，更绝不是 loopback。",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` 与 `p4_staged_server` 是同一个发行版。**用更新的代码树构建的 agent 会在 READY 阶段因缺少 HELLO 能力而失败——而且是在整个模型加载完之后。",
          "**放置是运营者的产物。**哪些层落在哪张 GPU 上、在哪个 load generation 下，来自一份放置方案（placement plan）；谁要求 bridge 去服务，它就回 `409`，而网关的看门狗说一次之后就不再问了。",
          "**一条流水线至少要两个 stage。**会话命令会拒绝单 stage 的流水线。",
        ],
      },
      { t: "h2", kick: "中继", text: "给一台笔记本一个可拨号的地址" },
      {
        t: "p",
        md: "边缘节点——桌面应用、手机——没有任何人能拨通的地址。**中继**给了它们一个：节点向外连接，用 ed25519 挑战证明自己的钱包密钥对，此后即可经由中继被访问。中继就是认证边界，且从不解析载荷。桌面安装包把 p4 agent 与应用一起装上，因此加入网络不需要第二次安装。",
      },
      { t: "h2", kick: "结算", text: "记账跟随参与" },
      {
        t: "p",
        md: "每个 stage 汇报自己跑过的 token 行数。bridge 按节点累计，网关每 30 秒轮询一次 `/api/contributions`，并以 `rows / 1000` 个单位、按节点的性能等级缩放后，记入 bridge 指名的那个钱包。**在一条流水线里每个 stage 看到的行数相同**，因此一个四 stage 的环会平均付给它的四个 stage，无论各自持有多少层——记账跟随参与，而不是权重份额。专家分片让节点各持一层的不同部分，那正是需要重新审视这一点的场景。",
      },
      { t: "h2", kick: "P4 Studio", text: "示意图中标为提案的部分" },
      {
        t: "p",
        md: "**P4 Studio** 是 p4 自己的运营者控制台。它想从 agent 那里获取的逐请求可观测性数据流，在上游还只是一份提案，并不是此处正在运行的东西——示意图为此把它画成虚线，与它并列的还有从边缘节点提供的专家分片：已完成设计，尚未运行。",
      },
    ],
  },
  bridge: {
    title: "Bridge",
    summary: "推理引擎的 HTTP 门面：加载了什么、谁贡献了多少、以及补全——除此之外什么都没有。",
    blocks: [
      {
        t: "p",
        md: "**bridge** 是结算网关在推理这件事上唯一对话的对象。它在 p4 的术语里是一个 **OUTER**：它在模型的各个 stage 上装配会话、向头部 stage 提交请求、汇集 token 流，并汇报每个节点贡献了什么。它不拥有放置、不做调度，除了一份\"什么已加载\"的目录之外不持有任何状态——它刻意做得很小，因为凡是它不决定的事情就不会走样。",
      },
      { t: "h2", kick: "契约", text: "四条路由，一个令牌" },
      {
        t: "table",
        head: ["路由", "它回答什么"],
        rows: [
          ["`/api/controllers`", "哪些模型已加载，以及每个 stage 的状态"],
          ["`/api/runtime`", "运营者钱包，以及它背后的机器"],
          ["`/api/contributions`", "每节点的 rows、units、请求数、吞吐"],
          ["`/c/<model>/v1/chat/completions`", "推理"],
        ],
      },
      {
        t: "p",
        md: "除 `/api/health` 之外的每条路由都需要一个共享服务令牌，经 `X-Kvasir-Service-Token` 发送。那个令牌是横在开放互联网与\"免费驱动这个环\"之间的**唯一**屏障，这也是为什么 bridge 绑定 loopback、经隧道访问，而不对外发布。",
      },
      { t: "h2", kick: "p4 留给它的", text: "引擎不肯做的两件事" },
      {
        t: "ul",
        items: [
          "**对话模板。**p4 把不透明的提示词原样交给 stage server，自己不套用任何轮次格式。由 bridge 渲染模型的格式——从 GGUF 中读出，并在目录里以 `prompt_format` 命名。跳过它，instruct 模型就会续写你的文本而不是回答它，永远不发出自己的轮次结束 token，每次都跑到 token 上限。",
          "**思考块。**推理模型开口先思考。bridge 把那部分作为 `reasoning_content` 返回，与 `content` 分开，并通过在提示词中把思考块闭合来遵守 `enable_thinking: false`——否则一次漫长的思考就能吃光整个预算，把一份已经付过钱的空答案交给调用方。",
        ],
      },
      { t: "h2", kick: "放置不归它管", text: "它为什么回 409" },
      {
        t: "p",
        md: "要求 bridge 去服务一个模型，得到的是 **409**。哪些层落在哪张 GPU 上、在哪个 load generation 下，来自运营者写好并加载的放置方案；这里没有可执行的远程重载。网关的环看门狗学会这一点之后就不再问，而不是反复重试一件不可能成功的事。",
      },
      {
        t: "callout",
        md: "**贡献计数器活在内存里。**bridge 重启会丢掉网关尚未轮询到的部分——它每 30 秒轮询一次——而当计数器倒退时，网关会重设基线而不是重复计数。bridge 不知道其所有者的节点会被**静默**跳过，因此一个未设置的运营者钱包读起来就是\"这些机器什么都没赚到\"。",
      },
    ],
  },
  gateway: {
    title: "网关",
    summary: "公共入口：OpenAI/Anthropic 兼容 API 与 KVR 按推理付费结算。",
    blocks: [
      {
        t: "p",
        md: "**网关**是开发者接触网络的地方。每个控制器都暴露 OpenAI 兼容端点（`/v1/chat/completions`、`/v1/responses`、`/v1/models`）和 Anthropic 兼容端点（`/anthropic/v1/messages`、`/anthropic/v1/models`），全部由同一个已加载模型支撑——现有客户端只需改 base URL 和密钥即可使用。",
      },
      {
        t: "code",
        caption: "对 Kvasir 网关的标准 OpenAI 风格调用。",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "计费", text: "KVR 按推理付费" },
      {
        t: "p",
        md: "用量以 KVR 通过三步流程结算——**quote → payment → inference**——请求在执行前定价，服务它的节点在执行后记账。网关还从每个可达的 bridge 聚合**实时模型目录**，因此 `/v1/models` 反映的是网络此刻真正能服务的内容。",
      },
      {
        t: "ul",
        items: [
          "网关主机因保持入口在线获得**按小时在线奖励**，并对其协助服务的每次推理获得 **×1.5 加成**。",
          "运营公共网关需要质押 **100,000 KVR**（与 bridge 相同）。",
          "公共部署用 **SIWS + 2FA** 保护运营者访问；裸 bridge 仅为可信主机 / LAN / VPN 设计。",
        ],
      },
    ],
  },
  node: {
    title: "节点",
    summary: "任何服务模型一部分的设备——GPU、CPU、NPU 或手机——按其所做的工作赚取 KVR。",
    blocks: [
      {
        t: "p",
        md: "**节点**是任何服务模型一部分的设备：GPU 主机、CPU 机器、NPU 设备或手机。节点只持有自己的份额——环上的层窗口，或蜂群中的专家切片——并按其实际完成的工作加权赚取 KVR。现役机队中混有 AMD MI250、NVIDIA GB10 与 RTX Pro 6000、MacBook、x86 Windows CPU 机器和移动节点。",
      },
      { t: "h2", kick: "从下载到入账", text: "节点的生命周期" },
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
          "**计算节点**按贡献单位赚取，以层份额加权、按性能等级缩放——无需质押。",
          "节点注册在所有者钱包名下；奖励支付到该钱包。四个不同所有者钱包各自赚取其层份额，已在单一运营方的测试机队上端到端验证。",
          "能力数据（后端、累加精度、资源预算）决定规划器可以在该节点上放什么——以及在蜂群中它可以担任哪些 rank。",
          "无法提供资源监控的节点会被排除在自适应加载之外，而不是被盲目信任。",
        ],
      },
    ],
  },

  p4: {
    title: "p4",
    summary: "Kvasir 背后的引擎：一套以事件寻址的协议——agent 拥有节点、stage server 持有层，而放置是运营者写明的，不是网络猜出来的。",
    blocks: [
      {
        t: "p",
        md: "**p4** 把一个模型切成若干 **stage**——连续的层切片——并让每个 stage 独占一个进程，从而在多台机器上运行它。**agent** 拥有一台主机上的节点：它启动 stage server、在它们之间路由事件，并为其生命周期负责。这里没有决定东西该放在哪里的调度器；运营者写一份放置方案、加载它，网络随后就精确地照此服务。",
      },
      {
        t: "code",
        caption: "一次请求在 p4 部署中的路径。",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "寻址", text: "一个 stage 拨通下一个 stage 的 agent" },
      {
        t: "p",
        md: "当一个 stage 跑完自己的层，它把结果交给下一个 stage 的方式，是请自己的 agent 去连接**那个 stage 的 agent，按那个 agent 所公示的地址**。因此公示地址绝非装饰：它必须能被环中其他每一台主机访问，并且应当指向它们共享的最快网络。公示 loopback，一个双主机的环就会悄悄地拨给自己。",
      },
      { t: "h2", kick: "生命周期", text: "一个数字把一次加载绑在一起" },
      {
        t: "ul",
        items: [
          "**load generation 由加载者选定**，并在每一次会话、推理、结算和卸载时按完全相等比对。它在机器上不留任何记录，所以加载者要在第一条命令发出*之前*把它写到磁盘上——没有它，已加载的模型连拆都拆不掉。",
          "**节点的 generation 与 load generation 是同一个数字。**适配器会把 release receipt 的来源 generation 与它所属的那次加载比对，不一致就停掉该节点——因此一个用两个不同数字加载的环，服务完一次请求就会失去头部。",
          "**模型加载之前必须有运行日志（operational journal）**：它是让一次加载可重放安全的准入记录，而不是调试辅助。",
        ],
      },
      { t: "h2", kick: "它不做什么", text: "刻意的省略" },
      {
        t: "p",
        md: "p4 **不套用任何对话模板**——它转发不透明的提示词，并期待调用方已经渲染好模型的轮次格式。它**不做任何放置决策**。它也没有\"谁该收钱\"的概念：stage 汇报自己跑过的 token 行数，结算是别人的契约。每一项都是 Kvasir 在 [bridge](/wiki/bridge) 中填补的接缝，也正因如此，引擎才窄到足以持续跟上上游。",
      },
      {
        t: "callout",
        md: "**agent 与原生 stage server 是同一个发行版。**用更新的代码树构建的 agent 会在 READY 阶段失败，原因是 stage server 的 HELLO 中缺少某项能力——而且是在整个模型加载完之后。两者要从同一个 checkout 构建。",
      },
    ],
  },
  "in-flight-ring": {
    title: "在途环",
    summary: "一条永不排空的流水线：多个请求同时占据不同 stage，没有哪个 stage 需要等前面那个跑完。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 以 **stage 流水线**的形式服务一个模型，每个 stage 持有其中一段连续的层。一个 stage 跑完自己的层，把边界——一个 hidden state，而不是权重——传给下一个。没有节点持有完整模型，数据路径中间也不坐着任何东西：bridge 向头部提交、从尾部读取，而各 stage 通过自己的 agent 互相递交结果。",
      },
      { t: "h2", kick: "在途的那部分", text: "为什么排空的流水线浪费掉大半台机器" },
      {
        t: "p",
        md: "如果一条流水线要先跑完一个请求才接纳下一个，那么任一时刻除一个 stage 之外全都闲着——四 stage 的环只跑出四分之一的硬件。**在途（in-flight）**设计让多个请求同时流动：stage 3 在解码一个请求时，stage 0 已经在为另一个做 prefill。stage 会汇报自己持有一个批次多久、又有多久无事可提交，因此一个被饿着的环和一个饱和的环看起来不一样。",
      },
      {
        t: "code",
        caption: "四个 stage、三个请求、同一个瞬间。",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "成员构成", text: "一个批次究竟是什么" },
      {
        t: "p",
        md: "来自不同请求的行被打包进同一个物理批次，而这份确切的成员构成会原样转发给下游每一个 stage，而不是每跳重新决定一次。正是它让一次 prefill 与若干次 decode 共享同一趟计算，也正因如此，批次大小是一次加载的属性：方案事先写明 row 与 micro-batch 宽度，而这些宽度决定了一个 stage 所能返回的最大结果。",
      },
      {
        t: "callout",
        md: "**一条流水线至少要两个 stage。**单 stage 的流水线会被直接拒绝——头部与尾部是不同的角色，一个节点把两者合一，那是另一种引擎，而不是更小的环。",
      },
      {
        t: "p",
        md: "环是**延迟**路径，它的粒度是层。专家分片通过在层内部切分移除了这个下限，并接入同一套服务网络。",
      },
    ],
  },
  "layer-window": {
    title: "层窗口与部分分片",
    summary: "节点负责的模型连续切片——可作为 mini-GGUF 下载，而非完整检查点。",
    blocks: [
      {
        t: "p",
        md: "**层窗口**是环节点服务的连续 transformer 层范围。服务它不需要完整检查点——**stage mini-GGUF** 只携带窗口的张量：122B 上是 **254 MB、26 个张量**（共 338 个），对比 77.6 GB 的完整模型；手机的单层窗口约 1.5 GB。",
      },
      {
        t: "code",
        caption: "rank manifest 的一行：谁服务什么、在什么预算内。",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "认领，而非指派", text: "自助报名" },
      {
        t: "ul",
        items: [
          "节点轮询**覆盖/需求图**，查看哪些窗口服务不足、各付多少。",
          "它挑选适合自己预算的**最高奖励**未覆盖窗口，只下载那部分，然后加入。",
          "覆盖会自愈：节点流失后其窗口重新变得稀缺——因而再度有利可图。",
          "已用 NAT 后的手机端到端验证：轮询 → 自助报名 → 部分下载 → Adreno GPU 加载 → 完成环推理，贡献入账。",
        ],
      },
      {
        t: "p",
        md: "专家蜂群在更细的 **(层, 专家范围)** 粒度上复用这套完全相同的市场——同一张图、同一套自助报名、同一套奖励，只是单位更小。",
      },
    ],
  },
  moe: {
    title: "专家混合（MoE）",
    summary: "FFN 由数百个独立专家组成、每个 token 只激活其中几个的模型。",
    blocks: [
      {
        t: "p",
        md: "**专家混合**模型把每层的单个 FFN 换成一组独立的专家 FFN，外加一个每 token 挑选几个的**路由器**。今日服务中的 428B MoE —— Step-3.7-Flash 每层有 288 个专家，采用 top-8 路由。下面详细展开的示例是 Qwen3.5-122B-A10B，因为它才是数字被端到端实测过的那一个：",
      },
      {
        t: "stats",
        items: [
          { n: "49", l: "层" },
          { n: "256", l: "专家 / 层" },
          { n: "8", l: "每 token 激活" },
          { n: "12,544", l: "专家总数" },
          { n: "5.3 MB", l: "单个专家 (Q4)" },
          { n: "86%", l: "权重中专家占比" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "完整检查点" },
        ],
      },
      {
        t: "p",
        md: "每层分为**稠密路径**——注意力 + KV、各类 norm、路由器（`ffn_gate_inp`）、共享专家——和以三个堆叠张量（`ffn_up_exps`、`ffn_gate_exps`、`ffn_down_exps`）存储的**专家库**。稠密路径只占少数字节；专家库占模型的 86%。",
      },
      {
        t: "ul",
        items: [
          "专家索引是 **GGUF 最外层维度**，每个专家都是连续、量化块对齐的板块——提取就是字节范围拷贝，无需反量化。",
          "每 token 每层只有 **256 个中的 8 个**专家点亮，解码时一层的专家流量只是对一个 hidden 向量的几次小矩阵乘（约 6 KB 调度量）。",
          "专家彼此独立——所有权可以散布在各设备之间并自由再平衡。",
        ],
      },
      {
        t: "p",
        md: "这就是 MoE 是蜂群天然基质的原因：权重出厂时就已打包成设备大小、可独立持有的单元。",
      },
    ],
  },
  "expert-sharding": {
    title: "专家分片",
    summary: "在专家粒度上切分 MoE：手机背 42–340 MB 的专家，而不是 1.4 GB 的层。",
    blocks: [
      {
        t: "p",
        md: "**专家分片**把蜂群的承载单位从层（122B 上约 1.4 GB）降到专家（**5.3 MB**）。弱设备下载 8–64 个专家的切片（**42–340 MB**），作为纯函数 worker 加载——没有注意力、没有 KV、没有采样器——在骨干的路由器选中自己的专家时进行计算。",
      },
      {
        t: "callout",
        md: "**引擎状态。**专家粒度分片是在 Kvasir 上一代引擎上构建并演示的，下面的结果来自那项工作。当前引擎 [p4](/wiki/p4) 今天以层为粒度提供服务；把专家分片迁移到它上面已完成设计、正在推进。凡是点到具体工具或路由的细节，都是当时在上一代引擎上运行的那一套。",
      },
      { t: "h2", kick: "两个角色", text: "骨干 × worker" },
      {
        t: "code",
        caption: "一个 MoE 层内的切分点（路由器只在骨干上跑一次）。",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "**骨干**保有稠密路径（注意力、norm、路由器、共享专家、combine），并把所有专家作为 RAM 卸载的后备副本常驻，以获得抗流失能力。",
          "**Worker**（`linkcpp-expert-worker --serve`）通过一条长连接 TCP 流回答 `(n_used, n_tokens, cur, sel) → experts`——手机上正是 443 中继隧道的那条流。",
          "覆盖通过**专家覆盖市场**自愈：`POST /api/expert-coverage` 心跳上报持有，`GET /api/expert-demand` 聚合稀缺度，`POST /api/expert-volunteer` 按节点预算分配最稀缺的范围。",
        ],
      },
      { t: "h2", kick: "测量而非承诺", text: "已在真实硬件上验证" },
      {
        t: "ul",
        items: [
          "分片计算 == 整体计算，**max|Δ| = 3.6e-12**（是精确的重组，不是近似）。",
          "真实 122B 解码的跨进程调度：**argmax MATCH**，logit cosine 0.99869——与进程内逐字节一致。",
          "一台 Galaxy S25 自主下载其 1.58 GB 切片并逐 token 计算 layer-0 专家：与本地运行 **8/8 token 完全一致**。",
          "一张位于公网另一端的远程 GPU——每 token 一次 WAN 往返——仍保持**贪心 8/8 完全一致**（cosine 0.99773）：在直连链路上**1.2% 吞吐开销**，经 CDN 边缘 ~28%。这是串行逐 token 调度的诚实代价，也是为何织物的杠杆是批处理，而非更低延迟。",
          "批量调度在 batch 512（ROCm）达到每 worker **53k tok/s**——让蜂群实用化的吞吐织物特性。",
        ],
      },
    ],
  },
  "router-authority": {
    title: "路由器权威",
    summary: "蜂群的一致性不变式：路由只在骨干上决定一次——worker 只接收专家 id。",
    blocks: [
      {
        t: "callout",
        md: "**不变式：**网络内唯一的离散决策是 MoE 路由（256 选 top-8）。Kvasir 让路由器**只在骨干上跑恰好一次**，只把选中的专家 id 派发给 worker。异构蜂群可能在每个专家输出的*幅值*上略有差异——但在*哪些专家运行*上绝不会分歧。",
      },
      {
        t: "callout",
        md: "**引擎状态。**专家粒度分片是在 Kvasir 上一代引擎上构建并演示的，下面的结果来自那项工作。当前引擎 [p4](/wiki/p4) 今天以层为粒度提供服务；把专家分片迁移到它上面已完成设计、正在推进。凡是点到具体工具或路由的细节，都是当时在上一代引擎上运行的那一套。",
      },
      {
        t: "p",
        md: "没有这条规则，每个后端都会重跑路由器，在边界 token 上挑出**不同的专家**——真正灾难性的分歧，因为从那个 token 起计算像换了随机种子一样分叉。有了它，硬件差异就化为概率加权 combine 能吸收的有界连续误差。",
      },
      { t: "h2", kick: "它防住了什么", text: "一个决策点关闭的分歧模式" },
      {
        t: "table",
        head: ["分歧模式", "无权威时", "有权威时"],
        rows: [
          ["路由不一致", "各后端在边界处选出不同的 top-8", "id 只决定一次，派发给持有者"],
          ["轨迹分叉", "一个被翻转的 token 让整个序列分叉", "解码/采样固定在一个节点上"],
          ["验证", "跨后端逐位比较（不可能）", "对良定义残差做容差检查"],
        ],
      },
      {
        t: "p",
        md: "代价可以忽略：骨干本来就在计算 `ffn_norm` 和路由器 logits；过线的只是 hidden 行加上选中的 id——每个解码步约 **6 KB**。",
      },
    ],
  },
  "numerical-equivalence": {
    title: "数值等价性",
    summary: "不同后端永远不会逐位一致；蜂群把测得的容差当作一等契约。",
    blocks: [
      {
        t: "p",
        md: "CUDA、ROCm、Adreno 和 CPU 以不同的归约顺序、FMA 融合、累加器和超越函数近似计算同一算子——结果每算子相差约 1e-6…1e-3，**这是设计使然，永远不会逐位一致**。由任何到场硬件组成的蜂群无法要求逐位精确，所以 Kvasir 转而测量等价性。",
      },
      {
        t: "table",
        head: ["后端对（真实 122B，layer-0 专家）", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["手机 ARM CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "整个后端矩阵已补全：两个 GPU 后端（CUDA、ROCm）共享内核源码，落在**实质逐位相同**（cosine 1.0000000000），而 GPU↔CPU 对以 ~0.9997 等价。一个 CUDA worker 与一个 ROCm worker 可互换；一个 GPU worker 与一个 CPU worker 数值等价。",
      },
      { t: "h2", kick: "为何不同", text: "浮点加法不满足结合律" },
      {
        t: "ul",
        items: [
          "**matmul 归约顺序** — tensor core、MFMA tile、OpenCL workgroup 和 SIMD lane 以不同顺序累加。",
          "**累加精度** — F16/BF16 存储配 F32 或 F16 累加器，是影响分歧幅度的最大杠杆。",
          "**超越函数近似** — exp（softmax）、silu（swiglu）和 rsqrt（norm）在各后端使用不同的多项式/查表变体。",
        ],
      },
      { t: "h2", kick: "契约", text: "容差、能力、单一权威" },
      {
        t: "ul",
        items: [
          "验证是**容差**——\"top-1 一致率 ≥ 99.x%，KL ≤ ε\"——绝不是逐位相等。",
          "后端与累加精度作为节点**能力**公示；F32 累加节点在输出敏感的 rank 上优先。",
          "超出容差的节点被标记为不适合敏感 rank，而不是被整体拒绝。",
          "离散决策（路由、采样）固定在单一权威上，连续误差永远无法变成离散分歧。",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "推理引擎使用的量化模型文件格式——正是它的布局让部分切片与专家切片变得廉价。",
    blocks: [
      {
        t: "p",
        md: "**GGUF** 是推理引擎生态的单文件模型格式：元数据（架构、层数、维度、量化方式）加上以原始量化字节存储的张量（如 Q4_K_M）。放置方案正是照着这份元数据写的——层范围、设备分配与大小估算；服务侧则对张量字节切片来生成下载。",
      },
      {
        t: "ul",
        items: [
          "**Stage mini-GGUF** 只携带一个层窗口的张量——122B 环 stage 是 254 MB，而非 77.6 GB。",
          "**专家分片 GGUF** 携带一个（层，专家范围）切片，由 `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` 以 node-token 鉴权提供。",
          "两者都是**有效的 GGUF 文件**：节点上的读取器用原版工具直接加载，没有自定义格式。",
        ],
      },
      {
        t: "code",
        caption: "专家切片为何是字节拷贝：专家索引是最外层维度。",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "路由器（`ffn_gate_inp`）和共享专家被**排除**在专家分片之外——它们属于骨干，而这正是路由器权威所要求的。",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "网络的实用型代币：开发者用它支付推理，贡献者以算力赚取它。",
    blocks: [
      {
        t: "p",
        md: "**KVR**（链上名称 \"Kvasir\"，6 位小数，Solana）是双向流动的同一种代币：开发者通过网关**花费** KVR 运行推理，贡献者以节点提供的算力**赚取** KVR。奖励按真实工作计算——实际服务的层与专家——而不是按参与。",
      },
      {
        t: "ul",
        items: [
          "**支出侧** — 通过网关按推理付费：quote → payment → inference。",
          "**收入侧** — 算力按贡献单位 × 层份额 × 性能等级；bridge/网关角色按小时在线奖励。",
          "**结算** — 在 Solana 上，直达每个节点所有者自己的钱包；结算服务为经手请求的每个节点记账。",
          "**得名于神话** — 由 Kvasir 酿成的诗之蜜酒，让每个饮者获得智慧：开放的访问，以及给每个注入者的奖励。",
        ],
      },
      {
        t: "callout",
        md: "**Devnet，实用型代币。**KVR 目前运行在 Solana devnet 上，是实用型/贡献型代币——不是可交易资产、价格或投资品。此处内容均不构成投资建议或收益承诺。",
      },
    ],
  },
  "contribution-units": {
    title: "贡献单位",
    summary: "奖励公式：单位随所服务 token 按层份额加权累积，再按性能等级缩放。",
    blocks: [
      {
        t: "code",
        caption: "算力奖励如何计算。",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "一个**单位** ≈ 服务 1k token，并按该节点在每次推理中的**层份额**加权——运行 49 层中 12 层的节点赚取每次推理单位的 12/49。等级乘数继而奖励测得的速度，基础设施角色再叠加按小时在线奖励。",
      },
      { t: "h2", kick: "算例", text: "一次推理，四个节点" },
      {
        t: "table",
        head: ["节点", "层数", "份额", "等级", "每 1k token 有效单位"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["手机", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "奖励跟随**真实工作**：什么都没服务的节点无论在线多久都赚不到（就计算角色而言）。",
          "角色可**叠加**——一台机器可以身兼计算 + 网关 + bridge，各条流水相加。",
          "一切都以 KVR 结算到节点自己的所有者钱包；仪表盘展示原始 × 等级 = 有效贡献及可领取余额。",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "性能等级",
    summary: "测得的吞吐决定乘数：S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7。",
    blocks: [
      {
        t: "table",
        head: ["等级", "测得吞吐", "乘数"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "节点测得的解码速度决定其等级，等级乘到所赚单位上——更快的硬件同样的工作按比例赚得更多。钱包的节点状态页在贡献旁边显示每个节点的等级。",
      },
      {
        t: "ul",
        items: [
          "等级**靠测量，不靠自报**——吞吐来自节点的真实服务表现，并随时间重新测量。",
          "C 级手机也在赚——其层份额的 ×0.7——这正是重点：门槛面向所有参与者，而不只属于数据中心。",
          "等级乘的是*有效*单位，它与层份额、网关加成相互复合而非替代。",
        ],
      },
    ],
  },
  staking: {
    title: "质押",
    summary: "质押 100,000 KVR 的钱包即有资格运营 bridge 或网关节点。",
    blocks: [
      {
        t: "p",
        md: "质押会锁定 KVR，使钱包具备运营者角色与节点奖励资格。运营 **bridge** 或**网关**节点需要质押 **100,000 KVR**；普通计算节点无需任何质押即可加入，并按其运行的层赚取。",
      },
      {
        t: "ul",
        items: [
          "质押在钱包仪表盘的质押面板完成：输入数量、点击**质押**，该仓位即计入运营者资格与节点奖励。",
          "10 万门槛是对他人流量所依赖的两个角色——入口与控制平面——的**利益绑定过滤器**。",
          "在 devnet 上，已质押的 KVR 存放在质押金库（vault）中；质押数量与节点奖励都显示在质押面板中。",
          "用于质押的 devnet KVR 来自分发水龙头；手续费用的 devnet SOL 来自公共水龙头。",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "非托管钱包",
    summary: "密钥只存在于用户设备上——web、桌面、iOS 和 Android——奖励直接结算到它。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 钱包**从设计上就是非托管的**：12 个助记词和密钥只存储在用户自己的设备上，绝不交给运营方。奖励在 Solana 上直接结算到每个节点的所有者钱包——已在测试机队中的四个不同所有者钱包上验证，各自赚取自己的层份额。质押在 devnet 上的运作方式不同：在链上质押程序上线之前，已质押的 KVR 存放在网关的 treasury 中，并记录在其账本里。",
      },
      {
        t: "ul",
        items: [
          "**平台** — web、桌面（macOS/Windows/Linux，钱包+节点合一的 Electron 应用）、iOS 和 Android。",
          "**一套助记词，所有设备** — 同样的 12 词助记词在桌面、手机和 web 恢复同一账户；本地口令解锁每个安装。",
          "**钱包 = 节点身份** — 钱包为节点的网络身份签名，\"这台设备的收益归谁\"是密码学事实，而不是某人服务器上的账户行。",
          "**丢了助记词就丢了账户** — 非托管是双刃剑；不存在能帮你重置的运营方。",
        ],
      },
      {
        t: "p",
        md: "移动应用同时是节点应用：保管你 KVR 的同一个钱包，配置手机的计算后端与节点模式、质押并领取奖励。",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "运营者登录 = 对服务器 nonce 的钱包签名（Sign-In With Solana），外加可选 TOTP 2FA。",
    blocks: [
      {
        t: "p",
        md: "在公共部署中，运营者对网关的访问通过 **Sign-In With Solana** 认证：运营者的钱包对服务器签发的 nonce 签名，无需任何密码或托管凭据即可证明所有权。在此之上，**TOTP 2FA** 和一次性备份码保护会话。",
      },
      {
        t: "ul",
        items: [
          "**任何地方都没有密码** — 钱包密钥即身份，nonce 防止重放；服务器端没有可被钓鱼或泄漏的东西。",
          "**按钱包的 TOTP 登记**持久化在网关账本中，因此 2FA 在重启后依然有效。",
          "**备份码一次性使用** — 每次登录消耗一个，用于认证设备不可用时的恢复。",
          "**范围如实声明** — bridge 与引擎端口为可信主机 / LAN / VPN 设计；SIWS + 2FA 是让*公共*域名可以安全暴露的那一层。",
        ],
      },
    ],
  },
  "token-economy": {
    title: "KVR 经济",
    summary: "消费者成本与节点奖励如何构成一个自我强化的循环——让网络越大越便宜的良性循环。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 是一个以单一代币结算的**双边市场**。消费者按推理向国库支付 **KVR**；节点则因其实际服务的工作赚取 **KVR**，直接付回各自的钱包。设计目标是让这两侧不彼此竞争，而是**相互复利**：更多供给让网络更便宜、更好，从而吸引更多需求，其支付又资助更丰厚的奖励，进而吸引更多供给。",
      },
      { t: "h2", kick: "飞轮", text: "使用与供给一起成长" },
      {
        t: "p",
        md: "由于推理**必须**用 KVR 支付，代币与真实使用绑定——是实用，而非投机。使用为节点所赚的 KVR 提供资金，使贡献保持吸引力，从而扩大容量，压低价格与延迟，进而吸引更多使用。Kvasir 最锋利的优势让这个循环更加紧凑：一个参与者可以**同时是消费者和供给者**（*产消者*），因此两侧往往在同一群人身上一起成长。",
      },
      {
        t: "callout",
        md: "**\"贡献即免费\"是净额免费，而非零成本。**你为自己的推理付费，为自己服务的工作赚取；贡献量大致等于消费量，两者便相互抵消。网络本身并不免费——免费的是*你的*账单。",
      },
      { t: "h2", kick: "保持良性", text: "三条不变式，及它们防止的螺旋" },
      {
        t: "table",
        head: ["不变式", "它防止的螺旋"],
        rows: [
          ["奖励由真实收入资助（增发仅用于启动，随后逐步收缩）", "通胀侵蚀 KVR，直到两侧一起崩溃"],
          ["KVR 是推理的强制媒介", "代币价值与使用脱钩，沦为纯粹投机"],
          ["价格在成本下限与低于市场的上限之间浮动", "过低会饿死节点；过高则把用户输给中心化 API"],
        ],
      },
      {
        t: "p",
        md: "Kvasir 已经在奖励**真实工作**（按服务 token 数 × 层份额计算的 KVR，而非仅仅在线），并直接支付到每个节点自有的钱包——这正是让收入资助的奖励保持诚实的难点所在。其余部分——由利用率驱动的价格，以及增发→收入的逐步收缩——是把\"更多节点 → 更便宜\"从直觉变成协议强制规则的经济路线图。**推理定价**条目讲价格一侧；**贡献单位**讲工作如何变成奖励。",
      },
    ],
  },
  "inference-pricing": {
    title: "推理定价",
    summary: "一次推理今天在 KVR 中的花费、去中心化网络为何在结构上更便宜，以及价格如何应随供给增长而下降。",
    blocks: [
      {
        t: "p",
        md: "访问网络采用**按推理付费**：网关为你的请求报出一个 KVR 价格，你的钱包在链上支付，然后环才运行模型。定价是一个小而透明的公式——每请求的下限加上每 token 的费率——预先报价，并在生成后按**实际** token 用量结算。",
      },
      {
        t: "code",
        caption: "结算公式——事前报价，事后按真实用量计费。",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "为何能更便宜", text: "没有中心化的利润要付" },
      {
        t: "p",
        md: "中心化 API 的定价是成本**加上**一大块利润与资本回收。去中心化网络的定价则接近贡献者的**边际成本**——电费与硬件摊销——再加一层薄薄的协议费。这个结构性差距无论规模大小都存在。而增长会拉大它：**专家分片**意味着更多节点各持一块更小的切片，于是更廉价的设备也能服务，降低参与的边际成本并加深供给。",
      },
      {
        t: "callout",
        md: "**价格是受治理的，而非任人设定。**费率是敏感的经济参数，只能由 genesis 钱包在钱包签名 + 2FA 之下更改——绝不通过环境变量。这让代币经济保持稳定且可审计。",
      },
      { t: "h2", kick: "走向何方", text: "由利用率驱动的价格" },
      {
        t: "p",
        md: "设计方向是一个**随网络利用率浮动**的价格，介于下限（保持在节点边际成本之上，使服务仍然值得）与上限（保持在中心化替代品之下，使其保持竞争力）之间。空闲的供给把价格往下推；拥塞把价格往上推。这正是最终让**\"更多共享节点 → 更低价格\"**在代码中成真的机制——**KVR 经济**的天然恒温器。",
      },
    ],
  },
  "run-expert-worker": {
    title: "运行一个专家 worker",
    summary: "把闲置的 GPU、CPU 或手机变成专家 worker：构建它、为最稀缺的切片报名、下载它、服务，并通过 443 向外拨号赚取 KVR。",
    blocks: [
      {
        t: "p",
        md: "一个**专家 worker** 是纯 `(hidden, ids) → out` 函数——没有注意力、没有 KV 缓存、没有采样器——在骨干的路由器选中它们时计算 MoE 模型专家的一个切片。你不选择服务什么；**覆盖市场**把裁剪到你预算内的、最稀缺、最高奖励的范围交给你，因此一台 4 GB 手机和一张数据中心 GPU 都能找到位置。",
      },
      {
        t: "callout",
        md: "**引擎状态。**专家粒度分片是在 Kvasir 上一代引擎上构建并演示的，下面的结果来自那项工作。当前引擎 [p4](/wiki/p4) 今天以层为粒度提供服务；把专家分片迁移到它上面已完成设计、正在推进。凡是点到具体工具或路由的细节，都是当时在上一代引擎上运行的那一套。",
      },
      { t: "h2", kick: "七个步骤", text: "构建 → 报名 → 服务 → 拨号 → 赚取" },
      {
        t: "code",
        caption: "整条路径——拨号脚本用重试循环等待骨干。",
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
          "**切片很小。** layer-0 的 128 专家切片对比 72 GB 的完整模型只有 **794 MB**——正是让弱设备也能参与的粒度。你只下载市场分配给你的范围。",
          "**只拨出，绝不拨入。** 第 5 步在 443 上打开一条出站 WebSocket，因此运营商 NAT 和 CDN 边缘都放行，而你不暴露任何入站端口——与手机所用的路径相同。",
          "**心跳是承重的。** 没有 `POST /api/expert-coverage`，你所服务的一切对需求图都不可见，你所做的一切也都不会入账。",
          "**奖励按工作计。** 桥接的工作累积到 bridge 的贡献账本；网关把 KVR 增量记入你**自己的**钱包。你需要一个钱包地址才能收款。",
        ],
      },
      {
        t: "callout",
        md: "worker 与数据中心内的 GPU 说同一套调度协议——通过一条长连接流交换 `(n_used, n_tokens, cur, sel) → experts`。部分分片 worker 只是把 `n_used = 1`。正是这种一致性，让手机、CPU 主机和 Blackwell 卡成为同一蜂群中可互换的成员。",
      },
    ],
  },
  "bridge-operations": {
    title: "运营 bridge",
    summary: "运营 bridge 及其背后那个环的笔记：加载一份方案、挺过重启、让贡献持续流动，并把引擎挡在互联网之外。",
    blocks: [
      {
        t: "p",
        md: "**网关**（公共入口）与 **bridge**（引擎门面）是运营者要保持健康的两个长期服务，其后是 p4 的 agent 和它们的 stage server。引擎完全不做认证——它假定彼此可达的机器就是应该彼此可达的——所以一切公共流量都汇聚到网关，而 bridge 经隧道访问、不对外发布。",
      },
      { t: "h2", kick: "加载", text: "一份方案，以及加载它所用的那个数字" },
      {
        t: "ul",
        items: [
          "**放置是你写的方案**，不是你发的请求：谁要求 bridge 去服务，它就回 `409`。先生成方案、dry-run，再用 `--confirm` 加载。",
          "**load generation 在第一条命令发出之前就写到磁盘上。**它由加载者选定，在每次会话和卸载时按完全相等校验，且在机器上不留任何记录——弄丢了，已加载的模型连拆都拆不掉。",
          "**节点的 generation 就是同一个数字。**用两个不同的数字加载一个环，它只服务一次请求头部就会停，下一次会话则会半加载地挂住。每次加载都用一个新值，否则上一次失败尝试遗留的注册会与它冲突。",
          "**没有运行日志（operational journal），agent 就拒绝加载。**那是可重放安全的加载所需的准入记录，不是调试开关。",
        ],
      },
      { t: "h2", kick: "挺过重启", text: "什么会回来，什么不会" },
      {
        t: "ul",
        items: [
          "**agent 重启会丢掉它的节点。**stage server 只存在于运行时；模型必须照方案重新加载一次。这是恢复流程，而不是哪里出了故障。",
          "**网关不会替你重新加载。**它的环看门狗会发现某个模型停止了服务，从 bridge 的 `409` 中得知放置在外部，说一次之后就不再问。",
          "**贡献计数器活在 bridge 的内存里。**网关每 30 s 轮询一次并增量记账；重启只丢掉尚未被轮询的部分，而当计数器倒退时网关会重设基线，而不是重复支付。",
        ],
      },
      { t: "h2", kick: "锁紧", text: "引擎不面向互联网" },
      {
        t: "ul",
        items: [
          "把 bridge 绑定到 loopback 并给它一个服务令牌。没有令牌它谁都不认证，任何能触达它的东西都能免费驱动整个环——它在启动时就会这样告诉你，而不是让你事后才发现。",
          "agent 公示的是其他 agent 要拨的地址。用主机之间最快的那张网，跨主机绝不用 loopback，并把那张网挡在公网之外。",
          "如果某个东西必须坐在公网 IP 上，记住 Docker 发布的端口**在 INPUT 链之前就已被 DNAT**，因此针对 `dport` 的规则不会命中。改在 `DOCKER-USER` 链中按 conntrack 的原始目的端口（`--ctorigdstport`）过滤，并用排在 `After=docker.service` 之后的 systemd oneshot 持久化。",
        ],
      },
      { t: "h2", kick: "自伤陷阱", text: "两个真花时间的坑" },
      {
        t: "ul",
        items: [
          "**未设置的运营者钱包看上去就是零收益。**网关会跳过任何没有所有者的贡献行，且不记录任何日志。节点明明在服务，看起来却像闲置。",
          "**`pkill` 会匹配到它自己的命令行。**`ssh host 'pkill -f server.js; ...'` 杀掉的是正在运行它的那个 shell。把匹配模式放进脚本文件而不是远程命令里，用字符类（`server[.]js`），并且记住以裸 `node server.js` 启动的进程没有路径可匹配——改用它监听的端口去找。",
        ],
      },
    ],
  },
  "wan-interconnect": {
    title: "广域互联（200G 光模块）",
    summary: "算力站点如何以 200 Gb/s 跨越一个房间、一个园区或一座城市互联：什么距离用什么光模块、什么插到哪里，以及真正跑满线速需要什么。",
    blocks: [
      {
        t: "p",
        md: "当两个站点都有公网路由时，专家调度的数据平面应当是**直连**——中继是给没有自己地址的边缘用的。本条目是用目录现货零件把那条直连做到 200 Gb/s 级别的具体配方。一条规则统领一切：**光纤是与速率无关的玻璃；速率活在两端的可插拔模块里。**",
      },
      { t: "h2", kick: "第 1 步 · 按距离挑选", text: "可达距离阶梯" },
      {
        t: "table",
        head: ["距离", "零件", "插入位置"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC，无需交换机"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / 交换机"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "NIC 或交换机的 QSFP56 笼位"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "NIC 或交换机的 QSFP56 笼位"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "交换机/路由器的 QSFP-DD 笼位（不是 NIC）"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "由你的交换机交接给运营商"],
        ],
      },
      { t: "h2", kick: "第 2 步 · 什么插到哪里", text: "NIC 侧 vs 交换机侧" },
      {
        t: "ul",
        items: [
          "**NIC 侧** — ConnectX-6/7 级别的网卡暴露 QSFP56 笼位；DAC/AOC/FR4/LR4/ER4 全都可直接插入 NIC。GB10 级别的主机板载已有两个 200 GbE QSFP 端口，因此两个站点之间的链路恰好只需一根线缆、零新硬件。",
          "**交换机侧** — 相干 ZR+ 光模块是 QSFP-DD 形态，应插入交换机或路由器；站点的 NIC 再经一条短 DAC 以 200G 接入那台交换机。当对端站点在数十公里之外时用这一档。",
          "**光纤本身** — 标准单模（G.652）双工 LC 对，按每芯以暗光纤租用。同一根玻璃今天承载 100G、日后承载 400G；升级只是换模块，绝非土建工程。",
          "**超过约 120 km** — 你不再购买零件，而是开始向运营商租用一个波长；分界点是你交换机上的一次以太网交接。",
        ],
      },
      {
        t: "code",
        caption: "三种参考搭建，从最便宜开始。",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "第 3 步 · 真正跑满 200G", text: "线速是配置出来的，不是买来的" },
      {
        t: "ul",
        items: [
          "在可用之处，为调度流使用 **RDMA (RoCE)**——GB10 级别的主机通过拆分的 PCIe 链路给 NIC 供数据，在拓扑正确映射的情况下 RoCE 下能测得满速（约 185–190 Gb/s）；映射错误的路径会卡在约一半速率，而未调优的普通 TCP 则低得多。",
          "端到端启用**巨型帧（MTU 9000）**，并在调度套接字上保持 `TCP_NODELAY`（bridge 已经设置了它）。",
          "要*验证*，别假设：每次物理改动后都在两个站点之间跑一次 perftest——95 与 190 Gb/s 之间的差距在测量之前是看不见的。",
          "把 **443 中继保留为回退路径**——拨号策略对公网对端是直连优先、对 NAT 走中继。中继的职责是可达性，直连的职责是速度。",
        ],
      },
      {
        t: "p",
        md: "这对架构为何重要：解码延迟受往返时间约束（光纤中约 5 µs/km——是物理规律，不受带宽影响），所以粗管道买到的是**prefill 速度、批量调度吞吐，以及近乎瞬时的专家切片分发**，而不是更低的每 token 延迟。这正是双层设计中站点层的角色：在粗管道层提供容量，在中继层提供可达性。",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "负载自适应扩展",
    summary: "Kvasir 的 MoE 服务路径随流量伸缩：饱和时协调器重新启用已验证的 worker，bridge 通过抬高专家需求招募闲置节点——全部拉取式，因此 NAT 后的设备也能加入。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 的 MoE 服务路径随负载弹性扩展，分为两个协作的层。清闲时协调器在本地服务一切，取得每 token 最快的路径；饱和时，下述两层把蜂群扩大——浪涌过去后再度收缩。",
      },
      {
        t: "callout",
        md: "**引擎状态。**专家粒度分片是在 Kvasir 上一代引擎上构建并演示的，下面的结果来自那项工作。当前引擎 [p4](/wiki/p4) 今天以层为粒度提供服务；把专家分片迁移到它上面已完成设计、正在推进。凡是点到具体工具或路由的细节，都是当时在上一代引擎上运行的那一套。",
      },
      { t: "h2", kick: "第 1 层", text: "协调器侧：负载自适应调度" },
      {
        t: "p",
        md: "骨干协调器（一台运行完整模型的 `linkcpp-server`）服务路由到的专家，要么在自己的 GPU 上（快、本地），要么把它们调度给远程 worker。一个后台线程每隔几秒决定用哪种方式：",
      },
      {
        t: "ul",
        items: [
          "它轮询自己**本机**的推理槽位。当 `busy >= saturation threshold`（默认 2）时，协调器处于负载之下。",
          "负载之下，若有一个**已验证（proven）**的 worker 在线——其 `last_serve_ms > 0`，即它此前真正计算过专家——协调器会越过正常的空闲超时继续向它调度，以总吞吐优先于每 token 延迟。",
          "连接过却从未服务过的 worker（一部拨通了 relay 却从未计算的手机）在负载下**不会**被招募，因为向它调度会用一条慢速回退替换掉快速的本地路径。新 worker 仍会通过一个短暂的宽限窗口获得首次尝试。",
          "自查询有时间上限，因此一次卡住的轮询绝不会阻塞调度。",
        ],
      },
      { t: "h2", kick: "第 2 层", text: "控制平面侧：负载自适应招募" },
      {
        t: "p",
        md: "控制平面盯着每个 MoE 协调器，并在需要时扩大 worker 池：",
      },
      {
        t: "ul",
        items: [
          "一个后台循环轮询每个协调器的槽位，并按模型记录饱和度。",
          "当一个模型处于饱和时，其**有效专家副本目标**被抬高（base + boost）。覆盖市场随即把已覆盖的专家重新读作稀缺，而**没有**任何在线 worker 的模型则从其 GGUF 元数据（专家数）播种，使需求即便从零也可见。",
          "闲置节点轮询需求市场（`/api/expert-volunteer`），被交予一份 `(layer, expert-range)` 切片去服务。它们下载切片、拨通 relay、注册覆盖；控制平面自动把它们接线到协调器的调度图。",
          "当负载退去，目标回落、需求消失，于是多余的 worker 不再被调度并逐渐老化退出。",
        ],
      },
      {
        t: "callout",
        md: "该设计是**拉取式（pull-based）**的：节点主动索取工作而非被推送，因此 NAT 后的 worker 无需任何入站连接即可参与。在第 2 层被招募、并开始服务的节点会成为一个**已验证（proven）**的 worker，随后第 1 层在负载下持续启用它——两层组合成一个弹性回路。",
      },
      {
        t: "p",
        md: "**可观测性：**`GET /api/moe/recruitment` 报告每个模型的 busy/saturation 及 base 与有效目标的对比；`/api/expert-demand` 带有一个 `recruiting` 标志。",
      },
    ],
  },
};
