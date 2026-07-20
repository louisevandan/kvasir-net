import json
import os
import sys
import tempfile
import types
import unittest
from unittest import mock
from pathlib import Path

from controller.protocol import ModelSource, NodeReport, ResourceSnapshot, RuntimeIdentity
from controller.release import RELEASE_VERSION
from controller.versioning import compatibility_report


def install_httpx_stub():
    if "httpx" in sys.modules:
        return
    httpx = types.ModuleType("httpx")

    class HTTPStatusError(Exception):
        def __init__(self, response=None):
            self.response = response

    class AsyncClient:
        def __init__(self, *args, **kwargs):
            pass

        async def __aenter__(self):
            return self

        async def __aexit__(self, *args):
            return False

    httpx.HTTPStatusError = HTTPStatusError
    httpx.AsyncClient = AsyncClient
    sys.modules["httpx"] = httpx


def install_fastapi_stub():
    if "fastapi" in sys.modules:
        return
    fastapi = types.ModuleType("fastapi")

    class HTTPException(Exception):
        def __init__(self, status_code=500, detail=None):
            super().__init__(detail)
            self.status_code = status_code
            self.detail = detail

    class Request:
        pass

    def Query(default=None, *args, **kwargs):
        return default

    class FastAPI:
        def __init__(self, *args, **kwargs):
            pass

        def _decorator(self, *args, **kwargs):
            def wrap(fn):
                return fn
            return wrap

        get = post = delete = api_route = on_event = middleware = websocket = _decorator

        def mount(self, *args, **kwargs):
            return None

    class WebSocket:
        pass

    fastapi.FastAPI = FastAPI
    fastapi.HTTPException = HTTPException
    fastapi.Request = Request
    fastapi.Query = Query
    fastapi.WebSocket = WebSocket
    sys.modules["fastapi"] = fastapi

    responses = types.ModuleType("fastapi.responses")

    class JSONResponse(dict):
        def __init__(self, content=None, status_code=200, *args, **kwargs):
            super().__init__(content or {})
            self.status_code = status_code

    class FileResponse:
        def __init__(self, *args, **kwargs):
            pass

    class StreamingResponse:
        def __init__(self, *args, **kwargs):
            pass

    responses.JSONResponse = JSONResponse
    responses.FileResponse = FileResponse
    responses.StreamingResponse = StreamingResponse
    sys.modules["fastapi.responses"] = responses

    staticfiles = types.ModuleType("fastapi.staticfiles")

    class StaticFiles:
        def __init__(self, *args, **kwargs):
            pass

    staticfiles.StaticFiles = StaticFiles
    sys.modules["fastapi.staticfiles"] = staticfiles


def install_gguf_stub():
    if "gguf" in sys.modules:
        return
    gguf = types.ModuleType("gguf")

    class GGUFReader:
        def __init__(self, *args, **kwargs):
            self.fields = {}
            self.tensors = []

    class GGUFValueType:
        STRING = "STRING"

    gguf.GGUFReader = GGUFReader
    gguf.GGUFValueType = GGUFValueType
    sys.modules["gguf"] = gguf


class ProtocolModelTests(unittest.TestCase):
    def test_model_source_redacts_hf_token(self):
        source = ModelSource(kind="huggingface", repo_id="org/model",
                             filename="model.gguf", hf_token="secret-token")
        redacted = source.redacted()
        self.assertEqual(redacted["hf_token"], "***")
        self.assertEqual(source.hf_token, "secret-token")

    def test_runtime_identity_defaults_to_unit_version(self):
        runtime = RuntimeIdentity()
        self.assertEqual(runtime.unit_version, RELEASE_VERSION)
        self.assertEqual(runtime.runtime_pack_version, RELEASE_VERSION)

    def test_release_version_comes_from_root_version_file(self):
        root_version = (Path(__file__).resolve().parents[2] / "VERSION").read_text(encoding="utf-8").strip()
        self.assertEqual(RELEASE_VERSION, root_version)

    def test_runtime_package_drift_is_advisory_but_rpc_abi_remains_strict(self):
        expected = {"unit_version": RELEASE_VERSION, "runtime_pack_version": RELEASE_VERSION,
                    "llama_cpp_version": "unknown", "rpc_abi": "llama.cpp-rpc"}
        older = {"unit_version": "0.0.1", "runtime_pack_version": "0.0.1",
                 "llama_cpp_version": "unknown", "rpc_abi": "llama.cpp-rpc"}
        report = compatibility_report(older, expected)
        self.assertTrue(report["compatible"])
        self.assertEqual({item["field"] for item in report["warnings"]},
                         {"unit_version", "runtime_pack_version"})

        incompatible = compatibility_report({**older, "rpc_abi": "different-rpc"}, expected)
        self.assertFalse(incompatible["compatible"])
        self.assertEqual(incompatible["mismatches"][0]["field"], "rpc_abi")


class NodeAgentControlTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        from controller import nodeagent

        self.nodeagent = nodeagent
        self.tmp = tempfile.TemporaryDirectory()
        nodeagent.MODEL_DIR = self.tmp.name
        nodeagent.STATE_FILE = os.path.join(self.tmp.name, "state.json")
        nodeagent.state.update({
            "node_id": "agent-test",
            "controller_id": None,
            "report_url": None,
            "name": "",
            "resource_limits": {},
            "worker": None,
            "stage": None,
            "worker_port": None,
            "desired_load": None,
            "operations": {},
            "models": {},
            "last_reports": [],
            "outbox": [],
            "seq": 0,
        })

    async def asyncTearDown(self):
        self.tmp.cleanup()

    async def test_join_binds_free_node(self):
        from controller.protocol import JoinRequest

        info = await self.nodeagent.control_join(JoinRequest(controller_id="ctrl-a", name="node-a"))
        self.assertEqual(info["bound_to"], "ctrl-a")
        self.assertEqual(info["name"], "node-a")
        self.assertIn("resources", info)

    async def test_join_applies_requested_resource_limits(self):
        from controller.protocol import JoinRequest

        info = await self.nodeagent.control_join(JoinRequest(
            controller_id="ctrl-a", vram_budget_gib=2, ram_budget_gib=4, cores_budget=1))
        self.assertEqual(info["resources"]["vram_budget_gib"], 2)
        self.assertEqual(info["resources"]["ram_budget_gib"], 4)
        self.assertEqual(info["resources"]["cores_budget"], 1)

    async def test_join_rejects_other_controller(self):
        from controller.protocol import JoinRequest
        from fastapi import HTTPException

        await self.nodeagent.control_join(JoinRequest(controller_id="ctrl-a"))
        with self.assertRaises(HTTPException) as ctx:
            await self.nodeagent.control_join(JoinRequest(controller_id="ctrl-b"))
        self.assertEqual(ctx.exception.status_code, 409)

    async def test_info_reports_native_backend_and_host_platform(self):
        with mock.patch.dict(os.environ, {
            "LINKCPP_LLAMA_CPP_BACKEND": "cpu",
            "LINKCPP_BACKEND_DEVICE": "test-cpu",
            "LINKCPP_VRAM_TOTAL": "4",
            "LINKCPP_VRAM_BUDGET": "3",
            "LINKCPP_RAM_BUDGET": "8",
            "LINKCPP_CORES": "2",
        }, clear=False):
            info = self.nodeagent._info()

        self.assertEqual(info["backend"]["backend_kind"], "cpu")
        self.assertEqual(info["backend"]["backend_device"], "test-cpu")
        self.assertEqual(info["resources"]["vram_budget_gib"], 3.0)
        self.assertEqual(info["resources"]["ram_budget_gib"], 8.0)
        self.assertEqual(info["resources"]["cores_budget"], 2)
        self.assertIn("system", info["host_platform"])
        self.assertTrue(info["capabilities"]["native_agent"])

class HubReportTests(unittest.TestCase):
    def setUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        install_gguf_stub()
        from controller import hub

        self.hub = hub
        hub.NODES.clear()
        hub.CTRLS.clear()
        hub.REMOTE_UNITS.clear()
        hub.INFERENCE.clear()
        self.archive_tmp = tempfile.TemporaryDirectory()
        self._old_archive_dir = hub.INFERENCE_ARCHIVE_DIR
        hub.INFERENCE_ARCHIVE_DIR = self.archive_tmp.name
        hub.NODES["agent-test"] = {
            "id": "agent-test",
            "kind": "agent",
            "name": "agent-test",
            "gpu_uuid": "",
            "gpu_name": "Managed",
            "vram": 10,
            "ram": 20,
            "cores": 8,
            "rpc_host": "127.0.0.1",
            "rpc_port": 50052,
            "bound_to": "ctrl-test",
            "worker": None,
            "worker_running": False,
            "ram_used": 0.0,
            "log": "",
            "resources": {},
            "operations": [],
            "models": {},
            "last_report": None,
            "capabilities": {},
            "host_platform": {},
        }
        hub.CTRLS["ctrl-test"] = {
            "id": "ctrl-test",
            "name": "ctrl-test",
            "nodes": ["agent-test"],
            "model": None,
            "ctx": 4096,
            "parallel": 1,
            "last_load": {},
            "phase": "idle",
            "detail": "",
            "plan": None,
            "master": None,
            "master_port": 8080,
            "operations": {},
        }

    def tearDown(self):
        self.hub.INFERENCE_ARCHIVE_DIR = self._old_archive_dir
        self.archive_tmp.cleanup()

    def test_node_report_updates_node_and_controller_operation(self):
        report = NodeReport(
            node_id="agent-test",
            controller_id="ctrl-test",
            op_id="load-1",
            op_type="load",
            phase="worker_started",
            status="done",
            progress=100,
            message="worker started",
            resources=ResourceSnapshot(vram_used_gib=3.5, ram_used_gib=2.0),
            model="m.gguf",
            seq=1,
        )
        result = self.hub.api_node_reports(report)
        self.assertTrue(result["accepted"])
        self.assertTrue(self.hub.NODES["agent-test"]["worker_running"])
        self.assertEqual(self.hub.NODES["agent-test"]["resources"]["vram_used_gib"], 3.5)
        self.assertIn("load-1", self.hub.CTRLS["ctrl-test"]["operations"])

    def test_node_load_monitor_report_updates_controller_operation(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        node = self.hub.NODES["agent-test"]
        resources = ResourceSnapshot(vram_used_gib=4.0, ram_used_gib=2.5)

        self.hub._node_load_report(
            ctrl,
            node,
            "node-load-1",
            "rpc_loading",
            "running",
            55.0,
            "rpc alloc 3",
            resources,
            model="m.gguf",
            details={"rpc_activity": {"alloc_buffer_count": 3}},
        )

        op = ctrl["operations"]["node-load-1"]
        self.assertEqual(op["type"], "node_load")
        self.assertEqual(op["node_id"], "agent-test")
        self.assertEqual(op["progress"], 55.0)
        self.assertEqual(node["operations"][0]["phase"], "rpc_loading")

    def test_load_monitor_progress_uses_rpc_activity(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl["phase"] = "loading"
        sample = {
            "resources": ResourceSnapshot(vram_used_gib=1.0, ram_used_gib=0.0),
            "activity": {"alloc_buffer_count": 4, "set_tensor_count": 2, "get_alloc_size_count": 80},
        }

        progress = self.hub._monitor_progress(
            sample,
            {"vram_used_gib": 1.0, "ram_used_gib": 0.0},
            {"n_layers": 10, "vram_used_gib": 6.0, "ram_used_gib": 0.0},
            ctrl,
        )

        self.assertGreater(progress, 10.0)

    def test_node_view_reports_desired_load(self):
        node = self.hub.NODES["agent-test"]
        node["desired_load"] = {"model": "Qwen.gguf", "layers": [0, 12]}
        node["host_platform"] = {"system": "linux", "machine": "x86_64"}

        view = self.hub.node_view(node)

        self.assertEqual(view["desired_load"]["model"], "Qwen.gguf")
        self.assertEqual(view["host_platform"]["system"], "linux")

    def test_unload_clears_plan_and_activity_after_node_sweep(self):
        import asyncio

        self.hub.NODES["agent-test"].update({
            "kind": "local",
            "name": "local-test",
            "worker": None,
            "worker_running": True,
            "ram_used": 2.0,
        })
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl.update({
            "model": "model.gguf",
            "phase": "running",
            "detail": "",
            "plan": {"feasible": True},
            "operations": {"old-load": {"type": "load", "phase": "running"}},
        })

        result = asyncio.run(self.hub._unload_ctrl(ctrl, "test"))

        self.assertTrue(result["stopped"])
        self.assertEqual(result["nodes"][0]["node_id"], "agent-test")
        self.assertEqual(result["nodes"][0]["status"], "done")
        self.assertIsNone(ctrl["plan"])
        self.assertEqual(ctrl["operations"], {})
        self.assertEqual(ctrl["phase"], "idle")
        self.assertIsNone(ctrl["model"])
        self.assertFalse(self.hub.NODES["agent-test"]["worker_running"])

    def test_infeasible_load_records_plan_and_operation(self):
        import asyncio

        async def run_case():
            original = self.hub._do_plan
            try:
                self.hub._do_plan = lambda c, req: {
                    "feasible": False,
                    "reason": "ran out of node VRAM at layer 3",
                    "need_vram_gib": 12.0,
                    "sum_vram_budget_gib": 8.0,
                    "sum_ram_budget_gib": 32.0,
                    "kv_total_gib": 2.0,
                    "model_ref": req.model,
                    "adaptive_load_available": True,
                }
                response = await self.hub.api_ctrl_serve(
                    "ctrl-test",
                    self.hub.ServeReq(model="too-large.gguf", ctx=4096, parallel=1),
                )
                return response
            finally:
                self.hub._do_plan = original

        response = asyncio.run(run_case())

        self.assertEqual(response.status_code, 400)
        ctrl = self.hub.CTRLS["ctrl-test"]
        self.assertFalse(ctrl["plan"]["feasible"])
        ops = list(ctrl["operations"].values())
        self.assertTrue(any(op["type"] == "load" and op["status"] == "error" for op in ops))

    def test_plan_endpoint_logs_request_and_result_summary(self):
        events = []
        original_plan = self.hub._do_plan
        original_log = self.hub._log_event
        try:
            self.hub._do_plan = lambda c, req: {
                "feasible": True,
                "model_ref": req.model,
                "kv_total_gib": 6.41,
                "kv_cache_location": "vram",
                "kv_offload_enabled": True,
                "nodes_used": 2,
                "tensor_split": [21, 20],
                "adaptive_load_available": True,
            }
            self.hub._log_event = lambda event, **fields: events.append((event, fields))

            result = self.hub.api_ctrl_plan(
                "ctrl-test",
                self.hub.ServeReq(model="model.gguf", ctx=40960, parallel=2),
            )
        finally:
            self.hub._do_plan = original_plan
            self.hub._log_event = original_log

        self.assertTrue(result["feasible"])
        self.assertEqual(self.hub.CTRLS["ctrl-test"]["plan"]["tensor_split"], [21, 20])
        event_names = [name for name, _ in events]
        self.assertIn("plan_request_received", event_names)
        self.assertIn("load_plan_result", event_names)
        result_event = next(fields for name, fields in events if name == "load_plan_result")
        self.assertEqual(result_event["plan"]["kv_cache_location"], "vram")

    def test_new_load_clears_previous_model_activity(self):
        import asyncio

        async def run_case():
            original = self.hub._do_plan
            try:
                self.hub._do_plan = lambda c, req: {
                    "feasible": False,
                    "reason": "planned failure",
                    "model_ref": req.model,
                    "adaptive_load_available": True,
                }
                self.hub.CTRLS["ctrl-test"]["operations"] = {
                    "old": {
                        "op_id": "old",
                        "type": "load",
                        "phase": "blocked",
                        "status": "error",
                        "model": "old-model.gguf",
                    }
                }
                return await self.hub.api_ctrl_serve(
                    "ctrl-test",
                    self.hub.ServeReq(model="new-model.gguf", ctx=4096, parallel=1),
                )
            finally:
                self.hub._do_plan = original

        response = asyncio.run(run_case())

        self.assertEqual(response.status_code, 400)
        ops = list(self.hub.CTRLS["ctrl-test"]["operations"].values())
        self.assertTrue(ops)
        self.assertTrue(all(op.get("model") != "old-model.gguf" for op in ops))

    def test_new_plan_clears_previous_idle_activity(self):
        original = self.hub._do_plan
        try:
            self.hub._do_plan = lambda c, req: {
                "feasible": True,
                "model_ref": req.model,
                "adaptive_load_available": True,
                "placement": [],
            }
            self.hub.CTRLS["ctrl-test"]["operations"] = {
                "old": {
                    "op_id": "old",
                    "type": "load",
                    "phase": "blocked",
                    "status": "error",
                    "model": "old-model.gguf",
                }
            }
            self.hub.api_ctrl_plan(
                "ctrl-test",
                self.hub.ServeReq(model="new-model.gguf", ctx=4096, parallel=1),
            )
        finally:
            self.hub._do_plan = original

        ops = list(self.hub.CTRLS["ctrl-test"]["operations"].values())
        self.assertEqual(len(ops), 1)
        self.assertEqual(ops[0]["type"], "plan")

    def test_controller_snapshot_persists_last_successful_load_settings(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl["last_load"] = {
            "model": "Qwen.gguf",
            "ctx": 40960,
            "parallel": 2,
            "kv_bits": 16,
            "cache_type_k": "q8_0",
            "cache_type_v": "f16",
        }

        snap = self.hub._controller_snapshot(ctrl)

        self.assertEqual(snap["last_load"]["model"], "Qwen.gguf")
        self.assertEqual(snap["last_load"]["ctx"], 40960)
        self.assertEqual(snap["last_load"]["cache_type_k"], "q8_0")

    def test_loading_view_exposes_cancel_not_unload(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl.update(phase="loading", model="Qwen.gguf", detail="staging model", master=None)

        view = self.hub.ctrl_view(ctrl)

        self.assertTrue(view["can_cancel_load"])
        self.assertFalse(view["can_unload"])
        self.assertFalse(view["runtime_loaded"])

    def test_duplicate_load_is_rejected_while_loading(self):
        import asyncio
        from fastapi import HTTPException

        self.hub.CTRLS["ctrl-test"].update(phase="loading", model="Qwen.gguf")

        async def run_case():
            return await self.hub.api_ctrl_serve(
                "ctrl-test",
                self.hub.ServeReq(model="other.gguf", ctx=4096, parallel=1),
            )

        with self.assertRaises(HTTPException) as ctx:
            asyncio.run(run_case())
        self.assertEqual(ctx.exception.status_code, 409)

    def test_inference_activity_tracks_parallel_queue(self):
        import asyncio

        async def run_case():
            ctrl = self.hub.CTRLS["ctrl-test"]
            ctrl["parallel"] = 1
            ctrl["model"] = "model.gguf"
            ctrl["plan"] = {
                "placement": [
                    {"n_layers": 1, "node_id": "agent-test", "node_name": "Slot 1", "layers": [0, 1]},
                ],
            }
            self.hub._reset_inference_activity(ctrl, "test")
            first = self.hub._new_inference_item(ctrl, "chat", {"messages": []})
            second = self.hub._new_inference_item(ctrl, "chat", {"messages": []})
            await self.hub._acquire_inference_slot(ctrl, first)
            pending = asyncio.create_task(self.hub._acquire_inference_slot(ctrl, second))
            await asyncio.sleep(0)
            queued = self.hub._inference_snapshot(ctrl)
            self.assertEqual(queued["active"], 1)
            self.assertEqual(queued["queued"], 1)
            self.hub._finish_inference(ctrl, first, "done", "complete")
            await pending
            running = self.hub._inference_snapshot(ctrl)
            self.assertEqual(running["active"], 1)
            self.assertEqual(running["queued"], 0)
            self.assertEqual(running["items"][0]["id"], second["id"])
            self.hub._finish_inference(ctrl, second, "done", "complete")
            return self.hub._inference_snapshot(ctrl)

        final = asyncio.run(run_case())
        self.assertEqual(final["items"], [])

    def test_inference_finish_persists_plan_nodes_metrics_and_transfer_edges(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl.update(model="model.gguf", parallel=1, phase="running")
        self.hub.NODES["remote-last"] = {
            "id": "remote-last",
            "kind": "remote_unit_node",
            "name": "remote-last",
            "gpu_uuid": "",
            "gpu_name": "Remote GPU",
            "vram": 8,
            "ram": 16,
            "cores": 8,
            "rpc_host": "192.168.0.22",
            "rpc_port": 50053,
            "bound_to": "ctrl-test",
            "worker": None,
            "worker_running": True,
            "ram_used": 0.0,
            "log": "",
            "resources": {},
            "operations": [],
            "models": {},
            "last_report": None,
            "capabilities": {},
            "host_platform": {},
            "remote_unit_url": "http://192.168.0.22:19000",
            "remote_source_node_id": "node-slot-2",
            "remote_source_rpc_endpoint": "127.0.0.1:50053",
        }
        ctrl["nodes"] = ["agent-test", "remote-last"]
        ctrl["plan"] = {
            "model": {"arch": "llama", "n_layer": 40, "n_embd": 4096, "n_head_kv": 8, "n_expert": 0},
            "cache_type_k": "f16",
            "cache_type_v": "f16",
            "placement": [
                {"node_id": "agent-test", "node_name": "Slot 1", "gpu_name": "Managed", "n_layers": 20,
                 "layers": [0, 20], "kv_vram_gib": 1.0, "kv_ram_gib": 0.0, "offload_policy": "none"},
                {"node_id": "remote-last", "node_name": "Slot 2", "gpu_name": "Remote GPU", "n_layers": 20,
                 "layers": [20, 40], "kv_vram_gib": 0.0, "kv_ram_gib": 1.0, "offload_policy": "ffn=RAM"},
            ],
        }
        item = self.hub._new_inference_item(ctrl, "chat", {"messages": [], "max_tokens": 32})
        item["started_at"] = item["created_at"] - 1.0

        self.hub._finish_inference(
            ctrl,
            item,
            "done",
            "complete",
            usage={"completion_tokens": 10},
            timings={"predicted_per_second": 2.5},
            finish_reason="stop",
        )

        self.assertTrue(os.path.exists(item["record_path"]))
        with open(item["record_path"], "r", encoding="utf-8") as f:
            record = json.load(f)
        self.assertEqual(record["metrics"]["output_tokens"], 10)
        self.assertEqual(record["metrics"]["tps"], 2.5)
        self.assertEqual(record["plan"]["model"]["n_embd"], 4096)
        self.assertEqual(record["metrics"]["transfer_edges"][0]["bytes_per_token"], 8192)
        self.assertTrue(record["metrics"]["transfer_edges"][-1]["cross_host"])
        self.assertEqual(record["metrics"]["node_metrics"][1]["kv_cache"]["location"], "ram")
        self.assertEqual(record["rpc_topology"][-1]["node_kind"], "remote_unit_node")
        history = self.hub._load_inference_history(ctrl, include_detail=True)
        self.assertEqual(history[0]["request_id"], item["id"])
        self.assertIn("detail", history[0])
        self.assertEqual(history[0]["detail"]["metrics"]["output_tokens"], 10)
        self.assertEqual(history[0]["detail"]["plan"]["model"]["n_embd"], 4096)
        self.assertEqual(history[0]["detail"]["rpc_topology"][-1]["node_kind"], "remote_unit_node")
        api_history = self.hub.api_ctrl_inference_history("ctrl-test", detail=True)
        self.assertIn("detail", api_history["history"][0])

    def test_live_inference_snapshot_includes_stream_samples_and_kv_metrics(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl.update(model="model.gguf", parallel=1, phase="running")
        ctrl["plan"] = {
            "model": {"arch": "llama", "n_layer": 1, "n_embd": 1024, "n_head_kv": 8, "n_expert": 0},
            "cache_type_k": "q8_0",
            "cache_type_v": "f16",
            "placement": [
                {"node_id": "agent-test", "node_name": "Slot 1", "gpu_name": "Managed", "n_layers": 1,
                 "layers": [0, 1], "kv_vram_gib": 0.25, "kv_ram_gib": 0.0, "offload_policy": "none"},
            ],
        }
        item = self.hub._new_inference_item(ctrl, "chat", {"messages": [], "max_tokens": 4}, stream=True)
        item.update(status="streaming", started_at=item["created_at"], slot_acquired=False)
        self.hub._record_stream_chunk(
            ctrl,
            item,
            b'data: {"choices":[{"delta":{"content":"hello"}}]}\n\n',
        )

        snap = self.hub._inference_snapshot(ctrl)

        self.assertEqual(snap["active"], 1)
        metrics = snap["items"][0]["metrics"]
        self.assertEqual(metrics["output_events"], 1)
        self.assertEqual(metrics["node_metrics"][0]["kv_cache"]["location"], "vram")
        self.assertEqual(metrics["transfer_edges"][0]["edge_type"], "last_node_to_master")
        self.assertGreaterEqual(len(snap["items"][0]["samples"]), 1)

    def test_rpc_activity_parser_extracts_compute_and_transfer(self):
        activity = self.hub._parse_rpc_activity_text("\n".join([
            "[set_tensor] buffer: 0x1, data: 0x2, offset: 0, size: 65536",
            "[graph_compute] device: 0, n_nodes: 95, n_tensors: 1514",
            "[get_tensor] buffer: 0x1, data: 0x3, offset: 0, size: 8192",
        ]))

        self.assertEqual(activity["counts"]["graph_compute"], 1)
        self.assertEqual(activity["bytes"]["set_tensor"], 65536)
        self.assertEqual(activity["bytes"]["get_tensor"], 8192)
        self.assertEqual(activity["latest"]["op"], "get_tensor")

    def test_live_inference_snapshot_marks_node_computing_from_rpc_log_delta(self):
        ctrl = self.hub.CTRLS["ctrl-test"]
        ctrl.update(model="model.gguf", parallel=1, phase="running")
        log_path = os.path.join(self.archive_tmp.name, "node.log")
        with open(log_path, "w", encoding="utf-8") as f:
            f.write("[graph_compute] device: 0, n_nodes: 95, n_tensors: 1514\n")
        node = self.hub.NODES["agent-test"]
        node.update(log=log_path, worker_running=True)
        ctrl["plan"] = {
            "model": {"arch": "llama", "n_layer": 1, "n_embd": 1024, "n_head_kv": 8, "n_expert": 0},
            "cache_type_k": "f16",
            "cache_type_v": "f16",
            "placement": [
                {"node_id": "agent-test", "node_name": "Slot 1", "gpu_name": "Managed", "n_layers": 1,
                 "layers": [0, 1], "kv_vram_gib": 0.25, "kv_ram_gib": 0.0, "offload_policy": "none"},
            ],
        }
        item = self.hub._new_inference_item(ctrl, "chat", {"messages": [], "max_tokens": 4}, stream=True)
        item.update(status="streaming", started_at=item["created_at"], slot_acquired=False)

        snap = self.hub._inference_snapshot(ctrl)

        runtime = snap["items"][0]["metrics"]["node_metrics"][0]["runtime"]
        self.assertEqual(runtime["state"], "computing")
        self.assertEqual(runtime["last_operation"]["op"], "graph_compute")
        self.assertEqual(runtime["delta"]["graph_compute_count"], 1)
        self.assertEqual(runtime["queue"]["association"], "single-active-request")

    def test_master_progress_parser_extracts_decode_rate(self):
        log = "\n".join([
            "16.06.069 I slot print_timing: id  0 | task 13 | prompt processing, n_tokens =     18, progress = 0.82, t =   4.43 s / 4.06 tokens per second",
            "19.36.333 I slot print_timing: id  0 | task 13 | n_decoded =    124, tg =   0.61 t/s, tg_3s =   0.58 t/s",
        ])

        progress = self.hub._parse_master_progress(log)

        self.assertEqual(progress[-1]["kind"], "decode_progress")
        self.assertEqual(progress[-1]["task"], 13)
        self.assertEqual(progress[-1]["n_decoded"], 124)
        self.assertEqual(progress[-1]["tokens_per_second"], 0.61)

    def test_generation_limits_default_and_cap(self):
        defaulted, default_info = self.hub._normalize_generation_limits({"messages": []})
        capped, cap_info = self.hub._normalize_generation_limits({"messages": [], "max_tokens": 200000})

        self.assertEqual(defaulted["max_tokens"], self.hub.DEFAULT_COMPLETION_TOKENS)
        self.assertFalse(default_info["capped"])
        self.assertEqual(capped["max_tokens"], self.hub.MAX_COMPLETION_TOKENS)
        self.assertTrue(cap_info["capped"])

    def test_gateway_invalid_json_returns_400(self):
        import asyncio
        import json

        class BadRequest:
            async def json(self):
                raise json.JSONDecodeError("bad", "{", 1)

        with self.assertRaises(Exception) as ctx:
            asyncio.run(self.hub._read_json_body(BadRequest()))

        self.assertEqual(getattr(ctx.exception, "status_code", None), 400)
        self.assertIn("invalid JSON body", str(ctx.exception))

    def test_gateway_model_lists_expose_running_controller_model(self):
        import asyncio

        class AliveProcess:
            def poll(self):
                return None

        async def run_case():
            ctrl = self.hub.CTRLS["ctrl-test"]
            ctrl.update(phase="running", model="author/model-Q4.gguf", master=AliveProcess())
            openai = await self.hub.gw_openai_models("ctrl-test")
            anthropic = await self.hub.gw_anthropic_models("ctrl-test")
            return openai, anthropic

        openai, anthropic = asyncio.run(run_case())

        self.assertEqual(openai["object"], "list")
        self.assertEqual(openai["data"][0]["id"], "author/model-Q4.gguf")
        self.assertEqual(openai["data"][0]["object"], "model")
        self.assertIn("created", openai["data"][0])
        self.assertEqual(openai["data"][0]["owned_by"], "linkcpp")
        self.assertEqual(anthropic["data"][0]["id"], "author/model-Q4.gguf")
        self.assertEqual(anthropic["data"][0]["type"], "model")
        self.assertEqual(anthropic["data"][0]["display_name"], "model-Q4")
        self.assertEqual(anthropic["first_id"], "author/model-Q4.gguf")
        self.assertEqual(anthropic["last_id"], "author/model-Q4.gguf")
        self.assertFalse(anthropic["has_more"])


class HubModelScanTests(unittest.TestCase):
    def setUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        install_gguf_stub()
        from controller import hub

        self.hub = hub
        self.tmp = tempfile.TemporaryDirectory()
        hub.MODEL_DIR = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def _write(self, rel, size=16):
        path = os.path.join(self.tmp.name, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(b"x" * size)
        return path

    def test_recursive_model_scan_hides_aux_and_groups_split_shards(self):
        self._write("org/plain/plain-Q4.gguf", 10)
        self._write("org/plain/mmproj-plain.gguf", 5)
        self._write("Qwen/Qwen3-Embedding-8B-GGUF/Qwen3-Embedding-8B-Q4_K_M.gguf", 17)
        self._write("org/split/split-Q4-00001-of-00002.gguf", 11)
        self._write("org/split/split-Q4-00002-of-00002.gguf", 13)
        self._write("org/split/split-mmproj-f16.gguf", 7)

        models = self.hub._scan_models()
        ids = [m["id"] for m in models]

        self.assertIn("org/plain/plain-Q4.gguf", ids)
        self.assertIn("org/split/split-Q4-00001-of-00002.gguf", ids)
        self.assertNotIn("org/plain/mmproj-plain.gguf", ids)
        self.assertNotIn("Qwen/Qwen3-Embedding-8B-GGUF/Qwen3-Embedding-8B-Q4_K_M.gguf", ids)
        split = next(m for m in models if m["id"].startswith("org/split/"))
        self.assertEqual(len(split["shards"]), 2)
        self.assertEqual(split["aux_files"], ["org/split/split-mmproj-f16.gguf"])
        self.assertEqual(split["label"], "split (0.00 GiB, 2 shards)")

    def test_model_label_uses_last_folder_and_trims_gguf_suffix(self):
        label = self.hub._model_label(
            "bartowski/deepriinforce-ai_Orinith-1.0-397B-GGUF-aaa-aa/xxxxxxx.gguf",
            226.56,
            7,
        )
        self.assertEqual(label, "deepriinforce-ai_Orinith-1.0-397B (226.56 GiB, 7 shards)")

    def test_resolve_model_accepts_relative_id(self):
        path = self._write("author/model/model-Q4.gguf", 10)
        resolved, meta = self.hub._resolve_model("author/model/model-Q4.gguf")
        self.assertEqual(os.path.abspath(path), resolved)
        self.assertEqual(meta["primary_file"], "author/model/model-Q4.gguf")


class HubLocalSlotTests(unittest.TestCase):
    def setUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        install_gguf_stub()
        from controller import hub

        self.hub = hub
        self.tmp = tempfile.TemporaryDirectory()
        self._orig_state_file = hub.HUB_STATE_FILE
        hub.NODES.clear()
        hub.CTRLS.clear()
        hub.REMOTE_UNITS.clear()
        hub.HUB_STATE_FILE = os.path.join(self.tmp.name, "hub-state.json")
        hub._LOCAL_SLOTS_LOADED = False
        self._orig_list_gpus = hub.list_gpus
        hub.list_gpus = lambda: [{"uuid": "gpu-a", "name": "NVIDIA GeForce RTX 4090", "vram_total_gib": 24.0}]

    def tearDown(self):
        self.hub.list_gpus = self._orig_list_gpus
        self.hub.HUB_STATE_FILE = self._orig_state_file
        self.hub._LOCAL_SLOTS_LOADED = False
        self.tmp.cleanup()

    def test_api_nodes_exposes_fixed_local_slots(self):
        result = self.hub.api_nodes()
        local = [n for n in result["nodes"] if n["kind"] == "local"]

        self.assertEqual(len(local), self.hub.MAX_NODES)
        self.assertEqual(local[0]["id"], "node-slot-1")
        self.assertEqual(local[0]["rpc_port"], self.hub.RPC_BASE)
        self.assertFalse(local[0]["assigned"])

    def test_create_node_assigns_requested_slot(self):
        result = self.hub.api_create_node(self.hub.CreateNode(
            slot_id="node-slot-2", gpu_uuid="gpu-a", name="gpu-slot", vram=12, ram=32, cores=8))

        self.assertEqual(result["id"], "node-slot-2")
        self.assertTrue(result["assigned"])
        self.assertEqual(result["name"], "gpu-slot")
        self.assertEqual(result["rpc_port"], self.hub.RPC_BASE + 1)

    def test_migrates_unbound_gb10_ram_budget_to_vram(self):
        self.hub.list_gpus = lambda: [{
            "uuid": "gpu-gb10", "name": "NVIDIA GB10", "vram_total_gib": 119.0,
        }]
        self.hub._ensure_local_slots()
        slot = self.hub.NODES["node-slot-2"]
        slot.update({"assigned": True, "gpu_uuid": "gpu-gb10", "gpu_name": "NVIDIA GB10",
                     "vram": 0.0, "ram": 30.0, "cores": 10, "bound_to": None})

        self.hub._migrate_unified_memory_local_slots()

        self.assertEqual(slot["vram"], 30.0)
        self.assertEqual(slot["ram"], 0.0)

    def test_does_not_migrate_bound_gb10_slot_while_agent_is_running(self):
        self.hub.list_gpus = lambda: [{
            "uuid": "gpu-gb10", "name": "NVIDIA GB10", "vram_total_gib": 119.0,
        }]
        self.hub._ensure_local_slots()
        slot = self.hub.NODES["node-slot-2"]
        slot.update({"assigned": True, "gpu_uuid": "gpu-gb10", "gpu_name": "NVIDIA GB10",
                     "vram": 0.0, "ram": 30.0, "cores": 10, "bound_to": "ctrl-a"})

        self.hub._migrate_unified_memory_local_slots()

        self.assertEqual(slot["vram"], 0.0)
        self.assertEqual(slot["ram"], 30.0)

    def test_slot_assignment_persists_across_hub_restart(self):
        self.hub.api_create_node(self.hub.CreateNode(
            slot_id="node-slot-3", gpu_uuid="gpu-a", name="persisted", vram=11, ram=22, cores=6))

        self.hub.NODES.clear()
        self.hub._LOCAL_SLOTS_LOADED = False
        result = self.hub.api_nodes()
        restored = next(n for n in result["nodes"] if n["id"] == "node-slot-3")

        self.assertTrue(restored["assigned"])
        self.assertEqual(restored["name"], "persisted")
        self.assertEqual(restored["vram"], 11)
        self.assertEqual(restored["ram"], 22)
        self.assertEqual(restored["cores"], 6)

    def test_controller_and_binding_persist_across_hub_restart(self):
        self.hub.api_create_node(self.hub.CreateNode(
            slot_id="node-slot-2", gpu_uuid="gpu-a", name="persisted-node", vram=12, ram=24, cores=6))
        ctrl = self.hub.api_create_ctrl(self.hub.CreateCtrl(name="persisted-ctrl"))
        import asyncio
        asyncio.run(self.hub.api_bind(ctrl["id"], self.hub.BindNode(node_id="node-slot-2")))

        self.hub.NODES.clear()
        self.hub.CTRLS.clear()
        self.hub.REMOTE_UNITS.clear()
        self.hub._LOCAL_SLOTS_LOADED = False

        controllers = self.hub.api_ctrls()["controllers"]
        nodes = self.hub.api_nodes()["nodes"]
        restored_ctrl = next(c for c in controllers if c["id"] == ctrl["id"])
        restored_node = next(n for n in nodes if n["id"] == "node-slot-2")

        self.assertEqual(restored_ctrl["name"], "persisted-ctrl")
        self.assertEqual(restored_ctrl["nodes"], ["node-slot-2"])
        self.assertEqual(restored_node["name"], "persisted-node")
        self.assertEqual(restored_node["bound_to"], ctrl["id"])
        self.assertEqual(restored_node["bound_to_name"], "persisted-ctrl")

    def test_bound_slot_rejects_resource_change_and_clear(self):
        self.hub.api_create_node(self.hub.CreateNode(slot_id="node-slot-1", gpu_uuid="gpu-a"))
        self.hub.CTRLS["ctrl-a"] = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": [], "model": None,
            "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        import asyncio
        asyncio.run(self.hub.api_bind("ctrl-a", self.hub.BindNode(node_id="node-slot-1")))

        with self.assertRaises(Exception) as change_ctx:
            self.hub.api_create_node(self.hub.CreateNode(slot_id="node-slot-1", gpu_uuid="gpu-a", vram=10))
        self.assertEqual(getattr(change_ctx.exception, "status_code", None), 409)

        with self.assertRaises(Exception) as clear_ctx:
            import asyncio
            asyncio.run(self.hub.api_del_node("node-slot-1"))
        self.assertEqual(getattr(clear_ctx.exception, "status_code", None), 409)

    def test_metal_resource_update_recovers_orphaned_agent_binding(self):
        """A recreated hub can safely reclaim its one idle Metal slot."""
        import asyncio

        self.hub.CTRLS["ctrl-current"] = {
            "id": "ctrl-current", "name": "Mac Metal Slot 1", "nodes": [],
            "model": None, "ctx": 4096, "parallel": 1, "phase": "idle",
            "detail": "", "plan": None, "master": None, "master_port": 8080,
            "operations": {},
        }
        self.hub.NODES["agent-metal"] = {
            "id": "agent-metal", "kind": "agent", "name": "Slot 1",
            "bound_to": "ctrl-gone", "vram": 48.0, "ram": 48.0, "cores": 14,
            "gpu_uuid": "metal0", "gpu_name": "Apple Metal", "rpc_port": 50052,
            "backend": {"backend_kind": "metal"}, "resources": {},
        }
        requests = []

        async def agent_request(node, method, path, body=None, **kwargs):
            requests.append((method, path, body))
            if path == "/control/join":
                return {"resources": {"vram_budget_gib": 36.0,
                                      "ram_budget_gib": 36.0,
                                      "cores_budget": 12},
                        "backend": {"backend_kind": "metal"}}
            return {}

        with mock.patch.object(self.hub, "_agent_request", side_effect=agent_request):
            result = asyncio.run(self.hub.api_update_metal_slot_resources(
                "ctrl-gone", self.hub.MetalSlotResources(
                    ram_budget_gib=36, cores_budget=12, node_id="agent-metal")))

        self.assertEqual(result["id"], "ctrl-current")
        self.assertEqual(self.hub.NODES["agent-metal"]["bound_to"], "ctrl-current")
        self.assertEqual(self.hub.CTRLS["ctrl-current"]["nodes"], ["agent-metal"])
        self.assertEqual(requests[0][1], "/unbind")
        self.assertEqual(requests[1][2]["controller_id"], "ctrl-current")


class HubRemoteUnitTests(unittest.TestCase):
    def setUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        install_gguf_stub()
        from controller import hub

        self.hub = hub
        hub.NODES.clear()
        hub.CTRLS.clear()
        hub.REMOTE_UNITS.clear()

    def test_remote_unit_url_accepts_common_separator_typos(self):
        cases = [
            "http://192.168.0.22;19000",
            "http;/192.168.0.22;19000",
            "192.168.0.22;19000",
        ]
        for unit_url in cases:
            with self.subTest(unit_url=unit_url):
                parsed = self.hub._parse_remote_unit_ref(self.hub.RegisterRemoteUnit(unit_url=unit_url))
                self.assertEqual(parsed, "http://192.168.0.22:19000")

    def test_remote_unit_projects_nodes_with_controller_info(self):
        remote_info = {
            "controllers": [{"id": "ctrl-a", "name": "serve-a"}],
            "nodes": [{
                "id": "node-a",
                "name": "3090-remote",
                "gpu_name": "NVIDIA GeForce RTX 3090",
                "vram": 24,
                "ram": 64,
                "cores": 16,
                "rpc_host": "10.0.0.5",
                "rpc_port": 50052,
                "bound_to": "ctrl-a",
            }],
        }

        unit = self.hub._upsert_remote_unit_nodes("unit-test", remote_info, "http://unit.local:9000", "unit-a")
        self.assertEqual(unit["node_ids"], ["runit-unit-test-node-a"])
        node = self.hub.NODES["runit-unit-test-node-a"]
        self.assertEqual(node["kind"], "remote_unit_node")
        self.assertEqual(node["remote_unit_name"], "unit-a")
        self.assertEqual(node["remote_controller_name"], "serve-a")
        self.assertEqual(node["rpc_host"], "unit.local")
        self.assertEqual(node["remote_source_rpc_endpoint"], "10.0.0.5:50052")

    def test_remote_unit_nodes_are_visible_only_to_the_registering_controller(self):
        self.hub.CTRLS.update({
            "ctrl-a": {"id": "ctrl-a", "name": "A", "nodes": [], "model": None,
                       "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
                       "plan": None, "master": None, "master_port": 8080, "operations": {}},
            "ctrl-b": {"id": "ctrl-b", "name": "B", "nodes": [], "model": None,
                       "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
                       "plan": None, "master": None, "master_port": 8081, "operations": {}},
        })
        self.hub.NODES["node-local"] = {
            "id": "node-local", "kind": "local", "name": "local", "assigned": True,
            "gpu_uuid": "gpu", "gpu_name": "GPU", "vram": 8, "ram": 16, "cores": 8,
            "rpc_host": "127.0.0.1", "rpc_port": 50052, "bound_to": None,
            "worker": None, "ram_used": 0.0, "log": "",
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052}],
        }, "http://unit.local:9000", "unit-a", "ctrl-a")

        a_nodes = {n["id"] for n in self.hub.ctrl_view(self.hub.CTRLS["ctrl-a"], full=True)["available_node_items"]}
        b_nodes = {n["id"] for n in self.hub.ctrl_view(self.hub.CTRLS["ctrl-b"], full=True)["available_node_items"]}
        global_nodes = {n["id"] for n in self.hub.api_nodes()["nodes"]}
        self.assertIn("runit-unit-test-node-a", a_nodes)
        self.assertNotIn("runit-unit-test-node-a", b_nodes)
        self.assertNotIn("runit-unit-test-node-a", global_nodes)

    def test_remote_unit_rewrites_advertised_rpc_endpoint_to_unit_host(self):
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{
                "id": "node-a",
                "rpc_endpoint": "127.0.0.1:50052",
            }],
        }, "http://192.168.0.22:19000", "unit-a")

        node = self.hub.NODES["runit-unit-test-node-a"]
        self.assertEqual(node["rpc_host"], "192.168.0.22")
        self.assertEqual(node["rpc_port"], 50052)
        self.assertEqual(node["remote_source_rpc_endpoint"], "127.0.0.1:50052")

    def test_remote_unit_worker_status_defaults_to_reported_state(self):
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "127.0.0.1", "rpc_port": 50052}],
        }, "http://192.168.0.22:19000", "unit-a")

        node = self.hub.NODES["runit-unit-test-node-a"]
        self.assertFalse(self.hub.node_view(node)["worker_running"])

        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "127.0.0.1", "rpc_port": 50052,
                       "worker_running": True}],
        }, "http://192.168.0.22:19000", "unit-a")

        self.assertTrue(self.hub.node_view(self.hub.NODES["runit-unit-test-node-a"])["worker_running"])

    def test_remote_unit_skips_unassigned_remote_slots(self):
        unit = self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [
                {"id": "node-a", "assigned": True, "gpu_uuid": "gpu-a", "rpc_host": "127.0.0.1", "rpc_port": 50052},
                {"id": "node-b", "assigned": False, "gpu_uuid": "", "rpc_host": "127.0.0.1", "rpc_port": 50053},
            ],
        }, "http://192.168.0.22:19000", "unit-a")

        self.assertEqual(unit["node_ids"], ["runit-unit-test-node-a"])
        self.assertIn("runit-unit-test-node-a", self.hub.NODES)
        self.assertNotIn("runit-unit-test-node-b", self.hub.NODES)

    def test_remote_unit_node_cannot_be_deleted_individually(self):
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052}],
        }, "http://unit.local:9000", "unit-a")

        with self.assertRaises(Exception) as ctx:
            import asyncio
            asyncio.run(self.hub.api_del_node("runit-unit-test-node-a"))
        self.assertEqual(getattr(ctx.exception, "status_code", None), 409)
        self.assertIn("unit-test", self.hub.REMOTE_UNITS)

    def test_ring_proxy_load_fails_closed_instead_of_falling_back_to_rpc(self):
        import asyncio

        ctrl = {
            "id": "ctrl-test", "name": "ctrl-test", "nodes": ["node-local"],
            "model": None, "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "last_load": {}, "master_port": 8080,
            "operations": {},
        }
        self.hub.CTRLS["ctrl-test"] = ctrl
        self.hub.NODES["node-local"] = {
            "id": "node-local", "kind": "local", "name": "local", "assigned": True,
            "gpu_uuid": "gpu", "gpu_name": "GPU", "vram": 8, "ram": 16, "cores": 8,
            "rpc_host": "127.0.0.1", "rpc_port": 50052, "bound_to": "ctrl-test",
            "worker": None, "ram_used": 0.0, "log": "",
        }
        original = self.hub._do_plan
        try:
            self.hub._do_plan = lambda c, req: {
                "feasible": True,
                "adaptive_load_available": True,
                "model_ref": req.model,
                "placement": [{"node": 0, "n_layers": 1, "layers": [0, 1]}],
                "runtime_mode": "ring_proxy",
                "data_plane": self.hub.data_plane_contract("ring_proxy", []),
            }
            response = asyncio.run(self.hub.api_ctrl_serve(
                "ctrl-test", self.hub.ServeReq(model="model.gguf", runtime_mode="ring_proxy")
            ))
        finally:
            self.hub._do_plan = original

        self.assertEqual(response.status_code, 501)
        self.assertEqual(response["error"], "runtime_mode_unavailable")
        self.assertEqual(response["runtime_mode"], "ring_proxy")
        self.assertFalse(response["data_plane"]["available"])
        self.assertFalse(ctrl.get("master"))

    def test_delete_remote_unit_removes_projected_nodes(self):
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052}],
        }, "http://unit.local:9000", "unit-a")

        self.hub._delete_remote_unit("unit-test")
        self.assertNotIn("runit-unit-test-node-a", self.hub.NODES)

    def test_backend_difference_does_not_block_remote_unit_node_bind(self):
        current = self.hub.runtime_identity()
        self.hub.CTRLS["ctrl-a"] = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": [], "model": None,
            "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "runtime": {**current, "llama_cpp_backend": "vulkan"},
            "backend": {"backend_kind": "vulkan", "backend_runtime_version": "1.3"},
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052}],
        }, "http://unit.local:9000", "unit-a")

        import asyncio
        result = asyncio.run(self.hub.api_bind("ctrl-a", self.hub.BindNode(node_id="runit-unit-test-node-a")))
        self.assertEqual(result["nodes"], ["runit-unit-test-node-a"])

    def test_remote_unit_bound_node_cannot_be_bound_locally(self):
        self.hub.CTRLS["ctrl-a"] = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": [], "model": None,
            "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [{"id": "remote-ctrl", "name": "remote serve"}],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052,
                       "bound_to": "remote-ctrl"}],
        }, "http://unit.local:9000", "unit-a")

        with self.assertRaises(Exception) as ctx:
            import asyncio
            asyncio.run(self.hub.api_bind("ctrl-a", self.hub.BindNode(node_id="runit-unit-test-node-a")))
        self.assertEqual(getattr(ctx.exception, "status_code", None), 409)
        self.assertIn("remote serve", str(ctx.exception))

    def test_remote_unit_not_ready_blocks_load_before_queue(self):
        import asyncio

        self.hub.CTRLS["ctrl-a"] = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": ["runit-unit-test-node-a"],
            "model": None, "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "name": "remote-a", "rpc_host": "127.0.0.1",
                       "rpc_port": 50052, "vram": 8, "ram": 16, "cores": 8}],
        }, "http://unit.local:9000", "unit-a")
        self.hub.NODES["runit-unit-test-node-a"]["bound_to"] = "ctrl-a"

        original_plan = self.hub._do_plan
        original_check = self.hub._check_remote_unit_nodes_ready
        try:
            self.hub._do_plan = lambda c, req: {
                "feasible": True,
                "model_ref": req.model,
                "adaptive_load_available": True,
                "placement": [{"node": 0, "n_layers": 1, "layers": [0, 1]}],
            }

            async def not_ready(c, req, active):
                return [{"node_id": "runit-unit-test-node-a", "node_name": "remote-a",
                         "endpoint": "unit.local:50052", "ready": False,
                         "worker_running": False, "tcp_reachable": True,
                         "error": "worker start endpoint unavailable"}]

            self.hub._check_remote_unit_nodes_ready = not_ready
            response = asyncio.run(self.hub.api_ctrl_serve(
                "ctrl-a",
                self.hub.ServeReq(model="model.gguf", ctx=4096, parallel=1),
            ))
        finally:
            self.hub._do_plan = original_plan
            self.hub._check_remote_unit_nodes_ready = original_check

        self.assertEqual(response.status_code, 409)
        self.assertEqual(response["error"], "remote_unit_not_ready")
        ops = list(self.hub.CTRLS["ctrl-a"]["operations"].values())
        self.assertTrue(any(op["phase"] == "remote_unit_not_ready" for op in ops))
        self.assertEqual(self.hub.CTRLS["ctrl-a"]["phase"], "idle")

    def test_remote_managed_agent_uses_live_rpc_after_accepted_start(self):
        import asyncio

        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "name": "metal-a", "rpc_host": "127.0.0.1",
                       "rpc_port": 50052, "vram": 8, "ram": 16, "cores": 8,
                       "capabilities": {"native_agent": True},
                       "backend": {"backend_kind": "metal"}}],
        }, "http://unit.local:9000", "unit-a")
        node = self.hub.NODES["runit-unit-test-node-a"]

        original_refresh = self.hub._refresh_remote_unit_node
        original_start = self.hub._start_remote_unit_worker
        original_tcp = self.hub._tcp_reachable
        original_wait = self.hub._wait_node_rpc_ready
        try:
            async def refresh(_node):
                _node["worker_running"] = False

            async def start(_node):
                _node["worker_running"] = False
                return {"node": {"worker_running": False}}

            async def wait_ready(_node, timeout=20.0):
                return {"worker_running": False, "tcp_reachable": True,
                        "rpc_log_ready": False, "error": "", "ready": False}

            self.hub._refresh_remote_unit_node = refresh
            self.hub._start_remote_unit_worker = start
            self.hub._tcp_reachable = lambda host, port, timeout=1.0: True
            self.hub._wait_node_rpc_ready = wait_ready
            check = asyncio.run(self.hub._remote_unit_node_ready_check(node, start=True))
        finally:
            self.hub._refresh_remote_unit_node = original_refresh
            self.hub._start_remote_unit_worker = original_start
            self.hub._tcp_reachable = original_tcp
            self.hub._wait_node_rpc_ready = original_wait

        self.assertTrue(check["started"])
        self.assertTrue(check["worker_running"])
        self.assertTrue(check["tcp_reachable"])
        self.assertTrue(check["ready"])

    def test_remote_unit_can_be_last_rpc_node_in_topology(self):
        import asyncio

        ctrl = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": ["node-local", "runit-unit-test-node-a"],
            "model": "model.gguf", "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        self.hub.CTRLS["ctrl-a"] = ctrl
        self.hub.NODES["node-local"] = {
            "id": "node-local", "kind": "local", "name": "local",
            "rpc_host": "127.0.0.1", "rpc_port": 50052,
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "controllers": [],
            "nodes": [{"id": "node-a", "name": "remote-last",
                       "rpc_endpoint": "127.0.0.1:50053", "worker_running": True}],
        }, "http://192.168.0.22:19000", "unit-a")
        active = [
            ("node-local", {"node": 0, "n_layers": 20, "layers": [0, 20]}),
            ("runit-unit-test-node-a", {"node": 1, "n_layers": 20, "layers": [20, 40]}),
        ]

        original_check = self.hub._remote_unit_node_ready_check
        try:
            async def ready_check(n, start=False):
                return {
                    "node_id": n["id"],
                    "node_name": n["name"],
                    "endpoint": self.hub._node_rpc_endpoint(n),
                    "ready": True,
                    "worker_running": True,
                    "tcp_reachable": True,
                    "error": "",
                    "started": False,
                }

            self.hub._remote_unit_node_ready_check = ready_check
            checks = asyncio.run(self.hub._check_remote_unit_nodes_ready(
                ctrl, self.hub.ServeReq(model="model.gguf", ctx=4096, parallel=1), active))
        finally:
            self.hub._remote_unit_node_ready_check = original_check

        topology = self.hub._rpc_topology(ctrl, active)
        self.assertEqual(topology[-1]["node_kind"], "remote_unit_node")
        self.assertTrue(topology[-1]["is_last"])
        self.assertEqual(topology[-1]["rpc_endpoint"], "192.168.0.22:50053")
        self.assertEqual(topology[0]["data_plane"]["mode"], "llama_rpc")
        self.assertEqual(topology[0]["data_plane"]["topology"], "master_star")
        self.assertEqual(checks[0]["placement"]["position"], 1)
        self.assertTrue(checks[0]["placement"]["is_last"])
        self.assertEqual(checks[0]["placement"]["remote_source_rpc_endpoint"], "127.0.0.1:50053")

    def test_rpc_abi_mismatch_blocks_remote_unit_node_bind(self):
        current = self.hub.runtime_identity()
        self.hub.CTRLS["ctrl-a"] = {
            "id": "ctrl-a", "name": "ctrl-a", "nodes": [], "model": None,
            "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
            "plan": None, "master": None, "master_port": 8080, "operations": {},
        }
        self.hub._upsert_remote_unit_nodes("unit-test", {
            "runtime": {**current, "rpc_abi": "different-rpc"},
            "backend": {"backend_kind": "cuda"},
            "controllers": [],
            "nodes": [{"id": "node-a", "rpc_host": "10.0.0.5", "rpc_port": 50052}],
        }, "http://unit.local:9000", "unit-a")

        with self.assertRaises(Exception) as ctx:
            import asyncio
            asyncio.run(self.hub.api_bind("ctrl-a", self.hub.BindNode(node_id="runit-unit-test-node-a")))
        self.assertEqual(getattr(ctx.exception, "status_code", None), 409)


class HubServeOptionTests(unittest.TestCase):
    def setUp(self):
        install_httpx_stub()
        install_fastapi_stub()
        install_gguf_stub()
        from controller import hub

        self.hub = hub

    def test_rpc_loader_has_no_stage_manifest_or_ring_adapter_dependency(self):
        import inspect

        source = inspect.getsource(self.hub._serve_llama_rpc)
        self.assertNotIn("stage_manifest", source)
        self.assertNotIn("ring_proxy", source)
        self.assertIn("_load_rpc_node", source)

    def test_perf_options_are_added_to_llama_server_command(self):
        req = self.hub.ServeReq(
            model="model.gguf",
            ctx=4096,
            parallel=2,
            batch=1024,
            ubatch=256,
            poll=50,
            cache_reuse=128,
            spec_type="ngram-cache",
            spec_draft_n_max=3,
            spec_draft_n_min=1,
            spec_draft_p_min=0.6,
            spec_ngram_mod_n_min=2,
            spec_ngram_mod_n_max=5,
            spec_ngram_mod_n_match=2,
            cont_batching=False,
        )

        cmd = self.hub._append_llama_perf_args(["llama-server"], req)

        self.assertIn("--spec-type", cmd)
        self.assertIn("ngram-cache", cmd)
        self.assertIn("-b", cmd)
        self.assertIn("1024", cmd)
        self.assertIn("-ub", cmd)
        self.assertIn("256", cmd)
        self.assertIn("--poll", cmd)
        self.assertIn("50", cmd)
        self.assertIn("--cache-reuse", cmd)
        self.assertIn("128", cmd)
        self.assertIn("--spec-draft-n-max", cmd)
        self.assertIn("3", cmd)
        self.assertIn("--spec-draft-p-min", cmd)
        self.assertIn("0.6", cmd)
        self.assertIn("--spec-ngram-mod-n-match", cmd)
        self.assertIn("2", cmd)
        self.assertIn("--no-cont-batching", cmd)

    def test_master_load_timeout_scales_with_model_weight(self):
        with mock.patch.dict("os.environ", {}, clear=False):
            self.assertEqual(self.hub._master_load_timeout_s({"total_weight_gib": 12.7}), 900)
            self.assertEqual(self.hub._master_load_timeout_s({"total_weight_gib": 87.21}), 1696)

    def test_load_stall_timeout_uses_twice_measured_full_transfer_time(self):
        gib = 1024 ** 3
        # 100 GiB at 100 MiB/s needs 1,024 s; allow two full transfers.
        self.assertEqual(
            self.hub._load_stall_timeout_s(100 * gib, 10 * gib, 100 * 1024 ** 2, 900),
            2048,
        )
        # Before a worker has transferred tensors, retain the bootstrap window.
        self.assertEqual(self.hub._load_stall_timeout_s(100 * gib, 0, 0, 1696), 1696)

    def test_rpc_loader_always_disables_mmap(self):
        import inspect

        source = inspect.getsource(self.hub._serve_llama_rpc)
        self.assertIn('cmd.append("--no-mmap")', source)
        self.assertIn('cmd += ["--fit", "off"]', source)

    def test_master_startup_fatal_recognizes_rpc_graph_failure(self):
        self.assertTrue(self.hub._master_startup_fatal(
            "Remote RPC server crashed or returned malformed response\n"
            "[create_node] invalid data ptr"
        ))
        self.assertFalse(self.hub._master_startup_fatal("loading model shard 3 of 4"))

    def test_perf_snapshot_omits_defaults_and_keeps_explicit_options(self):
        req = self.hub.ServeReq(
            model="model.gguf",
            ctx=4096,
            parallel=1,
            spec_type="ngram-mod",
            spec_ngram_mod_n_min=2,
            spec_ngram_mod_n_max=6,
            spec_ngram_mod_n_match=3,
            cont_batching=True,
        )

        snapshot = self.hub._serve_req_snapshot(req)

        self.assertEqual(snapshot["performance"]["spec_type"], "ngram-mod")
        self.assertEqual(snapshot["performance"]["spec_ngram_mod_n_max"], 6)
        self.assertNotIn("cont_batching", snapshot["performance"])
        self.assertNotIn("batch", snapshot["performance"])

    def test_invalid_spec_type_is_rejected(self):
        from fastapi import HTTPException

        with self.assertRaises(HTTPException) as ctx:
            self.hub._validate_spec_types("none,ngram-cache")
        self.assertEqual(ctx.exception.status_code, 400)

        with self.assertRaises(HTTPException) as ctx:
            self.hub._validate_spec_types("unknown")
        self.assertEqual(ctx.exception.status_code, 400)

    def test_rpc_log_activity_is_ready_after_startup_banner_ages_out(self):
        self.assertTrue(self.hub._rpc_log_ready("[graph_compute] device: 0, n_nodes: 12"))
        self.assertTrue(self.hub._rpc_log_ready("[graph_recompute] device: 0"))
        self.assertFalse(self.hub._rpc_log_ready("failed to initialize CUDA"))

    def test_rpc_activity_parser_accumulates_server_timings(self):
        parsed = self.hub._parse_rpc_activity_text(
            "[rpc_timing] op=graph_compute device=0 n_nodes=12 n_tensors=100 elapsed_us=1200\n"
            "[rpc_timing] op=graph_recompute device=0 elapsed_us=800\n"
            "[rpc_timing] op=get_tensor bytes=4096 elapsed_us=70\n"
        )

        self.assertEqual(parsed["timings_us"], {
            "graph_compute": 1200,
            "graph_recompute": 800,
            "get_tensor": 70,
        })


class PlannerHeuristicTests(unittest.TestCase):
    def test_read_model_accumulates_tensor_tables_from_split_shards(self):
        from controller import planner

        class Tensor:
            def __init__(self, name, n_bytes):
                self.name = name
                self.n_bytes = n_bytes

        class Reader:
            def __init__(self, fields, tensors):
                self.fields = fields
                self.tensors = tensors

        fields = {
            "general.architecture": "qwen",
            "qwen.block_count": 2,
            "qwen.attention.head_count": 1,
            "qwen.attention.head_count_kv": 1,
            "qwen.embedding_length": 2,
        }
        readers = {
            "primary.gguf": Reader(fields, []),
            "part-2.gguf": Reader({}, [
                Tensor("blk.0.attn_q.weight", 10),
                Tensor("blk.0.ffn_up_exps.weight", 20),
            ]),
            "part-3.gguf": Reader({}, [
                Tensor("blk.1.attn_q.weight", 30),
                Tensor("output.weight", 40),
            ]),
        }
        with mock.patch.object(planner, "GGUFReader", side_effect=lambda path: readers[path]), \
             mock.patch.object(planner, "_scalar", side_effect=lambda value: value):
            model = planner.read_model("primary.gguf", ["primary.gguf", "part-2.gguf", "part-3.gguf"])

        self.assertEqual(model["weight_layer"], [30, 30])
        self.assertEqual(model["expert_layer"], [20, 0])
        self.assertEqual(model["boundary_bytes"], 40)
        self.assertEqual(model["total_weight"], 100)

    def test_read_model_accepts_per_layer_attention_head_counts(self):
        from controller import planner

        class Reader:
            fields = {
                "general.architecture": "step",
                "step.block_count": 2,
                "step.attention.head_count": [64, 96],
                "step.attention.head_count_kv": [8, 8],
                "step.embedding_length": 4096,
                "step.attention.key_length": 128,
                "step.attention.value_length": 128,
            }
            tensors = []

        with mock.patch.object(planner, "GGUFReader", return_value=Reader()), \
             mock.patch.object(planner, "_scalar", side_effect=lambda value: value):
            model = planner.read_model("step.gguf")

        self.assertEqual(model["n_head_kv_by_layer"], [8, 8])
        self.assertEqual(model["n_head_kv"], 8)

    def test_planner_prioritizes_kv_body_then_ffn_ram(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 2,
            "n_head_kv": 1,
            "head_dim_k": 1024,
            "head_dim_v": 1024,
            "n_expert": 2,
            "weight_layer": [2 * planner.GiB, 2 * planner.GiB],
            "expert_layer": [1 * planner.GiB, 1 * planner.GiB],
            "body_tensor_names": [["blk.0.attn_q.weight"], ["blk.1.attn_q.weight"]],
            "expert_tensor_names": [["blk.0.ffn_up_exps.weight"], ["blk.1.ffn_up_exps.weight"]],
            "boundary_bytes": 0,
            "total_weight": 4 * planner.GiB,
        }
        result = planner.plan(model, [{"vram": 4.2, "ram": 4, "cores": 8}], 32768, 1, reserve_mib=512)

        self.assertTrue(result["feasible"])
        placement = result["placement"][0]
        self.assertGreater(placement["kv_vram_gib"], 0)
        self.assertGreater(placement["layer_body_vram_gib"], 0)
        self.assertGreater(placement["offload_ram_gib"], 0)
        self.assertEqual(placement["offload_policy"], "ffn=RAM")

    def test_planner_uses_body_cpu_when_kv_cannot_fit_body(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 1,
            "n_head_kv": 1,
            "head_dim_k": 1024,
            "head_dim_v": 1024,
            "n_expert": 0,
            "weight_layer": [2 * planner.GiB],
            "expert_layer": [0],
            "body_tensor_names": [["blk.0.attn_q.weight", "blk.0.ffn_down.weight"]],
            "expert_tensor_names": [[]],
            "boundary_bytes": 0,
            "total_weight": 2 * planner.GiB,
        }
        result = planner.plan(model, [{"vram": 1.0, "ram": 8, "cores": 8}], 32768, 1, reserve_mib=512)

        self.assertTrue(result["feasible"])
        placement = result["placement"][0]
        self.assertEqual(placement["layer_body_vram_gib"], 0)
        self.assertGreater(placement["layer_body_ram_gib"], 0)
        self.assertTrue(placement["body_on_cpu"])
        self.assertIn("body=RAM", placement["offload_policy"])
        self.assertIn("blk\\.0\\.attn_q\\.weight=CPU", placement["ot"])

    def test_planner_rejects_cpu_offload_that_exceeds_master_ram(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 1, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 0,
            "weight_layer": [4 * planner.GiB], "expert_layer": [0],
            "body_tensor_names": [["blk.0.attn_q.weight"]], "expert_tensor_names": [[]],
            "boundary_bytes": 0, "total_weight": 4 * planner.GiB,
        }
        result = planner.plan(
            model, [{"vram": 1, "ram": 8, "cores": 8}], 1, 1,
            reserve_mib=0, master_ram_gib=2,
        )

        self.assertFalse(result["feasible"])
        self.assertIn("master RAM", result["reason"])
        self.assertEqual(result["master_ram_budget_gib"], 2.0)

    def test_planner_counts_mmap_resident_model_weight_against_master_ram(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 1, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 0,
            "weight_layer": [4 * planner.GiB], "expert_layer": [0],
            "body_tensor_names": [["blk.0.attn_q.weight"]], "expert_tensor_names": [[]],
            "boundary_bytes": 0, "total_weight": 4 * planner.GiB,
        }
        result = planner.plan(
            model, [{"vram": 8, "ram": 8, "cores": 8}], 1, 1,
            reserve_mib=0, master_ram_gib=3,
        )

        self.assertFalse(result["feasible"])
        self.assertEqual(result["master_mmap_resident_required_gib"], 4.0)
        self.assertIn("mmap-resident", result["reason"])

    def test_infeasible_plan_reports_master_ram_lower_bound(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 1, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 0,
            "weight_layer": [12 * planner.GiB], "expert_layer": [0],
            "body_tensor_names": [["blk.0.attn_q.weight"]], "expert_tensor_names": [[]],
            "boundary_bytes": 0, "total_weight": 12 * planner.GiB,
        }
        result = planner.plan(
            model, [{"vram": 4, "ram": 64, "cores": 8}], 1, 1,
            reserve_mib=0, no_cpu_offload=True, master_ram_gib=2,
        )

        self.assertFalse(result["feasible"])
        self.assertEqual(result["master_cpu_ram_lower_bound_gib"], 8.0)
        self.assertEqual(result["master_mmap_resident_lower_bound_gib"], 12.0)
        self.assertEqual(result["master_ram_lower_bound_gib"], 12.0)
        self.assertIn("master RAM", result["reason"])

    def test_dense_planner_can_fall_back_to_ram_kv(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 2,
            "n_head_kv": 8,
            "head_dim_k": 4096,
            "head_dim_v": 4096,
            "n_expert": 0,
            "weight_layer": [1 * planner.GiB, 1 * planner.GiB],
            "expert_layer": [0, 0],
            "body_tensor_names": [["blk.0.attn_q.weight"], ["blk.1.attn_q.weight"]],
            "expert_tensor_names": [[], []],
            "boundary_bytes": 0,
            "total_weight": 2 * planner.GiB,
        }
        result = planner.plan(model, [{"vram": 2.5, "ram": 40, "cores": 8}], 65536, 2, reserve_mib=512)

        self.assertTrue(result["feasible"])
        self.assertEqual(result["kv_cache_location"], "ram")
        self.assertFalse(result["kv_offload_enabled"])
        placement = result["placement"][0]
        self.assertEqual(placement["kv_vram_gib"], 0)
        self.assertGreater(placement["kv_ram_gib"], 0)

    def test_planner_uses_independent_k_and_v_cache_types(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 2,
            "n_head_kv": 4,
            "head_dim_k": 1024,
            "head_dim_v": 1024,
            "n_expert": 0,
            "weight_layer": [1 * planner.GiB, 1 * planner.GiB],
            "expert_layer": [0, 0],
            "body_tensor_names": [["blk.0.attn_q.weight"], ["blk.1.attn_q.weight"]],
            "expert_tensor_names": [[], []],
            "boundary_bytes": 0,
            "total_weight": 2 * planner.GiB,
        }

        fp16 = planner.plan(model, [{"vram": 12, "ram": 20, "cores": 8}], 32768, 2,
                            cache_type_k="f16", cache_type_v="f16")
        mixed = planner.plan(model, [{"vram": 12, "ram": 20, "cores": 8}], 32768, 2,
                             cache_type_k="q4_0", cache_type_v="q8_0")

        self.assertTrue(fp16["feasible"])
        self.assertTrue(mixed["feasible"])
        self.assertEqual(mixed["cache_type_k"], "q4_0")
        self.assertEqual(mixed["cache_type_v"], "q8_0")
        self.assertLess(mixed["kv_total_gib"], fp16["kv_total_gib"])

    def test_planner_accounts_for_per_layer_kv_heads(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 2,
            "n_head_kv": 8,
            "n_head_kv_by_layer": [8, 1],
            "head_dim_k": 1024,
            "head_dim_v": 1024,
            "n_expert": 0,
            "weight_layer": [1 * planner.GiB, 1 * planner.GiB],
            "expert_layer": [0, 0],
            "body_tensor_names": [[], []],
            "expert_tensor_names": [[], []],
            "boundary_bytes": 0,
            "total_weight": 2 * planner.GiB,
        }

        per_layer = planner.kv_bytes_per_layer(model, 1024, 1, cache_type_k="f16", cache_type_v="f16")

        self.assertEqual(len(per_layer), 2)
        self.assertEqual(per_layer[0], per_layer[1] * 8)

    def test_no_cpu_offload_moves_dense_layers_to_next_node(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 3,
            "n_head_kv": 1,
            "head_dim_k": 1,
            "head_dim_v": 1,
            "n_expert": 0,
            "weight_layer": [2 * planner.GiB, 2 * planner.GiB, 2 * planner.GiB],
            "expert_layer": [0, 0, 0],
            "body_tensor_names": [[], [], []],
            "expert_tensor_names": [[], [], []],
            "boundary_bytes": 0,
            "total_weight": 6 * planner.GiB,
        }

        result = planner.plan(
            model,
            [{"vram": 5, "ram": 8, "cores": 8}, {"vram": 5, "ram": 8, "cores": 8}],
            1,
            1,
            reserve_mib=512,
            no_cpu_offload=True,
        )

        self.assertTrue(result["feasible"])
        self.assertEqual(result["tensor_split"], [2, 1])
        self.assertTrue(result["no_cpu_offload"])
        self.assertTrue(all(p["offload_policy"] == "none" for p in result["placement"] if p["n_layers"]))

    def test_moe_planner_optimizes_layer_boundary_to_reduce_cpu_experts(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 4,
            "n_head_kv": 1,
            "head_dim_k": 1,
            "head_dim_v": 1,
            "n_expert": 2,
            "weight_layer": [1 * planner.GiB] * 4,
            "expert_layer": [1 * planner.GiB] * 4,
            "body_tensor_names": [[], [], [], []],
            "expert_tensor_names": [[f"blk.{i}.ffn_exps"] for i in range(4)],
            "boundary_bytes": 0,
            "total_weight": 4 * planner.GiB,
        }

        result = planner.plan(
            model,
            [{"vram": 1.5, "ram": 8, "cores": 8}, {"vram": 3.1, "ram": 8, "cores": 8}],
            1,
            1,
            reserve_mib=0,
            optimize_locality=True,
        )

        self.assertTrue(result["feasible"])
        self.assertEqual(result["tensor_split"], [1, 3])
        self.assertEqual(result["layer_split_strategy"], "cpu-offload-optimized")
        self.assertEqual(sum(p["ffn_ram_gib"] for p in result["placement"]), 0.0)

    def test_moe_promotion_keeps_requested_vram_reserve(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 1, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 2,
            "weight_layer": [3 * planner.GiB],
            "expert_layer": [2 * planner.GiB],
            "body_tensor_names": [[]],
            "expert_tensor_names": [["blk.0.ffn_exps"]],
            "boundary_bytes": 0,
            "total_weight": 3 * planner.GiB,
        }

        result = planner.plan(
            model, [{"vram": 3, "ram": 8, "cores": 8}], 1, 1,
            reserve_mib=1024, placement_strategy="cpu-minimized",
        )

        self.assertTrue(result["feasible"])
        placement = result["placement"][0]
        self.assertLessEqual(placement["vram_used_gib"], 2.0)
        self.assertGreater(placement["ffn_ram_gib"], 0.0)

    def test_cpu_minimized_search_recovers_from_infeasible_balanced_split(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 6, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 2,
            "weight_layer": [2 * planner.GiB] * 6,
            "expert_layer": [1 * planner.GiB] * 6,
            "body_tensor_names": [[] for _ in range(6)],
            "expert_tensor_names": [[f"blk.{i}.ffn_exps"] for i in range(6)],
            "boundary_bytes": 0, "total_weight": 12 * planner.GiB,
        }
        result = planner.plan(
            model,
            [{"vram": 4, "ram": 8, "cores": 8}, {"vram": 3, "ram": 8, "cores": 8},
             {"vram": 1.5, "ram": 8, "cores": 8}],
            1, 1, reserve_mib=0, placement_strategy="cpu-minimized",
        )

        self.assertTrue(result["feasible"])
        self.assertNotEqual(result["tensor_split"], [2, 2, 2])
        self.assertEqual(result["layer_split_strategy"], "cpu-offload-optimized")

    def test_planner_can_skip_constrained_final_node_after_all_node_failure(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 3, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 0,
            "weight_layer": [1 * planner.GiB] * 3, "expert_layer": [0, 0, 0],
            "body_tensor_names": [[], [], []], "expert_tensor_names": [[], [], []],
            "boundary_bytes": 2 * planner.GiB, "total_weight": 5 * planner.GiB,
        }
        result = planner.plan(
            model,
            [{"vram": 6, "ram": 8, "cores": 8}, {"vram": 6, "ram": 8, "cores": 8},
             {"vram": 1, "ram": 8, "cores": 8}],
            1, 1, reserve_mib=0, placement_strategy="vram-weighted",
        )

        self.assertTrue(result["feasible"])
        self.assertEqual(result["active_node_indexes"], [0, 1])
        self.assertEqual(result["tensor_split"], [2, 1])
        self.assertEqual(result["placement"][1]["node"], 1)

    def test_vram_weighted_strategy_distributes_layers_by_usable_capacity(self):
        from controller import planner

        model = {
            "arch": "test",
            "n_layer": 12,
            "n_head_kv": 1,
            "head_dim_k": 1,
            "head_dim_v": 1,
            "n_expert": 0,
            "weight_layer": [1] * 12,
            "expert_layer": [0] * 12,
            "body_tensor_names": [[] for _ in range(12)],
            "expert_tensor_names": [[] for _ in range(12)],
            "boundary_bytes": 0,
            "total_weight": 12,
        }

        result = planner.plan(
            model,
            [{"vram": 24, "ram": 8, "cores": 8}, {"vram": 12, "ram": 8, "cores": 8},
             {"vram": 6, "ram": 8, "cores": 8}],
            1,
            1,
            reserve_mib=0,
            placement_strategy="vram-weighted",
        )

        self.assertTrue(result["feasible"])
        self.assertEqual(result["tensor_split"], [6, 4, 2])
        self.assertEqual(result["layer_split_strategy"], "vram-weighted")

    def test_ring_stage_strategy_keeps_all_ranks_and_moves_output_to_largest_gpu(self):
        from controller import planner

        model = {
            "arch": "test", "n_layer": 12, "n_head_kv": 1,
            "head_dim_k": 1, "head_dim_v": 1, "n_expert": 0,
            "weight_layer": [1 * planner.GiB] * 12,
            "expert_layer": [0] * 12,
            "body_tensor_names": [[] for _ in range(12)],
            "expert_tensor_names": [[] for _ in range(12)],
            "boundary_bytes": 1 * planner.GiB,
            "total_weight": 13 * planner.GiB,
        }
        result = planner.plan(
            model,
            [{"vram": 6, "ram": 8, "cores": 8}, {"vram": 12, "ram": 8, "cores": 8},
             {"vram": 24, "ram": 8, "cores": 8}],
            1, 1, reserve_mib=0, placement_strategy="ring-stage-vram-weighted",
        )

        self.assertTrue(result["feasible"])
        self.assertTrue(result["requires_stage_runtime"])
        self.assertEqual(result["nodes_used"], 3)
        self.assertEqual(result["placement"][-1]["node"], 2)
        self.assertTrue(all(item["n_layers"] for item in result["placement"]))


if __name__ == "__main__":
    unittest.main()
