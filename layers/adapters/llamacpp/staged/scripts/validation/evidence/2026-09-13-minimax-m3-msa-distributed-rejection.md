# MiniMax M3 MSA 분산 적재 거부

2026-09-13. 상태: **RED — 정상 MSA GGUF를 stage로 나눈 LOAD/SESSION과 추론은 지원되지 않는다.**

## 시험 구성

- 모델: `bartowski/MiniMax-M3-GGUF`, `MiniMax-M3-Q5_K_S`, 8 shard,
  295,228,545,984 bytes, MSA indexer metadata/tensor 포함
- 실행: `--flash-attn on --no-kv-unified`, context 4,096, sequence 1,
  batch 128, ubatch 64, f16 KV
- 장치: 중앙 RTX 3090만 사용하고 RTX 4080은 제외했다. 중간 stage는 Spark CUDA
  unified memory와 Mac21 Metal을 사용했다. 중앙 discrete stage의 expert weight만
  용량 부족분을 RAM에 배치했다.
- 연속 layer cut: 중앙 CUDA0 `0..14`, Spark CUDA0 `14..38`, Mac21 MTL0
  `38..50`, 중앙 CUDA0 `50..60`

원시 실행 자료는 `target/minimax-m3-msa-20260913/smoke/`에 있다. 이 디렉터리는
로컬 하드웨어 산출물이며 저장소 증거로 커밋하지 않는다.

## 판정

동일 구성의 native `--inspect-memory-plan`은 CUDA, Spark CUDA unified memory,
Metal의 모든 stage에서 exit 7로 거부됐다. 공통 최초 원인은 다음과 같다.

```text
llama_init_from_model: failed to initialize the context:
llama.cpp memory implementation does not declare stage-local residency support
llama.cpp failed to create the no-alloc context plan
```

계획 모드만의 결함인지 분리하기 위해 새 node id와 generation으로 실제 CREATE/LOAD를
한 번 실행했다. node 4개는 모두 생성됐지만 Mac21 stage가
`stage runtime initialization failed`, native exit 5로 LOAD를 거부했다. SESSION은
0건이며 질의는 제출하지 않았다. 따라서 정상 응답과 prefill/generated TPS는 모두
미측정이다. 구형 indexer 없는 dense-fallback 실행의 TPS와 비교하지 않는다.

## 코드 정리

compat patch `0029-minimax-m3-msa-stage-residency.patch`는 MSA wrapper에 static flag만
추가했다. 실제 수용 검사는 `llama_memory_i::supports_linkcpp_stage_residency()` virtual
호출이므로 그 flag는 wrapper의 수용 상태를 바꾸지 않았고, 주석의 지원 주장도 틀렸다.
불완전한 opt-in과 그에 따른 지원 문구를 제거하고 27-patch fail-closed 상태로 되돌렸다.
어댑터의 indexer 누락, flash attention 비활성, 다중 sequence와 unified KV 조합 거부는
유지한다.

실패 뒤 native 자식이 없음을 확인했다. 시험에 참여한 중앙, Spark, Mac21 agent는
재기동했고 세 endpoint가 다시 listen하는 것을 확인해 실패 node 원장을 회수했다.
이 버전에서 M3 stage residency 구현이나 재시험을 계속하지 않는다.
