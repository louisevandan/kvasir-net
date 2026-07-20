"""Content-addressed installation and selection of ring adapter runtime packs."""

from __future__ import annotations

import hashlib
import asyncio
import json
import os
import platform
import re
import shutil
import tarfile
import tempfile
import threading
import urllib.request
from functools import lru_cache
from pathlib import Path, PurePosixPath
from typing import Any, AsyncIterable, Mapping

from controller.proxy.capability import RING_ADAPTER_ABI, RING_PROTOCOL, runtime_pair_info
from controller import host_resources


PACK_SCHEMA = 1
PACK_ID_RE = re.compile(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
MAX_ARCHIVE_BYTES = int(os.environ.get("LINKCPP_RUNTIME_PACK_MAX_ARCHIVE", str(2 << 30)))
MAX_EXTRACTED_BYTES = int(os.environ.get("LINKCPP_RUNTIME_PACK_MAX_EXTRACTED", str(8 << 30)))
MAX_FILES = int(os.environ.get("LINKCPP_RUNTIME_PACK_MAX_FILES", "4096"))
_INSTALL_LOCK = threading.Lock()


def pack_root() -> Path:
    model_dir = os.environ.get("LINKCPP_MODEL_DIR", "/models")
    data_dir = os.environ.get("LINKCPP_DATA_DIR", os.path.join(model_dir, "linkcpp"))
    return Path(os.environ.get(
        "LINKCPP_RUNTIME_PACK_DIR", os.path.join(data_dir, "runtime-packs", "ring"),
    ))


def install_enabled() -> bool:
    return os.environ.get("LINKCPP_ALLOW_RUNTIME_PACK_INSTALL", "").strip().lower() in {
        "1", "true", "yes",
    }


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _safe_relative(value: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if (not value or path.is_absolute() or "\\" in value or "\x00" in value
            or any(part in ("", ".", "..") for part in path.parts)):
        raise ValueError(f"unsafe runtime-pack path: {value!r}")
    return path


def _load_manifest_bytes(raw: bytes) -> dict[str, Any]:
    try:
        manifest = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"invalid runtime-pack manifest: {exc}") from exc
    if not isinstance(manifest, dict) or manifest.get("schema") != PACK_SCHEMA:
        raise ValueError("unsupported runtime-pack manifest schema")
    pack_id = str(manifest.get("pack_id") or "")
    build_id = str(manifest.get("build_id") or "")
    if not PACK_ID_RE.fullmatch(pack_id):
        raise ValueError("invalid runtime-pack id")
    if not build_id or build_id == "unknown" or len(build_id) > 256:
        raise ValueError("runtime-pack build_id must be immutable and non-empty")
    if manifest.get("protocol") != RING_PROTOCOL:
        raise ValueError("runtime-pack protocol mismatch")
    if int(manifest.get("adapter_abi") or 0) != RING_ADAPTER_ABI:
        raise ValueError("runtime-pack adapter ABI mismatch")
    target = manifest.get("target")
    if not isinstance(target, dict) or not all(
        str(target.get(field) or "") for field in ("system", "machine", "backend")
    ):
        raise ValueError("runtime-pack target system, machine, and backend are required")
    files = manifest.get("files")
    if not isinstance(files, list) or not files or len(files) > MAX_FILES:
        raise ValueError("runtime-pack file list is invalid")
    seen: set[str] = set()
    total = 0
    for entry in files:
        if not isinstance(entry, dict):
            raise ValueError("runtime-pack file entry is invalid")
        path = str(_safe_relative(str(entry.get("path") or "")))
        sha256 = str(entry.get("sha256") or "").lower()
        size = int(entry.get("size") if entry.get("size") is not None else -1)
        if path in seen or not SHA256_RE.fullmatch(sha256) or size < 0:
            raise ValueError(f"invalid runtime-pack file metadata: {path}")
        seen.add(path)
        total += size
    if "bin/linkcpp-node" not in seen or "bin/linkcpp-server" not in seen:
        raise ValueError("runtime-pack must contain both ring executables")
    if total > MAX_EXTRACTED_BYTES:
        raise ValueError("runtime-pack extracted size exceeds the configured limit")
    return manifest


def _extract_verified(archive: Path, destination: Path) -> dict[str, Any]:
    with tarfile.open(archive, mode="r:*") as bundle:
        members = bundle.getmembers()
        if len(members) > MAX_FILES + 1:
            raise ValueError("runtime-pack contains too many archive entries")
        by_name: dict[str, tarfile.TarInfo] = {}
        for member in members:
            name = str(_safe_relative(member.name))
            if name in by_name:
                raise ValueError(f"duplicate runtime-pack archive path: {name}")
            if not (member.isfile() or member.isdir()):
                raise ValueError(f"runtime-pack links and device entries are forbidden: {name}")
            by_name[name] = member
        manifest_member = by_name.get("manifest.json")
        if manifest_member is None or not manifest_member.isfile():
            raise ValueError("runtime-pack manifest.json is missing")
        manifest_stream = bundle.extractfile(manifest_member)
        if manifest_stream is None:
            raise ValueError("runtime-pack manifest cannot be read")
        manifest = _load_manifest_bytes(manifest_stream.read(4 << 20))
        expected = {str(entry["path"]): entry for entry in manifest["files"]}
        actual_files = {name for name, member in by_name.items() if member.isfile()}
        if actual_files != set(expected) | {"manifest.json"}:
            raise ValueError("runtime-pack archive and manifest file sets differ")
        for name, entry in expected.items():
            member = by_name[name]
            if member.size != int(entry["size"]):
                raise ValueError(f"runtime-pack size mismatch: {name}")
            source = bundle.extractfile(member)
            if source is None:
                raise ValueError(f"runtime-pack file cannot be read: {name}")
            target = destination.joinpath(*PurePosixPath(name).parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            digest = hashlib.sha256()
            written = 0
            with target.open("xb") as output:
                while True:
                    chunk = source.read(4 << 20)
                    if not chunk:
                        break
                    written += len(chunk)
                    if written > int(entry["size"]):
                        raise ValueError(f"runtime-pack expanded size mismatch: {name}")
                    digest.update(chunk)
                    output.write(chunk)
            if written != int(entry["size"]) or digest.hexdigest() != entry["sha256"]:
                raise ValueError(f"runtime-pack SHA-256 mismatch: {name}")
        for binary in ("bin/linkcpp-node", "bin/linkcpp-server"):
            path = destination.joinpath(*PurePosixPath(binary).parts)
            path.chmod(path.stat().st_mode | 0o500)
        (destination / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8",
        )
        return manifest


def _runtime_environment(directory: Path) -> dict[str, str]:
    env = dict(os.environ)
    lib = str(directory / "lib")
    previous = env.get("LD_LIBRARY_PATH", "")
    env["LD_LIBRARY_PATH"] = lib + (os.pathsep + previous if previous else "")
    return env


def _verify_executables(directory: Path, manifest: Mapping[str, Any]) -> dict[str, Any]:
    info = runtime_pair_info(
        str(directory / "bin" / "linkcpp-node"),
        str(directory / "bin" / "linkcpp-server"),
        env=_runtime_environment(directory),
    )
    if not info.get("available"):
        raise ValueError(f"runtime-pack executable verification failed: {info.get('blocker')}")
    for field in ("protocol", "adapter_abi", "build_id"):
        if info.get(field) != manifest.get(field):
            raise ValueError(f"runtime-pack executable {field} does not match manifest")
    return info


def install_archive(
    archive: str | os.PathLike[str], archive_sha256: str, *,
    expected_build_id: str | None = None,
) -> dict[str, Any]:
    archive_path = Path(archive)
    requested_sha = archive_sha256.strip().lower()
    if not SHA256_RE.fullmatch(requested_sha):
        raise ValueError("a lowercase 64-character archive SHA-256 is required")
    if archive_path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError("runtime-pack archive exceeds the configured limit")
    if _sha256_file(archive_path) != requested_sha:
        raise ValueError("runtime-pack archive SHA-256 mismatch")
    root = pack_root()
    root.mkdir(parents=True, exist_ok=True)
    with _INSTALL_LOCK:
        temp: Path | None = Path(tempfile.mkdtemp(prefix=".install-", dir=root))
        try:
            manifest = _extract_verified(archive_path, temp)
            if expected_build_id and manifest["build_id"] != expected_build_id:
                raise ValueError("runtime-pack build_id differs from the requested build")
            info = _verify_executables(temp, manifest)
            final = root / manifest["pack_id"]
            complete = {
                "archive_sha256": requested_sha,
                "pack_id": manifest["pack_id"],
                "build_id": manifest["build_id"],
            }
            (temp / ".complete.json").write_text(
                json.dumps(complete, sort_keys=True) + "\n", encoding="utf-8",
            )
            if final.exists():
                current = _read_pack(final)
                if current and current.get("archive_sha256") == requested_sha:
                    return current
                raise ValueError(f"runtime-pack id already exists with different content: {final.name}")
            os.replace(temp, final)
            temp = None
            _verified_installed_pack.cache_clear()
            return {**complete, "manifest": manifest, "runtime": info, "path": str(final)}
        finally:
            if temp is not None and temp.exists():
                shutil.rmtree(temp, ignore_errors=True)


def download_and_install(
    url: str, archive_sha256: str, *, expected_build_id: str | None = None,
) -> dict[str, Any]:
    if not (url.startswith("https://") or url.startswith("http://")):
        raise ValueError("runtime-pack URL must use http or https")
    root = pack_root()
    root.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".download-", suffix=".tar", dir=root)
    os.close(fd)
    path = Path(temporary)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "linkcpp-runtime-pack/1"})
        with urllib.request.urlopen(request, timeout=120) as source, path.open("wb") as output:
            total = 0
            while True:
                chunk = source.read(4 << 20)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_ARCHIVE_BYTES:
                    raise ValueError("runtime-pack download exceeds the configured limit")
                output.write(chunk)
        return install_archive(path, archive_sha256, expected_build_id=expected_build_id)
    finally:
        path.unlink(missing_ok=True)


async def stream_and_install(
    chunks: AsyncIterable[bytes], archive_sha256: str, *,
    expected_build_id: str | None = None,
) -> dict[str, Any]:
    """Receive a controller-pushed pack without buffering it in process memory."""
    root = pack_root()
    root.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".upload-", suffix=".tar", dir=root)
    os.close(fd)
    path = Path(temporary)
    try:
        total = 0
        with path.open("wb") as output:
            async for chunk in chunks:
                total += len(chunk)
                if total > MAX_ARCHIVE_BYTES:
                    raise ValueError("runtime-pack upload exceeds the configured limit")
                output.write(chunk)
        return await asyncio.to_thread(
            install_archive, path, archive_sha256, expected_build_id=expected_build_id,
        )
    finally:
        path.unlink(missing_ok=True)


def _read_pack(directory: Path) -> dict[str, Any] | None:
    try:
        manifest = _load_manifest_bytes((directory / "manifest.json").read_bytes())
        complete = json.loads((directory / ".complete.json").read_text(encoding="utf-8"))
        if (manifest["pack_id"] != directory.name
                or complete.get("build_id") != manifest["build_id"]
                or not SHA256_RE.fullmatch(str(complete.get("archive_sha256") or ""))):
            return None
        return {**complete, "manifest": manifest, "path": str(directory)}
    except (OSError, ValueError, json.JSONDecodeError, TypeError):
        return None


def list_packs() -> list[dict[str, Any]]:
    root = pack_root()
    if not root.is_dir():
        return []
    packs = []
    for directory in sorted(root.iterdir(), key=lambda item: item.name):
        if directory.is_dir() and not directory.name.startswith("."):
            pack = _read_pack(directory)
            if pack:
                packs.append(pack)
    return packs


def bundled_artifacts(directory: str | os.PathLike[str] | None = None) -> list[dict[str, Any]]:
    root = Path(directory or os.environ.get("LINKCPP_BUNDLED_RUNTIME_PACK_DIR", "/app/runtime-packs"))
    if not root.is_dir():
        return []
    artifacts = []
    for archive in sorted(root.glob("*.tar.gz")):
        try:
            if archive.stat().st_size > MAX_ARCHIVE_BYTES:
                continue
            with tarfile.open(archive, mode="r:*") as bundle:
                member = bundle.getmember("manifest.json")
                stream = bundle.extractfile(member)
                if stream is None:
                    continue
                manifest = _load_manifest_bytes(stream.read(4 << 20))
            artifacts.append({
                "filename": archive.name,
                "size": archive.stat().st_size,
                "sha256": _sha256_file(archive),
                "manifest": manifest,
                "path": str(archive),
            })
        except (OSError, KeyError, ValueError, tarfile.TarError):
            continue
    return artifacts


@lru_cache(maxsize=32)
def _verified_installed_pack(directory_value: str, archive_sha256: str) -> dict[str, Any] | None:
    directory = Path(directory_value)
    pack = _read_pack(directory)
    if not pack or pack.get("archive_sha256") != archive_sha256:
        return None
    manifest = pack["manifest"]
    try:
        for entry in manifest["files"]:
            path = directory.joinpath(*PurePosixPath(entry["path"]).parts)
            if path.stat().st_size != int(entry["size"]) or _sha256_file(path) != entry["sha256"]:
                return None
        info = _verify_executables(directory, manifest)
    except (OSError, ValueError):
        return None
    return {**pack, "runtime": info}


def resolve_runtime(
    *, coordinator: bool, expected_protocol: str | None,
    expected_adapter_abi: int | None, expected_build_id: str | None,
    baked_stage: str, baked_server: str,
) -> tuple[str, dict[str, str], str]:
    """Resolve one request without changing the process-global active runtime."""
    baked = baked_server if coordinator else baked_stage
    if not expected_build_id and expected_adapter_abi is None and not expected_protocol:
        return baked, dict(os.environ), "baked-default"
    if (expected_protocol != RING_PROTOCOL or expected_adapter_abi != RING_ADAPTER_ABI
            or not expected_build_id or expected_build_id == "unknown"):
        raise ValueError("stage request does not identify one compatible immutable ring runtime")
    baked_info = runtime_pair_info(baked_stage, baked_server)
    if baked_info.get("available") and baked_info.get("build_id") == expected_build_id:
        return baked, dict(os.environ), "baked-" + expected_build_id
    host_system = platform.system().lower()
    host_machine = platform.machine().lower()
    host_backend = str(host_resources.accelerator().get("backend_kind") or "").lower()
    for pack in list_packs():
        manifest = pack["manifest"]
        target = manifest.get("target") or {}
        if (manifest.get("protocol") == expected_protocol
                and manifest.get("adapter_abi") == expected_adapter_abi
                and manifest.get("build_id") == expected_build_id
                and str(target.get("system") or "").lower() == host_system
                and str(target.get("machine") or "").lower() == host_machine
                and str(target.get("backend") or "").lower() == host_backend):
            verified = _verified_installed_pack(pack["path"], pack["archive_sha256"])
            if verified is None:
                continue
            directory = Path(verified["path"])
            binary = directory / "bin" / ("linkcpp-server" if coordinator else "linkcpp-node")
            return str(binary), _runtime_environment(directory), str(manifest["pack_id"])
    raise ValueError(f"required ring runtime pack is not installed: {expected_build_id}")
