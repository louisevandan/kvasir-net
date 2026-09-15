import importlib.util
import struct
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("inspect-gguf-metadata.py")
SPEC = importlib.util.spec_from_file_location("inspect_gguf_metadata", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def string(value):
    raw = value.encode()
    return struct.pack("<Q", len(raw)) + raw


def metadata(key, kind, value):
    if kind == 8:
        encoded = string(value)
    elif kind in (4, 5):
        encoded = struct.pack("<I" if kind == 4 else "<i", value)
    else:
        raise AssertionError(kind)
    return string(key) + struct.pack("<I", kind) + encoded


def tensor(name, dimensions):
    return (string(name) + struct.pack("<I", len(dimensions)) +
            b"".join(struct.pack("<Q", value) for value in dimensions) +
            struct.pack("<IQ", 0, 0))


def fixture(path, split_no=0):
    entries = [
        ("general.architecture", 8, "qwen35moe"),
        ("general.size_label", 8, "test"),
        ("general.quantization_version", 4, 2),
        ("general.file_type", 4, 16),
        ("qwen35moe.block_count", 4, 2),
        ("qwen35moe.context_length", 4, 128),
        ("qwen35moe.expert_count", 4, 4),
        ("qwen35moe.expert_used_count", 4, 2),
        ("qwen35moe.nextn_predict_layers", 4, 0),
        ("tokenizer.chat_template", 8, "template"),
        ("split.no", 4, split_no),
        ("split.count", 4, 1),
        ("split.tensors.count", 4, 2),
    ]
    tensors = [tensor("blk.0.ffn_gate_exps.weight", [3, 4]), tensor("output.weight", [5])]
    raw = (b"GGUF" + struct.pack("<IQQ", 3, len(tensors), len(entries)) +
           b"".join(metadata(*entry) for entry in entries) + b"".join(tensors))
    path.write_bytes(raw)


class InspectTests(unittest.TestCase):
    def test_exact_total_and_active_parameter_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "model.gguf"
            fixture(path)
            result = MODULE.inspect([path])
            self.assertEqual(result["total_parameters"], 17)
            self.assertEqual(result["expert_parameters"], 12)
            self.assertEqual(result["active_parameters_per_token"], 11)
            self.assertEqual(result["tensor_count"], 2)

    def test_duplicate_split_number_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.gguf"
            second = Path(directory) / "second.gguf"
            fixture(first)
            fixture(second)
            with self.assertRaisesRegex(ValueError, "split membership"):
                MODULE.inspect([first, second])


if __name__ == "__main__":
    unittest.main()
