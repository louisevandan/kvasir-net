# Local IPC byte framing

Status: blocking binary stream transport contract v1, implemented with the Python standard library.
P4 wire registration, the Rust bridge, the model worker and tensor/identity validation are not implemented yet.
This contract does not interpret model bytes.

## Format

| byte offset | Length | Meaning |
| --- | --- | --- |
| 0 | 4 | ASCII `P4HF` |
| 4 | 1 | version `1` |
| 5 | 3 | reserved; all 0 |
| 8 | 8 | unsigned big-endian payload byte count |
| 16 | declared length | opaque payload |

A 0-byte payload is a valid frame. Only EOF before a header starts is a normal end; EOF after part of a header/body has been received is a failure.
The caller must specify a positive limit with `FrameLimits(max_payload_bytes)`. Unknown magic/version/reserved values and
over-limit sizes are rejected before body allocation/read. No capability negotiation, compression or tensor conversion is performed.

## Python consumption path

The imports below are used in an environment where the repository's `python/` is on the Python search path.

```python
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender

limits = FrameLimits(max_payload_bytes=1024)  # example limit; not a model execution budget
receiver = FrameReceiver(binary_input_stream, limits)
sender = FrameSender(binary_output_stream, limits)
payload = receiver.receive()  # bytes, or None on clean EOF
if payload is not None:
    sender.send(payload)
```

The sender accepts only immutable `bytes`, checks the size, then writes the header and body in order and flushes.
An input rejection writes 0 bytes, and the sender can be reused. Short reads/writes continue with only the remaining bytes.
I/O failures, writes that make no progress, receive truncation and invalid received headers leave that direction in a failed state.
Subsequent calls are rejected with `StreamClosed`; there is no ad-hoc resynchronization and no resending of the same payload.
A `ValueError` from a closed stream and OS I/O errors become a `TransportIOError` that preserves the cause.

## Ownership and limits

- The caller owns the stream and its shutdown, deadlines and concurrent access control. Each direction is used serially by one caller.
- The transport does not close the stream. On failure, the caller must clean up both directions of that connection and the upper-level execution.
- Send input bytes are not modified. After a partial write/flush failure, whether the peer received or executed the data is uncertain.
- The internal buffer being received exists only within the call; ownership of the completed bytes passes to the caller on return.
  No queue keeps state, output or requests after completion. An exception traceback can hold the buffer, so take care when preserving upper-level errors.
- The receive body buffer plus the copy into the returned bytes need about 2× the payload, plus a read chunk of up to 64 KiB and header and Python object costs.
  The stream's own buffering and the caller's queue are separate budgets. The frame limit is not a full memory reservation ledger.
- A successful flush is not evidence of device completion, P4 acceptance, settlement or KV release. Those connections must be implemented separately per the [planned API](../../history/initial/api.md).
- Nonblocking streams, read timeouts, forced process termination, thread safety, cross-host transport and tensor schemas are not supported.

For verification, see the [test plan](../../../tests/plans/framing-20260913.md) and the
[run report](../../../tests/reports/framing/20260913_174516.md).
