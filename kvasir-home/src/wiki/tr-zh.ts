/* 中文 — 维基条目翻译。结构（slug、分类、区块顺序、代码）严格镜像 entries.ts
   （英文源）；技术术语与标识符（KVR, linkcpp, 推理引擎, GGUF, MoE, ring runtime,
   tok/s 等）保持原文。合规表述（devnet、实用型代币、非托管）保持不变。 */
import type { WikiTranslation } from "./entries";

export const zhWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Kvasir 网络",
    summary: "一个去中心化 AI 推理网络（DePIN）：日常设备共同服务开源模型并赚取 KVR。",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** 是一个去中心化 AI 推理网络：大型开源模型通过 **linkcpp** 引擎分布在共享硬件上，任何单一节点都不持有完整模型。任何人都可以贡献 GPU、CPU、NPU——甚至手机——并按设备实际服务的层或专家赚取 **KVR**。开发者通过 OpenAI/Anthropic 兼容网关访问网络，按推理付费。",
      },
      {
        t: "ul",
        items: [
          "**源码可得的引擎** — linkcpp 采用 BSL 许可（开发与测试免费，生产用途需许可证）；其下的 推理引擎 数据平面保持原版、可审查。",
          "**非托管** — 奖励结算到每个节点所有者自己的 Solana 钱包；密钥从不离开用户。",
          "**已在真实硬件上验证** — 一个 122B 模型曾拆分运行在 4 张 AMD MI250 上，并在 GPU/CPU/NPU/移动设备混合的异构机队中端到端记账每个节点的贡献。",
          "**名字源自北欧神话** — Kvasir：由众神的精华汇聚而生、却不属于任何一位神的最智慧存在。",
        ],
      },
      { t: "h2", kick: "一个请求，众多设备", text: "一次推理如何流动" },
      {
        t: "code",
        caption: "每一跳都是普通的 HTTP/TCP；被分布的是模型本身。",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "角色可以**叠加**：一台机器可以同时是计算节点、网关主机和 hub 主机，奖励相加。网络的职责是让这个集合看起来像一台机器——前面是一个端点，后面是成千上万台并不完美的设备。",
      },
      {
        t: "p",
        md: "目前网络运行在 **Solana devnet** 上；KVR 是实用型/贡献型代币，不是可交易资产或投资品，本页内容均不构成任何投资建议。",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "控制平面：发现设备、规划层放置、启动 worker、编排环。",
    blocks: [
      {
        t: "p",
        md: "**Hub** 是网络的控制平面，由 linkcpp 以单个 Docker 镜像提供（`controller.hub:app`，端口 **19000** 的 FastAPI 服务）。它发现设备、检查运行时兼容性、用规划器计算放置、启动原版 推理引擎 worker，并暴露每个控制器的网关。它刻意做成\"无聊\"的基础设施：请求/响应式 HTTP、重启安全的状态、没有花哨的传输层。",
      },
      { t: "h2", kick: "三扇门", text: "机器如何加入 hub" },
      {
        t: "ul",
        items: [
          "**本地节点槽** — 每个 hub 五个固定槽位，映射到 RPC 端口 **50052–50056**。槽位始终存在；你编辑槽位的 GPU + VRAM/RAM/CPU 预算而不是创建任意节点，且资源**仅在槽位未绑定时**可编辑——这保护了运行中控制器之下的容量契约。",
          "**远程单元** — 注册另一个运行中的 linkcpp hub 并导入其可见节点。数据平面端点始终从注册的*单元* URL 加上单元暴露的 worker 端口推导——绝不采用远端系统自报的节点主机。",
          "**受管节点代理** — 仅作 worker 的服务（`nodeagent.py`），通过普通请求/响应 HTTP（`/control/join|status|download|load|unload`）加入并经 `POST /api/node-reports` 汇报。刻意**不用**持久流，以便在简单的 LAN/VPN 路由下存活。",
        ],
      },
      { t: "h2", kick: "未经验证，一概不加载", text: "兼容性闸门" },
      {
        t: "p",
        md: "每个单元、节点和代理都汇报协议/运行时包身份及后端细节。单元、运行时包、推理引擎 修订版和 RPC ABI 不匹配会在 bind/plan/load/infer **之前被硬性阻断**；后端差异（CUDA/Metal/Vulkan/CPU）作为节点能力记录，而不是拒绝理由。无法提供安全规划所需资源监控的节点也会被排除在自适应加载之外。",
      },
      {
        t: "code",
        caption: "重启后什么会保留，什么不会。",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "由于 hub 是最关键的角色，hub 主机获得**最高的按小时在线奖励**。运营公共 hub 需要质押 **100,000 KVR**。",
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
        md: "用量以 KVR 通过三步流程结算——**quote → payment → inference**——请求在执行前定价，服务它的节点在执行后记账。网关还从每个可达的 hub 聚合**实时模型目录**，因此 `/v1/models` 反映的是网络此刻真正能服务的内容。",
      },
      {
        t: "ul",
        items: [
          "网关主机因保持入口在线获得**按小时在线奖励**，并对其协助服务的每次推理获得 **×1.5 加成**。",
          "运营公共网关需要质押 **100,000 KVR**（与 hub 相同）。",
          "公共部署用 **SIWS + 2FA** 保护运营者访问；裸 hub 仅为可信主机 / LAN / VPN 设计。",
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
          "节点注册在所有者钱包名下；奖励非托管地结算到该钱包。四个不同所有者钱包各自赚取其层份额已被端到端验证。",
          "能力数据（后端、累加精度、资源预算）决定规划器可以在该节点上放什么——以及在蜂群中它可以担任哪些 rank。",
          "无法提供资源监控的节点会被排除在自适应加载之外，而不是被盲目信任。",
        ],
      },
    ],
  },
  "relay-443": {
    title: "443 中继",
    summary: "面向 NAT 后设备的数据平面：两端都通过 443 端口的 WebSocket 桥向外拨号。",
    blocks: [
      {
        t: "p",
        md: "运营商 NAT 后的手机无法接受入站连接，而 Cloudflare 这类边缘只放行 80/443 端口。**443 中继**同时解决两者：带 **1 字节 role preamble** 的每边缘 WebSocket 桥让两端都**向外**拨号，因此手机**不开放任何入站端口**即可参与数据平面。",
      },
      {
        t: "code",
        caption: "两个出站连接在中间相遇；preamble 表明谁是谁。",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "在生产中淬炼", text: "三个真实 bug，三个修复" },
      {
        t: "ul",
        items: [
          "**构建指纹一致性** — 在任何张量字节流动之前，两端必须证明运行同一运行时包。",
          "**node-token 下载鉴权** — 部分分片下载用应用已持有的、由钱包派生的 node token 鉴权。",
          "**`Int.ushr` 帧停滞** — Kotlin 的 `ushr` 只取移位量的低 5 位，`len ushr 56` 变成了 `len ushr 24`，静默损坏所有 ≥ 64 KiB 的帧（593 KB 的 `result_output` 是第一个受害者）。改用 `Long` 移位打包长度修复——对经常超过 64 KiB 的批量专家调度而言是承重修复。",
        ],
      },
      {
        t: "p",
        md: "中继承载拓扑所需的一切——环的层边界或专家调度流——为环验证过的同一机制，正是蜂群中生产环境手机 worker 所用的。",
      },
      {
        t: "p",
        md: "`/api/expert-relay` 与 `/api/ring-relay` 两个升级端点都是**裸拼接**的：网关逐字节转发 WebSocket 帧而不加解析，因此中继始终是一根轻薄、与模型无关的管道。它仍会**按会话计量所桥接的字节数**，而这份测得的工作流入 hub 的贡献账本，并以 **KVR** 结算到 worker 自己的钱包——为 NAT 后的手机做中继，与直连节点赚得分毫不差。",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "源码可得（BSL）的控制平面，把日常硬件变成分布式推理引擎。",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** 是 Kvasir 背后的引擎：围绕 推理引擎 RPC 数据平面的控制平面，用*原版* `ggml-rpc-server` / `llama-server` 二进制在多张 GPU 和多台机器上运行大型 AI 模型。它增加的一切都是编排——GPU 发现、节点槽、层放置规划、worker 启动，以及 OpenAI/Anthropic 网关。",
      },
      { t: "h2", kick: "架构", text: "一个 hub，原版 worker" },
      {
        t: "code",
        caption: "经过 linkcpp 部署的请求路径。",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**以 BSL 提供源码**——用于开发和测试可免费阅读、运行并在其上构建；生产用途需购买许可证。",
          "推理引擎 数据平面保持**未分叉**（除一个固定的移动端 GPU-over-RPC 补丁外），上游的性能改进持续流入。",
          "以**单个 Docker 镜像**交付：FastAPI hub 加两个 推理引擎 二进制；原生 worker 节点在 Docker 之外为 CUDA/Metal/Vulkan/CPU 构建。",
        ],
      },
      { t: "h2", kick: "规划器", text: "输入 GGUF 元数据，输出放置方案" },
      {
        t: "p",
        md: "规划器读取 GGUF 元数据，产出每节点连续层窗口、对应的 `--tensor-split`、每节点 KV 缓存/层/专家 VRAM 估算——以及可选的 MoE 专家 FFN 向节点 RAM 卸载，以 推理引擎 `-ot` 规则形式生成（如 `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`）并经 `--override-tensor` 传到启动。放不下的方案会在加载前被报为 **infeasible**，而不是在运行时以 OOM 的形式被发现。",
      },
      {
        t: "p",
        md: "运行时兼容性是一等概念：协议、运行时包、推理引擎 修订版和 RPC ABI 都会被校验，任何不匹配都在 bind/plan/load/推理之前被硬性阻断。",
      },
    ],
  },
  "ring-runtime": {
    title: "环运行时",
    summary: "无 master 的流水线推理：每台设备运行自己的层窗口，只向邻居传递边界。",
    blocks: [
      {
        t: "p",
        md: "**环运行时**是 Kvasir 的低延迟服务拓扑。每台设备只加载自己连续的**层窗口**，然后恰好打开两条链路——前驱与后继。hidden-state 边界沿环流动；最后一个 rank 采样 token 并送回。**没有中心 master，也没有节点持有完整模型。**",
      },
      { t: "h2", kick: "为什么不是星形", text: "RPC master 问题" },
      {
        t: "p",
        md: "在经典 RPC 拓扑中，一个 master 打开**整个 GGUF** 并向每个 worker 拨号。这在开放网络中有三处失效：master 必须持有并服务完整检查点；每个 worker 都必须可被拨号——运营商 NAT 后的手机不行；master 还是这个本不该有所有者的网络中的单一所有者。环消除了这三点：每个 stage 拥有自己的窗口，连接只在邻居之间，中继让 NAT 设备可达。",
      },
      {
        t: "code",
        caption: "4-stage 环上的一个解码步。",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "放置来自规划器的 **rank manifest**——例如 Qwen3.5-122B 的 49 层分布到 GPU、CPU、NPU 和手机上。",
          "边界很小（每 token 一个 hidden-state 向量），即使链路很弱，跳的代价也低。",
          "移动 GPU **直接**运行环 stage（Adreno 经 OpenCL）——通往手机 GPU 的 RPC 路径不可行，因为 Adreno 的缓冲布局无法经受 RPC 序列化；而本地 stage 自己拥有后端，只有边界过线。",
        ],
      },
      {
        t: "p",
        md: "环是**延迟**路径；它的下限是层粒度（122B 上约 1.4 GB）。专家分片蜂群移除了这个下限，并接入同一服务网络。",
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
        md: "**专家混合**模型把每层的单个 FFN 换成一组独立的专家 FFN，外加一个每 token 挑选几个的**路由器**。Qwen3.5-122B-A10B 是网络的旗舰示例：",
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
    summary: "推理引擎 使用的量化模型文件格式——正是它的布局让部分与专家切片变得廉价。",
    blocks: [
      {
        t: "p",
        md: "**GGUF** 是 推理引擎 生态的单文件模型格式：元数据（架构、层数、维度、量化方式）加上以原始量化字节存储的张量（如 Q4_K_M）。linkcpp 的规划器读取元数据来计算放置与大小估算；服务侧对张量字节切片来生成下载。",
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
          "**收入侧** — 算力按贡献单位 × 层份额 × 性能等级；hub/网关角色按小时在线奖励。",
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
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "角色可**叠加**——一台机器可以身兼计算 + 网关 + hub，各条流水相加。",
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
    summary: "质押 KVR 赚取 APR 利息；质押 100,000 KVR 的钱包才有资格运营 hub 或网关节点。",
    blocks: [
      {
        t: "p",
        md: "质押把 KVR 锁在你自己的钱包里以赚取 **APR 利息**并获得节点奖励资格。运营 **hub** 或**网关**节点需要质押 **100,000 KVR**；普通计算节点无需任何质押即可加入，并按其运行的层赚取。",
      },
      {
        t: "ul",
        items: [
          "质押在钱包仪表盘的质押面板完成：输入数量、点击**质押**，仓位即开始累积 APR 与节点奖励资格。",
          "10 万门槛是对他人流量所依赖的两个角色——入口与控制平面——的**利益绑定过滤器**。",
          "质押与其他一切一样是非托管的：仓位存于你自己的钱包，本金、累计利息和节点奖励都显示在质押面板中。",
          "用于质押的 devnet KVR 通过分发或兑换获得（SOL/ETH ↔ KVR 兑换：即将上线）；手续费用的 devnet SOL 来自公共水龙头。",
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
        md: "Kvasir 钱包**从设计上就是非托管的**：12 个助记词和密钥只存储在用户自己的设备上，绝不交给运营方。奖励在 Solana 上直接结算到每个节点的所有者钱包——已在四个不同所有者钱包上验证，各自赚取自己的层份额。",
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
        md: "在公共部署中，hub 与网关的运营者访问通过 **Sign-In With Solana** 认证：运营者的钱包对服务器签发的 nonce 签名，无需任何密码或托管凭据即可证明所有权。在此之上，**TOTP 2FA** 和一次性备份码保护会话——hub 与网关两侧皆然。",
      },
      {
        t: "ul",
        items: [
          "**任何地方都没有密码** — 钱包密钥即身份，nonce 防止重放；服务器端没有可被钓鱼或泄漏的东西。",
          "**按钱包的 TOTP 登记**持久化在 hub 状态中，2FA 与槽位、绑定一起在重启后保留。",
          "**备份码一次性使用** — 每次登录消耗一个，用于认证设备不可用时的恢复。",
          "**范围如实声明** — 裸 hub 与 RPC 端口为可信主机 / LAN / VPN 设计；SIWS + 2FA 是让*公共*域名可以安全暴露的那一层。",
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
        md: "由于推理**必须**用 KVR 支付，每一单位使用都是对代币的真实需求——是实用，而非投机。这份需求支撑着节点所赚 KVR 的价值，使贡献保持吸引力，从而扩大容量，压低价格与延迟，进而吸引更多使用。Kvasir 最锋利的优势让这个循环更加紧凑：一个参与者可以**同时是消费者和供给者**（*产消者*），因此两侧往往在同一群人身上一起成长。",
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
        md: "Kvasir 已经在奖励**真实工作**（按服务 token 数 × 层份额计算的 KVR，而非仅仅在线），并以非托管方式结算——这正是让收入资助的奖励保持诚实的难点所在。其余部分——由利用率驱动的价格，以及增发→收入的逐步收缩——是把\"更多节点 → 更便宜\"从直觉变成协议强制规则的经济路线图。**推理定价**条目讲价格一侧；**贡献单位**讲工作如何变成奖励。",
      },
    ],
  },
  "inference-pricing": {
    title: "推理定价",
    summary: "一次推理今天在 KVR 中的花费、去中心化网络为何在结构上更便宜，以及价格如何应随供给增长而下降。",
    blocks: [
      {
        t: "p",
        md: "访问网络采用**按推理付费**：网关为你的请求报出一个 KVR 价格，你的钱包在链上支付，然后 hub 才运行模型。定价是一个小而透明的公式——每请求的下限加上每 token 的费率——预先报价，并在生成后按**实际** token 用量结算。",
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
          "**奖励按工作计。** 桥接的工作累积到 hub 的贡献账本；网关把 KVR 增量记入你**自己的**钱包（非托管）。你需要一个钱包地址才能收款。",
        ],
      },
      {
        t: "callout",
        md: "worker 与数据中心内的 GPU 说同一套调度协议——通过一条长连接流交换 `(n_used, n_tokens, cur, sel) → experts`。部分分片 worker 只是把 `n_used = 1`。正是这种一致性，让手机、CPU 主机和 Blackwell 卡成为同一蜂群中可互换的成员。",
      },
    ],
  },
  "hub-operations": {
    title: "运营 hub",
    summary: "运营 hub 与网关的运营者笔记：不重建即热补丁、挺过重启、保持目录注册、并把暴露面收拢到 443。",
    blocks: [
      {
        t: "p",
        md: "hub（控制平面）和网关（公共入口）是运营者要保持健康的两个长期运行服务。hub 及其 RPC 端口**在设计上不做认证**——仅限可信主机 / LAN / VPN——所有公共流量都汇聚到网关唯一的 443 暴露面。以下运营笔记让这套安排在代码变更、服务重启和系统重启之间保持稳定。",
      },
      { t: "h2", kick: "部署与热补丁", text: "不重建即改代码" },
      {
        t: "ul",
        items: [
          "**快捷路径：**用 `docker cp <file> <container>:/app/...` + `docker restart` 更新 hub/网关代码——无需重建镜像。但**新增环境变量无法这样做**（它需要重建容器）；应改用持久化到 hub-state 的运行时配置 API。",
          "**Compose 漂移：**长期运行的容器可能偏离其 compose 文件（网络模式、entrypoint、env）。在 `docker compose up -d` 重建之前，务必先 `docker inspect` 真实配置——如果已漂移，重建会抹掉生产设置。改用 cp + restart。",
          "**打补丁前先 diff：**替换之前，先把容器内文件 `docker cp` 出来并与仓库 HEAD 比对，以免前一次会话的热补丁被静默丢失。",
        ],
      },
      { t: "h2", kick: "挺过重启", text: "状态会保留；已加载的模型不会" },
      {
        t: "ul",
        items: [
          "hub 重启会**停止服务。**槽位、控制器和绑定从 `hub-state.json` 恢复，但已加载的模型只存在于运行时。重启后，读取每个控制器的 `last_load` 并重新触发 `POST /api/controllers/{cid}/serve`——得益于页缓存，即使大模型也能在约 1 分钟内回来。",
          "**网关看门狗：**每 30 s 用 1-token 请求探测每个已服务模型，失败时从 `last_load` 自动重载（带冷却）。**探测*所有*模型，而不是 `catalog[0]`**——一旦来自另一个 hub 的健康模型排到最前，只探首个的做法会漏掉正在宕掉的大模型（一个真实的 bug，现已修复）。",
          "**目录 TTL：**`POST /api/pay/hub/register` 有 90 s TTL，所以用约 60 s 的心跳循环保持注册存活，并用 `@reboot` cron 或 systemd unit 让它在系统重启后依然可靠。",
        ],
      },
      { t: "h2", kick: "锁紧", text: "所有公共流量都走 443" },
      {
        t: "ul",
        items: [
          "hub（:19000）和 RPC 端口假定处于可信网络；唯一应当面向互联网的只有 443 上的网关（包括其 WebSocket 中继透传）。",
          "如果 hub 必须处于公网 IP 上，用防火墙将其限制到可信 IP——但 Docker 发布的端口在 **INPUT 链之前就已被 DNAT**，因此针对 `dport` 的规则不会命中。改在 `DOCKER-USER` 链中用 conntrack 的原始目的端口（`--ctorigdstport`）过滤，并用排在 `After=docker.service` 之后的 systemd oneshot 持久化这些规则。",
        ],
      },
      { t: "h2", kick: "结算与自伤陷阱", text: "拉取而非推送——以及一个 shell 陷阱" },
      {
        t: "ul",
        items: [
          "**结算是拉取而非推送：**hub 累积贡献；网关轮询 `GET /api/contributions` 并增量记入 KVR。如果 hub 重启重置了计数器，网关会重设基线，因此不会重复支付。专家工作的费率由 `LINKCPP_EXPERT_UNITS_PER_MB` 设定。",
          "**`pkill` 自伤陷阱：**`ssh host 'pkill -f X; ...'` 会匹配到它*自己*的命令行并杀掉自己。在模式中用字符类（`X[x]`），且绝不要把启动进程和 pkill 放进同一条远程命令里。",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Hub 广域互联（200G 光模块）",
    summary: "hub 如何以 200 Gb/s 跨越一个房间、一个园区或一座城市互联：什么距离用什么光模块、什么插到哪里、以及真正跑满线速需要什么。",
    blocks: [
      {
        t: "p",
        md: "当两个 hub 都有公网路由时，专家调度的数据平面应当是**直连**——443 中继是给 NAT 后的边缘用的。本条目是用目录现货零件把那条直连做到 200 Gb/s 级别的具体配方。一条规则统领一切：**光纤是与速率无关的玻璃；速率活在两端的可插拔模块里。**",
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
          "**NIC 侧** — ConnectX-6/7 级别的网卡暴露 QSFP56 笼位；DAC/AOC/FR4/LR4/ER4 全都可直接插入 NIC。GB10 级别的 hub 板载已有两个 200 GbE QSFP 端口，因此两个 hub 的链路恰好只需一根线缆、零新硬件。",
          "**交换机侧** — 相干 ZR+ 光模块是 QSFP-DD 形态，应插入交换机或路由器；hub 的 NIC 再经一条短 DAC 以 200G 接入那台交换机。当对端 hub 在数十公里之外时用这一档。",
          "**光纤本身** — 标准单模（G.652）双工 LC 对，按每芯以暗光纤租用。同一根玻璃今天承载 100G、日后承载 400G；升级只是换模块，绝非土建工程。",
          "**超过约 120 km** — 你不再购买零件，而是开始向运营商租用一个波长；分界点是你交换机上的一次以太网交接。",
        ],
      },
      {
        t: "code",
        caption: "三种参考搭建，从最便宜开始。",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "第 3 步 · 真正跑满 200G", text: "线速是配置出来的，不是买来的" },
      {
        t: "ul",
        items: [
          "在可用之处，为调度流使用 **RDMA (RoCE)**——GB10 级别的主机通过拆分的 PCIe 链路给 NIC 供数据，在拓扑正确映射的情况下 RoCE 下能测得满速（约 185–190 Gb/s）；映射错误的路径会卡在约一半速率，而未调优的普通 TCP 则低得多。",
          "端到端启用**巨型帧（MTU 9000）**，并在调度套接字上保持 `TCP_NODELAY`（hub 已经设置了它）。",
          "要*验证*，别假设：每次物理改动后都在两个 hub 之间跑一次 perftest——95 与 190 Gb/s 之间的差距在测量之前是看不见的。",
          "把 **443 中继保留为回退路径**——拨号策略对公网对端是直连优先、对 NAT 走中继。中继的职责是可达性，直连的职责是速度。",
        ],
      },
      {
        t: "p",
        md: "这对架构为何重要：解码延迟受往返时间约束（光纤中约 5 µs/km——是物理规律，不受带宽影响），所以粗管道买到的是**prefill 速度、批量调度吞吐，以及近乎瞬时的专家切片分发**，而不是更低的每 token 延迟。这正是双层设计中 hub 层的角色：在粗管道层提供容量，在中继层提供可达性。",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "负载自适应扩展",
    summary: "Kvasir 的 MoE 服务路径随流量伸缩：饱和时协调器重新启用已验证的 worker，hub 通过抬高专家需求招募闲置节点——全部拉取式，因此 NAT 后的设备也能加入。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 的 MoE 服务路径随负载弹性扩展，分为两个协作的层。清闲时协调器在本地服务一切，取得每 token 最快的路径；饱和时，下述两层把蜂群扩大——浪涌过去后再度收缩。",
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
      { t: "h2", kick: "第 2 层", text: "hub 侧：负载自适应招募" },
      {
        t: "p",
        md: "控制 hub 盯着每个 MoE 协调器，并在需要时扩大 worker 池：",
      },
      {
        t: "ul",
        items: [
          "一个后台循环轮询每个协调器的槽位，并按模型记录饱和度。",
          "当一个模型处于饱和时，其**有效专家副本目标**被抬高（base + boost）。覆盖市场随即把已覆盖的专家重新读作稀缺，而**没有**任何在线 worker 的模型则从其 GGUF 元数据（专家数）播种，使需求即便从零也可见。",
          "闲置节点轮询需求市场（`/api/expert-volunteer`），被交予一份 `(layer, expert-range)` 切片去服务。它们下载切片、拨通 relay、注册覆盖；hub 自动把它们接线到协调器的调度图。",
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
