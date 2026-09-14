"""Single-owner blocking frame receipt; failure fences this stream direction."""

from typing import BinaryIO

from p4hfadapter.transport.framing.decoding import decode_header
from p4hfadapter.transport.framing.errors import StreamClosed, TransportIOError, TruncatedFrame
from p4hfadapter.transport.framing.layout import HEADER
from p4hfadapter.transport.framing.limits import FrameLimits


class FrameReceiver:
    """Borrow a blocking binary stream; the caller owns closing and deadlines."""

    def __init__(self, stream: BinaryIO, limits: FrameLimits):
        self._stream = stream
        self._limits = limits
        self._failed = False
        self._eof = False

    def receive(self) -> bytes | None:
        if self._failed:
            raise StreamClosed("receiver failed; do not resynchronize or retry")
        if self._eof:
            return None
        try:
            header = self._read_exact(HEADER.size, allow_empty_eof=True)
            if header is None:
                self._eof = True
                return None
            size = decode_header(header, self._limits)
            return self._read_exact(size)
        except BaseException:
            self._failed = True
            raise

    def _read_exact(self, size: int, *, allow_empty_eof: bool = False) -> bytes | None:
        buffer = bytearray(size)
        offset = 0
        while offset < size:
            requested = min(size - offset, 64 * 1024)
            try:
                chunk = self._stream.read(requested)
            except (OSError, ValueError) as error:
                raise TransportIOError(f"read failed at {offset}/{size} bytes") from error
            if type(chunk) is not bytes or len(chunk) > requested:
                raise TransportIOError("blocking read must return bytes within the requested length")
            if not chunk:
                if allow_empty_eof and offset == 0:
                    return None
                raise TruncatedFrame(f"EOF at {offset}/{size} bytes")
            buffer[offset:offset + len(chunk)] = chunk
            offset += len(chunk)
        return bytes(buffer)
