import unittest
from unittest import mock

from controller import device_profiles, host_resources


class HostResourcesTests(unittest.TestCase):
    def test_gb10_unified_memory_is_reported_as_vram(self):
        gpu = device_profiles.cuda_device(
            "GPU-GB10", "NVIDIA GB10", "[N/A]", "[N/A]", system_memory_gib=119.2,
        )

        self.assertEqual(gpu["uuid"], "GPU-GB10")
        self.assertEqual(gpu["name"], "NVIDIA GB10")
        self.assertEqual(gpu["vram_gib"], 119.2)
        self.assertEqual(gpu["backend_kind"], "cuda")

    def test_dedicated_nvidia_vram_is_unchanged(self):
        self.assertEqual(device_profiles.cuda_vram_gib("NVIDIA RTX 5090", "32768", 64), 32.0)

    def test_metal_uses_unified_memory_budget(self):
        metal = device_profiles.metal_device("metal0", "Apple M4 Max", system_memory_gib=64)
        self.assertEqual(metal["vram_gib"], 64.0)
        self.assertEqual(metal["backend_kind"], "metal")

    def test_host_discovers_cuda_through_device_profile(self):
        report = "GPU-GB10, NVIDIA GB10, [N/A], [N/A]"
        with mock.patch.object(host_resources, "_run", return_value=report), \
             mock.patch.object(host_resources, "_memory_info", return_value={"total": 119.2, "used": 8.0}):
            gpus = host_resources.local_gpus()
        self.assertEqual(gpus[0]["vram_total_gib"], 119.2)


if __name__ == "__main__":
    unittest.main()
