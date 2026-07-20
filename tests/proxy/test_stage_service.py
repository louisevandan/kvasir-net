import os
import tempfile
import unittest
from unittest import mock

from controller.proxy import stage_service
from controller.proxy.protocol import StageStartRequest


class ProxyStageServiceTests(unittest.TestCase):
    def test_start_runs_rank_local_binary_after_proxy_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            model = "stage-model.gguf"
            with open(os.path.join(temporary, model), "wb") as stream:
                stream.write(b"gguf")
            stage_bin = os.path.join(temporary, "linkcpp-node")
            with open(stage_bin, "wb") as stream:
                stream.write(b"binary")
            process = mock.Mock(pid=42)
            process.poll.return_value = None
            owner = {}
            request = StageStartRequest(
                model=model, layers=[4, 8], role="middle", listen_port=51053,
                next_endpoint="10.0.0.3:51054", gpu_layers=0, kv_offload=False,
            )
            with mock.patch.object(
                stage_service.packs, "resolve_runtime",
                return_value=(stage_bin, dict(os.environ), "test-pack"),
            ), mock.patch.object(stage_service.subprocess, "Popen", return_value=process) as popen:
                result = stage_service.start(
                    owner, request, model_dir=temporary,
                    log_path=os.path.join(temporary, "stage.log"),
                    baked_stage=stage_bin, baked_server=stage_bin,
                )

        self.assertTrue(result["accepted"])
        command = popen.call_args.args[0]
        self.assertEqual(command[:2], [stage_bin, "--model"])
        self.assertIn("4:8", command)
        self.assertEqual(command[command.index("--gpu-layers") + 1], "0")
        self.assertIn("middle", command)
        self.assertIn("--no-kv-offload", command)
        self.assertFalse(owner["desired_load"]["kv_offload"])
        self.assertEqual(owner["desired_load"]["runtime_pack_id"], "test-pack")
