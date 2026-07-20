import importlib.util
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "check_ring_architecture_coverage.py"
SPEC = importlib.util.spec_from_file_location("ring_architecture_coverage", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class RingArchitectureCoverageTests(unittest.TestCase):
    def test_pinned_registry_uses_one_architecture_neutral_adapter(self):
        result = MODULE.audit(ROOT)
        self.assertGreater(result["pinned_architecture_count"], 100)
        self.assertEqual(result["architecture_branches_in_ring_cutter"], [])
        self.assertEqual(result["missing_descriptor_fields"], [])
        self.assertEqual(result["missing_terminal_api"], [])
        self.assertEqual(result["gaps"], [])
        self.assertTrue(result["ok"])


if __name__ == "__main__":
    unittest.main()
