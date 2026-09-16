import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("inspect-h0-host.py")
SPEC = importlib.util.spec_from_file_location("inspect_h0_host", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NvidiaInventoryTests(unittest.TestCase):
    @patch.object(MODULE, "run")
    def test_unavailable_power_cap_is_explicit(self, mocked_run):
        mocked_run.side_effect = [
            {"stdout": "0, GPU-a, NVIDIA GB10, 580.159.03, [N/A], [N/A]"},
            {"stdout": ""},
        ]
        result = MODULE.platform_accelerator()
        self.assertEqual(result["devices"][0]["uuid"], "GPU-a")
        self.assertEqual(result["power_cap"], {
            "status": "unavailable",
            "reason": "nvidia_smi_reports_NA_for_power_limit",
        })

    @patch.object(MODULE, "run")
    def test_numeric_power_cap_is_preserved(self, mocked_run):
        mocked_run.side_effect = [
            {"stdout": "0, GPU-b, Test GPU, 1.2.3, 24576, 350.00"},
            {"stdout": ""},
        ]
        result = MODULE.platform_accelerator()
        self.assertEqual(result["power_cap"], {"status": "measured", "watts": [350.0]})


class ProtectedConnectionTests(unittest.TestCase):
    @patch.object(MODULE.sys, "platform", "linux")
    @patch.object(MODULE, "run")
    def test_linux_counts_only_listener_owned_connection_states(self, mocked_run):
        mocked_run.return_value = {
            "stdout": "LISTEN 0 128 0.0.0.0:52005 0.0.0.0:*\n"
                      "CLOSE-WAIT 0 0 192.168.0.26:52005 192.168.0.6:50000"
        }
        result = MODULE.protected_connection_state()
        self.assertEqual(result["state_counts"], {"CLOSE-WAIT": 1, "LISTEN": 1})
        self.assertEqual(result["non_listener_count"], 1)

    @patch.object(MODULE.sys, "platform", "darwin")
    @patch.object(MODULE, "run")
    def test_macos_excludes_outbound_connection_to_peer_agent_port(self, mocked_run):
        mocked_run.return_value = {
            "stdout": "tcp4 0 0 *.52005 *.* LISTEN 0 0\n"
                      "tcp4 0 0 192.168.0.20.52005 192.168.0.19.50000 CLOSE_WAIT 0 0\n"
                      "tcp4 0 0 192.168.0.20.60000 192.168.0.19.52005 CLOSE_WAIT 0 0"
        }
        result = MODULE.protected_connection_state()
        self.assertEqual(result["state_counts"], {"CLOSE_WAIT": 1, "LISTEN": 1})
        self.assertEqual(result["non_listener_count"], 1)


if __name__ == "__main__":
    unittest.main()
