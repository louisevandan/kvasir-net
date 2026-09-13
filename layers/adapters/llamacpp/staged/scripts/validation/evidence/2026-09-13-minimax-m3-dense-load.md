# MiniMax M3 구형 dense-fallback GGUF 분산 적재

2026-09-13. 상태: **구형 변환본의 4-stage 적재, 단일 추론, capacity 1 및 capacity 4 웨이브 통과**.
이 문서는 MSA 정보가 누락된 MiniMax M3 변환본의 호환 수정과 두 물리 Windows 호스트의
실제 적재·추론을 기록한다. capacity 4 결과는 짧은 고정 workload의 수용 증거이며,
긴 context 및 서비스 성능 승인은 아직 아니다.

## 2026-09-13 MSA 재감사와 판정 정정

이 실행은 MiniMax M3의 정상 sparse attention 실행을 승인하지 않는다. 사용한 여덟
GGUF shard의 전체 header를 다시 검사한 결과 `minimax-m3.attention.indexer.*`
metadata가 0개이고 `indexer` tensor도 0개였다. 따라서 어댑터 옵션으로 MSA를 켤 수
있는 artifact가 아니다. metadata만 만들어 넣어도 필요한 indexer projection/norm
weight가 없으므로 의미가 복구되지 않는다.

현재 llama.cpp의 MSA 실행 조건도 함께 감사했다. 정상 MSA GGUF라도 flash attention이
꺼져 있거나, `n_seq_max > 1`에서 unified KV를 쓰면 dense attention으로 fallback한다.
아래 capacity 4 실행은 `--flash-attn on --kv-unified`였으므로 정상 MSA GGUF로 교체해도
같은 계획을 재사용할 수 없다. staged 서버의 비통합 KV 경로는 요청별 HOP 실행으로
안전하게 fallback하므로, 첫 수용 구성은 `--flash-attn on --no-kv-unified`다. 비통합
KV에서 여러 sequence를 한 native decode 호출로 합치는 최적화는 별도 구현·검증 대상이다.

비교 대상으로 확인한 `bartowski/MiniMax-M3-GGUF` Q5_K_S 첫 shard는 indexer head 4,
key length 128, top-k 16, block size 128, local block 1과 실제 indexer tensor 28개를
포함한다. 같은 양자화의 여덟 shard를 NAS에 준비한 뒤 동일 LAN의 CUDA·Metal 노드로
확대한다. 제품 어댑터는 이제 MiniMax M3에서 indexer 누락, flash attention 비활성,
다중 sequence와 unified KV의 조합을 명시적으로 거부한다. 이 게이트의 로컬 컴파일
시험은 통과했으며 정상 MSA GGUF의 다중 머신 실기는 아직 진행 중이다.

## 원인과 수정

모델은 `unsloth/MiniMax-M3-GGUF`, `MiniMax-M3-UD-Q5_K_S`, 8 shard,
298,756,339,264 bytes(278.239 GiB), 60 layers, 128 experts / 4 active다.
기존 `434ddbbc0` native는 첫 stage 초기화에서
`GGML_ASSERT(hparams.indexer_block_size > 0)`로 종료했다. 이 GGUF는 dense M3라
`has_msa=false`이고 indexer block size가 없다. `0015-official-minimax-m3-dense-gguf.patch`가
해당 assert를 `has_msa` 분기 안에서만 실행하도록 고쳤다. 이 수정은 당시 구형 변환본의
실행을 가능하게 한 호환 조치이며, 위 재감사 뒤에는 제품 수용 경로에서 거부한다.

수정 커밋은 `f4b0feb62`. pristine `434ddbbc0`에 26개 patch를 순서대로 다시 적용했고
manifest 검증 7/7을 통과했다. 최종 identity는 다음과 같다.

| 항목 | 값 |
| --- | --- |
| upstream | `434ddbbc0e30522e897670681e503b797c12b7c1` |
| patch set | `ebf53e855f11e5a848ccddf34c8cc517056f839d91148b702c04f4eb4c218379` |
| patched tree | `73b67197837f10811bb2560ec93324886c18360a` |
| Windows CUDA server SHA256 | `b4e2f59965ffebb6bc98af798e1cc30ebb2cabbfb4b946854310f64d3584673f` |

## 실제 적재

실행 generation은 `1789245852580`, session은 `minimax-m3-1789245852580`이다.
M42 server2의 RTX 3090 두 장과 중앙 호스트의 RTX 4080 + RTX 3090을 사용했다.
각 stage는 15개 layer를 소유하고, 소유 layer의 expert FFN은 CPU RAM,
나머지 layer body와 4K KV는 지정 CUDA device에 배치했다. batch/ubatch는 128/64,
sequence capacity는 1이다.

| stage | 호스트 / device | layer / KV range | 결과 |
| --- | --- | --- | --- |
| 0 | M42 / CUDA0 | `[0,15)` | loaded, session ready |
| 1 | M42 / CUDA1 | `[15,30)` | loaded, session ready |
| 2 | 중앙 / CUDA0 | `[30,45)` | loaded, session ready |
| 3 | 중앙 / CUDA1 | `[45,60)` | loaded, session ready |

네 `LOADED` 응답은 upstream, patch set, stage wire ABI가 모두 같았다.
네 `SESSION_READY` 응답도 모두 수신했다. 전체 소요는 1,653.6초이며 대부분 NAS에서
stage당 약 70 GiB의 expert weight를 읽는 시간이다. 완료 직후 중앙 호스트는 RAM
65.1 GiB free, GPU 사용 메모리 4,731 / 10,784 MiB였고, M42는 RAM 약 105.7 GiB free,
GPU 사용 메모리 4,839 / 4,839 MiB였다. 이 순간의 GPU utilization 표본은 성능 지표가 아니다.

Spark, Ubuntu 노트북, Mac mini 두 대, TUF에는 agent만 남고 native stage는 0개였다.
MI250 두 대에도 P4 프로세스가 없었다. 따라서 이번 결과는 두 물리 호스트·네 GPU의
CUDA 적재 증거이며 전체 LAN 클러스터나 Metal/ROCm 적재 증거가 아니다.

## 단일 출력과 capacity 1 웨이브

같은 상주 세션에 MiniMax M3의 raw chat template와 thinking-disabled prefix를 적용한
한국어 설비 계산 프롬프트를 제출했다. 단일 요청은 157 prefill token과 EOS까지의
125 generated token을 처리해 `completed=1`, `released=1`, `passed=true`였다. TTFT는
187.043초, 논리 prefill은 0.839 token/s, 생성 구간은 141.834초로 약 0.88 token/s였다.
응답은 `17 L/min × 13 min = 221 L`를 계산하고 일정 유량 가정 및 두 현장 측정값을
구분했다. 229자 응답에 UTF-8 replacement character는 없었다.

이어 같은 capacity 1 세션에 2건을 즉시, 60초 뒤 2건을 추가하는 고정 workload를
실행했다. 네 요청 모두 EOS로 완료·해제됐고 84, 138, 42, 162 L 계산과 요청별 설명을
충족했다. UTF-8 replacement character는 네 응답 모두 0이었다.

| 항목 | 값 |
| --- | --- |
| 완료 / 해제 / 오류 | 4 / 4 / 없음 |
| wall / generated token | 334.387초 / 426 |
| aggregate generated TPS | 1.274 token/s |
| 총 prefill token | 525 |
| physical batch | 436 (prefill 10 / decode 426 / mixed 0) |
| 물리 batch 폭 | 평균 2.181, 최대 64, UBATCH 64 평균 채움 3.408% |
| ready sequence / open batch 관측 최대 | 1 / 0 |

이 결과는 backlog를 capacity 1 슬롯으로 순차 처리하고 슬롯을 네 번 재사용한 증거다.
동시 sequence가 없었으므로 in-flight batch 포화나 capacity 상향의 성능 효과를
입증하지 않는다. 이를 확인하기 위해 동일 workload를 sequence capacity 4에서 다시
실행했다.

## Capacity 4 동시 웨이브

sequence capacity와 총 KV context를 1/4K에서 4/16K로 늘리고 같은 네 프롬프트,
도착 시각(2건 즉시, 2건 60초 뒤), max token, sampling, batch/ubatch 128/64 및 layer
cut을 유지했다. M42에는 NAS 로그인 세션에 의존하지 않도록 같은 여덟 shard를 로컬
SSD로 복사했다. 각 파일 길이와 총 298,756,339,264 bytes 및 전송 종료 코드는
일치한다. 제한 시간 안에 278 GiB를 다시 전부 읽는 별도 SHA256 비교는 하지 않았으나,
native loader가 여덟 shard 전체를 읽고 같은 model metadata로 네 stage를 적재했다.

두 호스트의 agent가 loopback 주소로 광고하므로 기존 helper 포트 42003(M42)과
42004(중앙)를 SSH local/reverse tunnel로 연결했다. 중앙의 52005는 Windows excluded
dynamic port range 51952--52151에 포함돼 bind할 수 없었다. agent 바이너리 SHA256은
두 호스트에서 `c551db0c77d93cff1d9f87ba2b00622bca6627cc6b67b2c6c6d84507e9dc234b`로
같았다.

첫 capacity 4 적재는 네 `LOADED`와 네 `SESSION_READY`까지 통과했지만, 웨이브를
시작한 뒤 약 100.5초에 M42 stage 1 native process가 종료됐다. Windows Application
Error는 fault module `nvptxJitCompiler64.dll`, exception `0xc0000005`, offset
`0x5853a`, WER report ID `d4f85b29-b61d-4e80-826d-8ca9b6bf1875`를 기록했다.
직전 native log에는 `CUDA graph warmup reset`이 반복됐다. 이 실행은 응답 조각 `**`만
받았고 `completed=0`, `released=0`이므로 실패이며 성능 표본이 아니다.

이 실패에 직접 대응해 네 stage에 `GGML_CUDA_DISABLE_GRAPHS=1`을 적용하고 나머지
조건을 유지해 generation `1789254973468`, session
`minimax-m3-batch-1789254973468`로 다시 적재했다. 네 `LOADED`와 네
`SESSION_READY`를 모두 받았고 적재 소요는 1,458.3초였다. 이어 실행한 같은 웨이브는
네 요청을 모두 EOS까지 완료하고 네 slot을 모두 해제했다. 네 응답은 각각 84, 138,
42, 162 L를 올바르게 계산했고 UTF-8 replacement character는 모두 0이었다.

| 항목 | capacity 1 | capacity 4 + CUDA graph off | 변화 |
| --- | ---: | ---: | ---: |
| 완료 / 해제 | 4 / 4 | 4 / 4 | 동일 |
| wall | 334.387초 | 151.745초 | -54.6% |
| generated token | 426 | 407 | sampling 결과 차이 |
| aggregate generated TPS | 1.274 | 2.682 | +110.5% |
| 총 prefill token | 525 | 525 | 동일 |
| physical batch | 436 | 416 | -20 |
| prefill / decode / mixed batch | 10 / 426 / 0 | 9 / 406 / 1 | mixed 1회 |
| batch 폭 평균 / 최대 | 2.181 / 64 | 2.240 / 64 | 평균 +2.7% |
| UBATCH 평균 채움 | 3.408% | 3.501% | +0.093%p |
| ready sequence / open batch 관측 최대 | 1 / 0 | 3 / 3 | 동시 진행 확인 |

요청별 TTFT는 capacity 1에서 16.155, 87.572, 109.185, 221.790초였고 capacity 4
실행에서 18.295, 26.356, 22.760, 22.231초였다. 첫 요청은 2.140초 느려졌지만 뒤의
세 요청은 각각 61.216, 86.425, 199.559초 빨라졌다. 이는 capacity 1의 긴 head-of-line
대기를 제거하고 여러 sequence를 실제로 진행했다는 증거다.

처리량 증가는 capacity 4와 CUDA graph 비활성화를 함께 바꾼 arm 사이의 값이다.
따라서 capacity 상향만의 인과 효과나 CUDA graph 비용을 이 실행으로 분리할 수 없다.
또한 물리 batch 평균 폭은 거의 그대로이고 p50/p90은 모두 1행이어서 UBATCH 포화는
일어나지 않았다. 이 결과가 승인하는 것은 네 동시 요청의 정상 완료, tail TTFT 감소와
해당 결합 조건에서의 aggregate TPS 증가다. 긴 prompt 혼합, 지속 arrival, GPU utilization,
서비스 지연 분포와 최적 capacity는 별도 게이트다.

## 실패 경계와 남은 검증

첫 수정 전 실행은 dense metadata assert로 실패했다. 수정 후 첫 원격 실행은 새 배포
디렉터리에 CUDA runtime DLL이 없어 `0xc0000135`로 종료했고, `cublas64_13.dll`,
`cublasLt64_13.dll`, `cudart64_13.dll`을 배포 manifest에 포함해 해결했다.

최종 capacity 4 loader 상태는 `created=4`, `loaded=4`, `session_ready=4`,
`passed=true`였다. 웨이브 뒤 네 native stage는 모두 상주했다. M42의 두 GPU는 각각
4,974 MiB를 사용했고 idle utilization 표본은 0%였다.

capacity 1 검증을 준비하던 첫 harness 시도에서 이전 load 연결의 outer
channel/generation을 재사용해
`event stream ended mid-frame`으로 즉시 실패했다. native stage 실행은 0회였고 성능
표본에서 제외했다. 새 channel/generation을 사용한 위 실행이 승인 표본이다.

로컬 원본은 `target/minimax-m3-load-20260913/` 아래 `full-load`, `single-inference`,
`sequential-wave`, `batch-load`, `batch-wave`와 `deployment-manifest.json`이다.
`target/`은 checkout 간 영구 증거가 아니므로 이 문서에는 판정에 필요한 identity,
generation, topology와 경계를 함께 기록했다.

## 검증 명령

- compat manifest: valid, 26 patches
- patch classification: valid, upstream_fix 4 / stage_hook 18 / model_feature 4
- 같은 Windows CUDA Release 빌드의 CTest: 16/16 passed
- `npm run docs-lint`: 96 files clean
- `node tools/scripts/docs-lint.mjs --all`: 로컬 `.cache/llama-pipeline-upstream`의
  무시된 upstream 문서 122개를 미등록 저장소 문서로 세어 실패. 추적 문서 게이트와
  구분하며 이 결과를 clean으로 보고하지 않는다.
