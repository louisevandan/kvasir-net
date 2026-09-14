import json
from pathlib import Path
import tempfile
import unittest

from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from p4hfadapter.models.qwen3_5_0_8b.scenarios import read_scenario
from tests.fixtures.qwen_plans import plan
from tests.fixtures.qwen_tokenizer import Tokenizer


class ScenarioTests(unittest.TestCase):
    def parse(self, request, schedule="round_robin"):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scenario.json"
            path.write_text(json.dumps({"name": "test", "schedule": schedule, "requests": [request]}), encoding="utf-8")
            return read_scenario(path, parse_plan(plan()), Tokenizer())

    def test_explicit_tokenizer_return_contract(self):
        value = {"id": "one", "prompt": "hello", "max_new_tokens": 8, "prefill_chunk": 2, "cancel_after": None}
        name, schedule, requests = self.parse(value)
        self.assertEqual(requests[0].tokens, [1, 2, 3])
        self.assertEqual(schedule, "round_robin")

    def test_bad_output_chunk_cancel_and_unknown_fields_are_rejected(self):
        for key, bad in (("max_new_tokens", 65), ("prefill_chunk", 0), ("cancel_after", True), ("cancel_after", 9)):
            value = {"id": "one", "prompt": "hello", "max_new_tokens": 8, "prefill_chunk": 2, "cancel_after": None}
            value[key] = bad
            with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                self.parse(value)

    def test_unsupported_schedule_is_rejected(self):
        with self.assertRaises(ValueError):
            self.parse({}, schedule="automatic-batching")
