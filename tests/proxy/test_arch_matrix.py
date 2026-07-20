import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "run_ring_arch_matrix", ROOT / "scripts" / "run_ring_arch_matrix.py",
)
matrix = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(matrix)


class RingArchitectureMatrixTests(unittest.TestCase):
    def test_matrix_targets_every_pinned_llama_model_factory(self):
        targets = matrix.architectures(
            ROOT / "external" / "llama.cpp" / "src" / "llama-arch.cpp",
            ROOT / "external" / "llama.cpp" / "src" / "llama-model.cpp",
            "",
        )
        self.assertEqual(len(targets), 132)
        self.assertEqual(len(targets), len(set(targets)))
        self.assertIn("qwen35moe", targets)
        self.assertIn("gemma4", targets)
        self.assertIn("llama-embed", targets)

    def test_completion_and_embedding_evidence_are_compared_by_output_kind(self):
        same, completion = matrix.compare_output(
            {"kind": "completion", "tokens": [7]},
            {"kind": "completion", "tokens": [7]},
        )
        self.assertTrue(same)
        self.assertEqual(completion["ring_tokens"], [7])
        same, embedding = matrix.compare_output(
            {"kind": "embedding", "values": [1.0, 2.0]},
            {"kind": "embedding", "values": [1.0, 2.0000001]},
        )
        self.assertTrue(same)
        self.assertLess(embedding["nmse"], 1e-6)

    def test_unknown_requested_architecture_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unknown architectures"):
            matrix.architectures(
                ROOT / "external" / "llama.cpp" / "src" / "llama-arch.cpp",
                ROOT / "external" / "llama.cpp" / "src" / "llama-model.cpp",
                "not-a-real-architecture",
            )


if __name__ == "__main__":
    unittest.main()
