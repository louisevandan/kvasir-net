"""Download the selected checkpoint into this repository's ignored model cache."""

import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(root / "python"))
from p4hfadapter.models.qwen3_5_0_8b.preparation import prepare
from p4hfadapter.models.qwen3_5_0_8b.evidence import checkpoint_identity

path = prepare(root.parents[2] / ".cache/hf/models/hub")
identity = checkpoint_identity(path)
(root.parents[2] / ".cache/hf/models/checkpoint-identity.json").write_text(json.dumps(identity, indent=2) + "\n", encoding="utf-8")
(root.parents[2] / ".cache/hf/models/checkpoint-path.txt").write_text(str(path), encoding="utf-8")
print(path)
