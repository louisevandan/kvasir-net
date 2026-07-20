import hashlib
import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from controller.proxy import packs as runtime_packs
from controller.proxy.capability import RING_ADAPTER_ABI, RING_PROTOCOL


class RuntimePackTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "packs"
        self.build_id = "build-immutable-a"

    def tearDown(self):
        self.temp.cleanup()

    def _archive(self, *, pack_id="pack-a", extra_member=None):
        archive = Path(self.temp.name) / f"{pack_id}.tar.gz"
        payloads = {
            "bin/linkcpp-node": b"node-binary",
            "bin/linkcpp-server": b"server-binary",
            "lib/libllama.so": b"llama-library",
        }
        files = [{
            "path": name, "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
        } for name, data in payloads.items()]
        manifest = {
            "schema": 1, "pack_id": pack_id, "protocol": RING_PROTOCOL,
            "adapter_abi": RING_ADAPTER_ABI, "build_id": self.build_id,
            "state_snapshot": True, "chunked_state": True,
            "target": {"system": "linux", "machine": "x86_64", "backend": "cuda"},
            "files": files,
        }
        with tarfile.open(archive, "w:gz") as bundle:
            manifest_data = json.dumps(manifest).encode()
            info = tarfile.TarInfo("manifest.json")
            info.size = len(manifest_data)
            bundle.addfile(info, io.BytesIO(manifest_data))
            for name, data in payloads.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                bundle.addfile(info, io.BytesIO(data))
            if extra_member:
                info = tarfile.TarInfo(extra_member)
                info.size = 1
                bundle.addfile(info, io.BytesIO(b"x"))
        return archive, manifest

    def _install(self, archive, manifest):
        runtime = {
            "available": True, "protocol": RING_PROTOCOL,
            "adapter_abi": RING_ADAPTER_ABI, "build_id": self.build_id,
        }
        with mock.patch.object(runtime_packs, "pack_root", return_value=self.root), \
                mock.patch.object(runtime_packs, "_verify_executables", return_value=runtime):
            return runtime_packs.install_archive(
                archive, runtime_packs._sha256_file(archive), expected_build_id=self.build_id,
            )

    def test_verified_pack_is_atomically_registered_and_selected_per_request(self):
        archive, manifest = self._archive()
        result = self._install(archive, manifest)
        self.assertEqual(result["pack_id"], "pack-a")
        self.assertTrue((self.root / "pack-a" / ".complete.json").is_file())
        with mock.patch.object(runtime_packs, "pack_root", return_value=self.root), \
                mock.patch.object(runtime_packs, "runtime_pair_info", return_value={"available": False}), \
                mock.patch.object(runtime_packs, "_verify_executables", return_value={"available": True}), \
                mock.patch.object(runtime_packs.platform, "system", return_value="Linux"), \
                mock.patch.object(runtime_packs.platform, "machine", return_value="x86_64"), \
                mock.patch.object(
                    runtime_packs.host_resources, "accelerator",
                    return_value={"backend_kind": "cuda"},
                ):
            binary, env, pack_id = runtime_packs.resolve_runtime(
                coordinator=False, expected_protocol=RING_PROTOCOL,
                expected_adapter_abi=RING_ADAPTER_ABI, expected_build_id=self.build_id,
                baked_stage="missing-node", baked_server="missing-server",
            )
        self.assertEqual(pack_id, "pack-a")
        self.assertEqual(Path(binary).name, "linkcpp-node")
        self.assertIn(str(self.root / "pack-a" / "lib"), env["LD_LIBRARY_PATH"])

    def test_old_rpc_request_keeps_baked_runtime_without_global_activation(self):
        binary, env, pack_id = runtime_packs.resolve_runtime(
            coordinator=False, expected_protocol=None, expected_adapter_abi=None,
            expected_build_id=None, baked_stage="baked-node", baked_server="baked-server",
        )
        self.assertEqual((binary, pack_id), ("baked-node", "baked-default"))
        self.assertNotIn("LINKCPP_RUNTIME_PACK_ACTIVE", env)

    def test_archive_hash_and_path_traversal_fail_closed(self):
        archive, manifest = self._archive(pack_id="bad", extra_member="../escape")
        with mock.patch.object(runtime_packs, "pack_root", return_value=self.root):
            with self.assertRaisesRegex(ValueError, "unsafe runtime-pack path"):
                runtime_packs.install_archive(archive, runtime_packs._sha256_file(archive))
            with self.assertRaisesRegex(ValueError, "archive SHA-256 mismatch"):
                runtime_packs.install_archive(archive, "0" * 64)
        self.assertFalse((Path(self.temp.name) / "escape").exists())

    def test_same_pack_id_cannot_be_replaced_with_different_archive(self):
        first, manifest = self._archive(pack_id="stable")
        self._install(first, manifest)
        second = Path(self.temp.name) / "second.tar.gz"
        second.write_bytes(first.read_bytes() + b"different")
        runtime = {
            "available": True, "protocol": RING_PROTOCOL,
            "adapter_abi": RING_ADAPTER_ABI, "build_id": self.build_id,
        }
        # Trailing bytes keep the tar valid while changing its content address.
        with mock.patch.object(runtime_packs, "pack_root", return_value=self.root), \
                mock.patch.object(runtime_packs, "_verify_executables", return_value=runtime):
            with self.assertRaisesRegex(ValueError, "different content"):
                runtime_packs.install_archive(second, runtime_packs._sha256_file(second))


if __name__ == "__main__":
    unittest.main()
