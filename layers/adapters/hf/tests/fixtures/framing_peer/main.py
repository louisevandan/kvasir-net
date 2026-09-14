"""Transport-only subprocess fixture; this is not a model worker."""

import hashlib
import json
from pathlib import Path
import sys

from p4hfadapter.transport.framing.errors import FramingError
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from p4hfadapter.transport.framing.sending import FrameSender
import p4hfadapter.transport.framing.decoding as decoding


def main() -> int:
    source = Path(decoding.__file__).resolve()
    print(json.dumps({"decoder_source": str(source), "decoder_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}),
          file=sys.stderr, flush=True)
    limits = FrameLimits(1024)
    receiver = FrameReceiver(sys.stdin.buffer, limits)
    sender = FrameSender(sys.stdout.buffer, limits)
    consumed = 0
    try:
        while (payload := receiver.receive()) is not None:
            consumed += 1
            sender.send(payload)
    except FramingError as error:
        print(json.dumps({"error": type(error).__name__, "consumed": consumed}), file=sys.stderr, flush=True)
        return 2
    print(json.dumps({"consumed": consumed}), file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
