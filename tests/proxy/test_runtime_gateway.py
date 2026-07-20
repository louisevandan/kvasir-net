import json
import unittest
from unittest import mock

from tests.unit.test_control_protocol import install_httpx_stub


install_httpx_stub()
from controller.runtimes import gateway  # noqa: E402


class RuntimeGatewayTests(unittest.IsolatedAsyncioTestCase):
    async def test_proxy_stream_relays_coordinator_sse(self):
        # Ring streaming now relays real token-by-token SSE from the coordinator
        # (native, incl. tool_calls) rather than faking a single buffered chunk.
        controller = {"plan": {"runtime_mode": "ring_proxy"}}
        frames = [
            b'data: {"choices":[{"delta":{"content":"an"}}]}\n\n',
            b'data: {"choices":[{"delta":{"content":"swer"}}]}\n\n',
            b"data: [DONE]\n\n",
        ]

        async def fake_stream(mode, ctrl, body, request_id=None):
            for frame in frames:
                yield frame

        with mock.patch.object(gateway, "stream_chat_completion", new=fake_stream):
            chunks = [chunk async for chunk in gateway.stream_chat(controller, {}, "request-1")]

        self.assertEqual(chunks, frames)
