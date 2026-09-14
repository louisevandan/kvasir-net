"""Repository entrypoint for the Qwen-specific plan runner."""

from pathlib import Path
import sys

root = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(root / "python"))
from p4hfadapter.models.qwen3_5_0_8b.cli import main

if __name__ == "__main__":
    raise SystemExit(main(root))
