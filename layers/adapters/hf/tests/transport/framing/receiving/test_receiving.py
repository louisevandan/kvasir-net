import io
import unittest

from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge, StreamClosed, TransportIOError, TruncatedFrame
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.receiving import FrameReceiver
from tests.fixtures.receiving_streams import FailedInput, FragmentedInput, HeaderOnlyInput, NotReadyInput


class ReceivingTests(unittest.TestCase):
    def test_short_reads_and_consecutive_frames(self):
        raw = b"P4HF\x01\x00\x00\x00" + (3).to_bytes(8, "big") + b"abc"
        stream = FragmentedInput(raw + b"P4HF\x01" + b"\x00" * 11)
        receiver = FrameReceiver(stream, FrameLimits(3))
        self.assertEqual(receiver.receive(), b"abc")
        self.assertEqual(receiver.receive(), b"")
        self.assertIsNone(receiver.receive())
        self.assertIsNone(receiver.receive())
        self.assertFalse(stream.closed)

    def test_oversize_rejected_before_body_read_and_terminal(self):
        stream = HeaderOnlyInput(b"P4HF\x01\x00\x00\x00" + (5).to_bytes(8, "big"))
        receiver = FrameReceiver(stream, FrameLimits(4))
        with self.assertRaises(PayloadTooLarge):
            receiver.receive()
        with self.assertRaises(StreamClosed):
            receiver.receive()
        self.assertEqual(stream.calls, 1)

    def test_every_truncation_is_terminal(self):
        raw = b"P4HF\x01\x00\x00\x00" + (3).to_bytes(8, "big") + b"abc"
        for end in range(1, len(raw)):
            with self.subTest(end=end):
                stream = io.BytesIO(raw[:end])
                receiver = FrameReceiver(stream, FrameLimits(3))
                with self.assertRaises(TruncatedFrame):
                    receiver.receive()
                with self.assertRaises(StreamClosed):
                    receiver.receive()

    def test_bad_header_cannot_scan_to_next_frame(self):
        raw = b"P4HF\x02" + b"\x00" * 11 + b"P4HF\x01" + b"\x00" * 11
        stream = io.BytesIO(raw)
        receiver = FrameReceiver(stream, FrameLimits(4))
        with self.assertRaises(InvalidFrame):
            receiver.receive()
        self.assertEqual(stream.tell(), 16)
        with self.assertRaises(StreamClosed):
            receiver.receive()
        self.assertEqual(stream.tell(), 16)

    def test_read_failure_preserves_cause_and_prevents_retry(self):
        error = OSError("read failed")
        receiver = FrameReceiver(FailedInput(error), FrameLimits(4))
        with self.assertRaises(TransportIOError) as caught:
            receiver.receive()
        self.assertIs(caught.exception.__cause__, error)
        with self.assertRaises(StreamClosed):
            receiver.receive()

    def test_nonblocking_none_is_not_eof(self):
        receiver = FrameReceiver(NotReadyInput(), FrameLimits(4))
        with self.assertRaises(TransportIOError):
            receiver.receive()

    def test_closed_stream_is_a_terminal_io_failure(self):
        stream = io.BytesIO()
        stream.close()
        receiver = FrameReceiver(stream, FrameLimits(4))
        with self.assertRaises(TransportIOError) as caught:
            receiver.receive()
        self.assertIsInstance(caught.exception.__cause__, ValueError)
        with self.assertRaises(StreamClosed):
            receiver.receive()
