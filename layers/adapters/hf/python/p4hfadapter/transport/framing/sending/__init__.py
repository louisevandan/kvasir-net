"""Single-owner blocking frame delivery; never replay after uncertain writes."""

from typing import BinaryIO

from p4hfadapter.transport.framing.encoding import encode_header
from p4hfadapter.transport.framing.errors import InvalidFrame, StreamClosed, TransportIOError
from p4hfadapter.transport.framing.limits import FrameLimits


class FrameSender:
    """Borrow a blocking binary stream; send is not an execution acknowledgment."""

    def __init__(self, stream: BinaryIO, limits: FrameLimits):
        self._stream = stream
        self._limits = limits
        self._failed = False

    def send(self, payload: bytes) -> None:
        if self._failed:
            raise StreamClosed("sender failed; delivery is uncertain and must not be retried")
        if type(payload) is not bytes:
            raise InvalidFrame("outgoing payload must be immutable bytes")
        header = encode_header(len(payload), self._limits)
        try:
            self._write_exact(header)
            self._write_exact(payload)
            try:
                self._stream.flush()
            except (OSError, ValueError) as error:
                raise TransportIOError("flush failed; delivery is uncertain") from error
        except BaseException:
            self._failed = True
            raise

    def _write_exact(self, data: bytes) -> None:
        view = memoryview(data)
        offset = 0
        while offset < len(view):
            try:
                written = self._stream.write(view[offset:])
            except (OSError, ValueError) as error:
                raise TransportIOError(f"write failed at {offset}/{len(view)} bytes") from error
            if type(written) is not int or not 0 < written <= len(view) - offset:
                raise TransportIOError("blocking write must report positive progress within the supplied length")
            offset += written
