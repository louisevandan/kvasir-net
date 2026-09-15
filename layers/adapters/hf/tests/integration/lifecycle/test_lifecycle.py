import copy
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

from p4hfadapter.integration.lifecycle import (
    ADAPTER_KIND,
    MAX_METADATA_BYTES,
    NODE_LIFECYCLE_RESULT,
    NODE_LOAD,
    NODE_UNLOAD,
    decode_request,
    decode_result,
    encode_request,
    encode_result,
)
from p4hfadapter.integration.packet import pack, unpack
from p4hfadapter.models.qwen3_5_0_8b.event_pipeline import (
    COMMAND,
    RESULT,
    Pipeline,
)


class LifecycleCodecTest(unittest.TestCase):
    def test_control_plane_import_needs_no_model_site_packages(self):
        root = Path(__file__).resolve().parents[3]
        code = (
            "import sys; "
            f"sys.path.insert(0, {str(root / 'python')!r}); "
            "import p4hfadapter.models.qwen3_5_0_8b.event_pipeline"
        )
        result = subprocess.run(
            [sys.executable, "-I", "-S", "-c", code],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_little_endian_metadata_and_opaque_bytes_match_contract(self):
        node = {"agent": "tcp://127.0.0.1:52001", "node": "n0", "generation": 7}
        opaque = b"\0opaque\xff"
        payload = encode_request(
            "load",
            node,
            COMMAND,
            opaque,
            {
                "queue_capacity": 1,
                "completion_capacity": 2,
                "retained_capacity": 3,
                "retained_bytes": 4096,
            },
        )
        size = int.from_bytes(payload[:4], "little")
        self.assertEqual(size, len(payload) - 4 - len(opaque))
        metadata, decoded = decode_request(payload, "load")
        self.assertEqual(metadata["adapter_kind"], ADAPTER_KIND)
        self.assertEqual(decoded, opaque)

    def test_schema_length_identity_and_false_success_are_rejected(self):
        node = {"agent": "tcp://127.0.0.1:52001", "node": "n0", "generation": 7}
        with self.assertRaises(ValueError):
            decode_request((MAX_METADATA_BYTES + 1).to_bytes(4, "little"), "load")
        result = {
            "schema": 1,
            "node_id": "n0",
            "node_generation": 7,
            "adapter_kind": ADAPTER_KIND,
            "adapter_content_type": RESULT,
            "operation": "load",
            "status": "succeeded",
            "resource_state": "present",
            "first_error": "hidden",
        }
        with self.assertRaises(ValueError):
            decode_result(encode_result(result), "load", node, RESULT)
        result["status"] = "failed"
        result.pop("first_error")
        with self.assertRaises(ValueError):
            decode_result(encode_result(result), "load", node, RESULT)

    def test_python_decoder_accepts_rust_field_order_fixture(self):
        header = (
            b'{"schema":1,"node_id":"n0","node_generation":7,'
            b'"adapter_kind":"hf-transformers",'
            b'"adapter_content_type":"application/vnd.p4.hf.command-v2",'
            b'"queue_capacity":1,"completion_capacity":2,'
            b'"retained_capacity":3,"retained_bytes":4096}'
        )
        metadata, opaque = decode_request(
            struct.pack("<I", len(header)) + header + b"rust-opaque", "load"
        )
        self.assertEqual(metadata["node_id"], "n0")
        self.assertEqual(opaque, b"rust-opaque")


class FakeClient:
    def __init__(self, fail_load=None, fail_cleanup=False):
        self.calls = []
        self.fail_load = fail_load
        self.fail_cleanup = fail_cleanup

    def exchange(self, target, content, payload, adapter=None):
        operation = "load" if content == NODE_LOAD else "unload"
        self.assert_lifecycle_call(target, content, adapter)
        request, opaque = decode_request(payload, operation)
        command, body = unpack(opaque)
        if body:
            raise AssertionError("fixture lifecycle command unexpectedly carried a body")
        self.calls.append(
            {
                "target": target,
                "content": content,
                "request": request,
                "command": command,
            }
        )
        index = next(
            index
            for index, node in enumerate(command.get("nodes", []))
            if node["node"] == request["node_id"]
        ) if operation == "load" else None
        status, resource, first_error, cleanup_error = "succeeded", "present", None, None
        reply = {"ok": True, "ready": {"report": {"node": request["node_id"]}}}
        if operation == "load" and index == self.fail_load:
            status, resource, first_error = "rejected", "absent", "fixture LOAD rejection"
            reply = {"ok": False, "error": first_error}
        elif operation == "unload":
            kind = command["job"]["kind"]
            reply = {"ok": True, "job": command["job"], "receipts": []}
            if kind == "abort" and self.fail_cleanup:
                status, resource = "failed", "unknown"
                first_error = "fixture abort failure"
                cleanup_error = "fixture child remains"
                reply = {"ok": False, "error": first_error}
            else:
                resource = "absent"
        result = {
            "schema": 1,
            "node_id": request["node_id"],
            "node_generation": request["node_generation"],
            "adapter_kind": ADAPTER_KIND,
            "adapter_content_type": RESULT,
            "operation": operation,
            "status": status,
            "resource_state": resource,
        }
        if first_error is not None:
            result["first_error"] = first_error
        if cleanup_error is not None:
            result["cleanup_error"] = cleanup_error
        envelope = {"source": target, "content": NODE_LIFECYCLE_RESULT}
        return envelope, encode_result(result, pack(reply))

    @staticmethod
    def assert_lifecycle_call(target, content, adapter):
        if target[0] != 0:
            raise AssertionError("lifecycle request bypassed the Agent")
        if content not in (NODE_LOAD, NODE_UNLOAD):
            raise AssertionError(f"legacy or direct lifecycle content type: {content}")
        if adapter != ADAPTER_KIND:
            raise AssertionError("lifecycle request omitted adapter identity")


class PipelineLifecycleTest(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        root = Path(self.scratch.name)
        self.bundle = root / "bundle.json"
        self.bundle.write_text("{}", encoding="utf-8")
        identity = root / "manifests/qwen3_5_0_8b/artifact/identity.json"
        identity.parent.mkdir(parents=True)
        identity.write_text(
            json.dumps(
                {
                    "revision": "fixture",
                    "files": {
                        "model.safetensors-00001-of-00001.safetensors": {
                            "sha256": "1" * 64
                        },
                        "tokenizer.json": {"sha256": "2" * 64},
                    },
                }
            ),
            encoding="utf-8",
        )
        self.nodes = [
            {"agent": "tcp://127.0.0.1:52001", "node": "head", "generation": 3},
            {"agent": "tcp://127.0.0.2:52001", "node": "tail", "generation": 4},
        ]
        self.plan = {
            "dtype": "float32",
            "nodes": [{"node_id": "head"}, {"node_id": "tail"}],
        }

    def tearDown(self):
        self.scratch.cleanup()

    def pipeline(self, client):
        return Pipeline(
            client,
            copy.deepcopy(self.plan),
            self.scratch.name,
            "python",
            self.bundle,
            copy.deepcopy(self.nodes),
            generation=9,
        )

    def test_load_and_shutdown_use_only_agent_lifecycle_and_empty_ownership(self):
        client = FakeClient()
        pipeline = self.pipeline(client)
        self.assertEqual(pipeline.ready, [0, 1])
        self.assertEqual(pipeline.owned, [0, 1])
        with self.assertRaisesRegex(ValueError, "Agent lifecycle"):
            pipeline.command(0, {"job": pipeline.job("unload"), "receipts": []})
        pipeline.shutdown()
        self.assertEqual(pipeline.ready, [])
        self.assertEqual(pipeline.owned, [])
        self.assertEqual(
            [call["content"] for call in client.calls],
            [NODE_LOAD, NODE_LOAD, NODE_UNLOAD, NODE_UNLOAD],
        )
        self.assertTrue(all(call["target"][0] == 0 for call in client.calls))

    def test_partial_load_rejection_aborts_prior_success_through_agent(self):
        client = FakeClient(fail_load=1)
        pipeline = Pipeline.__new__(Pipeline)
        with self.assertRaisesRegex(RuntimeError, "fixture LOAD rejection"):
            pipeline.__init__(
                client,
                copy.deepcopy(self.plan),
                self.scratch.name,
                "python",
                self.bundle,
                copy.deepcopy(self.nodes),
                generation=9,
            )
        self.assertEqual(pipeline.ready, [])
        self.assertEqual(pipeline.owned, [])
        self.assertEqual(
            [call["content"] for call in client.calls],
            [NODE_LOAD, NODE_LOAD, NODE_UNLOAD],
        )
        self.assertEqual(client.calls[-1]["command"]["job"]["kind"], "abort")

    def test_partial_load_preserves_first_and_cleanup_errors(self):
        client = FakeClient(fail_load=1, fail_cleanup=True)
        pipeline = Pipeline.__new__(Pipeline)
        with self.assertRaises(RuntimeError) as caught:
            pipeline.__init__(
                client,
                copy.deepcopy(self.plan),
                self.scratch.name,
                "python",
                self.bundle,
                copy.deepcopy(self.nodes),
                generation=9,
            )
        message = str(caught.exception)
        self.assertIn("fixture LOAD rejection", message)
        self.assertIn("fixture child remains", message)
        self.assertEqual(pipeline.ready, [0])
        self.assertEqual(pipeline.owned, [0])


if __name__ == "__main__":
    unittest.main()
