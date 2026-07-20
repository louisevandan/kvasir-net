/* 한국어 — 기술 블로그 번역. 구조(slug·카테고리·블록 순서·코드·img 위치)는
   articles.ts(영어 원본)를 그대로 따르고, 기술 용어·식별자·수치는 원문 유지. */
import type { TechTranslation } from "./articles";

export const koTech: Record<string, TechTranslation> = {
  "expert-sharded-swarm-design": {
    title: "전문가 샤딩 스웜 추론: 설계",
    dek: "122B MoE의 86%는 서로 독립적인 5.3 MB 전문가 12,544개다. 그 알갱이로 모델을 쪼개면 폰도 프런티어 추론의 실질적 몫을 질 수 있다.",
    blocks: [
      {
        t: "callout",
        md: "**테제:** Qwen3.5-122B 무게의 86%는 상호 독립적인 5.3 MB 전문가 12,544개다. 전문가 단위로 샤딩하면 약한 기기는 \"1.4 GB 레이어\" 대신 \"전문가 8–64개(42–340 MB)\"만 지면 된다 — 폰이 실제로 감당할 수 있는 바로 그 단위다. MoE는 스웜의 천연 기질이다.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "기질 · Qwen3.5-122B-A10B (Q4_K_M)", text: "무게가 이미 스웜 크기의 단위로 포장돼 있다" },
      {
        t: "stats",
        items: [
          { n: "49", l: "레이어" },
          { n: "256", l: "전문가 / 레이어" },
          { n: "8", l: "토큰당 활성" },
          { n: "5.3 MB", l: "전문가 1개 (Q4)" },
          { n: "12,544", l: "전문가 총수" },
          { n: "86%", l: "무게 중 전문가 비중" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "전문가 차원 = 최외곽" },
        ],
      },
      {
        t: "p",
        md: "전문가 인덱스는 모든 MoE 텐서의 **최외곽 차원**이라, 각 전문가는 연속된 양자블록 정렬 슬랩이다. 전문가 단위로 슬라이스한 mini-GGUF는 깨끗한 바이트 레인지 복사 — dequant도 re-pack도 없다.",
      },
      { t: "h2", kick: "두 역할", text: "백본 스테이지 × 전문가 워커" },
      {
        t: "ul",
        items: [
          "**백본 스테이지(강한 노드):** 어텐션 + KV 캐시, 모든 norm, **라우터**, shared expert, residual combine — dense 경로 전부. 모든 전문가를 fallback 복제본(RAM offload)으로 상주시켜 스웜에 churn 내성을 준다.",
          "**전문가 워커(폰):** 트랜스포머가 아니다. 어텐션도 KV도 샘플러도 없는 순수 함수 `(hidden, local_ids) → out` — mat-mul 3개로 이루어지며, 자기 전문가 슬라이스만 상주시킨다. 4 GB 폰까지 어떤 예산에도 맞는다.",
        ],
      },
      {
        t: "code",
        caption: "컷 지점: 라우터는 백본에서 단 한 번, 권위를 갖고 실행된다.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "라우터가 백본에서 **정확히 한 번** 실행되므로, 선택된 전문가는 각자 소유 노드가 정확히 한 번씩 계산한다. **근사는 없다** — 샤딩은 mat-mul이 일어나는 위치만 옮길 뿐이다.",
      },
      { t: "h2", kick: "새 서브시스템이 아니다", text: "스웜 = 검증된 보상 마켓을, 더 잘게" },
      {
        t: "p",
        md: "Kvasir엔 이미 **레이어** 샤드용 자율 희소성 마켓이 있고 실기기 검증됐다: NAT 뒤의 폰이 수요 맵을 폴링해 **최대 보상** 구간에 self-enroll하고, 그 window만 부분 다운로드해 Adreno GPU에 로드하고, 링 추론을 완주해 기여 보상까지 받았다. 전문가 샤딩은 이 전부 — 커버리지 맵·최대보상 self-enroll·부분 다운로드·노드별 보상 — 를 그대로 재사용하고, 커버리지 단위만 *레이어 구간*에서 *(레이어, 전문가 범위)*로 바꾼다.",
      },
      { t: "h2", kick: "이미 실기기 검증된 두 혁신", text: "부분-가중치 참여 + 443 릴레이" },
      {
        t: "ul",
        items: [
          "**보상 주도 부분-가중치 다운로드:** 기존 RPC/TP/PP는 전체 체크포인트를 모든 rank에 내려보내고 스케줄러가 배치를 지시한다. Kvasir에서 노드는 **자기가 계산할 슬라이스만** 내려받고, 그 슬라이스를 **스스로, 보상 기준으로** 고른다 — 77.6 GB 전체 모델 대비 254 MB 스테이지 mini-GGUF. 4 GB 폰이 자기보다 훨씬 큰 모델에 참여하는 메커니즘이다.",
          "**443 릴레이 데이터 플레인:** Cloudflare의 80/443 전용 엣지 + 캐리어 NAT는 양방향 직접 접속을 모두 막는다. 1바이트 role preamble을 쓰는 엣지별 WebSocket 브리지로 **양쪽 모두 아웃바운드 접속**한다(폰은 인바운드 포트 0개). 이를 안착시키며 실제 버그 셋을 잡았다 — 빌드 핑거프린트 정합, node-token 다운로드 인증, 그리고 64 KiB 이상 모든 프레임을 조용히 손상시킨 Kotlin `Int.ushr` 프레임 길이 버그(`ushr`은 시프트의 하위 5비트만 사용, `len ushr 56`이 `len ushr 24`가 됨) — `Long` 시프트로 수정.",
        ],
      },
      { t: "h2", kick: "솔직한 크럭스", text: "저지연 디코더가 아니라 처리량 패브릭" },
      {
        t: "p",
        md: "디코드는 49레이어 직렬이고, 레이어당 인터넷 왕복은 토큰당 2.5–10초다. 그래서 스웜의 승부처는 **아무도 혼자 못 올리는 모델을 함께 서빙하는 것** — 총처리량이다: 배치 dispatch가 RTT를 상각하고, 백본이 hot-expert 캐시를 유지하고, 요청은 가까운 복제본으로 라우팅된다. 저지연 경로는 파이프라인 링이 맡는다.",
      },
      { t: "h2", kick: "로드맵", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — 백본 전문가 RAM offload: 그래프 수술 없이 코디네이터 한 대에서 122B 실행.",
          "**M1** — 단일 호스트 expert-parallel 증명: expert-sliced mini-GGUF + 워커 런타임 + dispatch, 로짓이 monolithic과 정확히 일치.",
          "**M2** — LAN + NAT 폰 워커가 443 릴레이 너머로 실제 122B 전문가 계산.",
          "**M3** — 복제본·churn fallback을 갖춘 전문가 단위 커버리지 마켓.",
          "**M4** — 처리량: 배치 dispatch + hot-expert 캐시, 워커 수에 비례하는 tokens/s.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "블루프린트에서 하드웨어까지: 검증된 것과 keystone",
    dek: "검증 캠페인의 회고 — 설계부터 M2 코어까지 실제 122B로 입증 — 그리고 나머지를 여는 단 하나의 통합 조각.",
    blocks: [
      {
        t: "p",
        md: "지난 몇 주간, 전문가 샤딩 스웜의 어렵고 새로운 조각들이 실제 **Qwen3.5-122B**에서 하나씩 입증됐다 — 시뮬레이션도, 장난감 크기도 아니다. 지금까지의 검증 궤적과, 마지막까지 남아 있던 단 하나의 keystone을 정리한다.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "궤적 · 전부 실제 122B로 검증", text: "지금까지 안착한 것들" },
      {
        t: "ul",
        items: [
          "**설계(블루프린트 이후 5회 보강)** — EP 아키텍처, 부분-가중치 자율 참여, 릴레이, 워커 커널 스펙, 그리고 교차 백엔드 수치 동등성을 핵심 기술로 문서화. 라우터 권위를 정합성 불변식으로 규정.",
          "**M0 — 백본 전문가 RAM offload(플래너 검증):** 122B가 단일 64 GB 코디네이터에 *feasible* — 전문가 10개 레이어를 RAM으로 offload, VRAM 62.6 GiB / RAM 14.2 GiB, `--override-tensor`로 배선.",
          "**M1 — 전문가 슬라이스 데이터 경로:** 전문가 단위 mini-GGUF 슬라이싱(ne[2] 슬랩, dequant 없는 바이트 복사) + `/expert-shard` 다운로드 엔드포인트.",
          "**M1 — 수치 oracle:** 실제 layer-0 전문가에서 dispatch + combine == monolithic, **max|Δ| = 3.6e-12** — 샤딩은 같은 가중합의 정확한 재그룹핑.",
          "**M1 — C++ 워커 하드웨어 검증:** `linkcpp-expert-worker`(순수 ggml/gguf)를 ROCm에서 빌드·실행, oracle 대비 **cosine 0.99995**; router → C++ 워커 2개 → combine이 monolithic과 cosine 0.9997–0.9999로 일치.",
          "**M2 코어 — 폰이 실제 122B 전문가 계산:** Android 크로스빌드, SM-S938N에서 실행, oracle 대비 **cosine 0.99992**.",
          "**수치 — 3-백엔드 동등성 행렬:** 같은 122B 계산을 ROCm × 폰 ARM CPU × numpy로 — ROCm↔폰 cosine 0.99990, ROCm↔numpy 0.99996, 폰↔numpy 0.99992. 모두 동등, 어느 것도 bit-identical 아님.",
        ],
      },
      { t: "h2", kick: "keystone", text: "live decode 안으로 통합되는 백본 dispatch" },
      {
        t: "callout",
        md: "모든 **구성요소** — 슬라이스, 워커, dispatch/combine 로직, 수치 동등성, 폰 계산 — 가 실기기 검증됐다. 남은 것은 이들을 **실제 추론엔진 decode 안에서** 연결하는 것: 그래프 중간에서 전문가를 소유 노드로 dispatch하는 `build_moe_ffn` 훅이다. 핀 고정된 추론엔진 서브모듈 수정과 여러 번의 빌드-검증 사이클이 필요했다. 이 keystone이 서면 **M2 릴레이 통합, M3 전문가 커버리지 마켓, M4 배치 처리량**이 차례로 열린다 — 모두 이 dispatch에 의존한다.",
      },
      {
        t: "p",
        md: "keystone은 그 후 안착했다: M2·M3·M4와 라이브 폰 데모 후속 글들이 바로 이 통합의 결과물이다.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "이종 백엔드 간 수치적 동등성",
    dek: "CUDA, ROCm, Adreno, CPU는 결코 bit 단위로 일치하지 않는다. 그럼에도 스웜이 하나의 일관된 모델을 내는 것은 운이 아니라 설계된 성질이다.",
    blocks: [
      { t: "h2", kick: "핵심 구분", text: "정확(exact) vs 동등(equivalent) — 서로 다른 두 성질" },
      {
        t: "ul",
        items: [
          "**같은 백엔드 안 — 정확(3.6e-12):** 전문가를 노드로 분할 후 combine하는 것은 같은 가중합의 재그룹핑이며, 차이는 부동소수점 누적 순서뿐. oracle로 검증됨.",
          "**다른 백엔드 사이 — 동등(1e-3…1e-6):** 같은 연산도 하드웨어가 다르면 op당 상대오차 ~1e-3–1e-6을 지니며, 절대 0이 아니다. **스웜은 바로 이 영역에 산다.**",
        ],
      },
      {
        t: "p",
        md: "\"정확\"은 한 기기 안에서 분해가 보장하는 것. \"동등\"은 이종 하드웨어가 주는 것. 스웜의 임무는 동등성이 발산으로 누적되지 않게 하는 것이다.",
      },
      { t: "h2", kick: "실측 · 실제 122B, 세 백엔드", text: "이론이 아니다 — 하드웨어에서 측정됨" },
      {
        t: "p",
        md: "동일한 Qwen3.5-122B layer-0 전문가 FFN을 `linkcpp-expert-worker`가 MI250(**ROCm**), 폰의 **ARM CPU**(SM-S938N), x86 **numpy** 레퍼런스로 계산 — 같은 입력, 같은 가중치, 다른 명령어셋과 리덕션 순서:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["백엔드 쌍", "max|Δ|", "cosine"],
        rows: [
          ["ROCm(GPU) vs numpy(x86)", "7.9e-7", "0.99996"],
          ["폰 ARM CPU vs numpy(x86)", "1.4e-6", "0.99992"],
          ["ROCm GPU vs 폰 ARM CPU", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "세 개의 명령어셋, 하나의 계산 — 모든 쌍이 동등(cosine ≈ 0.9999)하고 어느 쌍도 bit-identical 아님(Δ ≈ 1e-6). 잔차가 작은 것은 **라우터 권위가 입력과 전문가 선택을 고정했기 때문**이다.",
      },
      {
        t: "p",
        md: "이후 실제 **NVIDIA GB10 Grace Blackwell** 하드웨어 실행이 마지막 백엔드에서 매트릭스를 닫았다: CUDA ↔ ROCm은 **cosine 1.0000000000**(max abs 3.5e-10, 두 GPU 백엔드가 커널 소스를 공유하므로 사실상 비트-동일), CUDA ↔ Grace ARM CPU는 cosine 0.99975 — 위에서 본 GPU↔CPU와 같은 패턴이다.",
      },
      { t: "h2", kick: "왜 백엔드가 다른가", text: "부동소수점 덧셈은 결합법칙이 없다" },
      {
        t: "ul",
        items: [
          "**matmul 리덕션 순서** — tensor core, MFMA 타일, OpenCL workgroup, SIMD 레인이 각기 다른 순서·타일링으로 누적.",
          "**FMA 융합** — `a*b+c`를 1회 반올림(FMA) 또는 2회로, 백엔드마다 다르게 융합.",
          "**누적 정밀도** — F16/BF16 저장 + F32 vs F16 accumulator(발산 크기의 최대 지렛대).",
          "**초월함수 근사** — exp(softmax)·silu/sigmoid(swiglu)·rsqrt(norm)의 다항/테이블 근사 차이.",
          "**dequant + matmul 경로** — dequant 후 matmul vs 융합 양자화 커널의 중간 반올림 차이.",
          "**비결정 커널** — atomic/split-K 리덕션은 같은 기기에서도 실행마다 달라질 수 있음.",
        ],
      },
      { t: "p", md: "버그가 아니다. 각 가속기의 빠른 경로가 치르는 대가다." },
      { t: "h2", kick: "그럼에도 옳은 이유", text: "결정은 하나의 권위에, 누적은 충분한 정밀도로" },
      {
        t: "callout",
        md: "**라우터 권위 — 핵심 불변식.** 네트워크 안의 유일한 이산 결정은 MoE 라우팅(256중 top-8)이다. 각 백엔드가 라우터를 재실행하면 경계 토큰에서 **서로 다른 전문가**를 골라 진짜 발산이 생긴다. Kvasir는 라우터를 **백본에서 한 번** 실행하고 워커에는 선택된 전문가 id만 보낸다 → 이종 스웜은 각 전문가 출력의 *크기*만 다를 뿐, *어떤 전문가가 도는지*는 절대 갈리지 않는다. 파국적 이산 발산을 유계 연속 오차로 바꾸는, 이종 전문가 샤딩의 정합성 규칙이다.",
      },
      {
        t: "ul",
        items: [
          "**이산 argmax:** 디코딩은 로짓에 대한 argmax다. 1e-3의 흔들림은 두 후보가 그만큼 근접할 때만 토큰을 바꾼다 — 대부분의 위치에서 마진이 훨씬 커서 **토큰은 동일**하게 나오고, 드물게 갈리는 곳은 다른 seed와 구분 불가능할 만큼 애매한 위치뿐이다.",
          "**combine은 덧셈:** 부분 결과는 확률 가중 **합**으로 결합된다. 독립적인 ~1e-4 오차는 비간섭적으로 더해져 k배가 아닌 √k로 자라고, 큰 값의 상쇄(cancellation)가 없어 잔차가 well-conditioned로 유지된다.",
        ],
      },
      { t: "h2", kick: "깨질 수 있는 곳 · 막는 규칙", text: "발산 모드와 방어" },
      {
        t: "table",
        head: ["발산 모드", "메커니즘", "규칙"],
        rows: [
          ["라우팅 불일치", "경계 토큰의 top-8을 백엔드마다 다르게 선택", "라우터 권위 — 백본 1회 결정, id dispatch"],
          ["궤적 분기", "토큰별 로짓 흔들림이 결국 토큰을 뒤집고, 이후 시퀀스가 새 seed처럼 갈림", "디코드/샘플링을 한 노드에 고정"],
          ["깊이 누적", "49층 × 각 ~1e-4 → 종단 로짓 최대 1e-2", "경계·combine에서 F32 누적"],
          ["자기-비결정", "atomic/split-K가 실행마다 다름", "combine은 결정 커널, 검증은 허용오차로"],
          ["정밀도 불일치", "한 노드는 F16 누적, 다른 노드는 F32", "누적 정밀도를 capability로 광고, 출력 랭크는 F32 노드 우선"],
        ],
      },
      { t: "h2", kick: "동등성은 숫자다", text: "측정 프로토콜" },
      {
        t: "ul",
        items: [
          "**op당 델타** — 같은 입력에서 A vs B의 matmul·swiglu·softmax·norm 상대오차.",
          "**레이어 경계 드리프트** — 한 층 통과 후 residual 델타를 적층해 깊이 누적이 √L인지 L인지 확인.",
          "**종단 로짓 발산** — 전체 forward의 L∞·L2·**KL divergence**.",
          "**결정 일치율** — top-1 토큰 일치율 + top-8 라우팅 일치율(라우터 권위의 필요성 검증).",
          "**생성 안정성** — greedy N토큰에서 A와 B가 **처음 갈리는 인덱스**.",
          "**태스크 레벨** — perplexity·eval 점수 델타: 사용자가 실제로 체감하는 유일한 지표.",
        ],
      },
      {
        t: "p",
        md: "합격 기준은 **허용오차**다 — \"top-1 일치율 ≥ 99.x%, KL ≤ ε\". 허용오차를 벗어난 노드는 민감 랭크에만 부적합으로 표시될 뿐, 통째로 거부되지 않는다.",
      },
      { t: "h2", kick: "왜 핵심 스웜 기술인가", text: "bit 일치는 불가능하고, 불필요하다" },
      {
        t: "p",
        md: "동종 클러스터는 bit-정확을 가정할 수 있지만 스웜은 못 한다 — 전제가 *어떤 하드웨어든 오는 대로*이기 때문이다. 그래서 Kvasir는 수치 동등성을 프로토콜 호환성과 똑같은 **일급·측정되는 계약**으로 다룬다: 백엔드와 누적 정밀도를 노드 capability로 광고하고, 라우터 권위를 불변식으로 강제하며, 모든 검증을 bit-일치가 아닌 허용오차로 한다. **측정된 수치 동등성 + 단일 권위의 이산 결정** — 이것이 지구상 모든 GPU에서 한 모델을 동시에 돌릴 수 있게 하는 것, 곧 스웜이다.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell이 스웜에 합류했다",
    dek: "GB10 Grace Blackwell이 실제 122B 전문가-FFN 슬라이스를 CUDA로 계산해 AMD ROCm과 비트-동일(cosine 1.0000000000), Grace ARM CPU와 허용오차 내 동등함을 실증했다. 교차백엔드 매트릭스가 완성됐다.",
    blocks: [
      {
        t: "p",
        md: "스웜의 전제는 *어떤 하드웨어든 오는 대로*다. 수치 동등성 — CUDA·ROCm·Adreno·CPU 워커가 모두 같은 토큰을 낸다는 증명 — 은 이미 ROCm·폰 ARM·numpy에서 측정됐다. NVIDIA는 stock ggml/추론엔진의 **기본이자 가장 최적화된** 경로지만, 매트릭스가 아직 닫히지 않은 유일한 백엔드였다. 실제 Blackwell 하드웨어 실행이 그것을 닫는다.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. 같은 실제 Qwen3.5-122B layer-0 전문가 슬라이스에서 두 GPU 백엔드는 사실상 비트-동일하다.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "실측 · 실제 Qwen3.5-122B-A10B, layer-0 전문가 슬라이스", text: "교차백엔드 매트릭스" },
      {
        t: "table",
        head: ["비교", "하드웨어", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "두 GPU 백엔드(CUDA·ROCm)는 커널 소스를 공유하므로 **사실상 비트-동일**(10⁻¹⁰)로 떨어진다. GPU↔CPU는 누산 순서 차이로 op당 ~10⁻³ 섭동을 지니지만 **cosine 0.99975**로 동등 — 앞선 ROCm↔폰-ARM 0.99992와 같은 패턴이다. 라우터-권위 원리는 NVIDIA에서도 유지된다: **이산 결정(argmax·전문가 선택)은 이 연속 섭동 위에서 불변**이다.",
      },
      { t: "h2", kick: "셋업", text: "무엇을, 어디서 실행했나" },
      {
        t: "ul",
        items: [
          "**기기** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124.5 GB 통합메모리.",
          "**툴킷** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; 순수 ggml/gguf expert-worker를 Blackwell 커널로 빌드.",
          "**모델** — Qwen3.5-122B-A10B-Q4_K_M, layer-0 전 전문가(256 experts, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**방법** — 1.58 GB L0 슬라이스를 MI250 → GB10으로 스트리밍(무손실 대조); 동일 입력(h/ids)을 CUDA·CPU·ROCm 세 백엔드로 실행; float32 출력 벡터(36,864)를 코사인·상대 L2·max-abs로 대조.",
        ],
      },
      {
        t: "callout",
        md: "**실기기 한 가지 함정:** GB10의 통합 GPU는 ggml 디바이스 타입이 `GPU`가 아닌 `ACCEL`로 분류돼 `init_by_type(GPU)`가 아무것도 못 찾았다. GPU 타입을 하드코딩하는 대신 첫 번째 비-CPU 디바이스를 선택하도록 수정.",
      },
      { t: "h2", kick: "왜 중요한가", text: "매트릭스가 닫혔다" },
      {
        t: "p",
        md: "이종 워커들이 한 모델을 서빙하려면 CUDA 머신과 ROCm 머신이 **교체 가능**해야 하고, GPU와 CPU가 **수치 동등**해야 한다. Blackwell 측정으로 전체 백엔드 매트릭스에서 둘 다 성립한다: CUDA↔ROCm 워커는 서로 대체 가능하고, GPU↔CPU 워커는 유계·well-conditioned 허용오차 내에서 일치한다. 지구상 가장 흔한 가속기가 이제 검증된 스웜 구성원이다.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "머니 패스 하드닝: KVR 정산의 트랜잭션 보안",
    dek: "결제 서명 재사용, 무인증 보상 발행, 레이스 이중지불 — 세 가지 실제 취약점 클래스를 게이트웨이 정산 서비스에서 찾아 익스플로잇 테스트로 입증하고 봉쇄했다.",
    blocks: [
      {
        t: "p",
        md: "DePIN에서 머니 패스는 컴퓨트 패스만큼 적대적이다: KVR을 적립해 주는 모든 엔드포인트는 결국, 일하지 않고 KVR을 원하는 누군가에게 찔려 본다. 온체인 결제를 검증하고 스테이킹·노드 보상·추론 요금을 적립하는 게이트웨이 정산 서비스에 대한 보안 점검에서 **세 가지 실제 취약점 클래스**를 찾아 봉쇄했다. 각각 수정 전 익스플로잇 스타일 테스트로 입증하고, 수정 후 재검증했다.",
      },
      { t: "h2", kick: "신뢰 모델", text: "클라이언트의 주장이 아니라 온체인 사실을 검증한다" },
      {
        t: "p",
        md: "Kvasir의 수탁 모델은 키를 사용자에게 둔다: 지갑이 트랜잭션에 서명하고, Solana가 이를 기록하며, 정산 서비스의 유일한 일은 잔액을 건드리기 전에 **체인에서 실제로 일어난 일을 검증**하는 것이다. 결제는 *quote → payment → inference*를 따르고, 소비된 모든 트랜잭션 서명은 일회용 `usedSignatures` 레지스트리에 기록돼 두 번 제시될 수 없다. 그래서 정산 서비스가 관문이 되고 — 절대 어겨선 안 되는 규칙은 하나다: 체인이 증명한 것만 적립하고, 클라이언트가 주장한 것은 절대 적립하지 않는다.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "수정 #1 · 발신자 바인딩", text: "결제를 지불자에게 묶는다" },
      {
        t: "p",
        md: "Solana 서명은 **공개**다. 스테이킹 검증 경로는 vault가 기대한 KVR을 *받았는지*만 확인했고 — *누가 보냈는지*는 확인하지 않았다. 공격자는 devnet에서 피해자의 KVR→vault 이체를 지켜보다가 `{owner: 공격자, signature: 피해자의 것}`을 제출하면 됐다: vault 수신 검사는 통과했고, 원금은 공격자에게 적립됐으며, unstake 한 번이면 자금은 공격자의 것이었다. 블록 익스플로러만으로 가능한 직접 절도다.",
      },
      {
        t: "code",
        caption: "수정: KVR이 적립 대상 owner 소유의 토큰 계정에서 출금됐어야 한다.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "수정 #2 · 신뢰된 리포터", text: "보상은 인증된 출처에서만" },
      {
        t: "p",
        md: "노드 보상 엔드포인트들은 **자가 신고 입력**으로 청구 가능한 KVR을 발행하고 있었다: `POST /api/node/contribution`은 클라이언트가 주장한 `units`를 그대로 적립했고 — `units: 1e9` 한 번과 claim 호출이면 vault를 비울 수 있었다 — register/heartbeat는 자칭 허브/게이트웨이 역할(시간당 인프라 보상)과 성능 점수(보상 배수)를 그대로 믿었다. 수정은 보상에 영향을 주는 모든 주장을 **신뢰된 리포터** 뒤로 게이트한다: 허브의 기여 폴링이 쓰는 M2M 서비스 토큰 또는 인증된 관리자만이 units·인프라 역할·성능 등급을 주장할 수 있다 — 이들은 KVR을 발행하므로 open LAN 모드에서도 강제된다. 토큰 비교는 상수 시간이고, 지갑↔노드 연결은 여전히 자유롭다 — 자기 보상을 스스로 주장할 수 없을 뿐이다.",
      },
      { t: "h2", kick: "수정 #3 · 정산 직렬화", text: "모든 잔액에 쓰는 자는 하나" },
      {
        t: "p",
        md: "정산 상태는 락 없는 read-modify-write였고, 모든 머니 연산은 중간에 온체인 지급이나 검증을 *await*한다 — 낡은 잔액을 든 채 이벤트 루프를 양보하는 것이다. 동시 claim 두 건이 같은 100 KVR 잔액을 읽고 둘 다 지급할 수 있었다. 이론이 아니었다: 익스플로잇 테스트에서 **동시 claim 3건이 100 잔액에 대해 300을 지급**했다.",
      },
      {
        t: "code",
        caption: "키별 비동기 직렬화: 같은 키의 머니 연산은 엄격히 순차 실행된다.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "심층 방어", text: "각 계층의 현재 상태" },
      {
        t: "table",
        head: ["계층", "메커니즘"],
        rows: [
          ["신원", "서버 nonce에 대한 SIWS 지갑 서명 로그인 + TOTP 2FA + 일회용 백업 코드"],
          ["전송", "샤드 다운로드의 지갑 파생 node token; 릴레이의 빌드 핑거프린트 정합"],
          ["결제", "스테이킹 이체의 발신자 바인딩; 일회용 usedSignatures; 추론의 비공개 requestId"],
          ["정산", "모든 잔액 쓰기를 감싸는 키별 락; 선지급-후차감; 멱등 재제출"],
          ["리포팅", "보상 관련 사실은 M2M 서비스 토큰 또는 관리자만, 상수 시간 비교"],
          ["수탁", "비수탁형 지갑 — 서비스는 vault가 가진 것만 움직일 수 있고, 사용자 키는 절대 못 만진다"],
        ],
      },
      { t: "h2", kick: "가정이 아니라 측정", text: "모든 수정에 자체 익스플로잇 테스트가 딸려 있다" },
      {
        t: "ul",
        items: [
          "피해자의 이체 서명을 공격자 지갑으로 재사용하는 시도는 이제 거부된다(\"not sent by owner\"); 정상 스테이킹·초과 청구·추론 결제는 변함없이 동작한다.",
          "한 잔액에 대한 동시 claim 3건은 **정확히 한 번** 지급되고, 이미 지급된 추론의 재제출은 멱등하게 같은 결과를 돌려준다.",
          "무인증 클라이언트의 자칭 `units`·허브/게이트웨이 역할·성능 등급은 이제 보상을 1 lamport도 움직이지 못한다.",
        ],
      },
      {
        t: "p",
        md: "세 수정의 관통선은 하나의 원칙을 세 방향으로 적용한 것이다: **체인이 진실의 원천이고, 서비스는 검증자이며, 모든 잔액에는 정확히 하나의 쓰기 주체가 있다**. 정산 서비스는 아직 Solana devnet에서 돈다 — 메인넷이 판돈을 올리기 전에 이런 클래스들을 찾아 익스플로잇하고 고치기에 정확히 알맞은 곳이다.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — 엔진: 추론엔진의 컨트롤 플레인, linkcpp",
    dek: "추론엔진에는 유능한 RPC 데이터 플레인은 있지만 컨트롤 플레인이 없다. linkcpp가 그 나머지 절반 — 탐색·계획·실행·게이트웨이 — 을 기본 바이너리 둘레에 더한다.",
    blocks: [
      {
        t: "p",
        md: "Kvasir가 돌아가는 모든 것은 여기서 시작한다. **linkcpp**는 추론엔진의 RPC 데이터 플레인을 감싸는 소스 공개 컨트롤 플레인(Business Source License)이다: *기본(stock)* `ggml-rpc-server` / `llama-server` 바이너리만으로 대형 AI 모델을 여러 GPU와 머신에 걸쳐 실행한다. 데이터 플레인은 포크되지 않는다 — linkcpp가 더하는 것은 전부 오케스트레이션이다.",
      },
      { t: "h2", kick: "빈틈", text: "컨트롤 플레인 없는 데이터 플레인" },
      {
        t: "p",
        md: "추론엔진는 이미 RPC로 모델을 여러 머신에 쪼갤 수 있다 — 하지만 누군가는 GPU를 탐색하고, 어느 레이어를 어디에 둘지 정하고, 알맞은 예산으로 알맞은 워커를 실행하고, 모든 노드가 같은 프로토콜을 쓰는지 검사하고, 개발자가 실제로 호출할 API를 노출해야 한다. 클러스터 하나면 손으로도 하지만, 낯선 이들의 기기로 이루어진 열린 네트워크에선 불가능하다. 그 조율 계층이 linkcpp다.",
      },
      { t: "h2", kick: "아키텍처", text: "하나의 허브, 기본 워커, 표준 게이트웨이" },
      {
        t: "code",
        caption: "요청 흐름 — 허브가 조율하고, 기본 바이너리가 계산한다.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of stock 추론엔진 engines below" },
      {
        t: "ul",
        items: [
          "**머신이 참여하는 세 가지 방법:** VRAM/RAM/CPU 예산을 편집할 수 있는 고정 **로컬 노드 슬롯**; 다른 허브를 등록해 노드를 가져오는 **원격 유닛**; 그리고 평범한 요청/응답 HTTP로 참여하는 워커 전용 서비스 **관리형 노드 에이전트** — 의도적으로 지속 스트림이 아니라서 단순한 LAN/VPN 라우팅에서도 살아남는다.",
          "**호환성 게이팅은 일급 개념:** 모든 유닛·노드·에이전트가 프로토콜/런타임 팩 정체성과 백엔드 상세를 보고한다. 유닛·런타임 팩·추론엔진 리비전·RPC ABI 불일치는 **bind/plan/load/infer 전에 하드 블록**되고, 백엔드 차이(CUDA/Metal/Vulkan/CPU)는 거부가 아닌 capability로 추적된다.",
          "**플래너**는 GGUF 메타데이터를 읽어 노드별 연속 레이어 배치, `--tensor-split`, KV 캐시/레이어/전문가 VRAM 추정치, 그리고 선택적 전문가-FFN RAM offload를 만든다.",
          "**게이트웨이:** 모든 컨트롤러가 OpenAI 호환(`/v1/chat/completions`, `/v1/responses`, `/v1/models`)과 Anthropic 호환(`/anthropic/v1/messages|models`) 엔드포인트를 같은 로드된 모델로 노출한다 — 기존 클라이언트가 수정 없이 동작한다.",
        ],
      },
      {
        t: "p",
        md: "이 의도적 분리 — 오픈 컨트롤 플레인 아래의 무수정 데이터 플레인 — 가 이후 모든 것의 토대다: 링 런타임, 레이어 마켓, 그리고 결국 전문가 샤딩 스웜까지, 전부 같은 기본 컴퓨트 위의 컨트롤 플레인 진화다.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — 링: 마스터 없는 파이프라인 추론",
    dek: "모든 기기는 자기 레이어 구간만 로드하고 작은 hidden-state 경계를 이웃에 넘긴다. 어떤 노드도 모델을 다 갖지 않고, 중앙 마스터도 없다.",
    blocks: [
      { t: "h2", kick: "왜 스타형이 아닌가", text: "RPC 마스터는 병목이자 관문지기" },
      {
        t: "p",
        md: "고전적인 RPC 토폴로지에서는 마스터 하나가 **전체 GGUF**를 열고 모든 워커에게 접속한다. 이 형태는 열린 네트워크에서 세 가지로 깨진다: 마스터가 전체 체크포인트를 보유·서빙해야 하고, 모든 워커가 접속 가능해야 하며 — 캐리어 NAT 뒤의 폰은 불가능 — 마스터는 소유자가 없어야 할 네트워크의 단일 소유자가 된다.",
      },
      { t: "h2", kick: "링", text: "레이어 구간 + 경계 전달" },
      {
        t: "ul",
        items: [
          "모든 기기는 같은 모델을 저장하되 **연속된 자기 레이어 구간만 로드**하고, 선행자와 후행자에게 정확히 두 개의 링크를 연다.",
          "요청이 링에 들어오면 각 노드는 자기 레이어를 실행하고 **hidden-state 경계**만 이웃에 넘긴다. 마지막 rank가 토큰을 샘플링해 되돌려보낸다 — 중앙 마스터 없음, 어떤 노드도 모델 전체를 갖지 않음.",
          "배치는 플래너의 **rank manifest**에서 나온다 — Qwen3.5-122B라면 49개 레이어를, 나타나는 GPU·CPU·NPU·폰의 어떤 조합에든 분산.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "약한 기기를 진짜 구성원으로", text: "부분 샤드, 모바일 GPU, 443 릴레이" },
      {
        t: "ul",
        items: [
          "**부분 샤드 다운로드:** 링 스테이지에 필요한 것은 체크포인트가 아니라 자기 구간이다. 스테이지 mini-GGUF는 그 텐서들만 담고(77.6 GB 전체 모델 대비 **26개 텐서 254 MB**), 폰은 1-레이어 구간에 ~1.5 GB만 받으면 된다.",
          "**모바일 GPU 경로:** 폰 GPU로의 RPC 경로는 불가능했지만(Adreno의 OpenCL 버퍼 레이아웃이 RPC 직렬화를 못 견딤), **링 스테이지는 Adreno GPU에서 직접 돈다** — 스테이지가 백엔드를 로컬로 소유하므로 회선을 건너는 것은 경계뿐이다.",
          "**NAT 우회:** 폰은 인바운드 연결을 못 받으므로 데이터 플레인이 **443 릴레이**를 지난다 — 1바이트 role preamble을 쓰는 엣지별 WebSocket 브리지로 양쪽이 아웃바운드 접속한다. 폰은 인바운드 포트를 하나도 열지 않는다.",
          "**Self-enrollment 마켓:** 스테이지는 배정되는 게 아니라 쟁취된다. 노드가 커버리지/수요 맵을 폴링해 **최대 보상** 미충족 구간을 고르고, 그 구간만 내려받아 참여한다 — NAT 뒤의 폰이 링 추론을 완주하고 기여 보상까지 받는 것으로 끝에서 끝까지 검증됐다.",
        ],
      },
      { t: "h2", kick: "링의 자리", text: "저지연 경로" },
      {
        t: "p",
        md: "링은 Kvasir의 **지연** 경로다: 경계는 작고 홉은 적으며, 디코드는 아무것도 중앙에 모으지 않고 루프를 돈다. 한계는 알갱이 크기다 — 노드가 질 수 있는 최소 단위가 레이어(122B 기준 ~1.4 GB)다. 그 바닥을 없애는 것이 전문가 샤딩 스웜이고, 링은 스웜이 꽂히는 서빙 백본으로 남는다.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — 122B MoE 해부: 가중치는 왜 쪼개지고 싶어 하는가",
    dek: "Qwen3.5-122B의 텐서 수준 분석: 바이트의 86%가 독립 전문가 슬랩 12,544개이며, 각각 바이트 레인지 복사 한 번이면 홀로 선다.",
    blocks: [
      {
        t: "p",
        md: "무언가를 설계하기 전에, 우리는 122B를 디스크 위에서 해부했다. 질문: 약한 기기들의 스웜이 이 모델을 지려면, 자연스러운 운반 단위는 무엇인가? 답은 GGUF 텐서 레이아웃 자체에서 떨어졌다.",
      },
      { t: "h2", kick: "해부 · Qwen3.5-122B-A10B (Q4_K_M)", text: "MoE 레이어는 실제로 무엇으로 만들어지는가" },
      {
        t: "stats",
        items: [
          { n: "49", l: "레이어" },
          { n: "256", l: "전문가 / 레이어" },
          { n: "8", l: "토큰당 활성" },
          { n: "12,544", l: "전문가 총수" },
          { n: "5.3 MB", l: "전문가 1개 (Q4)" },
          { n: "86%", l: "무게 중 전문가 비중" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "전체 체크포인트" },
        ],
      },
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "각 레이어는 **dense 경로** — 어텐션 + KV, norm들, 라우터(`ffn_gate_inp`), shared expert — 와 세 개의 적층 텐서(`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`)로 저장되는 **전문가 뱅크**(독립 FFN 256개)로 나뉜다. dense 경로는 바이트의 소수이고, 전문가 뱅크가 모델의 86%다.",
      },
      { t: "h2", kick: "레이아웃의 선물", text: "전문가는 연속된 블록 정렬 슬랩이다" },
      {
        t: "ul",
        items: [
          "전문가 인덱스는 모든 전문가 텐서의 **최외곽 ggml 차원**(`ne[2]`)이다 — 전문가 *e*는 원시 양자화 바이트의 연속·양자블록 정렬 슬랩 하나를 차지한다.",
          "그래서 전문가 단위 추출은 **바이트 레인지 복사**다: `data[a:b]`, dequant 없음, re-pack 없음 — expert-sliced mini-GGUF는 만들기 싸고 bit 단위로 충실하다.",
          "토큰당 레이어마다 라우터가 고른 **256개 중 8개**만 켜진다 — 디코드 시 레이어의 전문가 트래픽은 hidden 벡터 하나에 대한 소수의 작은 행렬곱이다.",
        ],
      },
      { t: "h2", kick: "함의", text: "운반 단위가 1.4 GB에서 5.3 MB로" },
      {
        t: "p",
        md: "레이어 단위에서 노드가 질 수 있는 최소는 ~**1.4 GB** — 앱·KV·OS가 제 몫을 가져가면 대부분의 폰엔 무리다. 전문가 단위에서 단위는 **5.3 MB**이고, 현실적인 기여는 전문가 8–64개(**42–340 MB**) — 어떤 최신 기기에도 넉넉히 들어간다. 전문가들은 상호 독립적이라 소유권을 임의로 흩뿌리고 자유롭게 재조정할 수 있다. 이 분석이 전문가 수준 샤딩을 설계의 베팅으로 만들었다: 가중치는 이미 스웜 크기 단위로 포장돼 있었고 — 네트워크는 그 포장을 존중하기만 하면 됐다.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — 백본 전문가 RAM Offload (M0)",
    dek: "MoE 전문가 FFN을 VRAM 대신 CPU RAM에서 스트리밍하면, 64 GB 코디네이터 한 대가 122B를 담는다 — 그래프 수술 없이.",
    blocks: [
      {
        t: "p",
        md: "전문가 FFN이 VRAM에 살아야 할 이유는 없다. CPU RAM에서 스트리밍하면 전문가가 VRAM을 초과하는 모델을 코디네이터 한 대가 담을 수 있다 — 약한 노드가 큰 MoE에 참여할 수 있게 하는 토대다.",
      },
      { t: "h2", kick: "플래너 검증 · 실제 122B GGUF", text: "122B가 단일 64 GB 코디네이터에 들어간다" },
      {
        t: "p",
        md: "이전의 링은 가중치를 VRAM 전용으로 배치해 122B(77.6 GB)가 64 GB GCD에 **infeasible**이었다. 전문가-offload 규칙을 켜면 dry-run이 **feasible**로 돌아온다:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "122B 링 플랜" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (전문가)" },
          { n: "10", l: "offload된 레이어" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "플래너 출력 — 추론엔진 -ot 규칙 형식.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # 추론엔진 -ot format`,
      },
      { t: "h2", kick: "무엇을 배선했나 · 순수 Python, C++ 재빌드 없음", text: "플래너의 offload 규칙을 실제 로드까지 나르기" },
      {
        t: "ul",
        items: [
          "**planner** — 각 placement에 이미 `ot`(콤마 결합 `-ot` 규칙)를 생성.",
          "**protocol.py** — `StageStartRequest.ot` 필드 추가.",
          "**runtime.py** — placement의 `ot`를 스테이지 요청에 전달.",
          "**stage_service.py** — 코디네이터가 `--override-tensor`로 실행.",
          "`linkcpp-server`는 미지의 인자를 기본 llama-server로 전달하므로 `-ot`가 그대로 적용된다.",
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
        md: "`ot` 프로토콜 왕복 테스트와 코디네이터 명령의 `--override-tensor` 방출 확인으로 검증했고, 허브는 회귀 없이 재배포됐다. 당시 남은 것: 전체 2-노드 77 GB 로드(offload 코디네이터 + ~1.5 GB 1-레이어 구간의 폰), 서버 가용성 대기. M0의 핵심 — 약한 노드가 큰 MoE에 참여하게 하는 백본 offload — 는 코드·플래너 수준에서 완결됐다.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — 전문가 슬라이스 데이터 경로 (M1)",
    dek: "약한 기기가 1.4 GB 레이어가 아니라 6 MB짜리 전문가 몇 개만 내려받는다 — 그리고 샤딩 계산은 monolithic과 3.6e-12까지 일치한다.",
    blocks: [
      { t: "h2", kick: "검증 · 실제 Qwen3.5-122B-A10B", text: "전문가 슬라이스는 바이트 복사 — dequant 없음" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "전문가 차원 슬라이스" },
          { n: "~6.1", l: "MB / 전문가 (Q4+Q6)" },
          { n: "206 MB", l: "2층 × 16전문가 다운로드" },
          { n: "200", l: "HTTP, 유효한 GGUF" },
        ],
      },
      {
        t: "p",
        md: "MoE 전문가 텐서는 모든 전문가를 최외곽 ggml 차원에 쌓으므로, 리더는 `(n_expert, rows, row_bytes)`의 원시 양자화 바이트를 노출한다. 전문가 *e*는 양자블록 정렬 연속 슬랩이다 — 슬라이스는 문자 그대로 `data[a:b]`이며, dequant도 re-pack도 없다.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — 검증된 라운드트립.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "수치 oracle", text: "dispatch + combine == monolithic, 정확히" },
      {
        t: "p",
        md: "실제 122B layer-0 전문가(dequant 레퍼런스)로, 전문가를 4개 샤드로 나눠 따로 계산하고 combine한 결과가 **monolithic MoE FFN과 일치**한다: 샤딩은 같은 가중합의 정확한 재그룹핑이지, 근사가 아니다.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "상대 오차" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "관여한 전문가" },
        ],
      },
      { t: "h2", kick: "C++ 워커, 하드웨어 검증", text: "linkcpp-expert-worker가 ROCm에서 oracle을 재현" },
      {
        t: "ul",
        items: [
          "**순수 ggml/gguf**(libllama 없음): 슬라이스를 GPU 백엔드에 로드하고 `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`을 실행.",
          "**ROCm 빌드 + 실행**(MI250): 122B layer-0, 전문가 [0,8), 4토큰.",
          "**oracle 대비 cosine 0.99995**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — 이 잔차 자체가 교차 백엔드 동등성(ROCm vs numpy)의 첫 실측 사례다.",
          "같은 코드 경로가 CUDA/Metal/Vulkan/CPU를 커버한다(`mul_mat_id`/`swiglu`는 기본 ggml; CUDA엔 전용 MoE 커널).",
        ],
      },
      {
        t: "p",
        md: "가장 어렵고 위험한 조각 — 온디바이스 워커 커널 — 이 여기서 검증됐다. 남은 것은 백본↔워커 오케스트레이션이었고, 워커는 이 슬라이스를 소비하는 검증된 순수 함수다.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — 분산 전문가 Dispatch (M2)",
    dek: "라이브 122B 디코드가 한 레이어의 전문가 계산을 TCP 너머 별도 워커 프로세스에 넘긴다 — 그리고 정확히 같은 토큰을 예측한다.",
    blocks: [
      { t: "h2", kick: "검증 · 실제 122B, 두 프로세스", text: "백본 decode → TCP → 워커 → experts → 같은 토큰" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "로짓 cosine" },
          { n: "0", l: "전송 손실 (byte-identical)" },
          { n: "2", l: "프로세스 (백본 + 워커)" },
        ],
      },
      {
        t: "p",
        md: "전문가 워커가 layer-0 슬라이스를 **별도 프로세스**(ROCm)로 서빙하고, 122B 백본의 `build_moe_ffn` dispatch 콜백이 `(cur, sel)`을 TCP로 보내 전문가 출력을 받는다. 로짓 cosine은 **in-process 값과 정확히 같다**(0.99868775) — 전송이 무손실이다. 전문가-병렬 스웜 계산이 프로세스 경계를 넘어 작동한다.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "하나의 long-lived TCP 연결 — 링/443 릴레이가 터널링할 수 있는 바로 그 스트림.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "완료", text: "분산 dispatch 파이프라인" },
      {
        t: "ul",
        items: [
          "`--serve` 모드: 슬라이스 로드, TCP listen, `(n_used, n_tokens, cur, sel) → experts` 응답.",
          "`--dispatch-port`: 백본 콜백이 in-process 계산 대신 별도 워커와 TCP로 송수신.",
          "layer-0을 프로세스 밖으로 dispatch한 라이브 122B 디코드에서 실측 → **argmax MATCH**, cosine 0.99869(= in-process, 무손실).",
          "M2 코어(먼저): 폰의 ARM이 실제 122B 전문가를 cosine 0.99992로 계산(Android 크로스빌드).",
        ],
      },
      {
        t: "p",
        md: "다음 단계: 같은 TCP 스트림을 **443 릴레이**로 터널링해 다른 머신·폰의 워커로(전송은 링 작업에서 이미 검증됨), 그다음 M3 커버리지 마켓과 M4 배치 처리량.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — 전문가 커버리지 마켓 (M3)",
    dek: "약한 노드가 어떤 (레이어, 전문가 범위)가 가장 희소하고 보상이 큰지 보고 스스로 채운다 — 검증된 레이어 마켓을 더 잘게.",
    blocks: [
      {
        t: "p",
        md: "Kvasir의 레이어 샤드 마켓 — 수요 맵, 최대보상 self-enrollment, 부분 다운로드, 노드별 보상 — 은 이미 실기기 검증됐다. M3는 같은 메커니즘을 **(레이어, 전문가 범위)** 단위로 재파라미터화해, 커버리지가 가장 복제 부족하고 보상 큰 전문가 범위를 향해 자가 치유되게 한다.",
      },
      { t: "h2", kick: "검증 · API", text: "희소성 집계 → 최대보상 범위 배정" },
      {
        t: "p",
        md: "워커 3개가 layer 0에 등록한다: A = [0,128), B = [128,256), C = [0,128) 2차 복제본, `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["레이어", "전문가", "복제본", "희소성"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (목표 달성)"],
          ["0", "[128, 256)", "1", "0.5 (목표 미달)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → 가장 희소한 범위를 노드 예산에 맞게 클립.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "완료 · 순수 Python (허브)", text: "전문가 단위의 수요/공급 마켓" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — 워커가 (레이어, 전문가 범위) 보유를 heartbeat.",
          "`GET /api/expert-demand` — 전문가별 복제본 집계 → 희소성 점수가 붙은 연속 전문가 범위 세그먼트.",
          "`POST /api/expert-volunteer` — 가장 희소한 범위를 노드 예산에 맞춰 배정.",
          "기존 레이어 마켓(self-enroll · 부분 다운로드 · 보상)을 (레이어, 전문가 범위)로 재파라미터화.",
        ],
      },
      {
        t: "p",
        md: "M4에서 이어질 것: 동시 요청의 배치 dispatch + hot-expert 캐시 — 워커 수에 비례하는 tokens/s — 그리고 복제본 라우팅(가장 가깝고 빠른 워커)과 churn fallback.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — 배치 Dispatch 처리량 (M4)",
    dek: "스웜은 지연 게임이 아니라 처리량 패브릭이다: dispatch 호출을 배치로 묶으면 요청당 오버헤드가 토큰당 77배 상각된다.",
    blocks: [
      { t: "h2", kick: "실측 · ROCm, 전문가 FFN, n_used = 8", text: "배치가 클수록 워커당 tok/s가 커진다" },
      {
        t: "table",
        head: ["배치", "워커당 tok/s"],
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
        md: "배치 1의 **1.45 ms/tok**에서 배치 512의 **0.019 ms/tok**으로 — 토큰당 77배 개선이다. 호출당 시간은 거의 그대로인데(1.45 → 9.6 ms) 배치는 512배로 커졌다 — GPU가 고정 오버헤드 뒤에서 배치를 거의 공짜로 처리한다. 이것이 expert-parallel을 실용화하는 **처리량-패브릭 성질**이다: 배치 dispatch가 요청당 RTT와 오버헤드를 상각한다.",
      },
      { t: "h2", kick: "완료", text: "배치 dispatch 처리량" },
      {
        t: "ul",
        items: [
          "워커 `--bench`: 배치 1…512의 compute_dispatch 타이밍 → tok/s.",
          "배치 512(ROCm)에서 워커당 **53k tok/s** — 배치가 오버헤드를 상각.",
          "이 위에 hot-expert 캐시와 다중 워커 집계 스케일링(복제본 라우팅)이 쌓인다.",
        ],
      },
      {
        t: "callout",
        md: "M4로써 **M0 → M4 전체 파이프라인이 실제 122B에서 실증**됐다: 백본 offload · 전문가 슬라이스 · 검증된 워커 · 라이브 디코드 dispatch(argmax MATCH) · 분산 프로세스 · 커버리지 마켓 · 배치 처리량.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "폰이 122B 추론에 참여했다",
    dek: "Galaxy S25가 허브에서 자기 전문가 슬라이스를 자율 다운로드하고, 라이브 122B 디코드의 매 스텝마다 한 레이어의 전문가를 계산했다. 출력은 정답이었다.",
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → 생성 결과(폰 참여): **\" Paris.\"** — 로컬 실행과 8/8 토큰 동일.",
      },
      { t: "h2", kick: "실측 · 실제 122B, 폰이 layer-0 계산", text: "정확성 + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "토큰 로컬과 동일" },
          { n: "4.01", l: "TPS 로컬 (기준)" },
          { n: "3.13", l: "TPS 폰 참여" },
          { n: "1.58 GB", l: "자율 다운로드" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "폰이 매 토큰마다 layer-0의 전문가를 계산해도 **생성 토큰은 로컬과 완전히 동일** — 정답 \"Paris.\"다. TPS는 4.01에서 3.13으로 떨어진다 — 폰 dispatch 왕복(MI250 → 터널 → 폰, 토큰당 ~100 ms)의 22% 비용이다. 처리량은 배치와 복제본으로 회복된다(M4).",
      },
      { t: "h2", kick: "자율 참여 흐름", text: "탐색 → 보상 기반 다운로드 → 계산 참여" },
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
      { t: "h2", kick: "검증된 것 vs 남은 것", text: "메커니즘은 완결, 앱 내 루프는 프로덕션화" },
      {
        t: "ul",
        items: [
          "부분 다운로드(expert-shard 엔드포인트), 워커 서빙, 백본 dispatch, 라이브 122B 생성과 TPS — 전부 실기기에서 검증.",
          "정확성: 폰이 참여해도 8/8 토큰이 로컬 실행과 같고, 답도 정답.",
          "남은 것: 앱 내 자율 루프(expert-demand 폴링 → volunteer → 다운로드 → serve → 등록)는 Kotlin 배선 — 이 데모는 메커니즘을 직접 구동했다.",
          "전송: 이 데모는 SSH 터널을 썼고, 프로덕션은 443 릴레이를 쓴다(링 작업에서 이미 검증됨).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "Kvasir 경제: 비용과 보상의 선순환",
    dek: "탈중앙 추론 네트워크는 소비자가 내는 가격과 노드가 버는 보상이 서로를 강화할 때만 작동한다. 우리가 향해 짓고 있는 플라이휠, 그것을 죽이는 악순환, 그리고 바퀴를 계속 돌리는 세 불변식을 소개한다.",
    blocks: [
      {
        t: "callout",
        md: "**테제:** Kvasir는 하나의 토큰으로 정산되는 양면 시장이다 — 소비자는 추론하려 KVR을 내고, 노드는 서빙해 KVR을 번다. 전체 설계의 성패는 하나의 성질에 달렸다: 이 두 면이 **선순환**을 이뤄, 한 바퀴가 다음 바퀴를 더 쉽게 만들어야 한다. 이를 그르치면 어떤 가격 정책도 결국 무너지고, 제대로 하면 네트워크는 *커질수록* *더 싸진다*.",
      },
      {
        t: "p",
        md: "비용과 보상을 줄다리기로 보기 쉽다 — 소비자가 아끼는 1달러는 노드가 못 버는 1달러라고. 그 프레임은 함정이다. 건강한 네트워크에서 둘은 양 끝에서 본 **같은 플라이휠**이다: 결제가 보상이 되고, 보상이 공급이 되고, 공급이 용량과 더 낮은 가격이 되고, 낮은 가격이 더 많은 사용이 되고, 더 많은 사용이 더 많은 결제가 된다. 문제는 고정된 파이를 어떻게 나누느냐가 아니라, 파이가 자라도록 바퀴를 어떻게 계속 돌리느냐다.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "플라이휠", text: "사용량과 공급이 함께 자라는 이유" },
      {
        t: "p",
        md: "이 순환의 엔진은 Kvasir에서 이미 참인 단 하나의 규칙이다: **추론은 반드시 KVR로 지불된다**. 그래서 모든 사용량이 토큰에 대한 실수요 한 단위가 된다 — 투기가 아니라 유틸리티다. 토큰 수요가 노드가 버는 KVR의 가치를 떠받치고, 매력적인 보상이 공급을 끌어오고, 공급이 용량을 키우며 경쟁과 더 세밀한 전문가 샤딩을 통해 서빙의 한계 비용을 낮추고, 더 싸고 빠르고 유능한 서비스가 더 많은 사용을 끌어온다. Kvasir는 어떤 중앙화 API도 복제할 수 없는 성질로 루프를 더 조인다: 참여자가 **소비자이자 공급자를 동시에** 될 수 있다는 점이다. 수요 측과 공급 측이 흔히 *같은 사람들* 안에서 자라며, 일면 시장을 망가뜨리는 불균형을 완화한다.",
      },
      { t: "h2", kick: "실패 모드", text: "바퀴를 거꾸로 돌리는 네 악순환" },
      {
        t: "p",
        md: "플라이휠은 올라가는 만큼 쉽게 내려갈 수도 있다. 죽음의 악순환에 이름을 붙이는 것이 그것들을 막도록 설계하는 방법이다:",
      },
      {
        t: "table",
        head: ["악순환", "어떻게 시작되나", "어디서 끝나나"],
        rows: [
          ["보상 희석", "더 많은 노드가 정체된 수요를 쫓음", "노드당 보상 하락, 노드 이탈, 용량 감소"],
          ["가격-과소", "싼 가격, 노드 비용 이하의 보상", "서빙이 수지 안 맞아 공급·품질 붕괴"],
          ["가격-과다", "좋은 보상이지만 시장 이상", "사용자가 더 싼 API로, 수익 고갈"],
          ["emission 의존", "보상을 수익이 아닌 발행으로 지급", "인플레이션이 KVR을 잠식해 양면이 포기"],
        ],
      },
      { t: "h2", kick: "불변식", text: "순환을 선하게 유지하는 세 규칙" },
      {
        t: "ul",
        items: [
          "**보상은 실제 수익으로 지급된다.** 정상 상태에서 노드가 버는 것은 소비자가 내는 것에서 나온다 — 무한정한 토큰 emission이 아니다. emission은 수수료 수익이 자라며 *축소(taper)*되어야 하는 부트스트랩 보조금이다. Kvasir는 여기서 이미 도움을 준다 — **실제 작업**에 보상하기 때문이다(단순 참여가 아니라 실제로 서빙한 토큰 × 레이어 몫당 KVR). 그래서 보조금이 유휴 '용병' 노드로 새지 않는다.",
          "**KVR은 필수 매개다.** KVR을 내지 않고는 추론할 수 없으므로, 사용은 토큰의 영구적 수요 흡수원이다. 이것이 토큰 가치를 투기가 아닌 실제 유틸리티에 닻 내린다 — 화폐와 칩의 차이다.",
          "**가격은 밴드 안에서 부동한다.** 노드 한계 비용 위로 유지된 하한이 서빙을 수지맞게 하고, 중앙화 대안 아래로 유지된 상한이 Kvasir의 경쟁력을 지킨다. 그 사이에서 가격이 움직이는데, 바로 거기서 네트워크의 성장이 마침내 더 낮은 비용으로 나타난다.",
        ],
      },
      { t: "h2", kick: "온도조절기", text: "\"더 많은 노드 → 더 저렴\"을 코드로 참이 되게" },
      {
        t: "p",
        md: "오늘 가격은 통제된 상수다 — devnet에는 합당하지만, 노드를 더해도 *용량*이 늘 뿐 저렴함은 늘지 않는다는 뜻이다. 설계 방향은 **가동률 주도 가격**이다: 유휴 공급은 가격을 하한 쪽으로 밀고, 혼잡은 상한 쪽으로 민다. 그 단일 신호가 *\"컴퓨트를 나눠 쓰는 사람이 많을수록 더 싸진다\"*는 직관을 프로토콜이 강제하는 규칙으로 바꾼다 — 그러면서 하한이 운영자를 흑자로 유지해, 저렴함을 만든 공급이 증발하지 않게 한다. 가격은 민감한 경제 파라미터이므로, **genesis 지갑 권한 아래 지갑 서명 + 2FA**로만 바뀌고 어쩌다 마주친 환경 변수로는 절대 바뀌지 않는다.",
      },
      {
        t: "callout",
        md: "**\"공짜\"는 가격이 아니라 순액이다.** 추론한 만큼 내고 서빙한 만큼 벌며, 소비하는 만큼 대략 기여하면 청구서가 0으로 상쇄된다. 어떤 구독 API도 — Claude Max, Codex 좌석 — 이걸 줄 수 없다. 당신은 결코 그들의 공급 측이 될 수 없기 때문이다. Kvasir에서는 자기 머신이 못 담는 모델을 돌리면서 *동시에* 남들이 자기 모델을 돌리도록 도운 대가를 받을 수 있다.",
      },
      {
        t: "p",
        md: "이 중 어느 것도 별난 메커니즘 설계를 요구하지 않는다. 세 가지에 대한 규율을 요구할 뿐이다: 수익에서 나오는 보상, 사용에서 나오는 가치, 유계 부동 가격에서 나오는 균형. Kvasir는 어렵고 정직한 부분을 이미 배포했다 — 비수탁형 정산, 작업 비례 보상, 네트워크를 쓰려면 실제로 써야만 하는 토큰. 나머지는 경제 로드맵이다: taper, 실패한 추론을 위한 보험 풀을 대는 수수료 분배, 그리고 온도조절기. 그 순서로 지으면 비용과 보상은 싸움을 멈추고 복리로 불어나기 시작한다.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "인터넷 너머의 GPU가 122B 추론에 참여했다",
    dek: "다른 도시의 Blackwell 워크스테이션이 아웃바운드 443 연결 하나를 걸어 라이브 122B 디코드의 전문가를 계산했다 — 로컬 실행과 바이트-동일, 그리고 한 일에 대해 KVR로 지불받았다.",
    blocks: [
      {
        t: "callout",
        md: "**무슨 일이 있었나:** 한 곳의 AMD 백본에서 도는 122B 디코드가 토큰별 전문가 작업을 다른 도시의 NVIDIA GB10(Grace Blackwell) 머신으로 보냈고 — 포트 443의 단일 아웃바운드 WebSocket을 통해 — 로컬 계산과 **정확히 같은 토큰**을 만드는 전문가 출력을 돌려받았다. 터널도, 포트 포워딩도, 인바운드 방화벽 구멍도 없다. 원격 머신은 서빙한 바이트에 대해 KVR을 벌었다.",
      },
      {
        t: "p",
        md: "Kvasir의 전제는 *어떤 하드웨어든 오는 대로*다 — 캐리어 NAT 뒤, 공개 인터넷 위, 다른 도시에 있는 하드웨어까지 포함해. Qwen3.5-122B-A10B는 **무게의 86%를 독립 전문가 12,544개**(48 레이어 × 256, top-8)에 담고, 각각은 5.3 MB 순수 함수다. 그 알갱이가 멀리 있는 무관한 머신이 슬라이스를 쥐고 기여하게 한다. 열린 질문은 결코 *쪼갤 수 있는가*가 아니었다 — *열린 인터넷 너머의 워커가 라이브 디코드에 실제로, 정확하고 회계 가능하게 참여할 수 있는가*였다. 이제 참여했다.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "아웃바운드 접속 하나", text: "터널도, 인바운드 포트도 없다" },
      {
        t: "p",
        md: "원격 워커는 **하나의** 연결을 연다 — 443의 공개 게이트웨이로 향하는 아웃바운드 `wss://`, 캐리어 NAT와 CDN 엣지가 확실히 통과시키는 유일한 포트다. 게이트웨이는 스트림을 파싱하지 않고, WebSocket을 LAN 전용 허브로 **raw-splice**하고, 허브는 이를 백본의 expert-dispatch 리스너로 브리지한다. 양 끝이 바깥으로 접속해 중간에서 만났다. 워커는 인바운드 포트를 하나도 노출하지 않고 공개 주소도 필요 없다.",
      },
      {
        t: "code",
        caption: "두 개의 아웃바운드 접속을 하나의 평범한 dispatch 스트림으로 splice.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "인터넷 너머에서 바이트-동일", text: "라우터가 한 번 결정하고, 계산은 정확히 재그룹핑된다" },
      {
        t: "p",
        md: "백본은 라우터를 **한 번**, 권위를 갖고 실행하고, 워커는 순수 `(hidden, ids) → out` 함수다. 그래서 그 함수를 대륙 너머로 옮겨도 곱셈이 *어디서* 일어나는지가 바뀔 뿐, *무엇을* 계산하는지는 아니다. layer-0 전문가를 원격으로 서빙한 라이브 122B 디코드에서: greedy 토큰 스트림은 **8/8 동일**(\" Paris.\"), 로짓 **cosine 0.99773**, argmax 일치. 이것은 CUDA↔ROCm↔CPU 이산 결정을 불변으로 유지하는 바로 그 라우터-권위 성질이다 — 이종 백엔드는 연속 오차로 유계에 머물 뿐, 파국적 분기는 없다.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "greedy 토큰 동일" },
          { n: "0.99773", l: "로짓 cosine, 원격 vs 로컬" },
          { n: "1.2%", l: "TPS 오버헤드, 직접 (13 ms RTT)" },
          { n: "0.00895", l: "워커에게 준 KVR, 첫 원격 세션" },
        ],
      },
      { t: "h2", kick: "정직한 비용은 RTT", text: "스웜이 저지연 디코더가 아니라 처리량 패브릭인 이유" },
      {
        t: "p",
        md: "직렬 토큰별 dispatch는 스텝마다 왕복을 치른다. 실측: 직접 링크(13 ms RTT)에서 처리량 오버헤드는 **1.2%**(4.220 → 4.169 tok/s)였고, 443의 CDN 엣지를 경유하면 **~28%**였다. 이를 정직하게 공개하는 것은 설계의 진실을 가리키기 때문이다 — WAN 스웜은 **RTT 바운드**라, 강점은 한 스트림의 지연이 아니라 **총 용량**이다. 배치가 왕복을 상각한다: 배치 전문가 dispatch는 배치 512에서 **토큰당 처리량의 77배**에 이른다. 바이트는 여유분이고, 왕복이야말로 숨겨야 할 것이다 — 이는 짝을 이루는 로드맵 글의 주제다.",
      },
      { t: "h2", kick: "딱 한 일만큼 지불", text: "계량된 바이트가 KVR이 된다" },
      {
        t: "p",
        md: "회계되지 않는 참여는 무가치하다. 릴레이는 **세션당 브리지된 바이트를 계량**해 허브의 기여 원장에 넣고, 게이트웨이는 그 원장을 폴링해 워커의 **자기** 지갑에 KVR을 델타 적립한다 — 다른 모든 것과 마찬가지로 비수탁형이다. 첫 인터넷 횡단 세션은 실제로 적립됐다: **1.28 MB의 작업 → 1.277952 units → 0.00895 KVR**이 대기 보상으로. 작지만, 바로 그게 핵심이다 — 참여 트로피가 아니라 진짜 작업 단위 정산이다.",
      },
      {
        t: "p",
        md: "같은 아웃바운드-443 경로가 바로 **폰**이 참여하는 방식이다: Galaxy S25가 이미 그 위에서 122B 전문가를 계산했다(8/8 동일 토큰, cosine 0.99992). 프런티어 규모 모델을, 한 곳의 백본과 다른 도시의 데이터센터 GPU와 누군가의 주머니 속 폰이 함께 서빙하며 — 모두 같은 토큰을 내고, 각자 자기 몫만큼 지불받는다. 다음은 WAN 왕복을 싸게 만드는 것이고, 그 로드맵은 남들의 프로덕션 수치와 우리 자신의 실측에 근거한다.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "WAN Dispatch를 싸게 만들기: 근거 있는 로드맵",
    dek: "원격 전문가 dispatch는 작동하고 바이트-동일하다 — 하지만 WAN 디코드는 왕복 바운드다. 비용을 줄이는 계획을, DeepSeek·Petals 등의 프로덕션 수치에 근거해 소개한다(로드맵, 배포된 것 아님).",
    blocks: [
      {
        t: "callout",
        md: "**프레이밍:** *우리가 측정한* 수치는 측정값으로 명시하고, 계획으로 서술된 모든 것은 배포된 결과가 아니라 **로드맵**이다. 목표는 원격 전문가 dispatch — 이미 정확하고 지불되는(짝 글 참조) — 를 가져와, 멀리 있는 GPU나 폰이 느린 구성원이 아니라 일급 스웜 구성원이 될 만큼 WAN 왕복을 싸게 만드는 것이다.",
      },
      {
        t: "p",
        md: "우리 자신의 실측, 담백하게 공개한다: dispatch는 **레이어당 토큰당 약 110 KB** 든다 — 나가는 12.3 KB(dispatch) + 돌아오는 98.3 KB(combine). 8× 비대칭은 선택된 각 전문가가 가중합 *이전에* 자기 전체 출력을 돌려주기 때문이다. 직접 링크로는 처리량 오버헤드 **1.2%**, CDN 릴레이를 통하면 **~28%**다. 이것이 사실이다. 이 글의 나머지는 우리가 그 격차를 어떻게 좁힐 것인가 — 그리고 왜 바이트가 쉬운 쪽인가다.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "지배하는 법칙", text: "WAN 디코드는 왕복 바운드다" },
      {
        t: "p",
        md: "여기서 가장 중요한 공개 결과는 우리 것이 아니라 Petals의 것이다: RTT가 <5 ms에서 100 ms로 가면 디코드가 **1.24에서 0.57 steps/s**로 떨어지는 반면, **대역폭을 10× 줄여도 변화는 ~0**이다. 지연이 지배하고, 대역폭은 여유다. 이것이 문제 전체를 재구성한다: 바이트를 깎는 것은 여유분이지만, **왕복을 줄이는 것이 본질**이다. 아래 모든 항목은 왕복 비용을 얼마나 없애는가로 순위 매긴다.",
      },
      { t: "h2", kick: "회선에서 더 싸게", text: "정확성 우선의 바이트 감축" },
      {
        t: "ul",
        items: [
          "**원시 전문가 출력이 아니라 가중 부분합을 돌려준다.** 선형성 덕에 백본의 combine은 어느 쪽이든 정확하지만, 워커가 8개 대신 하나의 합산 벡터를 돌려준다 — 그것이 ~8× combine 감축이며, DeepSeek-V3 / DeepEP가 프로덕션에서 하는 바로 그것이다.",
          "다중 워커에 걸쳐 **직렬 체인이 아니라 병렬 스타**: ΣRTT가 max RTT로 붕괴한다.",
          "**회선에 F16** — 우리는 이미 교차 백엔드 cosine ~0.998을 받아들이므로 F16 전송은 기존 허용오차 안이다; **블록 단위 INT8/FP8은 나중에**, 우리 자신의 argmax/cosine 게이트가 Q4_K_M 가중치 대비 통과시킨 뒤에(Petals는 실제 인터넷에서 품질 손실 없는 INT8을 보였다).",
          "이들을 합쳐 **~110 KB → 토큰당 9–12 KB (~12×)**를 겨냥한다 — 실질적이지만, 병목이 아니라 *여유분*임을 기억하라.",
        ],
      },
      { t: "h2", kick: "왕복 상각", text: "본질: 왕복을 줄이고, 왕복을 숨기고" },
      {
        t: "ul",
        items: [
          "**Speculative decoding**은 여러 토큰을 하나의 왕복으로 바꾼다. 측정된 80 ms WAN에서 손익분기는 겨우 **~1.15–1.2 accepted tokens/step**이라 — 약한 n-gram 추측조차 이긴다(순정 Jacobi는 역효과를 낼 수 있어 기법 선택이 중요하다). 우리 dispatch 프로토콜은 이미 `n_tokens > 1`을 실어 나르므로 회선 변경이 필요 없다.",
          "**게이트웨이에서의 continuous batching**은 동시 요청을 하나의 왕복으로 접고, **slot-affinity prefix caching**은 세션을 같은 복제본에 붙여 둔다.",
          "**지연 은닉**: shared expert는 독립적인 덧셈 항이라, 백본이 원격 왕복 동안 그것을 *로컬로* 계산한다(ScMoE는 PCIe에서 재학습 없이 1.82×를 보고). **hot 전문가는 로컬로** 두고 cold만 원격으로 보낸다(EPLB는 가장 뜨거운 ~32개를 복제해 프로덕션에서 2.54× 디코드 가속).",
        ],
      },
      { t: "h2", kick: "정책 & 굵은 파이프의 미래", text: "피어별 라우팅, 그리고 200 Gb/s가 바꾸는 것" },
      {
        t: "p",
        md: "경로 정책: 공개 라우팅 가능한 피어는 **직접** 경로(1.2% 경로)를 타고, 릴레이는 NAT에 묶인 기기 전용이다. 그리고 넓은 200 Gb/s 링크가 오면 110 KB는 **~4.4 µs**에 직렬화된다 — 위의 감축 이전에도 대역폭 항이 사라지고, 794 MB 슬라이스가 ~32 ms에 전송된다. 하지만 **RTT는 물리이고, 줄어들지 않는다** — 그래서 200 G에서도 speculative decoding과 overlap이 진짜 지렛대로 남는다. 굵은 파이프가 진정 중요해지는 곳은 다중 백본 페더레이션(여러 백본이 하나의 전문가 풀을 공유)과 대역폭 바운드 작업이다: 긴 프롬프트 prefill과 대규모 배치 처리량.",
      },
      {
        t: "callout",
        md: "**정직하게 밝히는 한 가지 단서:** 전송 계층 자체(WebSocket vs QUIC, 마스킹 오버헤드, NAT hole-punching)는 **인용할 외부 결과가 없다** — 무엇이든 주장하기 전에 우리가 직접 측정할 엔지니어링이다. 위의 모든 것은 공개된 프로덕션 수치(DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB)와 우리 자신의 실측에 기댄다; 로드맵 항목이 배포되면 그 수치와 시제가 여기서 갱신된다.",
      },
      { t: "h2", kick: "다음 차례", text: "온보딩 대상" },
      {
        t: "p",
        md: "우리의 dispatch 훅은 `build_moe_ffn` 위에 앉는다 — 추론엔진에서 **43개 MoE 아키텍처가 공유하는 단일 함수**다. 세 불변식은 모델과 무관하다: MoE 수학(routed = Σ wᵢ·Eᵢ(x), 선형), 공유 코드 경로, 그리고 GGUF의 표준 적층 전문가 텐서(최외곽 `ne[2]` → 블록 정렬 슬라이싱). 그래서 새 모델 온보딩은 재설계가 아니라 — 모델별 argmax/cosine 검증 게이트를 한 번 통과하는 것이다.",
      },
      {
        t: "table",
        head: ["모델", "전문가 · 라우팅", "전문가당 (Q4≈)", "shared expert", "상태"],
        rows: [
          ["Qwen3.5-122B (현재 서빙 중)", "256 · top-8", "5.3 MB (실측)", "있음", "프로덕션 중"],
          ["GLM-4.5-Air 106B", "128 · top-8", "~10 MB", "있음", "준비됨 — 첫 후보"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "있음", "준비됨 (훅 검증)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "없음", "준비됨 (훅 검증)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "있음", "준비됨 (deepseek2 그래프)"],
          ["Kimi K2 1T", "384 · top-8", "~25 MB", "있음", "준비됨 (deepseek 계열)"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "없음", "준비됨"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "없음", "준비됨"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "있음", "준비됨 (한 레이어 건너 MoE)"],
          ["MiniMax M3 428B", "128 · top-4", "미정 (GGUF)", "있음", "업스트림 엔진 대기"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "없음", "작동 — GPU 워커만"],
        ],
      },
      {
        t: "p",
        md: "업계는 세밀한(fine-grained) MoE로 수렴하고 있다 — 더 작은 전문가, 더 많은 수, 더 높은 희소성(DeepSeek, Qwen, Kimi, GLM, gpt-oss 모두 이 길로 갔다). 그 방향의 모든 걸음이 스웜의 참여 단위를 더 작게, 희소성 마켓의 알갱이를 더 잘게 만든다. 위의 모델들은 희망 목록이 아니다; 각각은 이미 우리가 프로덕션에서 돌리는 같은 dispatch 훅을 지난다 — 온보딩은 엔지니어링 프로젝트가 아니라 검증 게이트다.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "200G 질문",
    dek: "우리 스웜 허브는 이미 진열대의 부품으로 200 Gb/s 링크를 걸 수 있다 — 하나는 GB10에 내장돼 있다. 굵은 파이프가 분산 MoE에 사 주는 것, 그리고 사 줄 수 없는 단 하나를 소개한다.",
    blocks: [
      {
        t: "callout",
        md: "**전제:** WAN 디코드는 대역폭 바운드가 아니라 RTT 바운드다 — 우리 통신 로드맵은 바이트가 쉬운 쪽임을 보였다. 그렇다면 허브가 200 Gb/s 링크를 얻으면 실제로 무엇이 바뀌나? *용량*에 관해선 거의 전부, *지연*에 관해선 거의 아무것도.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "이미 상자 안에 · ConnectX-7", text: "하드웨어는 미래적이지 않다 — 하나가 우리 GB10 워커 안에 들어 있다" },
      {
        t: "p",
        md: "우리 122B 전문가를 계산하는 GB10 Grace Blackwell은 **200 GbE QSFP 포트 2개를 가진 NVIDIA ConnectX-7**을 보드에 얹고 있다. 이 머신 두 대는 ~$100짜리 QSFP56 DAC 케이블 하나로 직접 연결된다 — 스위치 0개의 200G 2-허브 클러스터다. 여기서 ARM은 일급 시민이다: x86 데이터센터에서 이 NIC들을 돌리는 바로 그 `mlx5` 드라이버 스택이 aarch64에서도 돌고, GB10이 정확히 그것이다.",
      },
      {
        t: "callout",
        md: "**작은 글씨:** GB10은 multi-host 모드에서 PCIe Gen5 x4 링크 2개로 ConnectX-7에 공급한다. 측정된 최대 속도(~185–190 Gb/s)는 **RoCE(RDMA)와 올바르게 매핑된 토폴로지**가 필요하다 — 잘못 매핑된 경로 위의 순진한 TCP는 ~95 Gb/s 이하에 그친다. 굵은 파이프는 케이블만이 아니라 설정으로 산다.",
      },
      { t: "h2", kick: "거리 사다리", text: "200G는 모든 도달거리에서 카탈로그 품목이다" },
      {
        t: "table",
        head: ["도달거리", "부품", "폼 팩터"],
        rows: [
          ["랙 (0.5–3 m)", "QSFP56 DAC 구리", "케이블, ~$100"],
          ["방 (~30 m)", "AOC 액티브 광", "케이블"],
          ["캠퍼스 (2–10 km)", "200G FR4 / LR4 광 모듈", "QSFP56 모듈"],
          ["메트로 (~40 km)", "200G ER4 광 모듈", "QSFP56 모듈"],
          ["리전 (~120 km)", "400G ZR+ 코히어런트, 200G 라인레이트로 운용", "QSFP-DD 모듈"],
          ["장거리 (수백 km)", "캐리어 200G 파장 / DWDM 라인 시스템", "임대 서비스"],
        ],
      },
      {
        t: "p",
        md: "WAN에서 *케이블*은 그냥 표준 싱글모드 광섬유다 — 이미 모든 도시를 잇는 속도 중립적 유리다. 속도는 양 끝의 플러거블 광 모듈에 있고, **OpenZR+는 120 km 너머 200G를 통신 프로젝트가 아니라 스위치에 꽂는 모듈로 만들었다**. 그 너머로는 파장을 임대한다.",
      },
      { t: "h2", kick: "무엇을 사 주나", text: "스웜의 모든 대역폭 항이 사라진다" },
      {
        t: "ul",
        items: [
          "dispatch 페이로드(오늘 ~110 KB/token/layer, 회선 로드맵 후 ~10 KB)는 **마이크로초** 단위로 직렬화된다 — 페이로드 크기가 더 이상 설계 제약이 전혀 아니게 된다.",
          "**전문가 슬라이스가 ~32 ms에 전송**되고(794 MB, 이론값) 122B 모델 전체가 **~3 s**에 동기화된다 — 커버리지 마켓 재조정과 신규 허브 온보딩이 거의 즉각적이 된다.",
          "긴 컨텍스트 prefill — 진정으로 대역폭이 무거운 유일한 단계 — 이 회선 속도로 움직여, 100K-토큰 프롬프트의 첫 토큰 시간이 백본 계산 바운드가 된다.",
          "**배치 dispatch가 회선 한계 없이 스케일한다**: 많은 사용자 스트림에 걸쳐 집계된 전문가 풀 트래픽은 정확히 굵은 파이프가 흡수하는 대역폭 무겁고 지연 관대한 부하다. 이것이 다중 백본 페더레이션 — 여러 허브가 각자 자기 사용자의 KV를 쥐고 하나의 전문가 풀을 공유 — 을 실용적으로 만든다.",
        ],
      },
      { t: "h2", kick: "사 줄 수 없는 것", text: "빛은 서두르지 않는다" },
      {
        t: "p",
        md: "광섬유는 빛을 ~5 µs/km로 나르고, 어떤 대역폭도 그것을 바꾸지 못한다. 13 ms 왕복은 200 Gb/s에서도 13 ms다. 자기회귀 디코드는 그 왕복을 샤딩된 레이어마다, 토큰마다 치른다 — 그래서 시장에서 가장 굵은 파이프로 이어진 허브들 사이에서도 **speculative decoding(왕복당 k 토큰)과 shared-expert overlap(dispatch가 비행 중인 동안 계산)이 필수로 남는다**. 대역폭은 처리량을 사고, 왕복 규율만이 지연을 산다.",
      },
      {
        t: "p",
        md: "그래서 아키텍처는 두 계층으로 정착한다. **허브 계층** — 200G급 링크로 이어진 백본과 hot 전문가, 용량이 사실상 무한한 곳 — 과 **엣지 계층** — 443 릴레이 위의 폰과 소형 기기, 희소성 마켓이 배정한 전문가의 롱테일을 쥔 곳. 굵은 파이프는 첫 계층을 한 대의 머신처럼 느끼게 하고, 릴레이는 둘째 계층을 누구에게나 열어 둔다. 어느 쪽도 다른 쪽을 대체하지 않는다: 그 분할이 *곧* 설계다.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "부하를 받으면 자라나는 스웜",
    dek: "필요할 때만 도움을 빌리는 거대 모델 — MoE 스웜이 이제 트래픽에 맞춰 스스로 규모를 조절한다: 한산할 땐 조밀하고 빠르게, 바쁠 땐 넓고 병렬로.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir는 어떤 단일 머신이 담는 것보다 훨씬 큰 모델을 서빙한다 — 122B 파라미터 Mixture-of-Experts 모델이 코디네이터 하나와 워커 스웜에 걸쳐 실행된다: LAN 위의 GPU, 200 Gb/s 링크 너머의 GPU, 심지어 인터넷 너머로 dial해 들어오는 폰까지. MoE 모델은 각 토큰을 소수의 전문가에게만 라우팅하므로, 언제든 대부분의 가중치는 놀고 있고, 그 유휴 전문가들은 메인 노드 **밖에서** — 그것들을 쥐겠다고 자원한 어떤 하드웨어에서든 — 살 수 있다.",
      },
      {
        t: "callout",
        md: "이번에 새로워진 것은 스웜이 이제 **부하에 맞춰 스스로 규모를 조절한다**는 점이다.",
      },
      { t: "h2", kick: "어떻게 동작하나", text: "한산할 땐 조밀하게, 바쁠 땐 넓게" },
      {
        t: "p",
        md: "트래픽이 가벼울 때 코디네이터는 모든 것을 자기 GPU에서 서빙한다 — 토큰당 가장 빠른 경로, 네트워크 홉 없음. 요청이 쌓이기 시작해 추론 슬롯이 포화되면, 두 가지가 자동으로 일어난다:",
      },
      {
        t: "ul",
        items: [
          "**이미 가진 워커를 다시 투입한다.** 코디네이터는 자기 큐를 지켜본다. 포화 상태에서는 라우팅된 전문가 작업을 **proven** 워커 — 실제로 서빙해 본 적 있는 워커 — 로 계속 흘려보내, 토큰당 지연을 조금 내주는 대신 총처리량을 훨씬 크게 얻는다. 접속만 했을 뿐 한 번도 계산하지 않은 워커에는 부하를 절대 맡기지 않는다; 갓 들어온 워커도 공정한 첫 시도는 받는다.",
          "**허브가 새 워커를 모집한다.** 컨트롤 허브는 같은 포화를 감지해 그 모델 전문가에 대한 \"수요\"를 높인다. 유휴 노드 — 누군가의 주머니 속 폰, 도시 건너편의 여분 GPU — 는 이미 그 수요 마켓을 폴링하고 있다. 수요가 오르는 순간, 이들은 서빙할 전문가 슬라이스를 배정받아 내려받고, dial해 합류한다. 급증이 지나가면 수요가 다시 내려가고 여분 워커는 조용히 빠져나간다.",
        ],
      },
      {
        t: "p",
        md: "아무도 이것을 스케줄링하지 않는다. 어떤 노드도 밀어붙여지지 않는다. 스웜은 부하와 함께 숨 쉰다: 한산할 땐 조밀하고 빠르게, 바쁠 땐 넓고 병렬로 — 그리고 모든 것이 pull 방식이라, 홈 라우터 뒤의 노드에서도 동작한다.",
      },
      {
        t: "p",
        md: "이것이 그 누구도 혼자 소유하지 않는 하드웨어로 조 단위 파라미터 모델을 서빙할 수 있는 네트워크의 형상이다: 유휴 용량은 초대할 가치가 있는 바로 그 순간에, 오직 그때만 초대된다.",
      },
    ],
  },
};
