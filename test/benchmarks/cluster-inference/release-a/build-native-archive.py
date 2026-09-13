"""Build a sealed source archive in a fresh POSIX directory; never alter a deployment."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile


def main(args):
    archive = args.archive.resolve(); out = args.output.resolve()
    if hashlib.sha256(archive.read_bytes()).hexdigest() != args.sha256:
        raise ValueError("source archive hash mismatch")
    out.mkdir(parents=True, exist_ok=False)
    with tarfile.open(archive) as tar:
        for member in tar.getmembers():
            target = (out / member.name).resolve()
            if not target.is_relative_to(out) or member.issym() or member.islnk():
                raise ValueError("archive contains an unsafe path or link")
        tar.extractall(out)
    env = os.environ.copy()
    env["PATH"] = "/opt/homebrew/bin:/usr/local/cuda/bin:" + env["PATH"]
    git = shutil.which("git", path=env["PATH"])
    if not git:
        raise RuntimeError("git is required to verify source identity before building")
    seal = json.loads((out / "seal.json").read_text())
    diff = subprocess.check_output([git, "diff", "--binary", "--full-index"], cwd=out / "upstream", env=env)
    if hashlib.sha256(diff).hexdigest() != seal["patch_set"]:
        raise ValueError("unpacked native source does not match the patch seal")
    cmake = shutil.which("cmake", path=env["PATH"])
    command = [cmake, "-S", str(out / "server"), "-B", str(out / "build"),
               "-DCMAKE_BUILD_TYPE=Release", "-DP4_STAGED_LLAMA_SOURCE_DIR=" + str(out / "upstream"),
               "-DGGML_NATIVE=OFF", "-DLLAMA_BUILD_TESTS=OFF", "-DLLAMA_BUILD_EXAMPLES=OFF",
               "-DP4_STAGED_CUDA=" + ("ON" if args.cuda_arch else "OFF")]
    if args.cuda_arch:
        command.append("-DCMAKE_CUDA_ARCHITECTURES=" + args.cuda_arch)
    record = {"source": seal, "archive_sha256": args.sha256, "commands": [], "ok": False}
    for name, cmd in [("configure", command),
                      ("build", [cmake, "--build", str(out / "build"), "--parallel", str(args.jobs)]),
                      ("ctest", [str(Path(cmake).with_name("ctest")), "--test-dir", str(out / "build"), "--output-on-failure"])]:
        with (out / (name + ".log")).open("wb") as log:
            result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, env=env)
        record["commands"].append({"name": name, "argv": cmd, "exit": result.returncode})
        (out / "result.json").write_text(json.dumps(record, indent=2))
        if result.returncode:
            return result.returncode
    record["ok"] = True
    record["binaries"] = {str(p.relative_to(out)): hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in (out / "build").rglob("*") if p.is_file() and
                          (p.name == "p4_staged_server" or ".so" in p.name or p.suffix == ".dylib")}
    (out / "result.json").write_text(json.dumps(record, indent=2))
    return 0


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--archive", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--sha256", required=True)
    p.add_argument("--cuda-arch")
    p.add_argument("--jobs", type=int, default=6)
    raise SystemExit(main(p.parse_args()))
