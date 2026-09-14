"""Private fresh-process stage probe used by the Qwen profile command."""
import argparse
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(root / "python"))
from p4hfadapter.models.qwen3_5_0_8b.profiling import measure_stage

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("request", "model-dir", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--device-id", required=True)
    parser.add_argument("--start", type=int, required=True)
    parser.add_argument("--end", type=int, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("sample output already exists")
    sample = measure_stage(root, args.model_dir, json.loads(args.request.read_text(encoding="utf-8")), args.device_id, args.start, args.end)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(sample, stream, indent=2)
