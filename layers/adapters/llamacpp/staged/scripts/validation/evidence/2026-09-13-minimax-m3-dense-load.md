# MiniMax M3 dense GGUF 분산 적재

2026-09-13. 상태: **4-stage 적재, 단일 추론, capacity 1 순차 웨이브 통과**.
이 문서는 dense MiniMax M3의 메타데이터 호환 수정과 두 물리 Windows 호스트의
실제 적재·추론을 기록한다. capacity 4 동시 배치, 긴 context 및 서비스 성능 승인은
아직 아니다.

## 원인과 수정

모델은 `unsloth/MiniMax-M3-GGUF`, `MiniMax-M3-UD-Q5_K_S`, 8 shard,
298,756,339,264 bytes(278.239 GiB), 60 layers, 128 experts / 4 active다.
기존 `434ddbbc0` native는 첫 stage 초기화에서
`GGML_ASSERT(hparams.indexer_block_size > 0)`로 종료했다. 이 GGUF는 dense M3라
`has_msa=false`이고 indexer block size가 없다. `0015-official-minimax-m3-dense-gguf.patch`가
해당 assert를 `has_msa` 분기 안에서만 실행하도록 고쳤다. sparse MSA 경로의 검사는 유지한다.

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
입증하지 않는다. 이를 분리하기 위해 동일 workload를 sequence capacity 4에서 다시
실행하는 A/B가 다음 게이트다.

## 실패 경계와 남은 검증

첫 수정 전 실행은 dense metadata assert로 실패했다. 수정 후 첫 원격 실행은 새 배포
디렉터리에 CUDA runtime DLL이 없어 `0xc0000135`로 종료했고, `cublas64_13.dll`,
`cublasLt64_13.dll`, `cudart64_13.dll`을 배포 manifest에 포함해 해결했다.

최종 loader 상태는 `created=4`, `loaded=4`, `session_ready=4`, `passed=true`였다.
단일 및 순차 웨이브 뒤 capacity 4 재적재를 위해 네 native stage를 종료했다. 재적재의
비교 조건은 프롬프트, 도착 시각, max token, sampling, batch/ubatch, layer cut을 유지하고
sequence capacity와 총 KV context만 1/4K에서 4/16K로 바꾼다.

첫 웨이브 시도에서 이전 load 연결의 outer channel/generation을 재사용해
`event stream ended mid-frame`으로 즉시 실패했다. native stage 실행은 0회였고 성능
표본에서 제외했다. 새 channel/generation을 사용한 위 실행이 승인 표본이다.

로컬 원본은 `target/minimax-m3-load-20260913/` 아래 `full-load`, `single-inference`,
`sequential-wave`와 `deployment-manifest.json`이다. `target/`은 checkout 간 영구 증거가
아니므로 이 문서에는 판정에 필요한 identity, generation, topology와 경계를 함께 기록했다.

## 검증 명령

- compat manifest: valid, 26 patches
- patch classification: valid, upstream_fix 4 / stage_hook 18 / model_feature 4
- 같은 Windows CUDA Release 빌드의 CTest: 16/16 passed
- `npm run docs-lint`: 95 files clean
- `node tools/scripts/docs-lint.mjs --all`: 로컬 `.cache/llama-pipeline-upstream`의
  무시된 upstream 문서 122개를 미등록 저장소 문서로 세어 실패. 추적 문서 게이트와
  구분하며 이 결과를 clean으로 보고하지 않는다.
