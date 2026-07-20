/* 한국어 — 위키 항목 번역. 구조(slug·카테고리·블록 순서·코드)는 entries.ts(영어
   원본)를 그대로 따르고, 기술 용어·식별자(KVR, linkcpp, 추론엔진, GGUF, MoE,
   ring runtime, tok/s 등)는 원문 그대로 유지한다. 컴플라이언스 프레이밍(devnet,
   유틸리티 토큰, 비수탁형) 유지. */
import type { WikiTranslation } from "./entries";

export const koWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Kvasir 네트워크",
    summary: "일상 기기들이 오픈 모델을 서빙하고 KVR을 획득하는 탈중앙 AI 추론 네트워크(DePIN).",
    blocks: [
      {
        t: "p",
        md: "**Kvasir**는 탈중앙 AI 추론 네트워크입니다. 대형 오픈 모델을 **linkcpp** 엔진으로 공유 하드웨어에 분산시켜, 어떤 노드도 모델 전체를 갖지 않습니다. 누구나 GPU, CPU, NPU — 심지어 폰까지 — 제공하고, 기기가 실제로 서빙한 레이어나 전문가만큼 **KVR**을 획득합니다. 개발자는 OpenAI/Anthropic 호환 게이트웨이로 네트워크에 접근해 추론당 지불합니다.",
      },
      {
        t: "ul",
        items: [
          "**소스 공개 엔진** — linkcpp는 BSL 라이선스이며(개발·테스트는 무료, 프로덕션 사용에는 라이선스 필요), 그 아래의 추론엔진 데이터 플레인은 수정 없이 검증 가능한 상태로 유지됩니다.",
          "**비수탁형** — 보상은 각 노드 소유자 자신의 Solana 지갑으로 정산되며, 키는 사용자를 떠나지 않습니다.",
          "**실기기 입증** — 122B 모델이 4장의 AMD MI250에 분산 실행됐고, GPU/CPU/NPU/모바일이 섞인 이기종 플릿에서 노드별 기여가 끝에서 끝까지 크레딧됐습니다.",
          "**북유럽 신화에서 딴 이름** — 모든 신의 정수가 모여 태어났으나 누구의 소유도 아니었던, 가장 현명한 존재 Kvasir.",
        ],
      },
      { t: "h2", kick: "하나의 요청, 여러 기기", text: "추론이 흐르는 길" },
      {
        t: "code",
        caption: "모든 홉은 평범한 HTTP/TCP — 분산되는 것은 모델 그 자체입니다.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "역할은 **겹칠 수 있습니다**: 한 머신이 연산 노드·게이트웨이 호스트·허브 호스트를 동시에 맡을 수 있고, 보상은 합산됩니다. 네트워크의 임무는 이 집합체를 한 대의 머신처럼 보이게 하는 것입니다 — 앞에는 하나의 엔드포인트, 뒤에는 수천 대의 불완전한 기기.",
      },
      {
        t: "p",
        md: "현재 네트워크는 **Solana devnet**에서 운영됩니다. KVR은 유틸리티 / 기여 토큰이며 거래 가능한 자산이나 투자 대상이 아니고, 이 페이지의 어떤 내용도 투자 조언이 아닙니다.",
      },
    ],
  },
  hub: {
    title: "허브",
    summary: "컨트롤 플레인: 기기를 탐색하고 레이어 배치를 계획하며 워커를 실행하고 링을 조율한다.",
    blocks: [
      {
        t: "p",
        md: "**허브**는 네트워크의 컨트롤 플레인으로, linkcpp가 단일 Docker 이미지(`controller.hub:app`, 포트 **19000**의 FastAPI 서비스)로 제공합니다. 기기를 탐색하고, 런타임 호환성을 검사하고, 플래너로 배치를 계획하고, 기본 추론엔진 워커를 실행하며, 컨트롤러별 게이트웨이를 노출합니다. 의도적으로 지루한 인프라입니다: 요청/응답 HTTP, 재시작에 안전한 상태, 별난 전송 계층 없음.",
      },
      { t: "h2", kick: "세 개의 입구", text: "머신이 허브에 참여하는 방법" },
      {
        t: "ul",
        items: [
          "**로컬 노드 슬롯** — 허브당 5개의 고정 슬롯, RPC 포트 **50052–50056**에 매핑. 슬롯은 항상 존재하며, 임의 노드를 만드는 대신 슬롯의 GPU + VRAM/RAM/CPU 예산을 편집합니다. 리소스는 **슬롯이 바인딩되지 않은 동안에만** 편집 가능 — 실행 중인 컨트롤러 아래의 용량 계약을 보호합니다.",
          "**원격 유닛** — 실행 중인 다른 linkcpp 허브를 등록하고 그 노드들을 가져옵니다. 데이터 플레인 엔드포인트는 항상 등록된 *유닛* URL + 유닛이 노출한 워커 포트에서 파생되며, 원격 시스템이 광고하는 노드 호스트에서 파생되지 않습니다.",
          "**관리형 노드 에이전트** — 평범한 요청/응답 HTTP(`/control/join|status|download|load|unload`)로 참여하고 `POST /api/node-reports`로 보고하는 워커 전용 서비스(`nodeagent.py`). 의도적으로 지속 스트림이 **아니라서** 단순한 LAN/VPN 라우팅에서도 살아남습니다.",
        ],
      },
      { t: "h2", kick: "검증 없이는 아무것도 로드되지 않는다", text: "호환성 게이팅" },
      {
        t: "p",
        md: "모든 유닛·노드·에이전트는 프로토콜 / 런타임 팩 정체성과 백엔드 상세를 보고합니다. 유닛, 런타임 팩, 추론엔진 리비전, RPC ABI 불일치는 bind/plan/load/infer **이전에 하드 블록**되고, 백엔드 차이(CUDA/Metal/Vulkan/CPU)는 거부 사유가 아닌 노드 능력치로 추적됩니다. 안전한 계획에 필요한 리소스 모니터링을 제공하지 못하는 노드는 적응형 로딩에서도 제외됩니다.",
      },
      {
        t: "code",
        caption: "재시작에도 남는 것과 사라지는 것.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "허브는 가장 중요한 역할이므로 허브 호스트는 **가장 높은 시간당 가동 보상**을 받습니다. 공개 허브 운영에는 **100,000 KVR** 스테이킹이 필요합니다.",
      },
    ],
  },
  gateway: {
    title: "게이트웨이",
    summary: "공개 진입점: OpenAI/Anthropic 호환 API와 KVR 추론당 과금 정산.",
    blocks: [
      {
        t: "p",
        md: "**게이트웨이**는 개발자가 네트워크를 만나는 곳입니다. 모든 컨트롤러는 OpenAI 호환 엔드포인트(`/v1/chat/completions`, `/v1/responses`, `/v1/models`)와 Anthropic 호환 엔드포인트(`/anthropic/v1/messages`, `/anthropic/v1/models`)를 노출하며, 모두 같은 로드된 모델이 뒷받침합니다 — 기존 클라이언트는 base URL과 키만 바꾸면 동작합니다.",
      },
      {
        t: "code",
        caption: "Kvasir 게이트웨이에 대한 표준 OpenAI 스타일 호출.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "과금", text: "KVR 추론당 지불" },
      {
        t: "p",
        md: "사용량은 **quote → payment → inference** 3단계 흐름으로 KVR로 정산됩니다 — 요청은 실행 전에 가격이 정해지고, 서빙한 노드들은 실행 후에 크레딧을 받습니다. 게이트웨이는 접근 가능한 모든 허브에서 **실시간 모델 카탈로그**를 집계하므로 `/v1/models`는 네트워크가 지금 실제로 서빙할 수 있는 것을 반영합니다.",
      },
      {
        t: "ul",
        items: [
          "게이트웨이 호스트는 진입점을 온라인으로 유지한 대가로 **시간당 가동 보상**과, 서빙을 도운 모든 추론에 대한 **×1.5 보너스**를 받습니다.",
          "공개 게이트웨이 운영에는 허브와 동일하게 **100,000 KVR** 스테이킹이 필요합니다.",
          "공개 배포는 운영자 접근을 **SIWS + 2FA**로 보호하며, 맨 허브는 신뢰된 호스트 / LAN / VPN 전용으로 설계돼 있습니다.",
        ],
      },
    ],
  },
  node: {
    title: "노드",
    summary: "모델의 일부를 서빙하는 모든 기기 — GPU, CPU, NPU, 폰 — 수행한 작업만큼 KVR을 획득.",
    blocks: [
      {
        t: "p",
        md: "**노드**는 모델의 일부를 서빙하는 모든 기기입니다: GPU 머신, CPU 머신, NPU 기기, 또는 폰. 노드는 자기 몫만 보유하고 — 링에서는 레이어 구간, 스웜에서는 전문가 슬라이스 — 정확히 수행한 작업만큼 가중된 KVR을 획득합니다. 라이브 플릿에는 AMD MI250, NVIDIA GB10, RTX Pro 6000, MacBook, x86 Windows CPU 머신, 모바일 노드가 한 네트워크에 섞여 있습니다.",
      },
      { t: "h2", kick: "다운로드에서 정산까지", text: "노드의 라이프사이클" },
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
          "**연산 노드**는 기여 단위당 획득하며, 레이어 몫으로 가중되고 성능 등급으로 배율이 적용됩니다 — 스테이킹은 필요 없습니다.",
          "노드는 소유자의 지갑 아래에 등록되고 보상은 그 지갑으로 비수탁형으로 정산됩니다. 서로 다른 4개의 소유자 지갑이 각자의 레이어 몫을 버는 것이 끝에서 끝까지 검증됐습니다.",
          "능력치 데이터(백엔드, 누적 정밀도, 리소스 예산)가 플래너가 이 노드에 무엇을 배치할 수 있는지 — 그리고 스웜에서 어떤 랭크를 맡을 수 있는지 — 를 결정합니다.",
          "리소스 모니터링을 제공하지 못하는 노드는 맹목적으로 신뢰되는 대신 적응형 로딩에서 제외됩니다.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "443 릴레이",
    summary: "NAT 뒤 기기를 위한 데이터 플레인: 양쪽이 443 포트의 WebSocket 브리지로 아웃바운드 접속한다.",
    blocks: [
      {
        t: "p",
        md: "캐리어 NAT 뒤의 폰은 인바운드 연결을 받을 수 없고, Cloudflare 같은 엣지는 80/443 포트만 통과시킵니다. **443 릴레이**는 둘 다 해결합니다: **1바이트 role preamble**을 쓰는 엣지별 WebSocket 브리지로 양쪽 모두 **아웃바운드**로 접속하므로, 폰은 **인바운드 포트를 하나도 열지 않고** 데이터 플레인에 참여합니다.",
      },
      {
        t: "code",
        caption: "두 아웃바운드 접속이 가운데서 만나고, preamble이 누가 누구인지 알려줍니다.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "프로덕션에서 단련됨", text: "실제 버그 셋, 수정 셋" },
      {
        t: "ul",
        items: [
          "**빌드 핑거프린트 정합** — 텐서 바이트가 흐르기 전에 양쪽이 같은 런타임 팩을 실행함을 증명해야 합니다.",
          "**node-token 다운로드 인증** — 부분 샤드 다운로드는 앱이 이미 보유한, 지갑에서 파생된 node token으로 인증됩니다.",
          "**`Int.ushr` 프레임 스톨** — Kotlin의 `ushr`은 시프트의 하위 5비트만 사용해 `len ushr 56`이 `len ushr 24`가 되었고, 64 KiB 이상의 모든 프레임이 조용히 손상됐습니다(593 KB `result_output`이 첫 희생자). 길이 패킹을 `Long` 시프트로 옮겨 수정 — 64 KiB를 일상적으로 넘는 배치 전문가 dispatch에 필수적인 수정입니다.",
        ],
      },
      {
        t: "p",
        md: "릴레이는 토폴로지가 필요로 하는 것은 무엇이든 — 링 레이어 경계든 전문가 dispatch 스트림이든 — 나르며, 링에서 검증된 바로 그 메커니즘을 스웜의 프로덕션 폰 워커가 그대로 사용합니다.",
      },
      {
        t: "p",
        md: "`/api/expert-relay`와 `/api/ring-relay` 업그레이드는 둘 다 **원시 바이트 그대로 스플라이스**됩니다: 게이트웨이는 WebSocket 프레임을 파싱하지 않고 바이트 단위로 그대로 전달하므로, 릴레이는 얇고 모델과 무관한 파이프로 남습니다. 그럼에도 **세션마다 브리지하는 바이트를 계량**하며, 그 측정된 작업은 허브의 기여 원장으로 흘러들어 워커 자신의 지갑으로 **KVR**로 정산됩니다 — NAT 뒤의 폰을 위해 릴레이하는 것은 직접 연결된 노드와 똑같이 법니다.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "일상 하드웨어를 분산 추론 엔진으로 바꾸는 소스 공개(BSL) 컨트롤 플레인.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp**는 Kvasir를 움직이는 엔진입니다: 추론엔진의 RPC 데이터 플레인을 감싸는 컨트롤 플레인으로, *기본(stock)* `ggml-rpc-server` / `llama-server` 바이너리만으로 대형 AI 모델을 여러 GPU와 머신에 걸쳐 실행합니다. linkcpp가 더하는 것은 전부 오케스트레이션입니다 — GPU 탐색, 노드 슬롯, 레이어 배치 계획, 워커 실행, 그리고 OpenAI/Anthropic 게이트웨이.",
      },
      { t: "h2", kick: "아키텍처", text: "하나의 허브, 기본 워커들" },
      {
        t: "code",
        caption: "linkcpp 배포를 통과하는 요청 경로.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**BSL로 소스 공개** — 개발·테스트 용도로는 무료로 읽고, 실행하고, 위에 빌드할 수 있습니다. 프로덕션 사용에는 라이선스가 필요합니다.",
          "추론엔진 데이터 플레인은 **포크 없이** 유지되므로(핀 고정된 모바일 GPU-over-RPC 패치 하나 제외) 업스트림의 성능 개선이 계속 흘러들어옵니다.",
          "**단일 Docker 이미지**로 배포: FastAPI 허브와 두 추론엔진 바이너리가 함께 들어 있고, 네이티브 워커 노드는 CUDA/Metal/Vulkan/CPU용으로 Docker 밖에서 빌드합니다.",
        ],
      },
      { t: "h2", kick: "플래너", text: "GGUF 메타데이터가 들어가면 배치가 나온다" },
      {
        t: "p",
        md: "플래너는 GGUF 메타데이터를 읽어 노드별 연속 레이어 구간, 그에 맞는 `--tensor-split`, 노드별 KV 캐시 / 레이어 / 전문가 VRAM 추정치를 만들고 — 선택적으로 MoE 전문가-FFN을 노드 RAM으로 오프로드하는 추론엔진 `-ot` 규칙(예: `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`)을 생성해 `--override-tensor`로 실행에 전달합니다. 들어가지 않는 계획은 런타임 OOM으로 발견되는 게 아니라 로드 전에 **infeasible**로 보고됩니다.",
      },
      {
        t: "p",
        md: "런타임 호환성은 일급 개념입니다: 프로토콜, 런타임 팩, 추론엔진 리비전, RPC ABI가 검증되고 불일치는 어떤 bind/plan/load/추론 전에도 하드 블록됩니다.",
      },
    ],
  },
  "ring-runtime": {
    title: "링 런타임",
    summary: "마스터 없는 파이프라인 추론: 각 기기가 자기 레이어 구간을 실행하고 이웃에 경계만 넘긴다.",
    blocks: [
      {
        t: "p",
        md: "**링 런타임**은 Kvasir의 저지연 서빙 토폴로지입니다. 모든 기기는 연속된 자기 **레이어 구간**만 로드한 뒤, 선행자와 후행자에게 정확히 두 개의 링크를 엽니다. hidden-state 경계가 링을 따라 순환하고, 마지막 rank가 토큰을 샘플링해 되돌려줍니다. **중앙 마스터도 없고, 어떤 노드도 모델 전체를 갖지 않습니다.**",
      },
      { t: "h2", kick: "왜 스타형이 아닌가", text: "RPC 마스터 문제" },
      {
        t: "p",
        md: "고전적인 RPC 토폴로지에서는 마스터 하나가 **전체 GGUF**를 열고 모든 워커에게 접속합니다. 이는 열린 네트워크에서 세 가지로 깨집니다: 마스터가 전체 체크포인트를 보유·서빙해야 하고, 모든 워커가 접속 가능해야 하며 — 캐리어 NAT 뒤의 폰은 불가능 — 마스터는 소유자가 없어야 할 네트워크의 단일 소유자가 됩니다. 링은 셋 다 제거합니다: 각 스테이지가 자기 구간을 소유하고, 연결은 이웃 간이며, 릴레이가 NAT 기기를 닿게 합니다.",
      },
      {
        t: "code",
        caption: "4-스테이지 링에서의 한 디코드 스텝.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "배치는 플래너의 **rank manifest**에서 나옵니다 — 예: Qwen3.5-122B의 49개 레이어를 GPU, CPU, NPU, 폰에 분산.",
          "경계는 작아서(토큰당 hidden-state 벡터 하나) 약한 링크에서도 홉 비용이 쌉니다.",
          "모바일 GPU는 링 스테이지를 **직접** 실행합니다(Adreno, OpenCL) — 폰 GPU로의 RPC 경로는 Adreno의 버퍼 레이아웃이 RPC 직렬화를 견디지 못해 불가능했지만, 로컬 스테이지는 백엔드를 스스로 소유하므로 경계만 회선을 건넙니다.",
        ],
      },
      {
        t: "p",
        md: "링은 **지연** 경로입니다. 그 바닥은 레이어 단위(122B 기준 ~1.4 GB)이고, 전문가 샤딩 스웜이 그 바닥을 없애며 같은 서빙 패브릭에 꽂힙니다.",
      },
    ],
  },
  "layer-window": {
    title: "레이어 구간 · 부분 샤드",
    summary: "노드가 맡는 모델의 연속 슬라이스 — 전체 체크포인트 대신 mini-GGUF로 내려받는다.",
    blocks: [
      {
        t: "p",
        md: "**레이어 구간(layer window)**은 링 노드가 서빙하는 연속된 트랜스포머 레이어 범위입니다. 이를 서빙하는 데 전체 체크포인트는 필요 없습니다 — **스테이지 mini-GGUF**가 구간의 텐서만 담습니다: 122B 기준 77.6 GB 전체 모델 대비 **26개 텐서(전체 338개 중) 254 MB**, 폰의 1-레이어 구간이라면 약 1.5 GB.",
      },
      {
        t: "code",
        caption: "rank manifest 한 줄: 누가 무엇을, 어떤 예산 안에서 서빙하는가.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "배정이 아니라 쟁취", text: "Self-enrollment" },
      {
        t: "ul",
        items: [
          "노드가 **커버리지/수요 맵**을 폴링해 어떤 구간이 부족하고 각각 얼마를 지불하는지 봅니다.",
          "자기 예산에 맞는 **최대 보상** 미충족 구간을 골라 정확히 그만큼만 내려받고 참여합니다.",
          "커버리지는 자가 치유됩니다: 노드가 이탈하면 그 구간이 다시 희소해지고 — 그래서 다시 수지맞는 구간이 됩니다.",
          "NAT 뒤의 폰으로 끝에서 끝까지 검증됨: 폴링 → self-enroll → 부분 다운로드 → Adreno GPU 로드 → 링 추론 완주, 기여 크레딧까지.",
        ],
      },
      {
        t: "p",
        md: "전문가 스웜은 바로 이 마켓을 더 잘게 **(레이어, 전문가 범위)** 단위로 재사용합니다 — 같은 맵, 같은 self-enrollment, 같은 보상, 더 작은 단위.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "FFN이 수백 개의 독립 전문가로 이루어져 토큰마다 몇 개만 켜지는 모델.",
    blocks: [
      {
        t: "p",
        md: "**Mixture-of-Experts** 모델은 각 레이어의 단일 FFN을 독립 전문가 FFN들의 뱅크와, 토큰마다 몇 개를 고르는 **라우터**로 대체합니다. Qwen3.5-122B-A10B가 네트워크의 대표 예시입니다:",
      },
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
      {
        t: "p",
        md: "각 레이어는 **dense 경로** — 어텐션 + KV, norm들, 라우터(`ffn_gate_inp`), shared expert — 와 세 개의 적층 텐서(`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`)로 저장되는 **전문가 뱅크**로 나뉩니다. dense 경로는 바이트의 소수이고, 전문가 뱅크가 모델의 86%입니다.",
      },
      {
        t: "ul",
        items: [
          "전문가 인덱스는 **GGUF의 최외곽 차원**이라 각 전문가는 연속·양자블록 정렬 슬랩입니다 — 추출은 바이트 레인지 복사이며 dequant가 없습니다.",
          "토큰당 레이어마다 **256개 중 8개**만 켜지므로, 디코드 시 레이어의 전문가 트래픽은 hidden 벡터 하나에 대한 소수의 작은 행렬곱입니다(dispatch ~6 KB).",
          "전문가들은 상호 독립적입니다 — 소유권을 기기들에 흩뿌리고 자유롭게 재배치할 수 있습니다.",
        ],
      },
      {
        t: "p",
        md: "이것이 MoE가 스웜의 천연 기질인 이유입니다: 가중치가 이미 기기 크기의, 독립적으로 소유 가능한 단위로 포장돼 있습니다.",
      },
    ],
  },
  "expert-sharding": {
    title: "전문가 샤딩",
    summary: "MoE를 전문가 단위로 쪼개, 폰이 1.4 GB 레이어 대신 42–340 MB의 전문가를 진다.",
    blocks: [
      {
        t: "p",
        md: "**전문가 샤딩**은 스웜의 운반 단위를 레이어(122B 기준 ~1.4 GB)에서 전문가(**5.3 MB**)로 낮춥니다. 약한 기기는 8–64개 전문가 슬라이스(**42–340 MB**)를 내려받아, 어텐션·KV·샘플러가 없는 순수 함수 워커로 로드하고, 백본의 라우터가 자기 전문가를 고를 때마다 계산합니다.",
      },
      { t: "h2", kick: "두 역할", text: "백본 × 워커" },
      {
        t: "code",
        caption: "MoE 레이어 하나 안의 컷 지점 (라우터는 백본에서 단 한 번).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "**백본**은 dense 경로(어텐션·norm·라우터·shared expert·combine)를 맡고, churn 내성을 위해 모든 전문가를 RAM 오프로드된 fallback 복제본으로 상주시킵니다.",
          "**워커**(`linkcpp-expert-worker --serve`)는 하나의 long-lived TCP 스트림으로 `(n_used, n_tokens, cur, sel) → experts`에 응답합니다 — 폰에서는 443 릴레이가 터널링하는 바로 그 스트림입니다.",
          "커버리지는 **전문가 커버리지 마켓**으로 자가 치유됩니다: `POST /api/expert-coverage`가 보유를 heartbeat하고, `GET /api/expert-demand`가 희소성을 집계하고, `POST /api/expert-volunteer`가 예산에 맞춰 가장 희소한 범위를 배정합니다.",
        ],
      },
      { t: "h2", kick: "약속이 아니라 측정", text: "실기기 검증" },
      {
        t: "ul",
        items: [
          "샤딩 계산 == monolithic, **max|Δ| = 3.6e-12** (근사가 아닌 정확한 재그룹핑).",
          "live 122B decode의 cross-process dispatch: **argmax MATCH**, 로짓 cosine 0.99869 — in-process와 바이트 단위로 동일.",
          "Galaxy S25가 자기 1.58 GB 슬라이스를 자율 다운로드하고 매 토큰 layer-0 전문가를 계산: 로컬 실행과 **8/8 토큰 동일**.",
          "공개 인터넷 너머의 원격 GPU — 토큰당 WAN 왕복 한 번 — 도 **greedy 8/8 동일**을 유지했습니다(cosine 0.99773): 직접 링크에서 **1.2% 처리량 오버헤드**, CDN 엣지를 거치면 ~28%. 토큰별 직렬 dispatch의 정직한 비용이자, 패브릭의 지렛대가 더 낮은 지연이 아니라 배치인 이유입니다.",
          "배치 dispatch는 배치 512(ROCm)에서 워커당 **53k tok/s** — 스웜을 실용화하는 처리량-패브릭 성질.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "라우터 권위",
    summary: "스웜의 정합성 불변식: 라우팅은 백본에서 단 한 번 결정되고, 워커는 전문가 id만 받는다.",
    blocks: [
      {
        t: "callout",
        md: "**불변식:** 네트워크 안의 유일한 이산 결정은 MoE 라우팅(256중 top-8)입니다. Kvasir는 라우터를 **백본에서 정확히 한 번** 실행하고, 워커에는 선택된 전문가 id만 dispatch합니다. 이종 스웜은 각 전문가 출력의 *크기*가 미세하게 다를 수는 있어도, *어떤 전문가가 도는지*는 절대 갈리지 않습니다.",
      },
      {
        t: "p",
        md: "이 규칙이 없다면 각 백엔드가 라우터를 다시 실행해 경계 토큰에서 **서로 다른 전문가**를 고르게 됩니다 — 그 토큰부터 계산이 다른 시드처럼 갈라지는 진짜 파국적 발산입니다. 규칙이 있으면 하드웨어 차이는 확률 가중 combine이 흡수하는 유계 연속 오차로 줄어듭니다.",
      },
      { t: "h2", kick: "무엇을 막는가", text: "하나의 결정 지점이 닫는 발산 모드들" },
      {
        t: "table",
        head: ["발산 모드", "권위 없이", "권위 있으면"],
        rows: [
          ["라우팅 불일치", "경계 토큰에서 백엔드마다 다른 top-8 선택", "id가 한 번 결정돼 소유자에게 dispatch"],
          ["궤적 분기", "뒤집힌 토큰 하나가 시퀀스 전체를 분기", "디코드/샘플링을 한 노드에 고정"],
          ["검증", "백엔드 간 bit 비교(불가능)", "잘 정의된 잔차에 대한 허용오차 검사"],
        ],
      },
      {
        t: "p",
        md: "비용은 무시할 수준입니다: 백본은 어차피 `ffn_norm`과 라우터 로짓을 계산하고 있었고, 회선을 건너는 것은 hidden 행들과 선택된 id들 — 디코드 스텝당 약 **6 KB** — 뿐입니다.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "수치적 동등성",
    summary: "백엔드끼리는 결코 bit 단위로 일치하지 않는다; 스웜은 측정된 허용오차를 일급 계약으로 다룬다.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno, CPU는 같은 연산을 서로 다른 리덕션 순서, FMA 융합, accumulator, 초월함수 근사로 계산합니다 — 그래서 결과는 op당 ~1e-6…1e-3씩 다르고, **설계상 절대 bit-identical하지 않습니다**. 어떤 하드웨어든 오는 대로 이뤄지는 스웜은 bit 일치를 요구할 수 없으므로, Kvasir는 대신 동등성을 측정합니다.",
      },
      {
        t: "table",
        head: ["백엔드 쌍 (실제 122B, layer-0 전문가)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA(GB10 Blackwell) vs ROCm(MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm(MI250) vs numpy(x86)", "7.9e-7", "0.99996"],
          ["폰 ARM CPU vs numpy(x86)", "1.4e-6", "0.99992"],
          ["CUDA(GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "전체 백엔드 매트릭스가 닫혔다: 두 GPU 백엔드(CUDA·ROCm)는 커널 소스를 공유해 **사실상 비트-동일**(cosine 1.0000000000)하고, GPU↔CPU 쌍은 ~0.9997로 동등하다. CUDA 워커와 ROCm 워커는 교체 가능하고, GPU 워커와 CPU 워커는 수치 동등하다.",
      },
      { t: "h2", kick: "왜 다른가", text: "부동소수점 덧셈은 결합법칙이 없다" },
      {
        t: "ul",
        items: [
          "**matmul 리덕션 순서** — tensor core, MFMA 타일, OpenCL workgroup, SIMD 레인이 각기 다른 순서로 누적합니다.",
          "**누적 정밀도** — F16/BF16 저장에 F32 vs F16 accumulator, 발산 크기의 최대 지렛대입니다.",
          "**초월함수 근사** — exp(softmax), silu(swiglu), rsqrt(norm)가 백엔드마다 다른 다항/테이블 변형을 씁니다.",
        ],
      },
      { t: "h2", kick: "계약", text: "허용오차, 능력치, 단일 권위" },
      {
        t: "ul",
        items: [
          "검증은 **허용오차**입니다 — \"top-1 일치율 ≥ 99.x%, KL ≤ ε\" — bit 일치가 아닙니다.",
          "백엔드와 누적 정밀도는 노드 **능력치**로 광고되고, F32 누적 노드가 출력 민감 랭크에 우선됩니다.",
          "허용오차를 벗어난 노드는 민감 랭크에 부적합으로 표시될 뿐 통째로 거부되지 않습니다.",
          "이산 결정(라우팅·샘플링)은 단일 권위에 고정돼 연속 오차가 이산 발산이 될 수 없습니다.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "추론엔진가 쓰는 양자화 모델 파일 포맷 — 부분·전문가 슬라이싱을 값싸게 만드는 레이아웃.",
    blocks: [
      {
        t: "p",
        md: "**GGUF**는 추론엔진 생태계의 단일 파일 모델 포맷입니다: 메타데이터(아키텍처, 레이어 수, 차원, 양자화)와 원시 양자화 바이트로 저장된 텐서들(예: Q4_K_M). linkcpp의 플래너는 메타데이터를 읽어 배치와 크기 추정을 계산하고, 서빙 측은 텐서 바이트를 슬라이스해 다운로드를 만듭니다.",
      },
      {
        t: "ul",
        items: [
          "**스테이지 mini-GGUF**는 레이어 구간 하나의 텐서만 담습니다 — 122B 링 스테이지 기준 77.6 GB 대신 254 MB.",
          "**전문가 샤드 GGUF**는 (레이어, 전문가 범위) 슬라이스 하나를 담고, `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16`이 node-token 인증으로 서빙합니다.",
          "둘 다 **유효한 GGUF 파일**입니다: 노드의 리더가 기본 도구로 그대로 로드하며, 커스텀 포맷이 아닙니다.",
        ],
      },
      {
        t: "code",
        caption: "전문가 슬라이싱이 바이트 복사인 이유: 전문가 인덱스가 최외곽 차원.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "라우터(`ffn_gate_inp`)와 shared expert는 전문가 샤드에서 **제외**됩니다 — 백본의 소유이며, 라우터 권위가 요구하는 바로 그 구조입니다.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "네트워크의 유틸리티 토큰: 개발자는 추론에 쓰고, 기여자는 연산으로 번다.",
    blocks: [
      {
        t: "p",
        md: "**KVR**(온체인 이름 \"Kvasir\", 6 decimals, Solana)은 양방향으로 흐르는 하나의 토큰입니다: 개발자는 게이트웨이로 추론을 실행하려 KVR을 **지불**하고, 기여자는 노드가 제공한 연산으로 KVR을 **획득**합니다. 보상은 실제 작업 — 실제로 서빙한 레이어와 전문가 — 에서 계산되며, 참여 자체로는 주어지지 않습니다.",
      },
      {
        t: "ul",
        items: [
          "**지불 측** — 게이트웨이를 통한 추론당 지불: quote → payment → inference.",
          "**획득 측** — 연산은 기여 단위 × 레이어 몫 × 성능 등급, 허브/게이트웨이 역할은 시간당 가동 보상.",
          "**정산** — Solana에서 각 노드 소유자 자신의 지갑으로; 정산 서비스가 요청을 거친 모든 노드에 크레딧을 줍니다.",
          "**신화에서 딴 이름** — Kvasir에서 빚어져 마시는 모두에게 지혜를 준 시의 벌꿀술: 열린 접근, 그리고 부어 넣는 모두에게 주어지는 보상.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, 유틸리티 토큰.** KVR은 현재 Solana devnet에서 운영되는 유틸리티 / 기여 토큰입니다 — 거래 가능한 자산, 가격, 투자 대상이 아닙니다. 여기의 어떤 내용도 투자 조언이나 수익 약속이 아닙니다.",
      },
    ],
  },
  "contribution-units": {
    title: "기여 단위",
    summary: "보상 공식: 서빙한 토큰 × 레이어 몫으로 units가 쌓이고, 성능 등급으로 배율이 적용된다.",
    blocks: [
      {
        t: "code",
        caption: "연산 보상 계산 방식.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "1 **unit** ≈ 서빙한 1k 토큰을 각 추론에서의 노드 **레이어 몫**으로 가중한 값입니다 — 49개 중 12개 레이어를 도는 노드는 각 추론 units의 12/49를 법니다. 그 위에 등급 배수가 측정된 속도에 보상하고, 인프라 역할은 시간당 가동 보상을 더합니다.",
      },
      { t: "h2", kick: "계산 예시", text: "추론 하나, 노드 넷" },
      {
        t: "table",
        head: ["노드", "레이어", "몫", "등급", "1k 토큰당 유효 units"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["폰", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "보상은 **실제 작업**을 따릅니다: 아무것도 서빙하지 않은 노드는 가동 시간과 무관하게 아무것도 벌지 않습니다(연산 역할 기준).",
          "역할은 **겹칩니다** — 한 머신이 연산 + 게이트웨이 + 허브를 겸하면 각 스트림이 합산됩니다.",
          "모든 것은 노드 자신의 소유자 지갑으로 KVR로 정산되고, 대시보드에 원 기여 × 등급 = 유효 기여와 청구 가능 잔액이 표시됩니다.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "성능 등급",
    summary: "측정된 처리량이 배수를 정한다: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["등급", "측정 처리량", "배수"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "노드의 측정된 디코드 속도가 등급을 정하고, 등급이 획득 units에 곱해집니다 — 빠른 하드웨어는 같은 작업으로 비례해 더 많이 법니다. 지갑의 노드 현황 화면에 각 노드의 등급이 기여도 옆에 표시됩니다.",
      },
      {
        t: "ul",
        items: [
          "등급은 **자가 신고가 아니라 측정**입니다 — 처리량은 노드의 실제 서빙 성능에서 나오고 시간이 지나며 재측정됩니다.",
          "C 등급 폰도 법니다 — 자기 레이어 몫의 ×0.7 — 그게 핵심입니다: 바닥은 데이터센터 전용이 아니라 참여 전체에 열려 있습니다.",
          "등급은 *유효* units에 곱해지므로 레이어 몫·게이트웨이 보너스를 대체하는 게 아니라 합성됩니다.",
        ],
      },
    ],
  },
  staking: {
    title: "스테이킹",
    summary: "KVR을 스테이킹해 APR 이자를 받고, 100,000 KVR 스테이킹으로 허브·게이트웨이 운영 자격을 얻는다.",
    blocks: [
      {
        t: "p",
        md: "스테이킹은 자신의 지갑에 KVR을 예치해 **APR 이자**를 받고 노드 보상 자격을 얻는 것입니다. **허브**나 **게이트웨이** 노드 운영에는 **100,000 KVR** 스테이킹이 필요하고, 일반 연산 노드는 예치 없이 참여해 실행한 레이어만큼 법니다.",
      },
      {
        t: "ul",
        items: [
          "스테이킹은 지갑 대시보드의 스테이킹 패널에서 이루어집니다: 수량을 입력하고 **예치**하면 포지션이 APR과 노드 보상 자격을 쌓기 시작합니다.",
          "100k 요건은 다른 사람들의 트래픽이 의존하는 두 역할 — 진입점과 컨트롤 플레인 — 을 위한 **책임 담보 필터**입니다.",
          "스테이킹도 다른 모든 것처럼 비수탁형입니다: 포지션은 자신의 지갑에 있고, 원금·누적 이자·노드 보상이 모두 스테이킹 패널에 표시됩니다.",
          "스테이킹할 devnet KVR은 배포나 스왑(SOL/ETH ↔ KVR 스왑: 지원 예정)으로, 수수료용 devnet SOL은 공개 faucet에서 받습니다.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "비수탁형 지갑",
    summary: "키는 사용자 기기에만 존재 — web·desktop·iOS·Android — 보상은 그 지갑으로 바로 정산.",
    blocks: [
      {
        t: "p",
        md: "Kvasir Wallet은 **설계부터 비수탁형**입니다: 12단어 복구 문구와 키는 사용자 자신의 기기에만 저장되고, 운영자에게 맡기지 않습니다. 보상은 Solana에서 각 노드 소유자 지갑으로 직접 정산됩니다 — 서로 다른 4개의 소유자 지갑이 각자의 레이어 몫을 버는 것이 검증됐습니다.",
      },
      {
        t: "ul",
        items: [
          "**플랫폼** — web, desktop(macOS/Windows/Linux, 지갑+노드가 하나의 Electron 앱), iOS, Android.",
          "**하나의 문구, 모든 기기** — 같은 12단어 복구 문구로 데스크톱·폰·웹에서 같은 계정을 복구하고, 로컬 패스프레이즈가 각 설치본을 잠금 해제합니다.",
          "**지갑 = 노드 정체성** — 지갑이 노드의 네트워크 정체성에 서명하므로, \"이 기기의 수익이 누구 것인가\"는 누군가의 서버에 있는 계정 행이 아니라 암호학적 사실입니다.",
          "**문구를 잃으면 계정을 잃습니다** — 비수탁은 양날입니다; 이를 초기화해 줄 운영자는 존재하지 않습니다.",
        ],
      },
      {
        t: "p",
        md: "모바일 앱은 노드 앱을 겸합니다: KVR을 보관하는 바로 그 지갑이 폰의 컴퓨트 백엔드와 노드 모드를 설정하고, 스테이킹하고, 보상을 청구합니다.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "운영자 로그인은 서버 nonce에 대한 지갑 서명(Sign-In With Solana) + 선택적 TOTP 2FA.",
    blocks: [
      {
        t: "p",
        md: "공개 배포에서 허브와 게이트웨이의 운영자 접근은 **Sign-In With Solana**로 인증됩니다: 운영자의 지갑이 서버가 발급한 nonce에 서명해, 비밀번호나 수탁 자격증명 없이 소유권을 증명합니다. 그 위에 **TOTP 2FA**와 일회용 백업 코드가 세션을 보호합니다 — 허브와 게이트웨이 양쪽 모두.",
      },
      {
        t: "ul",
        items: [
          "**비밀번호가 어디에도 없습니다** — 지갑 키가 곧 정체성이고 nonce가 재사용을 막으므로, 서버 측에 피싱당하거나 유출될 것이 없습니다.",
          "**지갑별 TOTP 등록**은 허브 상태에 영속되므로, 2FA는 슬롯·바인딩과 함께 재시작을 견딥니다.",
          "**백업 코드는 일회용**입니다 — 로그인마다 하나씩 소모되며, 인증 기기를 쓸 수 없을 때의 복구 수단입니다.",
          "**적용 범위를 정직하게** — 맨 허브와 RPC 포트는 신뢰된 호스트 / LAN / VPN용으로 설계됐고, SIWS + 2FA는 *공개* 도메인 노출을 안전하게 만드는 계층입니다.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "KVR 경제",
    summary: "소비자 비용과 노드 보상이 어떻게 하나의 자기 강화 루프를 이루는가 — 네트워크가 커질수록 더 싸지게 만드는 선순환.",
    blocks: [
      {
        t: "p",
        md: "Kvasir는 하나의 토큰으로 정산되는 **양면 시장**입니다. 소비자는 추론당 **KVR**을 트레저리에 지불하고, 노드는 자신이 서빙한 정확한 작업만큼 **KVR**을 벌어 자기 지갑으로 되돌려받습니다. 설계 목표는 이 두 면이 경쟁하지 않고 — 서로를 **증폭**하는 것입니다: 공급이 늘면 네트워크가 더 싸고 좋아지고, 그것이 더 많은 수요를 끌어오고, 그 결제가 더 풍성한 보상을 대며, 그것이 다시 더 많은 공급을 끌어옵니다.",
      },
      { t: "h2", kick: "플라이휠", text: "사용량과 공급이 함께 자란다" },
      {
        t: "p",
        md: "추론은 **반드시** KVR로 지불되어야 하므로, 모든 사용량은 토큰에 대한 실수요입니다 — 투기가 아니라 유틸리티입니다. 그 수요가 노드가 버는 KVR의 가치를 떠받치고, 그것이 기여를 계속 매력적으로 유지하고, 그것이 용량을 키우고, 그것이 가격과 지연을 낮추고, 그것이 더 많은 사용을 끌어옵니다. Kvasir의 가장 날카로운 강점이 이 루프를 한층 더 조입니다: 참여자는 **소비자이자 공급자**가 동시에 될 수 있어(*프로슈머*), 두 면이 흔히 같은 사람 안에서 함께 자랍니다.",
      },
      {
        t: "callout",
        md: "**\"기여하면 공짜\"는 순-공짜이지 무비용이 아닙니다.** 추론한 만큼 내고 서빙한 만큼 벌며, 소비하는 만큼 대략 기여하면 둘이 상쇄됩니다. 네트워크가 공짜인 게 아니라 — *당신의* 청구서가 공짜인 것입니다.",
      },
      { t: "h2", kick: "선순환 유지하기", text: "세 불변식과 그것들이 막는 악순환" },
      {
        t: "table",
        head: ["불변식", "막는 악순환"],
        rows: [
          ["보상은 실제 수익으로 지급(emission은 부트스트랩에만, 이후 축소)", "인플레이션이 KVR을 잠식해 양면이 함께 붕괴"],
          ["KVR은 추론의 필수 매개", "토큰 가치가 사용량과 분리돼 순수 투기로"],
          ["가격은 비용 하한과 시장 이하 상한 사이에서 부동", "너무 낮으면 노드 고갈; 너무 높으면 중앙화 API에 사용자 이탈"],
        ],
      },
      {
        t: "p",
        md: "Kvasir는 이미 **실제 작업**에 보상하고(단순 참여가 아니라 서빙한 토큰 × 레이어 몫당 KVR) 비수탁형으로 정산하는데, 이것이 수익 기반 보상을 정직하게 만드는 어려운 부분입니다. 나머지 — 가동률 주도 가격과 emission→수익 축소(taper) — 는 \"더 많은 노드 → 더 저렴\"을 직관에서 프로토콜이 강제하는 규칙으로 바꾸는 경제 로드맵입니다. **추론 가격** 항목이 가격 측면을, **기여 단위**가 작업이 보상이 되는 과정을 다룹니다.",
      },
    ],
  },
  "inference-pricing": {
    title: "추론 가격",
    summary: "오늘 추론 하나가 KVR로 얼마인지, 탈중앙 네트워크가 구조적으로 왜 더 저렴한지, 그리고 공급이 늘수록 가격이 어떻게 떨어지도록 설계됐는지.",
    blocks: [
      {
        t: "p",
        md: "네트워크 접근은 **추론당 지불**입니다: 게이트웨이가 요청에 대한 KVR 가격을 견적하고, 지갑이 온체인으로 지불하면, 그제야 허브가 모델을 실행합니다. 가격은 작고 투명한 공식 — 요청당 하한 + 토큰당 요율 — 으로, 미리 견적되고 생성 후 **실제** 토큰 사용량으로 정산됩니다.",
      },
      {
        t: "code",
        caption: "정산 공식 — 미리 견적하고, 이후 실제 사용량으로 과금.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "왜 더 저렴할 수 있는가", text: "치러야 할 중앙 마진이 없다" },
      {
        t: "p",
        md: "중앙화 API는 비용에 **더해** 큰 마진과 자본 회수를 얹어 가격을 매깁니다. 탈중앙 네트워크는 기여자의 **한계 비용** — 전기와 하드웨어 감가상각 — 에 얇은 프로토콜 수수료만 더해 가격을 매깁니다. 이 구조적 격차는 규모와 무관하게 존재합니다. 성장은 격차를 넓힙니다: **전문가 샤딩** 덕에 더 많은 노드가 각자 더 작은 슬라이스를 쥐므로, 더 저렴한 기기도 서빙할 수 있어 참여의 한계 비용이 낮아지고 공급이 깊어집니다.",
      },
      {
        t: "callout",
        md: "**가격은 통제되며, 무법지대가 아닙니다.** 요율은 민감한 경제 파라미터로, 지갑 서명 + 2FA 아래 genesis 지갑만 변경할 수 있습니다 — 환경 변수로는 절대 불가합니다. 이것이 토큰 경제를 안정적이고 감사 가능하게 유지합니다.",
      },
      { t: "h2", kick: "어디로 향하는가", text: "가동률 주도 가격" },
      {
        t: "p",
        md: "설계 방향은 하한(노드 한계 비용 위로 유지해 서빙이 수지맞도록)과 상한(중앙화 대안 아래로 유지해 경쟁력을 지키도록) 사이에서 **네트워크 가동률에 따라 부동**하는 가격입니다. 유휴 공급은 가격을 아래로, 혼잡은 위로 밀칩니다. 이것이 **\"더 많은 공유 노드 → 더 낮은 가격\"**을 마침내 코드로 참이 되게 하는 메커니즘 — **KVR 경제**의 자연스러운 온도조절기입니다.",
      },
    ],
  },
  "run-expert-worker": {
    title: "전문가 워커 실행하기",
    summary: "남는 GPU·CPU·폰을 전문가 워커로: 빌드하고, 가장 희소한 슬라이스에 자원하고, 내려받고, 서빙하고, 443으로 아웃바운드 접속해 KVR을 획득한다.",
    blocks: [
      {
        t: "p",
        md: "**전문가 워커**는 순수 `(hidden, ids) → out` 함수입니다 — 어텐션도, KV 캐시도, 샘플러도 없이 — 백본의 라우터가 고를 때마다 MoE 모델 전문가의 한 슬라이스를 계산합니다. 무엇을 서빙할지는 직접 고르지 않습니다; **커버리지 마켓**이 예산에 맞춰 잘린, 가장 희소하고 보상이 높은 범위를 건네므로, 4 GB 폰과 데이터센터 GPU 모두 자리를 찾습니다.",
      },
      { t: "h2", kick: "일곱 단계", text: "빌드 → 자원 → 서빙 → 접속 → 획득" },
      {
        t: "code",
        caption: "전체 경로 — dial 스크립트는 재시도 루프로 백본을 기다립니다.",
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
          "**슬라이스는 작습니다.** layer-0의 128-전문가 슬라이스는 72 GB 전체 모델 대비 **794 MB** — 약한 기기가 참여할 수 있게 하는 단위입니다. 마켓이 배정한 범위만 내려받습니다.",
          "**아웃바운드 접속, 인바운드 수신 없음.** 5단계는 443에 하나의 아웃바운드 WebSocket을 열어, 캐리어 NAT와 CDN 엣지가 통과시키고 인바운드 포트를 하나도 노출하지 않습니다 — 폰이 쓰는 바로 그 경로입니다.",
          "**heartbeat는 필수입니다.** `POST /api/expert-coverage` 없이는 수요 맵이 아는 것을 아무것도 서빙하지 못하고, 하는 일 무엇도 크레딧되지 않습니다.",
          "**보상은 작업당입니다.** 브리지된 작업은 허브의 기여 원장에 쌓이고, 게이트웨이가 KVR을 당신 **자신의** 지갑으로 델타 크레딧합니다(비수탁형). 지불받으려면 지갑 주소가 필요합니다.",
        ],
      },
      {
        t: "callout",
        md: "워커는 데이터센터 내 GPU와 똑같은 dispatch 프로토콜을 씁니다 — 하나의 long-lived 스트림으로 `(n_used, n_tokens, cur, sel) → experts`. 부분 샤드 워커는 단지 `n_used = 1`로 둘 뿐입니다. 이 균일성이 폰, CPU 머신, Blackwell 카드가 같은 스웜의 교체 가능한 구성원인 이유입니다.",
      },
    ],
  },
  "hub-operations": {
    title: "허브 운영하기",
    summary: "허브와 게이트웨이 운영 노트: 리빌드 없이 핫패치하고, 재시작을 견디고, 카탈로그 등록을 유지하고, 노출면을 443으로 잠근다.",
    blocks: [
      {
        t: "p",
        md: "허브(컨트롤 플레인)와 게이트웨이(공개 진입점)는 운영자가 건강하게 유지하는 두 개의 장수 서비스입니다. 허브와 그 RPC 포트는 **설계상 인증이 없으며** — 신뢰된 호스트 / LAN / VPN 전용 — 모든 공개 트래픽은 게이트웨이의 단일 443 노출면으로 수렴합니다. 다음은 그 구성을 코드 변경·재시작·재부팅에 걸쳐 안정적으로 유지하는 운영 노트입니다.",
      },
      { t: "h2", kick: "배포 & 핫패치", text: "리빌드 없이 코드 바꾸기" },
      {
        t: "ul",
        items: [
          "**빠른 경로:** 허브/게이트웨이 코드를 `docker cp <file> <container>:/app/...` + `docker restart`로 갱신 — 이미지 리빌드 없이. 하지만 **환경 변수 추가는 이 방식으로 불가능하며**(컨테이너 재생성이 필요) 대신 hub-state에 영속되는 런타임 설정 API를 쓰는 편이 낫습니다.",
          "**Compose 드리프트:** 오래 실행된 컨테이너는 compose 파일과 어긋날 수 있습니다(network mode, entrypoint, env). `docker compose up -d` 재생성 전에 항상 `docker inspect`로 실제 설정을 확인하세요 — 드리프트했다면 재생성이 프로덕션 설정을 지워버립니다. cp + restart를 쓰세요.",
          "**패치 전에 diff:** 컨테이너 안 파일을 `docker cp`로 꺼내 repo HEAD와 diff한 뒤 교체하여, 이전 세션의 핫패치가 조용히 사라지지 않게 하세요.",
        ],
      },
      { t: "h2", kick: "재시작을 견디기", text: "상태는 남고, 로드된 모델은 사라진다" },
      {
        t: "ul",
        items: [
          "허브 재시작은 **서빙을 멈춥니다.** 슬롯·컨트롤러·바인딩은 `hub-state.json`에서 복원되지만, 로드된 모델은 런타임 전용입니다. 재시작 후에는 각 컨트롤러의 `last_load`를 읽어 `POST /api/controllers/{cid}/serve`를 다시 쏘세요 — 큰 모델도 페이지 캐시 덕에 약 1분 만에 돌아옵니다.",
          "**게이트웨이 워치독:** 서빙 중인 모든 모델을 30초마다 1-토큰 요청으로 프로브하고, 실패 시 `last_load`에서 자동 재로드합니다(쿨다운 포함). **`catalog[0]`이 아니라 *모든* 모델을 프로브하세요** — 다른 허브의 건강한 모델이 맨 앞으로 정렬되는 순간, 첫 번째만 프로브하면 다운되는 큰 모델을 놓칩니다(실제 버그, 이후 수정됨).",
          "**카탈로그 TTL:** `POST /api/pay/hub/register`는 90초 TTL이므로, 약 60초 heartbeat 루프로 등록을 살려 두고, `@reboot` cron이나 systemd 유닛으로 재부팅에도 견디게 하세요.",
        ],
      },
      { t: "h2", kick: "잠그기", text: "공개되는 모든 것은 443을 지난다" },
      {
        t: "ul",
        items: [
          "허브(:19000)와 RPC 포트는 신뢰된 네트워크를 가정합니다; 인터넷을 향해야 할 유일한 것은 443의 게이트웨이(그 WebSocket 릴레이 패스스루 포함)뿐입니다.",
          "허브가 공개 IP에 놓여야 한다면 신뢰된 IP로 방화벽을 거세요 — 하지만 Docker의 published 포트는 **INPUT 체인 이전에 DNAT되므로** `dport` 규칙은 매치되지 않습니다. 대신 `DOCKER-USER` 체인에서 conntrack의 원본 목적지 포트(`--ctorigdstport`)로 필터하고, 규칙을 `After=docker.service`로 정렬된 systemd oneshot으로 영속화하세요.",
        ],
      },
      { t: "h2", kick: "정산 & 함정", text: "푸시가 아니라 풀 — 그리고 셸 함정 하나" },
      {
        t: "ul",
        items: [
          "**정산은 푸시가 아니라 풀입니다:** 허브가 기여를 누적하고, 게이트웨이가 `GET /api/contributions`를 폴링해 KVR을 델타 크레딧합니다. 허브 재시작으로 카운터가 리셋되면, 게이트웨이가 다시 기준선을 잡아 이중 지불이 없게 합니다. 전문가-작업 요율은 `LINKCPP_EXPERT_UNITS_PER_MB`로 설정됩니다.",
          "**`pkill` 함정:** `ssh host 'pkill -f X; ...'`은 *자기 자신의* 명령줄에 매치돼 스스로를 죽입니다. 패턴에 문자 클래스(`X[x]`)를 쓰고, spawn과 pkill을 절대 같은 원격 명령에 넣지 마세요.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "허브 WAN 인터커넥트 (200G 광학)",
    summary: "허브가 방 하나·캠퍼스·도시를 가로질러 200 Gb/s로 연결되는 방법: 어느 거리에 어느 광학 모듈을, 무엇을 어디에 꽂는지, 그리고 실제로 라인 레이트에 도달하려면 무엇이 필요한지.",
    blocks: [
      {
        t: "p",
        md: "두 허브가 모두 공개 경로를 가질 때, 전문가-dispatch 데이터 플레인은 **직접 링크**여야 합니다 — 443 릴레이는 NAT된 엣지를 위한 것입니다. 이 항목은 그 직접 링크를 카탈로그 부품으로 200 Gb/s급으로 만드는 구체적 레시피입니다. 하나의 규칙이 전부를 정리합니다: **광섬유는 속도 중립적인 유리이고, 속도는 양 끝의 pluggable에 있습니다.**",
      },
      { t: "h2", kick: "Step 1 · 거리로 고르기", text: "도달 거리 사다리" },
      {
        t: "table",
        head: ["거리", "부품", "꽂는 곳"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC, 스위치 없음"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / 스위치"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "NIC 또는 스위치의 QSFP56 케이지"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "NIC 또는 스위치의 QSFP56 케이지"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "스위치/라우터의 QSFP-DD 케이지 (NIC 아님)"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "스위치가 캐리어로 핸드오프"],
        ],
      },
      { t: "h2", kick: "Step 2 · 무엇을 어디에 꽂나", text: "NIC 쪽 vs 스위치 쪽" },
      {
        t: "ul",
        items: [
          "**NIC 쪽** — ConnectX-6/7급 카드는 QSFP56 케이지를 노출하며, DAC/AOC/FR4/LR4/ER4가 모두 NIC에 직접 꽂힙니다. GB10급 허브는 이미 온보드에 200 GbE QSFP 포트 두 개가 있어, 두-허브 링크에는 정확히 케이블 하나와 새 하드웨어 0이 필요합니다.",
          "**스위치 쪽** — coherent ZR+ 광학은 QSFP-DD 폼팩터로 스위치나 라우터에 속합니다; 그러면 허브의 NIC는 짧은 DAC로 그 스위치에 200G로 붙습니다. 먼 허브가 수십 킬로미터 떨어져 있을 때 이 계층을 쓰세요.",
          "**광섬유 자체** — 표준 single-mode(G.652) duplex LC 쌍으로, 가닥 단위 dark fiber로 임대합니다. 같은 유리가 지금은 100G를, 나중엔 400G를 나릅니다; 업그레이드는 모듈 교체일 뿐 토목 공사가 아닙니다.",
          "**~120 km를 넘으면** — 부품을 사는 대신 캐리어에게서 파장(wavelength)을 임대하기 시작합니다; 경계점은 당신의 스위치에서의 Ethernet 핸드오프입니다.",
        ],
      },
      {
        t: "code",
        caption: "레퍼런스 구성 셋, 저렴한 순.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Step 3 · 실제로 200G 내기", text: "라인 레이트는 구매가 아니라 설정이다" },
      {
        t: "ul",
        items: [
          "가능하면 dispatch 스트림에 **RDMA (RoCE)**를 쓰세요 — GB10급 호스트는 분할된 PCIe 링크로 NIC에 급전하며, 올바르게 매핑된 토폴로지에서 RoCE로 측정된 풀 스피드(~185–190 Gb/s)가 나옵니다; 잘못 매핑된 경로는 절반 요율 근처에서 막히고, 튜닝 안 된 평범한 TCP는 훨씬 더 낮게 떨어집니다.",
          "**점보 프레임(MTU 9000)**을 종단 간에 켜고 dispatch 소켓에 `TCP_NODELAY`를 유지하세요(허브가 이미 설정합니다).",
          "가정하지 말고 *검증*하세요: 물리적 변경마다 허브 간 perftest를 돌리세요 — 95와 190 Gb/s의 차이는 측정하기 전엔 보이지 않습니다.",
          "**443 릴레이를 폴백 경로로** 유지하세요 — dial 정책은 공개 피어에는 직접 우선, NAT에는 릴레이입니다. 릴레이의 임무는 도달, 직접 링크의 임무는 속도입니다.",
        ],
      },
      {
        t: "p",
        md: "이것이 아키텍처에 왜 중요한가: 디코드 지연은 왕복 시간에 묶입니다(광섬유에서 ~5 µs/km — 대역폭과 무관한 물리 법칙). 그래서 굵은 파이프는 토큰당 지연을 낮추는 게 아니라 **prefill 속도, 배치-dispatch 처리량, 거의 즉각적인 전문가-슬라이스 배포**를 삽니다. 이것이 바로 2-계층 설계에서의 허브-계층 역할입니다: 굵은-파이프 계층은 용량, 릴레이 계층은 도달.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "부하-적응 확장",
    summary: "Kvasir의 MoE 서빙 경로가 트래픽에 따라 커지고 줄어든다: 코디네이터는 포화 상태에서 proven 워커를 다시 투입하고, 허브는 전문가 수요를 높여 유휴 노드를 모집한다 — 모두 pull 방식이라 NAT 뒤의 기기도 합류한다.",
    blocks: [
      {
        t: "p",
        md: "Kvasir의 MoE 서빙 경로는 부하에 맞춰 탄력적으로 확장되며, 두 개의 협력하는 계층으로 이루어집니다. 한산할 때 코디네이터는 토큰당 가장 빠른 경로를 위해 모든 것을 로컬에서 서빙하고; 포화되면 아래 두 계층이 스웜을 키웁니다 — 그리고 급증이 지나가면 다시 줄입니다.",
      },
      { t: "h2", kick: "Layer 1", text: "코디네이터 측: 부하-적응 dispatch" },
      {
        t: "p",
        md: "백본 코디네이터(전체 모델을 실행하는 `linkcpp-server`)는 라우팅된 전문가를 자기 GPU에서 직접(빠르고 로컬) 서빙하거나 원격 워커로 dispatch합니다. 백그라운드 스레드가 몇 초마다 어느 쪽일지 결정합니다:",
      },
      {
        t: "ul",
        items: [
          "자기 **자신의** 추론 슬롯을 폴링합니다. `busy >= saturation threshold`(기본값 2)이면 코디네이터는 부하 상태입니다.",
          "부하 상태에서, **proven** 워커가 살아 있으면 — `last_serve_ms > 0`인, 즉 실제로 전문가를 계산해 본 적 있는 워커 — 코디네이터는 정상 유휴 타임아웃을 넘겨서까지 그 워커로 계속 dispatch하며, 토큰당 지연보다 총처리량을 우선합니다.",
          "접속했지만 한 번도 서빙하지 않은 워커(릴레이를 dial했으나 계산은 하지 않은 폰)는 부하 상태에서 **모집되지 않습니다** — 그런 워커로 dispatch하면 빠른 로컬 경로를 느린 폴백으로 바꾸는 셈이기 때문입니다. 신규 워커도 짧은 grace window를 통해 첫 시도는 받습니다.",
          "자기 조회(self-query)는 시간 제한이 걸려 있어, 멈춘 폴이 dispatch를 막는 일이 절대 없습니다.",
        ],
      },
      { t: "h2", kick: "Layer 2", text: "허브 측: 부하-적응 모집" },
      {
        t: "p",
        md: "컨트롤 허브는 모든 MoE 코디네이터를 지켜보며 필요할 때 워커 풀을 키웁니다:",
      },
      {
        t: "ul",
        items: [
          "백그라운드 루프가 각 코디네이터의 슬롯을 폴링해 모델별 포화도를 기록합니다.",
          "모델이 포화된 동안 그 모델의 **유효 전문가-복제본 목표치**가 상향됩니다(base + boost). 그러면 커버리지 마켓은 이미 커버된 전문가를 다시 희소한 것으로 읽고, 살아 있는 워커가 **하나도 없는** 모델은 GGUF 메타데이터(전문가 수)로부터 시딩되어 수요가 0에서도 보이게 됩니다.",
          "유휴 노드는 수요 마켓(`/api/expert-volunteer`)을 폴링해 서빙할 `(layer, expert-range)` 슬라이스를 배정받습니다. 이들은 슬라이스를 내려받고, 릴레이를 dial하고, 커버리지를 등록합니다; 허브는 이들을 코디네이터의 dispatch 맵에 자동 배선합니다.",
          "부하가 빠지면 목표치가 원래대로 돌아가고 수요가 사라져, 여분 워커는 더 이상 dispatch되지 않고 age out됩니다.",
        ],
      },
      {
        t: "callout",
        md: "이 설계는 **pull 방식**입니다: 노드는 밀어붙여지는 대신 스스로 일을 요청하므로, NAT 뒤의 워커도 인바운드 연결 없이 참여합니다. Layer 2에서 모집된 노드가 서빙을 시작하면 **proven** 워커가 되고, 그러면 Layer 1이 그 워커를 부하 상태에서 계속 투입합니다 — 두 계층이 하나의 탄력적 루프로 합쳐집니다.",
      },
      {
        t: "p",
        md: "**관측성:** `GET /api/moe/recruitment`은 모델별 busy/saturation과 base 대 effective 목표치를 보고하고; `/api/expert-demand`는 `recruiting` 플래그를 담습니다.",
      },
    ],
  },
};
