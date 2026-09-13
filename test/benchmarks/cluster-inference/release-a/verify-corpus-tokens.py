"""Retokenize frozen prompt bytes and require exact token-ID parity."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main(args):
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    corpus = json.loads(args.corpus.read_text(encoding="utf-8"))
    lines = []
    for request in corpus["requests"]:
        prompt = args.prompts / (request["id"] + ".prompt.txt")
        if sha(prompt) != request["prompt_sha256"]:
            raise ValueError("frozen prompt changed: " + request["id"])
        lines.append(str(prompt.resolve()) + "\t" + str(out / (request["id"] + ".tokens.bin")))
    with (out / "stderr.log").open("wb") as stderr:
        result = subprocess.run([str(args.tokenizer.resolve()), str(args.model)],
                                input=("\n".join(lines) + "\n").encode(), stdout=subprocess.PIPE,
                                stderr=stderr, timeout=300)
    (out / "stdout.log").write_bytes(result.stdout)
    counts = [int(x) for x in result.stdout.splitlines()] if result.returncode == 0 else []
    rows = []
    for i, request in enumerate(corpus["requests"]):
        tokens = out / (request["id"] + ".tokens.bin")
        count = counts[i] if i < len(counts) else None
        digest = sha(tokens) if tokens.exists() else None
        rows.append({"id": request["id"], "count": count, "sha256": digest,
                     "ok": count == request["input_tokens"] and digest == request["token_ids_sha256"]})
    record = {"ok": result.returncode == 0 and len(counts) == len(rows) and all(r["ok"] for r in rows),
              "exit": result.returncode, "requests": rows, "corpus_sha256": sha(args.corpus),
              "tokenizer_sha256": sha(args.tokenizer), "model": str(args.model),
              "dlls": {p.name: sha(p) for p in args.tokenizer.parent.glob("*.dll")}}
    (out / "equivalence.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    print(json.dumps({"ok": record["ok"], "requests": len(rows), "exit": result.returncode}))
    return 0 if record["ok"] else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    for name in ["tokenizer", "corpus", "prompts", "model", "output"]:
        p.add_argument("--" + name, type=Path, required=True)
    raise SystemExit(main(p.parse_args()))
