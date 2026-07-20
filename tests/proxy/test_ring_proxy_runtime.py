import unittest
from types import SimpleNamespace
from unittest import mock

from controller.proxy import runtime as ring_proxy
from tests.unit.test_control_protocol import (
    install_fastapi_stub,
    install_gguf_stub,
    install_httpx_stub,
)


install_httpx_stub()
install_fastapi_stub()
install_gguf_stub()


class RingProxyRuntimeTests(unittest.IsolatedAsyncioTestCase):
    ring_runtime = {
        "protocol": "linkcpp-stage-v1", "adapter_abi": 4,
        "build_id": "test-build", "available": True,
    }

    def test_topology_enrichment_is_owned_by_proxy_driver(self):
        item = ring_proxy.enrich_topology_item(
            {"rpc_port": 50053, "stage_port": 51053},
            {"node_id": "node-b", "rpc_endpoint": "host-b:50053"},
        )
        self.assertEqual(item["stage_endpoint"], "host-b:51053")

    async def test_serve_starts_adjacent_rank_local_stages(self):
        from controller import hub

        nodes = {
            "node-a": {
                "id": "node-a", "name": "first", "kind": "local",
                "rpc_port": 50052, "stage_port": 51052,
            },
            "node-b": {
                "id": "node-b", "name": "last", "kind": "agent",
                "rpc_port": 50053, "stage_port": 51053,
            },
        }
        active = [
            ("node-a", {"layers": [0, 10], "n_layers": 10, "kv_ram_gib": 1.0}),
            ("node-b", {"layers": [10, 20], "n_layers": 10, "kv_ram_gib": 1.0}),
        ]
        data_plane = {"established": False}
        topology = [
            {"node_id": "node-a", "stage_endpoint": "host-a:51052", "data_plane": data_plane},
            {"node_id": "node-b", "stage_endpoint": "host-b:51053", "data_plane": data_plane},
        ]
        manifests = [
            {"stage_index": 0, "layers": [0, 10], "tensor_names": ["a"],
             "identity": {"architecture": "qwen35moe", "digest": "m0"}},
            {"stage_index": 1, "layers": [10, 20], "tensor_names": ["b"],
             "identity": {"architecture": "qwen35moe", "digest": "m1"}},
        ]
        request = SimpleNamespace(model="model.gguf", ctx=2048, parallel=1)
        result = {
            "model_ref": "model.gguf", "model_shards": ["model.gguf"],
            "kv_cache_location": "ram",
            "data_plane": data_plane,
        }
        controller = {"id": "ctrl", "nodes": ["node-a", "node-b"]}

        with (
            mock.patch.object(hub, "NODES", nodes),
            mock.patch.object(hub, "_rpc_topology", return_value=topology),
            mock.patch.object(hub, "_record_ctrl_op"),
            mock.patch.object(hub, "_persist_hub_state"),
            mock.patch.object(hub, "_ensure_agent_models_for_ring", new=mock.AsyncMock()),
            mock.patch.object(hub, "_serve_req_snapshot", return_value={"runtime_mode": "ring_proxy"}),
            mock.patch.object(ring_proxy, "build_stage_manifests", return_value=manifests),
            mock.patch.object(
                ring_proxy, "_validate_node_runtimes",
                new=mock.AsyncMock(return_value=self.ring_runtime),
            ),
            mock.patch.object(ring_proxy, "_start_node", new=mock.AsyncMock()) as start,
            mock.patch.object(ring_proxy, "_wait_ready", new=mock.AsyncMock(return_value={})) as ready,
        ):
            await ring_proxy.serve(controller, request, result, active)

        self.assertEqual(start.await_count, 2)
        first_request = start.await_args_list[0].args[2]
        last_request = start.await_args_list[1].args[2]
        self.assertEqual((first_request.role, first_request.layers), ("first", [0, 10]))
        self.assertEqual(first_request.next_endpoint, "host-b:51053")
        self.assertTrue(first_request.coordinator)
        self.assertEqual(first_request.server_port, 52052)
        self.assertFalse(first_request.single_node)
        self.assertEqual(first_request.parallel, 1)
        self.assertFalse(first_request.kv_offload)
        self.assertEqual(first_request.ring_build_id, "test-build")
        self.assertEqual((last_request.role, last_request.layers), ("last", [10, 20]))
        self.assertEqual(last_request.next_endpoint, "host-a:51052")
        self.assertFalse(last_request.coordinator)
        self.assertFalse(last_request.kv_offload)
        ready.assert_awaited_once()
        self.assertEqual(controller["proxy_first_node"], "node-a")
        self.assertEqual(controller["phase"], "running")
        self.assertTrue(result["data_plane"]["established"])

    async def test_serve_allows_one_node_without_a_degenerate_ring(self):
        from controller import hub

        nodes = {"node-a": {"id": "node-a", "name": "metal", "kind": "agent",
                            "rpc_port": 50052, "stage_port": 51052}}
        active = [("node-a", {"layers": [0, 40], "n_layers": 40})]
        data_plane = {"established": False}
        request = SimpleNamespace(model="publisher/model.gguf", ctx=1024, parallel=1)
        result = {"model_ref": "publisher/model.gguf", "model_shards": ["publisher/model.gguf"],
                  "data_plane": data_plane}
        controller = {"id": "ctrl", "nodes": ["node-a"]}
        manifest = {"stage_index": 0, "layers": [0, 40], "tensor_names": ["a"],
                    "identity": {"architecture": "qwen35moe", "digest": "m0"}}

        with (
            mock.patch.object(hub, "NODES", nodes),
            mock.patch.object(hub, "_rpc_topology", return_value=[
                {"node_id": "node-a", "stage_endpoint": "host-a:51052", "data_plane": data_plane},
            ]),
            mock.patch.object(hub, "_record_ctrl_op"),
            mock.patch.object(hub, "_persist_hub_state"),
            mock.patch.object(hub, "_ensure_agent_models_for_ring", new=mock.AsyncMock()),
            mock.patch.object(hub, "_serve_req_snapshot", return_value={"runtime_mode": "ring_proxy"}),
            mock.patch.object(ring_proxy, "build_stage_manifests", return_value=[manifest]),
            mock.patch.object(ring_proxy, "_validate_node_runtimes",
                              new=mock.AsyncMock(return_value=self.ring_runtime)),
            mock.patch.object(ring_proxy, "_start_node", new=mock.AsyncMock()) as start,
            mock.patch.object(ring_proxy, "_wait_ready", new=mock.AsyncMock(return_value={})) as ready,
        ):
            await ring_proxy.serve(controller, request, result, active)

        start.assert_awaited_once()
        stage = start.await_args.args[2]
        self.assertTrue(stage.coordinator)
        self.assertTrue(stage.single_node)
        self.assertEqual(stage.layers, [0, 40])
        ready.assert_awaited_once()

    async def test_serve_rejects_disagreeing_manifest_architectures(self):
        from controller import hub

        nodes = {
            "node-a": {"id": "node-a", "kind": "local", "rpc_port": 50052},
            "node-b": {"id": "node-b", "kind": "local", "rpc_port": 50053},
        }
        active = [
            ("node-a", {"layers": [0, 1], "n_layers": 1}),
            ("node-b", {"layers": [1, 2], "n_layers": 1}),
        ]
        manifests = [
            {"stage_index": 0, "layers": [0, 1], "tensor_names": [],
             "identity": {"architecture": "llama"}},
            {"stage_index": 1, "layers": [1, 2], "tensor_names": [],
             "identity": {"architecture": "gemma4"}},
        ]
        request = SimpleNamespace(model="model.gguf", ctx=128, parallel=1)
        controller = {"id": "ctrl", "nodes": list(nodes)}
        with (
            mock.patch.object(hub, "NODES", nodes),
            mock.patch.object(hub, "_rpc_topology", return_value=[
                {"stage_endpoint": "a:51052"}, {"stage_endpoint": "b:51053"},
            ]),
            mock.patch.object(ring_proxy, "build_stage_manifests", return_value=manifests),
            mock.patch.object(
                ring_proxy, "_validate_node_runtimes",
                new=mock.AsyncMock(return_value=self.ring_runtime),
            ),
            mock.patch.object(ring_proxy, "_start_node", new=mock.AsyncMock()) as start,
        ):
            with self.assertRaisesRegex(RuntimeError, "disagree on model architecture"):
                await ring_proxy.serve(controller, request, {"model_ref": "model.gguf"}, active)
        start.assert_not_awaited()

    async def test_proxy_chat_forwards_raw_body_to_coordinator(self):
        # Native passthrough: the raw OpenAI body (incl. tools) reaches the
        # coordinator, and its response (incl. tool_calls) is returned as-is.
        from controller import hub
        from controller.proxy import stage_service
        node = {"kind": "local",
                "desired_load": {"role": "first", "coordinator": True, "server_port": 9999}}
        hub.NODES["n0"] = node
        controller = {"model": "model.gguf", "proxy_first_node": "n0"}
        native = {"id": "x", "object": "chat.completion",
                  "choices": [{"index": 0, "finish_reason": "tool_calls",
                               "message": {"role": "assistant", "content": None,
                                           "tool_calls": [{"id": "1", "type": "function",
                                                           "function": {"name": "f", "arguments": "{}"}}]}}]}
        try:
            with mock.patch.object(
                stage_service, "run_chat", new=mock.AsyncMock(return_value=native),
            ) as run_chat:
                body = {"messages": [{"role": "user", "content": "hi"}],
                        "tools": [{"type": "function", "function": {"name": "f"}}], "max_tokens": 8}
                result = await ring_proxy.chat_completion(controller, body, 7)
            run_chat.assert_awaited_once()
            forwarded = run_chat.await_args.args[1]
            self.assertEqual(forwarded["tools"], body["tools"])   # tools pass through
            self.assertFalse(forwarded["stream"])
            self.assertEqual(result, native)                       # tool_calls returned as-is
        finally:
            hub.NODES.pop("n0", None)

    async def test_proxy_stream_chat_relays_coordinator_sse(self):
        # Streaming: raw SSE frames from the coordinator are relayed unchanged.
        from controller import hub
        from controller.proxy import stage_service
        node = {"kind": "local",
                "desired_load": {"role": "first", "coordinator": True, "server_port": 9999}}
        hub.NODES["n0"] = node
        controller = {"model": "model.gguf", "proxy_first_node": "n0"}
        frames = [b'data: {"choices":[{"delta":{"content":"an"}}]}\n\n',
                  b'data: {"choices":[{"delta":{"content":"swer"}}]}\n\n',
                  b"data: [DONE]\n\n"]

        async def fake_stream(owner, body):
            self.assertTrue(body["stream"])
            for frame in frames:
                yield frame

        try:
            with mock.patch.object(stage_service, "run_chat_stream", new=fake_stream):
                out = []
                async for chunk in ring_proxy.stream_chat_completion(
                    controller, {"messages": [{"role": "user", "content": "hi"}]}, 7,
                ):
                    out.append(chunk)
            self.assertEqual(out, frames)
        finally:
            hub.NODES.pop("n0", None)


if __name__ == "__main__":
    unittest.main()
