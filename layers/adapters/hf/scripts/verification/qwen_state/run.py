"""Required model-runtime admission checks, separate from dependency-free tests."""

from pathlib import Path
import sys
import unittest

root = Path(__file__).resolve().parents[3]
sys.path[:0] = [str(root / "python"), str(root)]
from tests.models.qwen3_5_0_8b.state.checks import StateRefusalTests

suite = unittest.defaultTestLoader.loadTestsFromTestCase(StateRefusalTests)
result = unittest.TextTestRunner(verbosity=2).run(suite)
raise SystemExit(0 if result.wasSuccessful() else 1)
