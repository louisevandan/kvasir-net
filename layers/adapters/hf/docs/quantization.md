# 양자화 설계

지위: 대화에서 논의한 설계와 후보. 실제 모델·장치에서 검증한 양자화 조합은 아직 없습니다.
공식 문서 근거는 [참고 자료](history/initial/references.md)의 Q1~Q9입니다.

## 분리할 세 축

| 대상 | 줄이는 것 | 첫 제안 |
| --- | --- | --- |
| 가중치 | 상주 모델 메모리·가중치 읽기 대역폭 | 4비트 후보를 장치별 검증 |
| KV/recurrent 상태 | 긴 context·동시 요청의 상태 메모리 | 모델의 검증된 원래 정밀도; KV는 FP16/BF16 후보 |
| 경계 활성값 | 노드 간 전송량 | FP16/BF16 유지 |

가중치가 4비트여도 입력/출력/KV가 자동으로 4비트가 되지 않습니다.
W4A16은 가중치 4비트·활성값 16비트 구성이고 FP8 W8A8과 같은 계산 경로가 아닙니다.
일부 norm/recurrent 누산은 FP32를 요구할 수 있으므로 모델 전체 dtype를 강제로 통일하지 않습니다.

```text
X(FP16/BF16) + QW(4bit) + scale/zero-point/packing metadata
  -> 지원되는 양자화 matmul 커널
  -> Y(FP16/BF16)
```

압축된 가중치를 커널 내부에서 복원해 계산하는 것은 정상 경로입니다.
모델 전체를 전역 메모리에 BF16으로 풀어 보관하는 fallback과 구별합니다.

## 가중치를 준비하는 세 경로

| 경로 | 절차 | 결정 근거 |
| --- | --- | --- |
| 공개 사전 양자화 | 제조사/배포자의 checkpoint·config·revision을 확인해 직접 적재 | 모델별 품질 근거, 정확한 포맷과 커널 호환 |
| 적재 중 양자화 | 원본 tensor를 bitsandbytes/HQQ/Metal 등의 지원 레이어로 변환 | 빠른 모델 대응, 보정 데이터 필요 여부, 적재 peak |
| 오프라인 PTQ | 대표 입력으로 GPTQ/AWQ 등의 보정을 수행하고 양자화 결과를 저장 | 반복 배포, 품질 제어, 전처리 자원과 시간 |

bitsandbytes의 표준 Linear 교체, 보정 데이터가 필요 없는 HQQ, GPTQModel의 보정 기반 GPTQ가 후보입니다.
AWQ도 알고리즘 후보지만 오래된 AutoAWQ/AutoGPTQ 설치 예제를 그대로 사용하지 않습니다.
정확한 모델 구조·라이브러리 버전·유지되는 backend를 확인하고 고정합니다.
이미 존재하는 신뢰 가능한 양자화 아티팩트를 먼저 조사하며, 직접 재양자화를 자동 선택하지 않습니다.
NF4 추론에는 LoRA 학습이 필수가 아닙니다. 양자화 적용 자체와 QLoRA fine-tuning을 구분합니다.

## P4용 준비 흐름

1. 원본 모델 ID/revision, 가중치 hash, tokenizer/chat template, 제조사 모델 코드를 고정합니다.
2. 모델별 양자화 대상과 제외 모듈, bit/group size, 대칭성, scale dtype, kernel layout을 정합니다.
3. 보정이 필요한 경우 대표 workload를 고정하고 평가 데이터와 분리합니다.
4. 양자화 결과를 검증한 뒤 레이어의 원래 이름과 모든 보조 tensor를 보존해 저장합니다.
5. 각 stage의 담당 레이어·공유 모듈·양자화 아티팩트를 manifest에 결속해 배포합니다.
6. 각 노드는 담당 tensor만 읽고 실제 커널로 warmup한 뒤 peak/resident 메모리를 보고합니다.
7. 노드 배치가 달라져도 양자화 레시피가 같으면 재양자화를 피합니다. 필요 시 물리 파일만 다시 묶습니다.

checkpoint의 파일 shard는 stage 경계와 같지 않습니다. 가중치·scales·zero points·group index·packing 정보까지
필요한 tensor 집합을 선택하는 로더를 구현해야 합니다. 전체 모델을 각 노드에 먼저 적재하고 나머지를 지우지 않습니다.

양자화 준비를 먼저 한다는 뜻은 전체 BF16 모델이 단일 GPU에 들어가야 한다는 뜻이 아닙니다.
레이어 순차 처리·CPU 오프로딩 가능 여부와 보정 활성값/workspace 크기를 도구별로 확인합니다.
초대형 모델의 디스크 스트리밍·분산 보정은 자동으로 제공된다고 가정하지 않습니다.
GPTQ/AWQ 보정은 앞선 레이어 출력에 의존하므로 서로 다른 노드가 임의 입력으로 독립 보정해서는 안 됩니다.

## 모델별 Python 대응

표준 `nn.Linear`이면 양자화 레이어로 교체하고 외부 입출력 형상을 유지하는 경로부터 확인합니다.
MoE의 fused expert tensor, 직접 `matmul`, 커스텀 op, packed projection은 Linear 교체만으로 커버되지 않을 수 있습니다.
실제 적용된 모듈 목록·압축 byte·예외 모듈을 보고합니다. 모델 일부만 양자화됐는데 전체 4비트로 표시하지 않습니다.
embedding/lm_head tied weights, norm, router, 민감한 recurrent/state 연산은 모델별 제외/공유 규칙을 갖습니다.
양자화 과정의 scale folding 등이 인접 모듈을 바꾸면 변경된 모듈까지 함께 아티팩트에 결속합니다.

## 이기종 노드

레이어 단위 PP에서는 stage별 내부 가중치 형식이 달라도 합의된 경계 tensor 형식으로 연결할 수 있습니다.
예: NVIDIA stage의 GPTQ 4bit와 Mac stage의 Metal 4bit 사이에 BF16 텐서를 전달하는 구성입니다.
이는 설계 가능성이고 해당 모델의 실제 검증 결과가 아닙니다.

| 장치 | 조사할 후보 | 미리 확정하지 않을 것 |
| --- | --- | --- |
| NVIDIA CUDA | bitsandbytes 4bit, GPTQModel와 지원 CUDA/Marlin 커널 | 모든 bit/group/대칭성·GPU 세대에서 같은 속도 |
| Apple MPS | Transformers MetalConfig의 2/4/8bit와 Metal 커널 | 같은 파일이 CUDA에서도 압축 실행됨 |
| AMD ROCm | 해당 버전 GPTQModel/HQQ/torchao 등 실제 지원 조합 조사 | CUDA 커널을 그대로 실행 가능 |
| FP8 지원 장치 | FineGrainedFP8 또는 지원 compressed-tensors FP8 경로 | FP8 파일만 있으면 구형 GPU에서도 가속 |
| CPU | CPU용 압축 연산/오프로딩을 별도 검증 | GPU용 양자화 설정이 CPU 메모리도 동일 비율 절감 |

같은 원본 revision에서 장치별 아티팩트를 만들고 전체 조합에 대해 품질을 평가합니다.
서로 다른 양자화 레시피는 단순 포맷 repack과 다릅니다. 다른 bit/그룹/보정은 새로운 수치 모델입니다.
가능하면 원본 가중치에서 생성하며, 이미 손실된 4비트를 복원해 다른 4비트로 재양자화하면 손실이 누적됩니다.
가중치 변환과 stage 이동을 자동 승인하지 않으며, tied/shared 상태가 걸친 stage는 특별 취급합니다.

## 실제 압축 실행 확인

- 적재 직후뿐 아니라 첫 forward와 장문/최대 배치 뒤의 메모리를 측정합니다.
- 실제 선택된 모듈 구현·커널·dtype·packing과 fallback 이유를 기록합니다.
- compressed-tensors는 저장 형식과 실행 모드가 다릅니다. 현재 공식 문서의 기본 경로는 첫 forward에서 복원하며,
  최적화 옵션도 지원 scheme/장치에 한정됩니다. 저장 byte로 실행 메모리를 승인하지 않습니다.
- GGUF도 읽기 지원을 압축 실행 보장으로 해석하지 않습니다. 현재 공식 문서의 제한된 GGUF 커널 경로와
  legacy dequantization을 모델·장치·버전별로 확인합니다. GGUF 재사용은 첫 구현의 필수 조건이 아닙니다.
- 성능 목적의 LOAD는 manifest가 요구한 압축 커널 대신 전량 dequantize하는 경우 명시적으로 거부합니다.
  진단용 dense fallback은 별도 arm과 실제 dtype으로 보고합니다.

## 용량·성능·품질

순수 가중치 이론 하한은 parameter_count × bits / 8 bytes입니다.
예를 들어 100B의 순수 4비트 payload는 50 GB(10진)이며, scales/비양자화 모듈/KV/workspace가 추가됩니다.
MoE의 활성 파라미터 수는 전체 가중치 상주 용량을 대신하지 않습니다.
stage별 예산은 가중치 + metadata + KV/state + activation + 커널 workspace + IPC/전송 buffer + runtime 여유입니다.
공유 unified RAM과 여러 stage가 같은 pool을 쓰는 경우 중복 가용량을 계산하지 않습니다.

용량 절감이 TPS 개선을 보장하지 않습니다. prefill/decode·배치·context별로 변환/커널/통신 비용을 측정합니다.
품질 기준선은 원본 고정밀도, 양자화 기준선은 동일 양자화의 비분산 실행으로 분리합니다.
혼합 장치 레시피의 전체 오차와 원본 대비 정상 응답 품질을 별도로 평가합니다.
구체적인 허용 오차·품질 하락·SLO 수치는 첫 모델 manifest에서 시험 전에 고정합니다.
