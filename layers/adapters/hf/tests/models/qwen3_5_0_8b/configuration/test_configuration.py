import unittest

from p4hfadapter.models.qwen3_5_0_8b.configuration import inspect_plan, parse_plan
from tests.fixtures.qwen_plans import plan


class QwenPlanTests(unittest.TestCase):
    def test_linear_only_and_attention_only_nodes_keep_global_ownership(self):
        actual = inspect_plan(parse_plan(plan()))
        self.assertEqual(actual["nodes"][0]["linear_attention_layers"], [0, 1, 2])
        self.assertEqual(actual["nodes"][0]["full_attention_layers"], [])
        self.assertEqual(actual["nodes"][1]["full_attention_layers"], [3])
        self.assertTrue(actual["nodes"][-1]["output_head"])
        self.assertFalse(actual["physical_hosts_verified"])

    def test_gap_overlap_reorder_empty_and_missing_tail_are_rejected(self):
        for cut in ([1, 3], [0, 0], [0, 25], [0, True], [0, 2.0]):
            value = plan()
            value["nodes"][0]["layers"] = cut
            with self.subTest(cut=cut), self.assertRaises(ValueError):
                parse_plan(value)
        for cut in ([2, 4], [4, 5]):
            value = plan()
            value["nodes"][1]["layers"] = cut
            with self.subTest(cut=cut), self.assertRaises(ValueError):
                parse_plan(value)
        value = plan()
        value["nodes"][-1]["layers"][1] = 23
        with self.assertRaises(ValueError):
            parse_plan(value)

    def test_duplicate_node_unknown_field_and_noninteger_limits_are_rejected(self):
        value = plan()
        value["nodes"][1]["node_id"] = "first"
        with self.assertRaises(ValueError):
            parse_plan(value)
        value = plan()
        value["device_map"] = "auto"
        with self.assertRaises(ValueError):
            parse_plan(value)
        value = plan()
        value["limits"]["context"] = True
        with self.assertRaises(ValueError):
            parse_plan(value)

    def test_wrong_model_revision_and_quantized_recipe_are_rejected(self):
        for key, wrong in (("model_id", "Qwen/another-model"), ("revision", "main"), ("quantization", "4bit"), ("dtype", "int4")):
            value = plan()
            value[key] = wrong
            with self.subTest(key=key), self.assertRaises(ValueError):
                parse_plan(value)

    def test_remote_placement_can_be_inspected_without_launching_it(self):
        value = plan()
        value["nodes"][0]["host"] = "machine-a"
        self.assertEqual(inspect_plan(parse_plan(value))["nodes"][0]["host"], "machine-a")
