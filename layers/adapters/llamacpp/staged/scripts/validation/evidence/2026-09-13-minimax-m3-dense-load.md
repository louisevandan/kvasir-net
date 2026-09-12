# MiniMax M3 dense GGUF 분산 적재

2026-09-13. 상태: **4-stage 적재와 세션 준비 통과, 추론 미실행**.
이 문서는 dense MiniMax M3의 메타데이터 호환 수정과 두 물리 Windows 호스트의
실제 적재만 기록한다. 정상 응답, 생성 TPS, 긴 context 및 서비스 성능 승인이 아니다.

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

## 실패 경계와 남은 검증

첫 수정 전 실행은 dense metadata assert로 실패했다. 수정 후 첫 원격 실행은 새 배포
디렉터리에 CUDA runtime DLL이 없어 `0xc0000135`로 종료했고, `cublas64_13.dll`,
`cublasLt64_13.dll`, `cudart64_13.dll`을 배포 manifest에 포함해 해결했다.

최종 loader 상태는 `created=4`, `loaded=4`, `session_ready=4`, `passed=true`이며 모델은
상주 상태로 남겼다. 이 실행은 inference를 제출하지 않았다. 다음 판정은 이 동일 session에
짧은 의미 프롬프트 1건을 보내 stage cut과 logits 반환을 확인하는 것이며, 그 전에는
“M3 추론 성공” 또는 TPS를 주장하지 않는다.

로컬 원본은 `target/minimax-m3-load-20260913/full-load/{config.json,load-progress2.json}`와
`deployment-manifest.json`이다. `target/`은 checkout 간 영구 증거가 아니므로 이 문서에는
판정에 필요한 identity, generation, topology와 경계를 함께 기록했다.

## 검증 명령

- compat manifest: valid, 26 patches
- patch classification: valid, upstream_fix 4 / stage_hook 18 / model_feature 4
- 같은 Windows CUDA Release 빌드의 CTest: 16/16 passed
- `npm run docs-lint`: 95 files clean
- `node tools/scripts/docs-lint.mjs --all`: 로컬 `.cache/llama-pipeline-upstream`의
  무시된 upstream 문서 122개를 미등록 저장소 문서로 세어 실패. 추적 문서 게이트와
  구분하며 이 결과를 clean으로 보고하지 않는다.
