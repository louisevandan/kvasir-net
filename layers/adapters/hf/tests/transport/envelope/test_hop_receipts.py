import hashlib
import socket
import struct
import threading
import unittest

from p4hfadapter.integration.transport import Client, text


def exact(sock, count):
    value = bytearray()
    while len(value) < count:
        block = sock.recv(count - len(value))
        if not block:
            raise EOFError("test peer closed")
        value.extend(block)
    return bytes(value)


def frame(sock):
    size, = struct.unpack("<I", exact(sock, 4))
    return exact(sock, size) if size else b""


def send_frame(sock, body):
    sock.sendall(struct.pack("<I", len(body)) + body)


class HopReceiptTests(unittest.TestCase):
    def test_bounded_pipeline_retires_only_exact_receipts_before_finish(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        port = listener.getsockname()[1]
        failures = []

        def peer():
            try:
                sock, _ = listener.accept()
                with sock:
                    hello = frame(sock)
                    self.assertEqual(hello[:9], b"P4H1" + struct.pack("<HHB", 1, 0, 1))
                    pos = 9
                    size, = struct.unpack("<I", hello[pos:pos + 4])
                    pos += 4 + size
                    generation, = struct.unpack("<Q", hello[pos:pos + 8])
                    ack = b"P4H1" + struct.pack("<HHBQ", 1, 0, 2, generation)
                    ack += text("agent-a") + struct.pack("<QIQ", 9, 2, 1024 * 1024)
                    send_frame(sock, ack)
                    receipts = []
                    request_acks = 0
                    while len(receipts) < 3 or request_acks < 3:
                        body = frame(sock)
                        self.assertEqual(body[:4], b"P4H1")
                        kind = body[8]
                        if kind == 3:
                            attempt, = struct.unpack("<Q", body[9:17])
                            digest = body[17:49]
                            size, = struct.unpack("<I", body[49:53])
                            event = body[53:53 + size]
                            self.assertEqual(hashlib.sha256(event).digest(), digest)
                            receipt = b"P4H1" + struct.pack("<HHBQ", 1, 0, 4, attempt)
                            receipt += digest + b"\1" + text("")
                            send_frame(sock, receipt)
                            receipts.append(attempt)
                        elif kind == 5:
                            request_acks += 1
                        else:
                            self.fail(f"unexpected hop kind {kind}")
                    self.assertEqual(frame(sock), b"")
                    sock.sendall(bytes(4))
            except BaseException as error:
                failures.append(error)

        worker = threading.Thread(target=peer)
        worker.start()
        client = Client("127.0.0.1", port, timeout=5)
        try:
            for number in range(3):
                client.send((0, "tcp://127.0.0.1:59999"), "application/test",
                            f"payload-{number}".encode())
            client.finish(timeout=5)
        finally:
            client.close()
            listener.close()
        worker.join(5)
        self.assertFalse(worker.is_alive())
        if failures:
            raise failures[0]


if __name__ == "__main__":
    unittest.main()
