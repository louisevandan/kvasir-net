#!/usr/bin/env python3
"""Build a self-verifying ring runtime pack from one CMake artifact set."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import tarfile
import tempfile
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def runtime_info(binary: Path, library_dir: Path) -> dict:
    env = dict(os.environ)
    paths = [str(library_dir)]
    cuda_stubs = Path("/usr/local/cuda/lib64/stubs")
    temporary_stubs = None
    if cuda_stubs.is_dir():
        stub = cuda_stubs / "libcuda.so"
        if stub.is_file() and not (cuda_stubs / "libcuda.so.1").exists():
            temporary_stubs = tempfile.TemporaryDirectory(prefix="cuda-stubs-")
            os.symlink(stub, Path(temporary_stubs.name) / "libcuda.so.1")
            paths.append(temporary_stubs.name)
        paths.append(str(cuda_stubs))
    if env.get("LD_LIBRARY_PATH"):
        paths.append(env["LD_LIBRARY_PATH"])
    env["LD_LIBRARY_PATH"] = os.pathsep.join(paths)
    try:
        output = subprocess.check_output(
            [str(binary), "--runtime-info"], text=True, env=env, timeout=30,
        )
    finally:
        if temporary_stubs is not None:
            temporary_stubs.cleanup()
    value = json.loads(output)
    if not isinstance(value, dict):
        raise RuntimeError(f"invalid runtime info from {binary}")
    return value


def copy_bytes(source: Path, target: Path, executable: bool = False) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(source.read_bytes())
    target.chmod(0o755 if executable else 0o644)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--node", required=True, type=Path)
    parser.add_argument("--server", required=True, type=Path)
    parser.add_argument("--library-dir", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--system", default=platform.system().lower())
    parser.add_argument("--machine", default=platform.machine().lower())
    args = parser.parse_args()

    node_info = runtime_info(args.node, args.library_dir)
    server_info = runtime_info(args.server, args.library_dir)
    required = ("protocol", "adapter_abi", "build_id", "state_snapshot", "chunked_state")
    if any(node_info.get(key) != server_info.get(key) for key in required):
        raise RuntimeError("node and server runtime identities differ")
    if not node_info.get("build_id") or node_info["build_id"] == "unknown":
        raise RuntimeError("ring binaries do not expose an immutable build_id")
    pack_id = "ring-{}-{}-{}-{}".format(
        args.system.lower(), args.machine.lower(), args.backend.lower(),
        hashlib.sha256(node_info["build_id"].encode()).hexdigest()[:16],
    )
    args.output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ring-pack-") as temporary:
        root = Path(temporary)
        copy_bytes(args.node, root / "bin" / "linkcpp-node", executable=True)
        copy_bytes(args.server, root / "bin" / "linkcpp-server", executable=True)
        libraries: dict[str, Path] = {}
        for pattern in ("*.so", "*.so.*", "*.dylib"):
            for source in args.library_dir.glob(pattern):
                libraries[source.name] = source
        if not libraries:
            raise RuntimeError("no ring runtime shared libraries were found")
        for name, source in sorted(libraries.items()):
            copy_bytes(source, root / "lib" / name)
        files = []
        for path in sorted(item for item in root.rglob("*") if item.is_file()):
            relative = path.relative_to(root).as_posix()
            files.append({"path": relative, "size": path.stat().st_size, "sha256": sha256(path)})
        manifest = {
            "schema": 1,
            "pack_id": pack_id,
            "protocol": node_info["protocol"],
            "adapter_abi": node_info["adapter_abi"],
            "build_id": node_info["build_id"],
            "state_snapshot": bool(node_info["state_snapshot"]),
            "chunked_state": bool(node_info["chunked_state"]),
            "target": {
                "system": args.system.lower(),
                "machine": args.machine.lower(),
                "backend": args.backend.lower(),
            },
            "files": files,
        }
        (root / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8",
        )
        archive = args.output_dir / f"{pack_id}.tar.gz"
        with tarfile.open(archive, "w:gz", format=tarfile.PAX_FORMAT) as bundle:
            for path in sorted(root.rglob("*")):
                arcname = path.relative_to(root).as_posix()
                info = bundle.gettarinfo(str(path), arcname=arcname)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mtime = 0
                if path.is_file():
                    with path.open("rb") as stream:
                        bundle.addfile(info, stream)
                else:
                    bundle.addfile(info)
        print(json.dumps({
            "archive": str(archive), "sha256": sha256(archive),
            "size": archive.stat().st_size, "manifest": manifest,
        }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
