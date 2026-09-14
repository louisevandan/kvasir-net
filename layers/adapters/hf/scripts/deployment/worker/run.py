"""Create a fresh worker bundle; existing bundles are never overwritten."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[3]


def build(output, label):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    for source in sorted((ROOT / "python").rglob("*.py")):
        target = output / source.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    for name in ("manifests/qwen3_5_0_8b/artifact/identity.json", "environments/qwen3_5_0_8b/requirements.lock"):
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, target)
    entry=Path(__file__).with_name("entry.py").read_text(encoding="utf-8")
    (output / "entry.py").write_text(entry.replace("serve(root)",f"serve(root, build_label={label!r})"),encoding="utf-8")
    (output / "build-label.txt").write_text(label + "\n", encoding="utf-8")
    files = {p.relative_to(output).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()
             for p in sorted(output.rglob("*")) if p.is_file()}
    (output / "bundle.json").write_text(json.dumps({"protocol":2,"entry":"entry.py","files":files}, indent=2)+"\n",encoding="utf-8")
    return output / "bundle.json"


if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--label", default="A")
    args=parser.parse_args()
    print(build(args.output,args.label))
