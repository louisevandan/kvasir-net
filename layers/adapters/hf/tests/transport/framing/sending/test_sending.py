import io
import unittest

from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge, StreamClosed, TransportIOError
from p4hfadapter.transport.framing.limits import FrameLimits
from p4hfadapter.transport.framing.sending import FrameSender
from tests.fixtures.sending_streams import FailedFlushOutput, FailedPayloadOutput, FragmentedOutput, InvalidCountOutput


class SendingTests(unittest.TestCase):
    def test_short_writes_and_flush(self):
        stream = FragmentedOutput()
        FrameSender(stream, FrameLimits(3)).send(b"abc")
        self.assertEqual(stream.getvalue(), b"P4HF\x01\x00\x00\x00" + (3).to_bytes(8, "big") + b"abc")
        self.assertTrue(stream.flushed)
        self.assertFalse(stream.closed)

    def test_rejected_payload_has_no_writes_and_sender_is_reusable(self):
        stream = io.BytesIO()
        sender = FrameSender(stream, FrameLimits(3))
        with self.assertRaises(PayloadTooLarge):
            sender.send(b"four")
        with self.assertRaises(InvalidFrame):
            sender.send(bytearray(b"x"))
        self.assertEqual(stream.getvalue(), b"")
        sender.send(b"ok")
        self.assertTrue(stream.getvalue().endswith(b"ok"))

    def test_partial_write_failure_is_terminal_and_keeps_input(self):
        error = OSError("pipe broken")
        stream = FailedPayloadOutput(error)
        payload = b"abc"
        sender = FrameSender(stream, FrameLimits(3))
        with self.assertRaises(TransportIOError) as caught:
            sender.send(payload)
        self.assertIs(caught.exception.__cause__, error)
        self.assertEqual(payload, b"abc")
        self.assertEqual(len(stream.getvalue()), 16)
        with self.assertRaises(StreamClosed):
            sender.send(payload)
        self.assertEqual(len(stream.getvalue()), 16)

    def test_invalid_write_counts_are_terminal(self):
        for count in (None, 0, -1, True, 17):
            with self.subTest(count=count):
                sender = FrameSender(InvalidCountOutput(count), FrameLimits(4))
                with self.assertRaises(TransportIOError):
                    sender.send(b"x")
                with self.assertRaises(StreamClosed):
                    sender.send(b"x")

    def test_flush_failure_is_uncertain_and_not_retried(self):
        stream = FailedFlushOutput()
        sender = FrameSender(stream, FrameLimits(4))
        with self.assertRaises(TransportIOError):
            sender.send(b"x")
        size = len(stream.getvalue())
        with self.assertRaises(StreamClosed):
            sender.send(b"x")
        self.assertEqual(len(stream.getvalue()), size)

    def test_closed_stream_is_a_terminal_io_failure(self):
        stream = io.BytesIO()
        stream.close()
        sender = FrameSender(stream, FrameLimits(4))
        with self.assertRaises(TransportIOError) as caught:
            sender.send(b"x")
        self.assertIsInstance(caught.exception.__cause__, ValueError)
        with self.assertRaises(StreamClosed):
            sender.send(b"x")
