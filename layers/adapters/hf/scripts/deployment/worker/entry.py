"""Copied as the root of a sealed, independently replaceable worker bundle."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parent
sys.path.insert(0, str(root / "python"))
from p4hfadapter.models.qwen3_5_0_8b.event_worker import serve

if __name__ == "__main__":
    serve(root)

