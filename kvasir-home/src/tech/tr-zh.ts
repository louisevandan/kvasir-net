/* 中文 — 技术博客翻译。结构（slug、分类、区块顺序、代码、img 位置）严格镜像
   articles.ts（英文源）；技术术语、标识符与数值保持原文。 */
import type { TechTranslation } from "./articles";

export const zhTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "专家分片蜂群推理：设计",
    dek: "一个 122B MoE 的 86% 是 12,544 个相互独立的 5.3 MB 专家。按这个颗粒切分模型，手机也能承担前沿推理的真实份额。",
    blocks: [
      {
        t: "callout",
        md: "**论点：**Qwen3.5-122B 权重的 86% 是 12,544 个相互独立的 5.3 MB 专家。按专家颗粒分片，弱设备背负的就不是\"一个 1.4 GB 的层\"，而是\"8–64 个专家（42–340 MB）\"——恰好是手机真正能承受的单位。MoE 是蜂群的天然基质。",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "基质 · Qwen3.5-122B-A10B (Q4_K_M)", text: "权重出厂时就已打包成蜂群大小的单位" },
      {
        t: "stats",
        items: [
          { n: "49", l: "层" },
          { n: "256", l: "专家 / 层" },
          { n: "8", l: "每 token 激活" },
          { n: "5.3 MB", l: "单个专家 (Q4)" },
          { n: "12,544", l: "专家总数" },
          { n: "86%", l: "权重中专家占比" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "专家维 = 最外层" },
        ],
      },
      {
        t: "p",
        md: "专家索引是每个 MoE 张量的**最外层维度**，因此每个专家都是一块连续、量化块对齐的板块。按专家切片的 mini-GGUF 就是一次干净的字节范围拷贝——无需反量化，无需重新打包。",
      },
      { t: "h2", kick: "两个角色", text: "骨干 stage × 专家 worker" },
      {
        t: "ul",
        items: [
          "**骨干 stage（强节点）：**注意力 + KV 缓存、所有 norm、**路由器**、共享专家、residual combine——整条稠密路径。它还把所有专家作为后备副本常驻（RAM 卸载），赋予蜂群抗流失能力。",
          "**专家 worker（手机）：**不是 transformer。没有注意力、没有 KV、没有采样器——只是纯函数 `(hidden, local_ids) → out`，由三次矩阵乘构成，只常驻自己的专家切片。任何预算都装得下，低至 4 GB 手机。",
        ],
      },
      {
        t: "code",
        caption: "切分点：路由器只在骨干上带权威地运行一次。",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "由于路由器在骨干上**恰好运行一次**，每个被选中的专家由持有它的节点恰好计算一次。**没有任何近似**——分片只是移动了矩阵乘发生的位置。",
      },
      { t: "h2", kick: "不是新的子系统", text: "蜂群 = 已验证的奖励市场，只是颗粒更细" },
      {
        t: "p",
        md: "Kvasir 已经为**层**分片运行着一套自治稀缺性市场，并在真机上验证过：NAT 后的手机轮询需求图，自助报名**最高奖励**的区段，只部分下载那个窗口，加载到 Adreno GPU，跑完环推理——并赚到贡献奖励。专家分片全盘复用这一切——覆盖图、最高奖励自助报名、部分下载、按节点奖励——只把覆盖单位从*层区间*换成*(层, 专家范围)*。",
      },
      { t: "h2", kick: "两项已在真机验证的创新", text: "部分权重参与 + 443 中继" },
      {
        t: "ul",
        items: [
          "**奖励驱动的部分权重下载：**传统 RPC/TP/PP 把完整检查点发给每个 rank，由调度器指定放置。在 Kvasir 中，节点只下载**自己要计算的切片**，且**自己按奖励**挑选切片——254 MB 的 stage mini-GGUF 对比 77.6 GB 的完整模型。这就是 4 GB 手机加入远大于自身的模型的机制。",
          "**443 中继数据平面：**Cloudflare 只放行 80/443 的边缘加上运营商 NAT，意味着两个方向都无法直连。带 1 字节 role preamble 的每边缘 WebSocket 桥让**双方都向外拨号**（手机开放的入站端口为零）。落地过程中修了三个真实 bug——构建指纹一致性、node-token 下载鉴权，以及静默损坏所有 ≥ 64 KiB 帧的 Kotlin `Int.ushr` 帧长 bug（`ushr` 只取移位量低 5 位，`len ushr 56` 变成 `len ushr 24`）——改用 `Long` 移位修复。",
        ],
      },
      { t: "h2", kick: "诚实的关键点", text: "是吞吐织物，不是低延迟解码器" },
      {
        t: "p",
        md: "解码是 49 层串行，每层一次跨互联网往返意味着每 token 2.5–10 秒。因此蜂群的赛场是**共同服务没人能独自托管的模型**，以总吞吐衡量：批量调度摊销 RTT，骨干维护 hot-expert 缓存，请求路由到就近副本。低延迟路径仍由流水线环负责。",
      },
      { t: "h2", kick: "路线图", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — 骨干专家 RAM 卸载：单台协调器运行 122B，无需图手术。",
          "**M1** — 单主机专家并行证明：专家切片 mini-GGUF + worker 运行时 + 调度，logits 与整体完全一致。",
          "**M2** — LAN + NAT 手机 worker 通过 443 中继计算真实 122B 专家。",
          "**M3** — 带副本与流失回退的专家颗粒覆盖市场。",
          "**M4** — 吞吐：批量调度 + hot-expert 缓存，tokens/s 随 worker 数扩展。",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "从蓝图到硬件：已验证的部分与拱顶石",
    dek: "验证战役的回顾——从设计到 M2 核心均在真实 122B 上得到证明——以及解锁其余一切的那一块集成拼图。",
    blocks: [
      {
        t: "p",
        md: "过去几周里，专家分片蜂群那些困难而新颖的部件在真实的 **Qwen3.5-122B** 上被逐一证明——不是模拟，不是玩具尺寸。这里是迄今的验证轨迹，以及最后剩下的那块拱顶石。",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "轨迹 · 全部在真实 122B 上验证", text: "迄今落地的成果" },
      {
        t: "ul",
        items: [
          "**设计（蓝图后 5 次修订）** — EP 架构、部分权重自治参与、中继、worker 内核规范，以及被确立为核心技术的跨后端数值等价性。路由器权威被写为一致性不变式。",
          "**M0 — 骨干专家 RAM 卸载（规划器验证）：**122B 在单台 64 GB 协调器上 *feasible* —— 10 个专家层卸载到 RAM，VRAM 62.6 GiB / RAM 14.2 GiB，经 `--override-tensor` 接线。",
          "**M1 — 专家切片数据路径：**按专家的 mini-GGUF 切片（ne[2] 板块，无反量化字节拷贝）+ `/expert-shard` 下载端点。",
          "**M1 — 数值 oracle：**在真实 layer-0 专家上 dispatch + combine == monolithic，**max|Δ| = 3.6e-12** —— 分片是同一加权和的精确重组。",
          "**M1 — C++ worker 硬件验证：**`linkcpp-expert-worker`（纯 ggml/gguf）在 ROCm 上构建并运行，对 oracle **cosine 0.99995**；router → 两个 C++ worker → combine 与 monolithic 一致，cosine 0.9997–0.9999。",
          "**M2 核心 — 手机计算真实 122B 专家：**Android 交叉构建，在 SM-S938N 上运行，对 oracle **cosine 0.99992**。",
          "**数值 — 三后端等价矩阵：**同一 122B 计算在 ROCm × 手机 ARM CPU × numpy 上 —— ROCm↔手机 cosine 0.99990，ROCm↔numpy 0.99996，手机↔numpy 0.99992。全部等价，无一逐位相同。",
        ],
      },
      { t: "h2", kick: "拱顶石", text: "集成进实时解码的骨干调度" },
      {
        t: "callout",
        md: "每个**组件**——切片、worker、dispatch/combine 逻辑、数值等价、手机计算——都已真机验证。剩下的是把它们**接进真实的 推理引擎 解码**：一个在图中途把专家调度到持有节点的 `build_moe_ffn` 钩子。它需要修改被钉住的 推理引擎 子模块和多轮构建-验证循环。这块拱顶石一旦立起，**M2 中继集成、M3 专家覆盖市场、M4 批量吞吐**将依次打开——它们全都依赖这个调度。",
      },
      {
        t: "p",
        md: "拱顶石随后已经落成：关于 M2、M3、M4 和实时手机演示的后续文章，正是这次集成的成果。",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "异构后端间的数值等价性",
    dek: "CUDA、ROCm、Adreno 和 CPU 永远不会逐位一致。蜂群仍能产出一个连贯的模型，这是设计出来的性质，不是运气。",
    blocks: [
      { t: "h2", kick: "关键区分", text: "精确（exact）vs 等价（equivalent）——两种不同的性质" },
      {
        t: "ul",
        items: [
          "**同一后端内 — 精确（3.6e-12）：**把专家拆到各节点再合并，是同一加权和的重组；差异只来自浮点累加顺序。已由 oracle 验证。",
          "**跨后端 — 等价（1e-3…1e-6）：**同一算子在不同硬件上带有约 1e-3–1e-6 的每算子相对误差，且永不为零。**蜂群就生活在这个区间里。**",
        ],
      },
      {
        t: "p",
        md: "\"精确\"是分解在单台设备内保证的；\"等价\"是异构硬件给你的。蜂群的任务是不让等价性累积成发散。",
      },
      { t: "h2", kick: "实测 · 真实 122B，三个后端", text: "不是理论——在硬件上测得" },
      {
        t: "p",
        md: "同一个 Qwen3.5-122B layer-0 专家 FFN，由 `linkcpp-expert-worker` 在 MI250（**ROCm**）、手机 **ARM CPU**（SM-S938N）和 x86 **numpy** 参考上计算——相同输入、相同权重，不同指令集与归约顺序：",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["后端对", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["手机 ARM CPU vs numpy (x86)", "1.4e-6", "0.99992"],
          ["ROCm GPU vs 手机 ARM CPU", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "三套指令集，一个计算——每一对都等价（cosine ≈ 0.9999），没有一对逐位相同（Δ ≈ 1e-6）。残差之所以小，**是因为路由器权威钉住了输入与专家选择**。",
      },
      {
        t: "p",
        md: "此后在真实 **NVIDIA GB10 Grace Blackwell** 硬件上的一次运行补全了最后一个后端：CUDA ↔ ROCm 落在 **cosine 1.0000000000**（max abs 3.5e-10，因两个 GPU 后端共享内核源码而实质逐位相同），CUDA ↔ Grace ARM CPU 为 cosine 0.99975——与上文所见的 GPU↔CPU 同一模式。",
      },
      { t: "h2", kick: "后端为何不同", text: "浮点加法不满足结合律" },
      {
        t: "ul",
        items: [
          "**matmul 归约顺序** — tensor core、MFMA tile、OpenCL workgroup 和 SIMD lane 以不同顺序与切块累加。",
          "**FMA 融合** — `a*b+c` 舍入一次（FMA）或两次，各后端融合方式不同。",
          "**累加精度** — F16/BF16 存储配 F32 或 F16 累加器（对发散幅度影响最大的杠杆）。",
          "**超越函数近似** — exp（softmax）、silu/sigmoid（swiglu）、rsqrt（norm）的多项式/查表变体不同。",
          "**反量化 + matmul 路径** — 先反量化再乘 vs 融合量化内核，中间舍入不同。",
          "**非确定内核** — atomic/split-K 归约在同一设备上也可能逐次运行不同。",
        ],
      },
      { t: "p", md: "这些都不是 bug，而是每个加速器的快速路径所付的代价。" },
      { t: "h2", kick: "为何仍然可行", text: "决策归于单一权威，累加保有足够精度" },
      {
        t: "callout",
        md: "**路由器权威——核心不变式。**网络内唯一的离散决策是 MoE 路由（256 选 top-8）。若每个后端都重跑路由器，边界 token 会选出**不同的专家**，产生真正的发散。Kvasir 让路由器**在骨干上只跑一次**，只把选中的专家 id 发给 worker。异构蜂群或许在每个专家输出的*幅值*上有差异——但*哪些专家运行*绝不分歧。这把灾难性的离散发散转化为有界的连续误差，是异构专家分片的一致性规则。",
      },
      {
        t: "ul",
        items: [
          "**离散 argmax：**解码是对 logits 取 argmax。1e-3 的抖动只有在两个候选相差不到 1e-3 时才会翻转 token——绝大多数位置的差距远大于此，因此 **token 完全一致**；罕见的翻转发生在与换一个随机种子无法区分的模糊位置。",
          "**combine 是加法：**部分结果以概率加权**求和**合并。独立的约 1e-4 误差非相干地相加——按 √k 而非 k 增长——且没有大数相消，残差保持良态。",
        ],
      },
      { t: "h2", kick: "可能失效之处 · 及阻止它的规则", text: "发散模式与防御" },
      {
        t: "table",
        head: ["发散模式", "机制", "规则"],
        rows: [
          ["路由不一致", "各后端对边界 token 选出不同 top-8", "路由器权威——骨干一次决定，调度 id"],
          ["轨迹分叉", "逐 token 的 logit 抖动终会翻转某个 token，序列像换了种子一样分叉", "解码/采样钉在一个节点"],
          ["深度累积", "49 层 × 各约 1e-4 → 最终 logits 可达 1e-2", "边界与 combine 用 F32 累加"],
          ["自身非确定", "atomic/split-K 逐次运行不同", "combine 用确定性内核；验证用容差"],
          ["精度不匹配", "一个节点 F16 累加，另一个 F32", "累加精度作为能力公示；输出 rank 优先 F32 节点"],
        ],
      },
      { t: "h2", kick: "等价性是一个数字", text: "测量协议" },
      {
        t: "ul",
        items: [
          "**每算子 delta** — 相同输入下 A vs B 在 matmul、swiglu、softmax、norm 上的相对误差。",
          "**层边界漂移** — 过一层后的 residual delta，逐层堆叠看深度累积是 √L 还是 L。",
          "**端到端 logit 发散** — 全前向的 L∞、L2 与 **KL 散度**。",
          "**决策一致率** — top-1 token 一致率加 top-8 路由一致率（验证路由器权威的必要性）。",
          "**生成稳定性** — greedy N 个 token，A 与 B **首次分歧的位置**。",
          "**任务层面** — perplexity 与评测分数的 delta：用户唯一能感知的指标。",
        ],
      },
      {
        t: "p",
        md: "合格标准是**容差**——\"top-1 一致率 ≥ 99.x%，KL ≤ ε\"。超出容差的节点只被标记为不适合敏感 rank，不会被整体拒绝。",
      },
      { t: "h2", kick: "为何这是核心蜂群技术", text: "逐位一致既不可能，也无必要" },
      {
        t: "p",
        md: "同构集群可以假设逐位精确；蜂群不行——它的前提是*来什么硬件用什么硬件*。所以 Kvasir 把数值等价当作与协议兼容性完全同级的**一等、可测量的契约**：后端与累加精度作为节点能力公示，路由器权威作为不变式强制，所有验证用容差而非逐位相等。**测得的数值等价 + 单一权威的离散决策**——这就是让一个模型同时跑在地球上每块 GPU 上的东西，也就是蜂群。",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell 加入了蜂群",
    dek: "一台 GB10 Grace Blackwell 用 CUDA 计算真实的 122B 专家 FFN 切片，与 AMD ROCm 逐位一致（cosine 1.0000000000），并在容差内与 Grace ARM CPU 等价。跨后端矩阵已补全。",
    blocks: [
      {
        t: "p",
        md: "蜂群的前提是*来什么硬件用什么硬件*。数值等价性——CUDA、ROCm、Adreno 与 CPU worker 都产出相同 token 的证明——此前已在 ROCm、手机 ARM 与 numpy 上测得。NVIDIA 是原版 ggml/推理引擎 的**默认且优化最好**的路径，却是矩阵尚未补全的唯一后端。在真实 Blackwell 硬件上运行将其补全。",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a：cosine 1.0000000000** —— max abs diff 3.5×10⁻¹⁰。在同一真实 Qwen3.5-122B layer-0 专家切片上，两个 GPU 后端实质逐位相同。",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "实测 · 真实 Qwen3.5-122B-A10B，layer-0 专家切片", text: "跨后端矩阵" },
      {
        t: "table",
        head: ["比较", "硬件", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "两个 GPU 后端（CUDA、ROCm）共享内核源码，因此落在**实质逐位相同**（10⁻¹⁰）。GPU↔CPU 因累加顺序不同带有每算子约 10⁻³ 的扰动，但仍以 **cosine 0.99975** 等价——与此前 ROCm↔手机 ARM 的 0.99992 同一模式。路由器权威原理在 NVIDIA 上再次成立：**离散决策（argmax、专家选择）在这一连续扰动之上不变。**",
      },
      { t: "h2", kick: "设置", text: "在什么上跑了什么" },
      {
        t: "ul",
        items: [
          "**设备** — NVIDIA GB10（Grace Blackwell），aarch64，compute 12.1 / sm_121a，124.5 GB 统一内存。",
          "**工具链** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3；纯 ggml/gguf 的 expert-worker 用 Blackwell 内核构建。",
          "**模型** — Qwen3.5-122B-A10B-Q4_K_M，layer-0 全部专家（256 专家，n_embd 3072，n_ff 1024，Q4_K/Q6_K）。",
          "**方法** — 1.58 GB 的 L0 切片从 MI250 → GB10 流式传输（无损对比）；同一输入（h/ids）在 CUDA、CPU、ROCm 三个后端运行；float32 输出向量（36,864）以 cosine、相对 L2 与 max-abs 对比。",
        ],
      },
      {
        t: "callout",
        md: "**一个真机小坑：**GB10 的集成 GPU 在 ggml 中被归类为设备类型 `ACCEL` 而非 `GPU`，因此 `init_by_type(GPU)` 什么也找不到。改为选择第一个非 CPU 设备，而不是硬编码 GPU 类型。",
      },
      { t: "h2", kick: "为何重要", text: "矩阵补全了" },
      {
        t: "p",
        md: "要让异构 worker 服务同一个模型，一台 CUDA 机器和一台 ROCm 机器必须**可互换**，一个 GPU 和一个 CPU 必须**数值等价**。随着 Blackwell 被测得，两者在整个后端矩阵上都成立：CUDA↔ROCm worker 可彼此替代，GPU↔CPU worker 在有界、良态的容差内一致。地球上最常见的加速器如今是经过验证的蜂群成员。",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "加固资金通路：KVR 结算中的交易安全",
    dek: "三类真实漏洞——支付签名重放、无鉴权奖励铸造、竞态双花——在网关结算服务中被发现、用测试利用并封堵。",
    blocks: [
      {
        t: "p",
        md: "在 DePIN 中，资金通路与算力通路同样充满对抗：每一个记账 KVR 的端点，终将被想不劳而获的人试探。对网关结算服务——验证链上支付并记账质押、节点奖励与推理费用的进程——的一次安全审查发现并封堵了**三类真实漏洞**。每一类都在修复前用攻击式测试演示，修复后重新验证。",
      },
      { t: "h2", kick: "信任模型", text: "验证链上事实，而非客户端声称" },
      {
        t: "p",
        md: "Kvasir 的托管模型把密钥留在用户手里：钱包签署交易，Solana 记录交易，结算服务唯一的职责是在触碰任何余额前**验证链上真正发生了什么**。支付遵循 *quote → payment → inference*，每个被消费的交易签名都记入一次性 `usedSignatures` 注册表，永远无法二次出示。这使结算服务成为咽喉要道——它绝不能违背的规则只有一条：只记账链证明的东西，绝不记账客户端声称的东西。",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "修复 #1 · 发送者绑定", text: "把支付绑定到付款人" },
      {
        t: "p",
        md: "Solana 签名是**公开的**。质押验证路径只检查 vault 是否*收到*了预期的 KVR——从不检查*是谁发的*。攻击者可以在 devnet 上盯着受害者的 KVR→vault 转账，然后提交 `{owner: 攻击者, signature: 受害者的}`：vault 收款检查通过，本金记到攻击者名下，一次 unstake 后资金就归攻击者了。仅凭一个区块浏览器即可完成的直接盗窃。",
      },
      {
        t: "code",
        caption: "修复：KVR 必须是从被记账 owner 拥有的代币账户中扣出的。",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "修复 #2 · 可信上报者", text: "奖励只来自经过认证的来源" },
      {
        t: "p",
        md: "节点奖励端点曾用**自报输入**铸造可领取的 KVR：`POST /api/node/contribution` 照单全收客户端声称的 `units`——`units: 1e9` 加一次 claim 就能掏空 vault——register/heartbeat 也照信自称的 hub/网关角色（按小时的基础设施奖励）和性能分（奖励乘数）。修复把每一项影响奖励的断言都关进**可信上报者**之后：只有 hub 贡献轮询所用的 M2M 服务令牌，或经认证的管理员，才能断言 units、基础设施角色或性能等级——即使在开放 LAN 模式下也强制执行，因为这些会铸造 KVR。令牌比较是常数时间的；钱包↔节点关联仍然自由，只是不能再自报奖励。",
      },
      { t: "h2", kick: "修复 #3 · 结算串行化", text: "每个余额只有一个写入者" },
      {
        t: "p",
        md: "结算状态是无锁的读-改-写，而每个资金操作中途都会 *await* 一次链上支付或验证——握着过期余额把事件循环让了出去。两个并发 claim 可以读到同一笔 100 KVR 待领余额并都付出去。这不是理论：攻击测试显示**三个并发 claim 对 100 的余额付出了 300**。",
      },
      {
        t: "code",
        caption: "按键异步串行化：同键资金操作严格先后执行。",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "纵深防御", text: "各层现状" },
      {
        t: "table",
        head: ["层", "机制"],
        rows: [
          ["身份", "对服务器 nonce 的 SIWS 钱包签名登录 + TOTP 2FA + 一次性备份码"],
          ["传输", "分片下载用钱包派生 node token；中继上的构建指纹一致性"],
          ["支付", "质押转账的发送者绑定；一次性 usedSignatures；推理的私有 requestId"],
          ["结算", "每次余额写入都有按键锁；先付后扣；幂等重提交"],
          ["上报", "影响奖励的事实仅限 M2M 服务令牌或管理员，常数时间比较"],
          ["托管", "非托管钱包——服务只能动 vault 里的钱，永远碰不到用户密钥"],
        ],
      },
      { t: "h2", kick: "测量而非假设", text: "每个修复都自带攻击测试" },
      {
        t: "ul",
        items: [
          "用攻击者钱包重放受害者转账签名现在会被拒绝（\"not sent by owner\"）；正常质押、超额领取与推理支付行为不变。",
          "对同一余额的三个并发 claim **恰好支付一次**；对已支付推理的重提交幂等地返回相同结果。",
          "无鉴权客户端自报的 `units`、hub/网关角色和性能等级，再也动不了奖励的一个 lamport。",
        ],
      },
      {
        t: "p",
        md: "三个修复的贯穿线是同一条原则的三种应用：**链是事实之源，服务是验证者，每个余额恰有一个写入者**。结算服务仍运行在 Solana devnet 上——这恰恰是在主网抬高赌注之前发现、利用并修复这些漏洞类别的最佳场所。",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — 引擎：推理引擎 的控制平面 linkcpp",
    dek: "推理引擎 自带能干的 RPC 数据平面，却没有控制平面。linkcpp 围绕原版二进制补上缺失的那一半——发现、规划、启动与网关。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 运行的一切都从这里开始。**linkcpp** 是围绕 推理引擎 RPC 数据平面的源码可得控制平面（Business Source License）：它用*原版* `ggml-rpc-server` / `llama-server` 二进制在多张 GPU 和多台机器上运行大型 AI 模型。数据平面不分叉——linkcpp 增加的一切都是编排。",
      },
      { t: "h2", kick: "缺口", text: "没有控制平面的数据平面" },
      {
        t: "p",
        md: "推理引擎 已经能通过 RPC 把模型拆到多台机器——但总得有人发现 GPU、决定哪些层放哪里、以正确预算启动正确的 worker、检查每个节点说同一种协议，并暴露开发者真正能调用的 API。为一个集群手工做这些是苦差；为一个由陌生人设备组成的开放网络做这些则不可能。那个协调层就是 linkcpp。",
      },
      { t: "h2", kick: "架构", text: "一个 hub、原版 worker、标准网关" },
      {
        t: "code",
        caption: "请求流——hub 负责编排，原版二进制负责计算。",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock 推理引擎 engines below" },
      {
        t: "ul",
        items: [
          "**机器加入的三种方式：**可编辑 VRAM/RAM/CPU 预算的固定**本地节点槽**；注册另一个 hub 并导入其节点的**远程单元**；以及**受管节点代理**——通过普通请求/响应 HTTP 加入的纯 worker 服务，刻意不用持久流，因而能在简单的 LAN/VPN 路由下存活。",
          "**兼容性闸门是一等概念：**每个单元、节点和代理都上报协议/运行时包身份及后端细节。单元、运行时包、推理引擎 修订版与 RPC ABI 不匹配会在 **bind/plan/load/infer 之前硬性阻断**——后端差异（CUDA/Metal/Vulkan/CPU）作为能力记录，而非拒绝。",
          "**规划器**读取 GGUF 元数据，产出按节点的连续层放置、`--tensor-split`、KV 缓存/层/专家 VRAM 估算，以及可选的专家 FFN RAM 卸载。",
          "**网关：**每个控制器都暴露 OpenAI 兼容（`/v1/chat/completions`、`/v1/responses`、`/v1/models`）与 Anthropic 兼容（`/anthropic/v1/messages|models`）端点，由同一个已加载模型支撑——现有客户端无需改动即可使用。",
        ],
      },
      {
        t: "p",
        md: "这种刻意的分离——开放控制平面之下的未改动数据平面——是后来一切的地基：环运行时、层市场，直至专家分片蜂群，全都是同一原版算力之上的控制平面演化。",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — 环：无 master 的流水线推理",
    dek: "每台设备只加载自己的层窗口，把小小的 hidden-state 边界传给邻居。没有节点持有模型，也不存在中心 master。",
    blocks: [
      { t: "h2", kick: "为什么不是星形", text: "RPC master 既是瓶颈也是守门人" },
      {
        t: "p",
        md: "在经典 RPC 拓扑中，一个 master 打开**整个 GGUF** 并拨号连接每个 worker。这个形态在开放网络中三处失效：master 必须持有并服务完整检查点；每个 worker 必须可被拨号——运营商 NAT 后的手机不行；master 还是这个本不该有所有者的网络中的单一所有者。",
      },
      { t: "h2", kick: "环", text: "层窗口 + 边界传递" },
      {
        t: "ul",
        items: [
          "每台设备存有同一模型，但**只加载自己连续的层窗口**，然后恰好打开两条链路：一条通向前驱，一条通向后继。",
          "请求进入环；每个节点运行自己的层，只把 **hidden-state 边界**传给邻居。最后一个 rank 采样 token 并送回——没有中心 master，没有节点持有完整模型。",
          "放置来自规划器的 **rank manifest**——对 Qwen3.5-122B，49 层被分到任意到场的 GPU、CPU、NPU 与手机组合上。",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "让弱设备成为真正的成员", text: "部分分片、移动 GPU 与 443 中继" },
      {
        t: "ul",
        items: [
          "**部分分片下载：**环 stage 需要的不是检查点而是自己的窗口。stage mini-GGUF 只携带那些张量（**26 个张量共 254 MB**，对比 77.6 GB 完整模型），手机为一个单层窗口只需拉取约 1.5 GB。",
          "**移动 GPU 路径：**通往手机 GPU 的 RPC 路线被证明不可行（Adreno 的 OpenCL 缓冲布局无法经受 RPC 序列化），但**环 stage 直接跑在 Adreno GPU 上**——stage 在本地拥有自己的后端，过线的只有边界。",
          "**NAT 穿越：**手机无法接受入站连接，因此数据平面经过 **443 中继**——带 1 字节 role preamble 的每边缘 WebSocket 桥，让两端都向外拨号。手机开放的入站端口为零。",
          "**自助报名市场：**stage 是被认领的，不是被指派的。节点轮询覆盖/需求图，挑选**最高奖励**的未覆盖窗口，下载那个窗口，然后加入——已用 NAT 后的手机端到端验证：完成环推理并赚到贡献。",
        ],
      },
      { t: "h2", kick: "环的位置", text: "低延迟路径" },
      {
        t: "p",
        md: "环是 Kvasir 的**延迟**路径：边界小、跳数少，解码沿环流动而不向中心汇聚任何东西。它的局限在颗粒度——节点能背负的最小单位是一层（122B 上约 1.4 GB）。移除这个下限正是专家分片蜂群要做的事；环仍是蜂群接入的服务骨干。",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — 122B MoE 解剖：权重为何想被分片",
    dek: "Qwen3.5-122B 的张量级分析：86% 的字节是 12,544 个独立专家板块，每一块离独立存在只差一次干净的字节范围拷贝。",
    blocks: [
      {
        t: "p",
        md: "在设计任何东西之前，我们先把 122B 在磁盘上拆开。问题是：如果要让一群弱设备背起这个模型，天然的承载单位是什么？答案从 GGUF 张量布局本身掉了出来。",
      },
      { t: "h2", kick: "解剖 · Qwen3.5-122B-A10B (Q4_K_M)", text: "MoE 的一层究竟由什么构成" },
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
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "每层分为**稠密路径**——注意力 + KV、各类 norm、路由器（`ffn_gate_inp`）、共享专家——和以三个堆叠张量（`ffn_up_exps`、`ffn_gate_exps`、`ffn_down_exps`）存储的**专家库**：256 个独立 FFN。稠密路径只占字节的少数；专家库占模型的 86%。",
      },
      { t: "h2", kick: "布局的馈赠", text: "专家是连续、块对齐的板块" },
      {
        t: "ul",
        items: [
          "专家索引是每个专家张量的**最外层 ggml 维度**（`ne[2]`）——专家 *e* 占据一整块连续、量化块对齐的原始量化字节板块。",
          "这让按专家提取成为一次**字节范围拷贝**：`data[a:b]`，无反量化、无重打包——专家切片 mini-GGUF 制作成本低且逐位忠实。",
          "每 token 每层只有路由器选中的 **256 个中的 8 个**专家点亮——解码时一层的专家流量只是对一个 hidden 向量的几次小矩阵乘。",
        ],
      },
      { t: "h2", kick: "意义", text: "承载单位从 1.4 GB 降到 5.3 MB" },
      {
        t: "p",
        md: "在层颗粒下，节点能背的最小值约 **1.4 GB**——一旦应用、KV 和操作系统各占其份，大多数手机就够不着了。在专家颗粒下，单位是 **5.3 MB**，现实的贡献是 8–64 个专家（**42–340 MB**）——任何现代设备都绰绰有余。专家彼此独立，所有权可以任意散布、自由再平衡。正是这项分析让专家级分片成为设计上的押注：权重出厂时就已打包成蜂群大小的单位——网络只需尊重这份包装。",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — 骨干专家 RAM 卸载 (M0)",
    dek: "把 MoE 专家 FFN 从 VRAM 改为从 CPU RAM 流式读取，一台 64 GB 协调器就装下 122B——无需图手术。",
    blocks: [
      {
        t: "p",
        md: "专家 FFN 不必住在 VRAM 里。从 CPU RAM 流式读取它们，让一台协调器装下专家超出其 VRAM 的模型——这是让弱节点得以加入大型 MoE 的地基。",
      },
      { t: "h2", kick: "规划器验证 · 真实 122B GGUF", text: "122B 装进单台 64 GB 协调器" },
      {
        t: "p",
        md: "此前环只按 VRAM 放置权重，122B（77.6 GB）在 64 GB GCD 上 **infeasible**。启用专家卸载规则后，dry-run 返回 **feasible**：",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "122B 环方案" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (专家)" },
          { n: "10", l: "卸载的层数" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "规划器输出——推理引擎 -ot 规则格式。",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # 推理引擎 -ot format`,
      },
      { t: "h2", kick: "接了什么线 · 纯 Python，无需重编 C++", text: "把规划器的卸载规则送进真实加载" },
      {
        t: "ul",
        items: [
          "**planner** — 每个 placement 中已生成 `ot`（逗号连接的 `-ot` 规则）。",
          "**protocol.py** — 新增 `StageStartRequest.ot` 字段。",
          "**runtime.py** — 把 placement 的 `ot` 转发进 stage 请求。",
          "**stage_service.py** — 协调器以 `--override-tensor` 启动。",
          "`linkcpp-server` 把未知参数转发给原版 llama-server，`-ot` 原样生效。",
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
        md: "以 `ot` 协议往返测试加上确认协调器命令输出 `--override-tensor` 完成验证；hub 无回归地重新部署。当时剩下的：完整的 2 节点 77 GB 加载（带卸载的协调器 + 持有约 1.5 GB 单层窗口的手机），等待服务器可用。M0 的核心——让弱节点参与大 MoE 的骨干卸载——已在代码与规划器层面完结。",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — 专家切片数据路径 (M1)",
    dek: "弱设备下载的是几个 6 MB 的专家，而非 1.4 GB 的层——且分片计算与整体一致到 3.6e-12。",
    blocks: [
      { t: "h2", kick: "验证 · 真实 Qwen3.5-122B-A10B", text: "专家切片是字节拷贝——无反量化" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "专家维切片" },
          { n: "~6.1", l: "MB / 专家 (Q4+Q6)" },
          { n: "206 MB", l: "2 层 × 16 专家下载" },
          { n: "200", l: "HTTP，有效 GGUF" },
        ],
      },
      {
        t: "p",
        md: "MoE 专家张量把所有专家沿最外层 ggml 维度堆叠，读取器因此暴露 `(n_expert, rows, row_bytes)` 的原始量化字节。专家 *e* 是量化块对齐的连续板块——切片字面上就是 `data[a:b]`，无反量化、无重打包。",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf——已验证的往返。",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "数值 oracle", text: "dispatch + combine == monolithic，精确成立" },
      {
        t: "p",
        md: "用真实 122B layer-0 专家（反量化参考），把专家拆成 4 个分片分别计算再合并，结果**与整体 MoE FFN 一致**：分片是同一加权和的精确重组，不是近似。",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "相对误差" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "涉及的专家" },
        ],
      },
      { t: "h2", kick: "C++ worker，硬件验证", text: "linkcpp-expert-worker 在 ROCm 上复现 oracle" },
      {
        t: "ul",
        items: [
          "**纯 ggml/gguf**（无 libllama）：把切片加载进 GPU 后端，运行 `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`。",
          "**ROCm 构建 + 运行**（MI250）：122B layer-0，专家 [0,8)，4 个 token。",
          "**对 oracle cosine 0.99995**，allclose(1e-3) = True，max|Δ| = 7.9e-7——这个残差本身就是跨后端等价（ROCm vs numpy）的首个实测案例。",
          "同一代码路径覆盖 CUDA/Metal/Vulkan/CPU（`mul_mat_id`/`swiglu` 是原版 ggml；CUDA 有专用 MoE 内核）。",
        ],
      },
      {
        t: "p",
        md: "最难、风险最高的部件——设备端 worker 内核——在此得到验证。剩下的是骨干↔worker 编排；worker 已是消费这些切片的、经过验证的纯函数。",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — 分布式专家调度 (M2)",
    dek: "实时 122B 解码把一层的专家计算经 TCP 交给独立 worker 进程——并预测出完全相同的 token。",
    blocks: [
      { t: "h2", kick: "验证 · 真实 122B，两个进程", text: "骨干解码 → TCP → worker → experts → 同一 token" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "logit cosine" },
          { n: "0", l: "传输损耗 (byte-identical)" },
          { n: "2", l: "进程 (骨干 + worker)" },
        ],
      },
      {
        t: "p",
        md: "专家 worker 作为**独立进程**（ROCm）服务 layer-0 切片，122B 骨干的 `build_moe_ffn` 调度回调把 `(cur, sel)` 经 TCP 发出并收回专家输出。logit cosine **与进程内数值完全一致**（0.99868775）——传输无损。专家并行的蜂群计算跨过了进程边界。",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "一条长连接 TCP——环/443 中继可以隧道的正是这条流。",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "完成", text: "分布式调度流水线" },
      {
        t: "ul",
        items: [
          "`--serve` 模式：加载切片、监听 TCP、应答 `(n_used, n_tokens, cur, sel) → experts`。",
          "`--dispatch-port`：骨干回调经 TCP 与独立 worker 收发，替代进程内计算。",
          "在 layer-0 调度到进程外的实时 122B 解码上实测 → **argmax MATCH**，cosine 0.99869（= 进程内，无损）。",
          "M2 核心（更早）：手机 ARM 以 cosine 0.99992 计算真实 122B 专家（Android 交叉构建）。",
        ],
      },
      {
        t: "p",
        md: "接下来：把同一条 TCP 流经 **443 中继**隧道到其他机器与手机上的 worker（传输已在环的工作中验证），然后是 M3 覆盖市场与 M4 批量吞吐。",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — 专家覆盖市场 (M3)",
    dek: "弱节点看到哪个 (层, 专家范围) 最稀缺、奖励最高，然后自己去填——已验证的层市场，换上更细的颗粒。",
    blocks: [
      {
        t: "p",
        md: "Kvasir 的层分片市场——需求图、最高奖励自助报名、部分下载、按节点奖励——已经真机验证。M3 把同一机制重新参数化到 **(层, 专家范围)** 颗粒，让覆盖朝着副本最不足、奖励最高的专家范围自我修复。",
      },
      { t: "h2", kick: "验证 · API", text: "稀缺度聚合 → 最高奖励范围分配" },
      {
        t: "p",
        md: "三个 worker 在 layer 0 上注册：A = [0,128)，B = [128,256)，C = [0,128) 作为第二副本，`target_replicas = 2`：",
      },
      {
        t: "table",
        head: ["层", "专家", "副本", "稀缺度"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (达标)"],
          ["0", "[128, 256)", "1", "0.5 (未达标)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → 把最稀缺范围裁剪到节点预算。",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "完成 · 纯 Python (hub)", text: "专家颗粒的供需市场" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — worker 以心跳上报其 (层, 专家范围) 持有。",
          "`GET /api/expert-demand` — 按专家聚合副本数 → 带稀缺度分数的连续专家范围区段。",
          "`POST /api/expert-volunteer` — 把最稀缺范围裁剪到节点预算后分配。",
          "既有层市场（自助报名 · 部分下载 · 奖励）重新参数化为 (层, 专家范围)。",
        ],
      },
      {
        t: "p",
        md: "M4 接续的内容：并发请求的批量调度加 hot-expert 缓存——tokens/s 与 worker 数成正比——以及副本路由（最近/最快的 worker）与流失回退。",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — 批量调度吞吐 (M4)",
    dek: "蜂群是吞吐织物而非延迟游戏：把调度调用打包成批，把每请求开销按 token 摊销 77 倍。",
    blocks: [
      { t: "h2", kick: "实测 · ROCm，专家 FFN，n_used = 8", text: "批越大，每 worker 的 tok/s 越高" },
      {
        t: "table",
        head: ["批量", "每 worker tok/s"],
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
        md: "从批量 1 的 **1.45 ms/tok** 到批量 512 的 **0.019 ms/tok**——每 token 77 倍的提升。每次调用的时间几乎不变（1.45 → 9.6 ms），而批量增长了 512 倍——GPU 在固定开销之后几乎免费地处理整批。这就是让专家并行变得实用的**吞吐织物特性**：批量调度摊销了每请求的 RTT 与开销。",
      },
      { t: "h2", kick: "完成", text: "批量调度吞吐" },
      {
        t: "ul",
        items: [
          "worker `--bench`：批量 1…512 的 compute_dispatch 计时 → tok/s。",
          "批量 512（ROCm）下每 worker **53k tok/s**——批处理摊销了开销。",
          "hot-expert 缓存与多 worker 聚合扩展（副本路由）在此之上叠加。",
        ],
      },
      {
        t: "callout",
        md: "随着 M4，**整条 M0 → M4 流水线已在真实 122B 上演示完毕**：骨干卸载 · 专家切片 · 验证过的 worker · 实时解码调度（argmax MATCH）· 分布式进程 · 覆盖市场 · 批量吞吐。",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "一部手机加入了 122B 推理",
    dek: "一台 Galaxy S25 从 hub 自主下载了自己的专家切片，并在实时 122B 解码的每一步计算一层的专家。输出是正确的。",
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → 生成结果（手机在环内）：**\" Paris.\"** —— 与本地运行 8/8 token 一致。",
      },
      { t: "h2", kick: "实测 · 真实 122B，手机计算 layer-0", text: "正确性 + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "token 与本地一致" },
          { n: "4.01", l: "TPS 本地 (基线)" },
          { n: "3.13", l: "TPS 手机参与" },
          { n: "1.58 GB", l: "自主下载" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "即使手机为每个 token 计算 layer-0 的专家，**生成的 token 也与本地完全一致**——正确的 \"Paris.\"。TPS 从 4.01 降到 3.13——手机调度往返（MI250 → 隧道 → 手机，每 token 约 100 ms）花掉 22%。吞吐将由批处理与副本找回（M4）。",
      },
      { t: "h2", kick: "自治参与流程", text: "发现 → 奖励驱动下载 → 加入计算" },
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
      { t: "h2", kick: "已验证 vs 剩余", text: "机制已完备；应用内循环属于产品化" },
      {
        t: "ul",
        items: [
          "部分下载（expert-shard 端点）、worker 服务、骨干调度、实时 122B 生成与 TPS——全部在真机上验证。",
          "正确性：手机参与时 8/8 token 等于本地运行，答案正确。",
          "剩余：应用内自治循环（轮询 expert-demand → volunteer → 下载 → serve → 注册）是 Kotlin 接线——本演示直接驱动了机制本身。",
          "传输：本演示用了 SSH 隧道；生产使用 443 中继（已在环的工作中验证）。",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "Kvasir 经济：成本与奖励的良性循环",
    dek: "去中心化推理网络唯有在消费者所付价格与节点所赚奖励相互强化时才能运转。这里讲我们正为之努力的飞轮、会杀死它的螺旋，以及让它持续转动的三条不变式。",
    blocks: [
      {
        t: "callout",
        md: "**论点：**Kvasir 是一个以单一代币结算的双边市场——消费者付 KVR 做推理，节点赚 KVR 做服务。整个设计的成败系于一个性质：这两侧必须构成一个**良性循环**，每一转都让下一转更容易。搞错了，任何定价策略终将崩溃；搞对了，网络就会越*大*越*便宜*。",
      },
      {
        t: "p",
        md: "把成本与奖励看作一场拔河很有诱惑力——消费者省下的每一块钱都是节点没赚到的一块钱。这种框架是个陷阱。在健康的网络里，它们是从两端看去的**同一个飞轮**：支付变成奖励，奖励变成供给，供给变成容量与更低的价格，更低的价格变成更多的使用，更多的使用又变成更多的支付。问题不在于如何切分一块固定的蛋糕，而在于如何让轮子持续转动，让蛋糕变大。",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "飞轮", text: "为何使用与供给一起成长" },
      {
        t: "p",
        md: "循环的引擎是 Kvasir 中已经成立的一条规则：**推理必须用 KVR 支付**。这使每一单位使用都成为对代币的一单位真实需求——是实用，而非投机。代币需求支撑着节点所赚 KVR 的价值；有吸引力的奖励拉来供给；供给扩大容量，并通过竞争与更细的专家分片压低服务的边际成本；更便宜、更快、更强的服务又拉来更多使用。Kvasir 用一个中心化 API 无法复制的性质让循环更紧：一个参与者可以**同时是消费者和供给者**。需求侧与供给侧往往在*同一群人*身上一起成长，这抑制了那些毁掉单边市场的失衡。",
      },
      { t: "h2", kick: "失效模式", text: "让轮子倒转的四种螺旋" },
      {
        t: "p",
        md: "飞轮既能加速，也同样容易减速。给死亡螺旋命名，正是防范它们的方式：",
      },
      {
        t: "table",
        head: ["螺旋", "如何开始", "终于何处"],
        rows: [
          ["奖励稀释", "更多节点争抢不增长的需求", "单节点奖励下降，节点离开，容量下滑"],
          ["价格过低", "价格便宜，奖励低于节点成本", "服务不再划算，供给与质量崩溃"],
          ["价格过高", "奖励丰厚，却高于市场", "用户转向更便宜的 API，收入枯竭"],
          ["增发依赖", "奖励靠铸造而非收入支付", "通胀侵蚀 KVR，直到两侧一起放弃"],
        ],
      },
      { t: "h2", kick: "不变式", text: "让循环保持良性的三条规则" },
      {
        t: "ul",
        items: [
          "**奖励由真实收入资助。**在稳态下，节点所赚来自消费者所付——而非无节制的代币增发。增发是一种启动补贴，必须随手续费收入增长而*逐步收缩*。Kvasir 在这里已有助力：它奖励**真实工作**——按实际服务 token 数 × 层份额计算的 KVR，而非仅仅在线——因此补贴无法漏给空闲的\"雇佣兵\"节点。",
          "**KVR 是强制媒介。**由于不付 KVR 就无法推理，使用便是代币的一个永久需求汇。这把代币价值锚定在真实实用而非投机上——这是货币与筹码之别。",
          "**价格在一个区间内浮动。**保持在节点边际成本之上的下限使服务仍然值得；保持在中心化替代品之下的上限使 Kvasir 保持竞争力。在两者之间，价格移动——这正是网络增长最终体现为更低成本之处。",
        ],
      },
      { t: "h2", kick: "恒温器", text: "让\"更多节点 → 更便宜\"在代码中成真" },
      {
        t: "p",
        md: "如今价格是一个受治理的常数——对 devnet 而言合理，但这意味着增加节点提升的是*容量*，而非可负担性。设计方向是一个**由利用率驱动的价格**：空闲的供给把价格往下推向下限，拥塞把价格往上推向上限。这一个信号就把*\"越多人共享算力，就越便宜\"*的直觉变成协议强制的规则——同时下限让运营者保持偿付能力，使那些让它变便宜的供给不会蒸发。由于价格是敏感的经济参数，它只能在 **genesis 钱包权威加钱包签名 + 2FA** 之下更改，绝不通过任何随意的环境变量。",
      },
      {
        t: "callout",
        md: "**\"免费\"指的是净额，而非价格。**你为自己的推理付费，为自己服务的工作赚取；贡献量大致等于消费量，你的账单便净为零。任何订阅制 API——Claude Max、一个 Codex 席位——都给不了这个，因为你永远无法成为它们的供给侧。有了 Kvasir，你既能运行自己机器装不下的模型，*又能*因帮助别人运行他们的模型而获得报酬。",
      },
      {
        t: "p",
        md: "这一切都不需要奇特的机制设计。它需要的是对三件事的自律：奖励来自收入，价值来自使用，平衡来自有界的浮动价格。Kvasir 已经交付了困难而诚实的部分——非托管结算、与工作成比例的奖励、一个你必须真正花费才能使用网络的代币。其余是经济路线图：逐步收缩、为失败推理的保险池注资的手续费分成，以及恒温器。按这个顺序建成，成本与奖励便不再相争，而开始相互复利。",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "一块跨越互联网的 GPU 加入了 122B 推理",
    dek: "另一座城市的一台 Blackwell 工作站向外拨出一条 443 连接，为实时 122B 解码计算专家——与本地运行逐位相同，并因所做的工作获得 KVR 报酬。",
    blocks: [
      {
        t: "callout",
        md: "**发生了什么：**一个地方的 AMD 骨干上运行的 122B 解码，把它每 token 的专家工作发给了另一座城市的一台 NVIDIA GB10（Grace Blackwell）机器——经由端口 443 上一条向外的 WebSocket——并收回了产出**完全相同 token** 的专家输出，与在本地计算一样。没有隧道，没有端口转发，没有入站防火墙开洞。这台远程机器因所服务的字节赚到了 KVR。",
      },
      {
        t: "p",
        md: "Kvasir 的前提是*来什么硬件用什么硬件*——包括运营商 NAT 后、公网上、另一座城市里的硬件。Qwen3.5-122B-A10B 把**权重的 86% 装在 12,544 个独立专家里**（48 层 × 256，top-8），每个都是一个 5.3 MB 的纯函数。正是这个颗粒让一台遥远、不相干的机器也能持有一份切片并做出贡献。悬而未决的问题从来不是*能不能拆分*——而是*一个跨越开放互联网的 worker 能否真的参与实时解码，且正确、可问责*。现在它做到了。",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "一次向外拨号", text: "没有隧道，没有入站端口" },
      {
        t: "p",
        md: "远程 worker 只打开**一条**连接——向外的 `wss://`，连到 443 上的公网网关，那是运营商 NAT 与 CDN 边缘唯一可靠放行的端口。网关不解析这条流；它把 WebSocket **原样拼接（raw-splice）**到仅限 LAN 的 hub，再由 hub 桥接到骨干的专家调度监听器。两端都向外拨号，在中间相遇。worker 暴露的入站端口为零，也不需要公网地址。",
      },
      {
        t: "code",
        caption: "两次向外拨号，拼接成一条普通的调度流。",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "跨越互联网逐位相同", text: "路由器只决定一次；数学精确重组" },
      {
        t: "p",
        md: "骨干**只带权威地运行一次**路由器；worker 是一个纯 `(hidden, ids) → out` 函数。因此把这个函数搬到另一块大陆，改变的是矩阵乘*发生在哪里*，而不是*它计算什么*。在 layer-0 专家远程服务的实时 122B 解码上：greedy token 流**8/8 相同**（\" Paris.\"），logit **cosine 0.99773**，argmax 一致。这与让 CUDA↔ROCm↔CPU 离散决策保持不变的路由器权威性质完全相同——异构后端始终被连续误差约束，绝不发生灾难性分支。",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "greedy token 相同" },
          { n: "0.99773", l: "logit cosine，远程 vs 本地" },
          { n: "1.2%", l: "TPS 开销，直连 (13 ms RTT)" },
          { n: "0.00895", l: "首次远程会话给 worker 的 KVR" },
        ],
      },
      { t: "h2", kick: "诚实的代价是 RTT", text: "为何蜂群是吞吐织物，而非低延迟解码器" },
      {
        t: "p",
        md: "串行的每 token 调度每步都要付一次往返。实测：直连（13 ms RTT）时吞吐开销为 **1.2%**（4.220 → 4.169 tok/s）；经 443 上的 CDN 边缘路由则为 **~28%**。我们如实公布这一点，因为它指向了设计上的真相——广域网蜂群是**受 RTT 约束的**，因此它的强项不是单条流的延迟，而是**总容量**。批处理摊销往返：批量专家调度在批量 512 时达到**每 token 吞吐的 77 倍**。字节是余量；往返才是要隐藏的东西——这正是配套路线图文章的主题。",
      },
      { t: "h2", kick: "精确按工作付酬", text: "计量的字节变成 KVR" },
      {
        t: "p",
        md: "参与若不可问责就毫无价值。中继把**每次会话桥接的字节计量**记入 hub 的贡献账本；网关轮询该账本，把 KVR 增量记到 worker **自己的**钱包——和其他一切一样非托管。首次跨互联网会话确实累积了：**1.28 MB 的工作 → 1.277952 units → 0.00895 KVR** 的待领奖励。数额很小，而这正是重点——它是真实的、按工作计的结算，不是参与奖杯。",
      },
      {
        t: "p",
        md: "同一条向外 443 的路径，正是**手机**加入的方式：一台 Galaxy S25 已经通过它计算过 122B 专家（8/8 token 相同，cosine 0.99992）。一个前沿规模的模型，由一个地方的骨干、另一座城市的数据中心 GPU、以及某人口袋里的手机共同服务——全都产出相同的 token，各自因其份额获得报酬。接下来是让广域网往返变便宜；那份路线图立足于他人的生产数据和我们自己的测量。",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "让广域网调度变便宜：一份有据可依的路线图",
    dek: "远程专家调度可行且逐位相同——但广域网解码受往返约束。这里是削减成本的计划，立足于 DeepSeek、Petals 等的生产数据（路线图，尚未交付）。",
    blocks: [
      {
        t: "callout",
        md: "**框定：***我们测量*的数字均按实测陈述；一切被描述为计划的内容都是**路线图**，而非已交付的结果。目标是把远程专家调度——已经正确且已付酬（见配套文章）——的广域网往返做得足够便宜，让一台遥远的 GPU 或一部手机成为一等的蜂群成员，而不是拖慢的那个。",
      },
      {
        t: "p",
        md: "我们自己的测量，坦白公布：调度每 token 每层约花 **110 KB**——12.3 KB 出站（dispatch）加 98.3 KB 返回（combine）。这 8× 的不对称，是因为每个被选中的专家在加权求和*之前*就返回其完整输出。直连时是 **1.2%** 的吞吐开销；经 CDN 中继则为 **~28%**。这些是事实。本文余下的部分讲我们打算如何弥合差距——以及为何字节才是容易的部分。",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "主导定律", text: "广域网解码受往返约束" },
      {
        t: "p",
        md: "这里最重要的一个已发表结果不是我们的——是 Petals 的：当 RTT 从 <5 ms 升到 100 ms，解码从 **1.24 降到 0.57 steps/s**，而**把带宽砍到 1/10 几乎不改变它**。延迟主导；带宽是富余的。这重新框定了整个问题：削减字节是余量，而**削减往返才是实质**。下面每一项都按它能去掉多少往返成本排序。",
      },
      { t: "h2", kick: "线路上更便宜", text: "精度优先的字节削减" },
      {
        t: "ul",
        items: [
          "**返回加权部分和，而非原始专家输出。**由线性性，骨干的 combine 无论哪种方式都精确，但 worker 返回一个求和后的向量而非 8 个——这就是那 ~8× 的 combine 削减，也正是 DeepSeek-V3 / DeepEP 在生产中所做的。",
          "**跨多个 worker 用并行星形，而非串行链**：ΣRTT 坍缩为 max RTT。",
          "**线路上用 F16**——我们已经接受约 0.998 的跨后端 cosine，因此 F16 传输落在既有容差之内；**之后再上分块 INT8/FP8**，在我们自己的 argmax/cosine 闸门对 Q4_K_M 权重放行之后（Petals 已展示在真实互联网上用 INT8 而无质量损失）。",
          "这些合起来瞄准**每 token ~110 KB → 9–12 KB（~12×）**——真实，但记住它是*余量*，而非瓶颈。",
        ],
      },
      { t: "h2", kick: "摊销往返", text: "实质：更少的往返、被隐藏的往返" },
      {
        t: "ul",
        items: [
          "**投机解码**把许多 token 变成一次往返。在实测的 80 ms 广域网上，盈亏平衡只需**每步 ~1.15–1.2 个被接受的 token**——因此即便是一个弱 n-gram 猜测也能取胜（原生 Jacobi 可能适得其反；技术选择很重要）。我们的调度协议已经携带 `n_tokens > 1`，因此无需改动线路。",
          "**网关上的连续批处理**把并发请求折叠进一次往返；**槽亲和的前缀缓存**让一个会话停留在同一批副本上。",
          "**延迟隐藏**：共享专家是一个独立的加性项，因此骨干在远程往返期间*在本地*计算它（ScMoE 报告在 PCIe 上 1.82×，无需重训）。把 **hot 专家留在本地**，只把 cold 的发到远程（EPLB 在生产中复制最热的约 32 个，换来 2.54× 的解码提速）。",
        ],
      },
      { t: "h2", kick: "策略与粗管道的未来", text: "按对端选路，以及 200 Gb/s 改变什么" },
      {
        t: "p",
        md: "路径策略：可公网路由的对端走**直连**路径（那条 1.2% 的路线）；中继只留给 NAT 后的设备。而当宽阔的 200 Gb/s 链路到来，110 KB 在 **~4.4 µs** 内序列化——甚至在上述削减之前，带宽项就已消失，一份 794 MB 的切片在 ~32 ms 内传完。但 **RTT 是物理规律；它不会缩小**——因此即便在 200 G 下，投机解码与重叠仍是真正的杠杆。粗管道真正要紧之处，在于多骨干联邦（多个骨干共享一个专家池）和受带宽约束的工作：长提示词 prefill 与大批量吞吐。",
      },
      {
        t: "callout",
        md: "**一个如实陈述的告诫：**传输层本身（WebSocket vs QUIC、掩码开销、NAT 打洞）**没有可供我们引用的外部结果**——那是我们会在宣称任何东西之前自己测量的工程。上文的一切都建立在已发表的生产数据（DeepEP / DeepSeek-V3、Petals、DeepSpeed-MoE、ScMoE、SGLang/EPLB）加我们自己的测量之上；当某个路线图项交付时，它的数字与时态会在此更新。",
      },
      { t: "h2", kick: "接下来", text: "接入目标" },
      {
        t: "p",
        md: "我们的调度钩子挂在 `build_moe_ffn` 上——**推理引擎中 43 种 MoE 架构共享的一个函数**。三条不变式与模型无关：MoE 数学（routed = Σ wᵢ·Eᵢ(x)，线性）、共享的代码路径，以及 GGUF 标准的堆叠专家张量（最外层 `ne[2]` → 块对齐切片）。因此接入一个新模型不是重新设计——而是过一遍针对每个模型的 argmax/cosine 验证闸门。",
      },
      {
        t: "table",
        head: ["模型", "专家 · 路由", "每专家 (Q4≈)", "共享", "状态"],
        rows: [
          ["Qwen3.5-122B（今日服务中）", "256 · top-8", "5.3 MB（实测）", "是", "生产中"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "是", "就绪 — 首个候选"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "是", "就绪（钩子已验证）"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "否", "就绪（钩子已验证）"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "是", "就绪（deepseek2 计算图）"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "是", "就绪（deepseek 家族）"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "否", "就绪"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "否", "就绪"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "是", "就绪（每隔一层是 MoE）"],
          ["MiniMax M3 428B", "128 · top-4", "待定（GGUF）", "是", "等待上游引擎"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "否", "可用 — 仅限 GPU worker"],
        ],
      },
      {
        t: "p",
        md: "整个行业正在向细粒度 MoE 收敛——专家更小、数量更多、稀疏度更高（DeepSeek、Qwen、Kimi、GLM、gpt-oss 都朝这个方向走）。朝那个方向的每一步，都让蜂群的参与单位更小、稀缺市场的颗粒更细。上面这些模型不是愿望清单；每一个都已经流过我们在生产中运行的同一个调度钩子——接入是一道验证闸门，而非一个工程项目。",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "200G 之问",
    dek: "我们的蜂群 hub 已经能用货架上的现成部件以 200 Gb/s 互联——其中一个就内置在 GB10 里。这里讲一条粗管道为分布式 MoE 买来什么，以及它买不到的那一样东西。",
    blocks: [
      {
        t: "callout",
        md: "**前提：**广域网解码受 RTT 约束，而非受带宽约束——我们的通信路线图表明字节是容易的部分。那么当 hub 拿到 200 Gb/s 链路时，真正改变的是什么？关于*容量*的几乎一切，关于*延迟*的几乎一无所变。",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "已在盒子里 · ConnectX-7", text: "硬件并不科幻——一个就随我们的 GB10 worker 一起出货" },
      {
        t: "p",
        md: "计算我们 122B 专家的那台 GB10 Grace Blackwell，板载了一块**带两个 200 GbE QSFP 端口的 NVIDIA ConnectX-7**。两台这样的机器用一根约 100 美元的 QSFP56 DAC 线缆直连——一个零交换机的 200G 双 hub 集群。ARM 在这里是一等公民：在 x86 数据中心驱动这些网卡的那套 `mlx5` 驱动栈，同样在 aarch64 上驱动它们，而 GB10 正是 aarch64。",
      },
      {
        t: "callout",
        md: "**细则：**GB10 在多主机模式下通过两条 PCIe Gen5 x4 链路馈送它的 ConnectX-7。实测满速（~185–190 Gb/s）需要 **RoCE（RDMA）和正确映射的拓扑**——在映射错误的路径上朴素地跑 TCP 只能到 ~95 Gb/s 甚至更差。粗管道是用配置买来的，不只是靠线缆。",
      },
      { t: "h2", kick: "距离阶梯", text: "200G 在每种距离上都是目录里的现货" },
      {
        t: "table",
        head: ["距离", "部件", "外形规格"],
        rows: [
          ["机架 (0.5–3 m)", "QSFP56 DAC 铜缆", "线缆，~$100"],
          ["房间 (~30 m)", "AOC 有源光缆", "线缆"],
          ["园区 (2–10 km)", "200G FR4 / LR4 光模块", "QSFP56 模块"],
          ["城域 (~40 km)", "200G ER4 光模块", "QSFP56 模块"],
          ["区域 (~120 km)", "400G ZR+ 相干，以 200G 线速运行", "QSFP-DD 模块"],
          ["长途 (数百 km)", "运营商 200G 波长 / DWDM 线路系统", "租用服务"],
        ],
      },
      {
        t: "p",
        md: "在广域网里，*线缆*不过是标准单模光纤——速率中性的玻璃，早已铺遍每一座城市。速率活在两端的可插拔光模块里，而 **OpenZR+ 让 200G 跑 120 km 成了一个你插进交换机的模块**，而非一个电信工程。再往上，你就得租一条波长了。",
      },
      { t: "h2", kick: "它买来什么", text: "蜂群里每一个带宽项都消失了" },
      {
        t: "ul",
        items: [
          "一个调度载荷（如今约 110 KB/token/层，线路路线图之后约 10 KB）在**微秒**内序列化——载荷大小彻底不再是设计约束。",
          "一份**专家切片在 ~32 ms 内传完**（794 MB，理论值），整个 122B 模型在 **~3 s** 内同步——覆盖市场再平衡与新 hub 接入变得近乎瞬时。",
          "长上下文 prefill——唯一真正吃带宽的阶段——以线速推进，因此 100K-token 提示词上的首 token 时间变成受骨干计算约束。",
          "**批量调度在没有线路上限的情况下扩展**：跨众多用户流聚合的专家池流量，恰恰是粗管道所吸收的那种吃带宽、耐延迟的负载。正是这一点让多骨干联邦——多个 hub，各自为自己的用户持有 KV，共享一个专家池——变得可行。",
        ],
      },
      { t: "h2", kick: "它买不到什么", text: "光不会赶路" },
      {
        t: "p",
        md: "光纤以 ~5 µs/km 传送光，再多的带宽也改变不了这一点。一次 13 ms 的往返，在 200 Gb/s 下仍是 13 ms。自回归解码为每个分片层、每个 token 都要付这次往返——这正是为什么即便在市面上最粗的管道相连的 hub 之间，**投机解码（每次往返 k 个 token）与共享专家重叠（在调度在途时进行计算）仍然不可或缺**。带宽买来吞吐；唯有对往返的自律才买得到延迟。",
      },
      {
        t: "p",
        md: "于是架构落定为两层。一个 **hub 层**——由 200G 级链路相连的骨干与 hot 专家，其容量实质上无界——和一个**边缘层**——443 中继上的手机与小设备，持有稀缺市场分配给它们的专家长尾。粗管道让第一层感觉像一台机器；中继让第二层向任何人敞开。两者互不替代：这种分割*就是*设计本身。",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "随负载生长的蜂群",
    dek: "一个只在需要时才借力的巨型模型——MoE 蜂群如今随流量自我伸缩：清闲时紧凑而快速，繁忙时铺开而并行。",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir 服务的模型远大于任何单台机器所能容纳——一个 122B 参数的专家混合（MoE）模型跑在一台协调器加一群 worker 上：LAN 上的 GPU、200 Gb/s 链路对端的 GPU，甚至通过互联网拨入的手机。由于 MoE 模型把每个 token 只路由给少数几个专家，任一时刻大多数权重都处于闲置，而这些闲置专家可以住在主节点**之外**——住在任何自愿持有它们的硬件上。",
      },
      {
        t: "callout",
        md: "新的部分在于蜂群如今会**随负载自我伸缩**。",
      },
      { t: "h2", kick: "行为方式", text: "清闲时紧凑，繁忙时铺开" },
      {
        t: "p",
        md: "流量清淡时，协调器在自己的 GPU 上服务一切——每 token 最快的路径，没有网络跳转。当请求开始堆积、其推理槽位饱和时，两件事会自动发生：",
      },
      {
        t: "ul",
        items: [
          "**它重新启用已有的 worker。**协调器盯着自己的队列。饱和时它持续把路由到的专家工作外送给**已验证（proven）**的 worker——那些真正服务过的 worker——以一点点每 token 延迟换来大得多的总吞吐。仅仅连接过却从未计算过的 worker 绝不会被托付负载；而全新的 worker 仍会获得一次公平的首试。",
          "**hub 招募新的 worker。**控制 hub 注意到同样的饱和，抬高该模型专家的“需求”。闲置节点——某人口袋里的一部手机、城里另一头的一张空闲 GPU——本就在轮询那个需求市场。需求一旦上升，它们就被提供一份专家切片去服务，下载它、拨入、加入。浪涌过去后，需求回落，多余的 worker 悄然退去。",
        ],
      },
      {
        t: "p",
        md: "没有人调度这一切。没有节点被推送。蜂群随负载呼吸：清闲时紧凑而快速，繁忙时铺开而并行——而且即便对家用路由器后的节点也奏效，因为一切都是拉取式（pull-based）的。",
      },
      {
        t: "p",
        md: "这就是一个能在无人独占的硬件上服务万亿参数模型的网络的形态：闲置算力恰好在值得邀请时才被邀请进来，且仅在此时。",
      },
    ],
  },
};
