import os
import tempfile
import unittest
from unittest import mock

from controller import llama_updater


class LlamaUpdaterTests(unittest.TestCase):
    def test_status_keeps_official_and_compatible_adapter_heads_separate(self):
        calls = []

        def runner(args, cwd=None, timeout=30):
            calls.append((args, cwd))
            if args[1:3] == ["rev-parse", "HEAD"]:
                return "a" * 40
            if args[1:3] == ["status", "--porcelain"]:
                return ""
            if llama_updater.OFFICIAL_URL in args:
                return "b" * 40 + "\tHEAD"
            if llama_updater.ADAPTER_URL in args:
                return "c" * 40 + "\tHEAD"
            raise AssertionError(args)

        with tempfile.TemporaryDirectory() as source, mock.patch.dict(
            os.environ, {"LINKCPP_LLAMA_SOURCE_DIR": source}
        ):
            status = llama_updater.update_status(runner)

        self.assertTrue(status["supported"])
        self.assertEqual(status["official_head"], "b" * 40)
        self.assertEqual(status["adapter_head"], "c" * 40)
        self.assertTrue(status["official_differs_from_adapter"])
        self.assertEqual(status["apply_track"], "adapter")

    def test_apply_rejects_a_target_other_than_compatible_adapter_head(self):
        status = {
            "supported": True,
            "dirty": False,
            "current": "a" * 40,
            "adapter_head": "c" * 40,
            "source_dir": "source",
        }
        with mock.patch.object(llama_updater, "update_status", return_value=status):
            with self.assertRaisesRegex(RuntimeError, "compatible adapter"):
                llama_updater.apply_adapter_update("a" * 40, "b" * 40)


if __name__ == "__main__":
    unittest.main()
