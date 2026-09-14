"""Run source-tree tests without installing model dependencies."""

import importlib.util
from pathlib import Path
import sys
import unittest


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    sys.path.insert(0, str(root))
    sys.path.insert(0, str(root / "python"))
    suite = unittest.TestSuite()
    for path in sorted((root / "tests").rglob("test_*.py")):
        name = "_".join(path.relative_to(root).with_suffix("").parts)
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
        suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
    if not suite.countTestCases():
        raise RuntimeError("no tests discovered")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
