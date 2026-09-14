# 로컬 IPC 바이트 framing

지위: Python 표준 라이브러리로 구현한 blocking binary stream 전송 계약 v1입니다.
P4 wire 등록·Rust bridge·모델 worker·tensor/identity 검증은 아직 구현하지 않았습니다.
이 계약은 모델의 bytes를 해석하지 않습니다.

## 형식

| byte offset | 길이 | 의미 |
| --- | --- | --- |
| 0 | 4 | ASCII `P4HF` |
| 4 | 1 | version `1` |
| 5 | 3 | reserved; 모두 0 |
| 8 | 8 | unsigned big-endian payload byte 수 |
| 16 | 명시 길이 | opaque payload |

0-byte payload는 정상 frame입니다. header 시작 전 EOF만 정상 종료이며 header/body 일부 수신 뒤 EOF는 실패입니다.
호출자는 `FrameLimits(max_payload_bytes)`로 양의 상한을 반드시 지정합니다. 알 수 없는 magic/version/reserved와
상한 초과를 body allocation/read 전에 거부합니다. 지원 capability 협상·압축·tensor 변환은 수행하지 않습니다.

## Python 소비 경로

아래 import는 저장소의 `python/`을 Python 검색 경로에 둔 환경에서 사용합니다.

```python
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender

limits = FrameLimits(max_payload_bytes=1024)  # 예시 상한; 모델 실행 예산이 아님
receiver = FrameReceiver(binary_input_stream, limits)
sender = FrameSender(binary_output_stream, limits)
payload = receiver.receive()  # bytes 또는 clean EOF의 None
if payload is not None:
    sender.send(payload)
```

sender는 immutable `bytes`만 받고 크기를 확인한 뒤 header/body를 순서대로 쓰고 flush합니다.
입력 거절은 write 0이며 sender를 재사용할 수 있습니다. 짧은 read/write는 남은 byte만 이어서 처리합니다.
I/O 실패·진행 없는 write·수신 절단·잘못된 수신 header는 해당 방향을 실패 상태로 남깁니다.
후속 호출은 `StreamClosed`로 거부하며 임의 재동기화나 같은 payload 재전송을 하지 않습니다.
닫힌 stream의 `ValueError`와 OS I/O 오류는 원인을 보존한 `TransportIOError`가 됩니다.

## 소유권과 제한

- 호출자가 stream과 종료·deadline·동시 접근 제어를 소유합니다. 한 방향은 한 호출자가 직렬 사용합니다.
- transport는 stream을 닫지 않습니다. 실패하면 호출자가 해당 연결의 양쪽 방향과 상위 실행을 정리해야 합니다.
- 송신 입력 bytes는 변경하지 않습니다. partial write/flush 실패 후 상대 수신·실행 여부는 불확실합니다.
- 수신 중인 내부 buffer는 호출 내에만 있고, 완성된 bytes의 소유권은 반환 시 호출자에게 넘어갑니다.
  완성 후 상태·출력·요청을 보관하는 큐는 없습니다. 예외 traceback이 buffer를 보유할 수 있으므로 상위 오류 보존 시 주의합니다.
- 수신 body buffer와 반환 bytes 복사로 payload의 약 2배, 최대 64 KiB read chunk, header와 Python 객체 비용이
  필요합니다. stream 자체 buffering과 호출자 큐는 별도 예산입니다. frame 상한은 전체 메모리 예약 원장이 아닙니다.
- flush 성공은 장치 완료·P4 수용·정산·KV 해제 증거가 아닙니다. 해당 연결은 [예정 API](../../history/initial/api.md)를 따로 구현해야 합니다.
- nonblocking stream·read timeout·프로세스 강제 종료·thread safety·호스트 간 transport·tensor schema는 미지원입니다.

검증은 [시험 계획](../../../tests/plans/framing-20260913.md)과
[실행 보고](../../../tests/reports/framing/20260913_174516.md)를 참조합니다.
