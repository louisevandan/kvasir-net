import os
import sys
import tempfile
import types
import unittest
from unittest import mock
from pathlib import Path

from controller.protocol import ModelSource, RuntimeIdentity
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
