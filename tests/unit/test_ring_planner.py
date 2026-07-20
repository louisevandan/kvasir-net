import sys
import types
import unittest

if "gguf" not in sys.modules:
    gguf = types.ModuleType("gguf")
    gguf.GGUFReader = object
    gguf.GGUFValueType = types.SimpleNamespace(STRING="string")
    sys.modules["gguf"] = gguf

from controller import planner
from controller.ring_placement import boundary_candidates


def dense_large_kv_model():
    layers = 48
    layer_weight = int(0.24 * planner.GiB)
    return {
        "arch": "gemma4-test",
        "n_layer": layers,
        "n_head_kv": 8,
        "n_head_kv_by_layer": [8] * layers,
        "head_dim_k": 437,
        "head_dim_v": 437,
        "n_embd": 3840,
        "n_expert": 0,
        "weight_layer": [layer_weight] * layers,
        "expert_layer": [0] * layers,
        "body_tensor_names": [[f"blk.{index}.weight"] for index in range(layers)],
        "expert_tensor_names": [[] for _ in range(layers)],
        "boundary_bytes": 1 * planner.GiB,
        "total_weight": layers * layer_weight + planner.GiB,
    }


class RingBoundaryCandidateTests(unittest.TestCase):
    def test_two_rank_search_enumerates_every_contiguous_boundary(self):
        candidates, exhaustive = boundary_candidates(48, 2, [31, 17])

        self.assertTrue(exhaustive)
        self.assertEqual(len(candidates), 47)
        self.assertEqual(candidates[0], (31, 17))
        self.assertIn((37, 11), candidates)


class RingPlannerRegressionTests(unittest.TestCase):
    def setUp(self):
        self.model = dense_large_kv_model()
        self.nodes = [
            {"vram": 10.0, "ram": 50.0, "cores": 8,
             "backend": {"backend_kind": "cuda"}},
            {"vram": 6.0, "ram": 15.0, "cores": 8,
             "backend": {"backend_kind": "cuda"}},
        ]

    def test_ring_recovers_when_vram_weighted_boundary_overloads_rank_ram(self):
        weighted = planner._plan_with_targets(
            self.model, self.nodes, 100_000, 1, 16, 1024,
            "f16", "f16", False, [31, 17],
        )
        self.assertFalse(weighted["feasible"])

        result = planner.plan(
            self.model, self.nodes, 100_000, 1,
            reserve_mib=1024,
            cache_type_k="f16",
            cache_type_v="f16",
            placement_strategy="ring-stage-vram-weighted",
        )

        self.assertTrue(result["feasible"])
        self.assertTrue(result["ring_search_exhaustive"])
        self.assertEqual(sum(result["tensor_split"]), 48)
        self.assertLessEqual(min(result["tensor_split"]), 11)
        self.assertEqual(result["kv_cache_location"], "ram")
        self.assertGreater(result["kv_total_gib"], 60)
        self.assertTrue(all(item["kv_ram_gib"] > 0 for item in result["placement"]))

    def test_ring_failure_preserves_real_kv_and_search_diagnostics(self):
        nodes = [
            dict(self.nodes[0], ram=10.0),
            dict(self.nodes[1], ram=10.0),
        ]
        result = planner.plan(
            self.model, nodes, 100_000, 1,
            reserve_mib=1024,
            cache_type_k="f16",
            cache_type_v="f16",
            placement_strategy="ring-stage-vram-weighted",
        )

        self.assertFalse(result["feasible"])
        self.assertTrue(result["ring_search_exhaustive"])
        self.assertEqual(result["ring_search_attempts"], 94)
        self.assertGreater(result["kv_total_gib"], 60)
        self.assertGreater(result["stuck_layer"], 0)
        self.assertIn("VRAM/RAM", result["reason"] + " " + " ".join(result["suggestions"]))


if __name__ == "__main__":
    unittest.main()
