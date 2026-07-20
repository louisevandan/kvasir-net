import io
import struct
import unittest

from controller.proxy import control as ring_control


class FakeProcess:
    def __init__(self, response):
        self.stdin = io.BytesIO()
        self.stdout = io.BytesIO(response)

    def poll(self):
        return None


class RingControlTests(unittest.TestCase):
    def test_binary_request_has_fixed_header_and_utf8_prompt(self):
        encoded = ring_control.encode_request("hello", 12, 99)
        self.assertEqual(len(encoded), ring_control.REQUEST.size + 5)
        header = ring_control.REQUEST.unpack(encoded[:ring_control.REQUEST.size])
        self.assertEqual(header[:2], (b"LKC1", 1))
        self.assertEqual(header[4:7], (99, 5, 12))
        self.assertEqual(encoded[ring_control.REQUEST.size:], b"hello")

    def test_process_roundtrip_decodes_text_and_metrics(self):
        text = "world".encode()
        response = ring_control.RESPONSE.pack(
            b"LKR1", 1, 0, 0, 77, len(text), 3, 125, 0,
        ) + text
        process = FakeProcess(response)

        result = ring_control.infer_process(process, "hello", 3, 77)

        self.assertEqual(result["text"], "world")
        self.assertEqual(result["tokens"], 3)
        self.assertEqual(result["elapsed_ms"], 125)
        request = process.stdin.getvalue()
        self.assertEqual(struct.unpack_from("<Q", request, 8)[0], 77)

    def test_chat_request_uses_bounded_binary_messages(self):
        encoded = ring_control.encode_request(
            "", 4, 101, [{"role": "user", "content": "hello"}],
        )
        header = ring_control.REQUEST.unpack(encoded[:ring_control.REQUEST.size])
        self.assertEqual(header[2], ring_control.FLAG_CHAT_MESSAGES)
        payload = encoded[ring_control.REQUEST.size:]
        self.assertEqual(struct.unpack_from("<I", payload, 0)[0], 1)
        role_size, content_size = struct.unpack_from("<II", payload, 4)
        self.assertEqual((role_size, content_size), (4, 5))

    def test_error_response_is_raised(self):
        text = b"decode failed"
        response = ring_control.RESPONSE.pack(
            b"LKR1", 1, 1, 0, 1, len(text), 0, 4, 0,
        ) + text
        with self.assertRaisesRegex(RuntimeError, "decode failed"):
            ring_control.infer_process(FakeProcess(response), "x", 1, 1)

    def test_raw_token_request_returns_numeric_argmax_without_a_tokenizer(self):
        response = ring_control.RESPONSE.pack(
            b"LKR1", 1, 0, ring_control.FLAG_TOKEN_IDS, 55, 4, 1, 9, 0,
        ) + struct.pack("<i", 37)
        process = FakeProcess(response)

        result = ring_control.infer_tokens_process(process, [1, 7, 11, 3], 55)

        self.assertEqual(result["token"], 37)
        request = process.stdin.getvalue()
        header = ring_control.REQUEST.unpack(request[:ring_control.REQUEST.size])
        self.assertEqual(header[2], ring_control.FLAG_TOKEN_IDS)
        self.assertEqual(
            struct.unpack("<4i", request[ring_control.REQUEST.size:]),
            (1, 7, 11, 3),
        )


if __name__ == "__main__":
    unittest.main()
