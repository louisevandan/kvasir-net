import unittest
from unittest import mock

from controller.stage_protocol import (
    ACK,
    DECODE,
    HEADER,
    StageFrame,
    TensorDescriptor,
    adjacent_links,
    decode_frame,
    decode_tensor_payload,
    encode_frame,
    encode_tensor_payload,
    stage_neighbors,
)
from controller.proxy import manifest as stage_manifest


class StageProtocolTests(unittest.TestCase):
    def test_frame_round_trip_has_fixed_header(self):
        frame = StageFrame(DECODE, 17, 3, 42, b"hidden", flags=2)
        encoded = encode_frame(frame)

        self.assertEqual(len(encoded) - len(frame.payload), HEADER.size)
        self.assertEqual(decode_frame(encoded), frame)

    def test_invalid_payload_length_is_rejected(self):
        encoded = encode_frame(StageFrame(ACK, 1, 1, 1, b"ok"))
        with self.assertRaises(ValueError):
            decode_frame(encoded[:-1])

    def test_tensor_payload_round_trip_preserves_boundary_metadata(self):
        descriptor = TensorDescriptor(
            role="stage_hidden",
            dtype="f16",
            shape=(2, 4096),
            strides=(8192, 2),
            sequence_id=7,
            position=91,
            view_offset=16,
            flags=3,
        )
        payload = encode_tensor_payload(descriptor, b"hidden-state")

        self.assertEqual(decode_tensor_payload(payload), (descriptor, b"hidden-state"))

    def test_tensor_payload_rejects_mismatched_lengths(self):
        descriptor = TensorDescriptor("stage_hidden", "f32", (4,), (4,))
        payload = encode_tensor_payload(descriptor, b"data")
        with self.assertRaises(ValueError):
            decode_tensor_payload(payload[:-1])

    def test_topology_only_connects_adjacent_stages(self):
        topology = [
            {"node_id": "a", "stage_endpoint": "10.0.0.1:51052", "layers": [0, 8]},
            {"node_id": "b", "stage_endpoint": "10.0.0.2:51052", "layers": [8, 16]},
            {"node_id": "c", "stage_endpoint": "10.0.0.3:51052", "layers": [16, 24]},
        ]

        self.assertEqual(
            adjacent_links(topology),
            [
                {"upstream": "a", "downstream": "b", "upstream_endpoint": "10.0.0.1:51052",
                 "downstream_endpoint": "10.0.0.2:51052", "layers": [[0, 8], [8, 16]]},
                {"upstream": "b", "downstream": "c", "upstream_endpoint": "10.0.0.2:51052",
                 "downstream_endpoint": "10.0.0.3:51052", "layers": [[8, 16], [16, 24]]},
                {"upstream": "c", "downstream": "a", "upstream_endpoint": "10.0.0.3:51052",
                 "downstream_endpoint": "10.0.0.1:51052", "layers": [[16, 24], [0, 8]]},
            ],
        )
        self.assertEqual(stage_neighbors(topology, "a"), {"upstream": "c", "downstream": "b"})
        self.assertEqual(stage_neighbors(topology, "b"), {"upstream": "a", "downstream": "c"})
        self.assertEqual(stage_neighbors(topology, "c"), {"upstream": "b", "downstream": "a"})

    def test_manifest_limits_local_tensor_names_and_rejects_mismatch(self):
        class Tensor:
            def __init__(self, name, size):
                self.name = name
                self.n_bytes = size

        class Reader:
            fields = {
                "general.architecture": "test",
                "test.block_count": 3,
                "test.embedding_length": 16,
            }
            tensors = [
                Tensor("token_embd.weight", 10), Tensor("blk.0.attn.weight", 20),
                Tensor("blk.1.attn.weight", 30), Tensor("blk.2.attn.weight", 40),
                Tensor("output.weight", 50),
            ]

        with mock.patch.object(stage_manifest, "GGUFReader", return_value=Reader()), \
             mock.patch.object(stage_manifest, "_scalar", side_effect=lambda value: value):
            manifests = stage_manifest.build_stage_manifests(
                plan_id="p", model_ref="models/test.gguf", model_root="/models",
                shard_refs=["models/test.gguf"],
                placements=[{"layers": [0, 1]}, {"layers": [1, 2]}, {"layers": [2, 3]}],
            )
            self.assertEqual(stage_manifest.GGUFReader.call_count, 1)
            manifest = manifests[1]
            self.assertEqual(manifest["tensor_names"], ["blk.1.attn.weight"])
            with mock.patch.object(stage_manifest.os.path, "isfile", return_value=True):
                verified = stage_manifest.validate_stage_manifest("/models", manifest)
            self.assertEqual(verified["local_tensor_bytes"], 30)

            bad = dict(manifest)
            bad["identity"] = dict(manifest["identity"], tensor_count=999)
            with mock.patch.object(stage_manifest.os.path, "isfile", return_value=True):
                with self.assertRaisesRegex(ValueError, "tensor_count"):
                    stage_manifest.validate_stage_manifest("/models", bad)


if __name__ == "__main__":
    unittest.main()
