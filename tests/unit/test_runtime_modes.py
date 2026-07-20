import unittest
from unittest import mock

from controller.runtime_modes import (
    LLAMA_RPC,
    RING_PROXY,
    data_plane_contract,
    normalize_runtime_mode,
)


class RuntimeModeTests(unittest.TestCase):
    def setUp(self):
        self.topology = [
            {"node_id": "a", "rpc_endpoint": "a:50052", "stage_endpoint": "a:51052"},
            {"node_id": "b", "rpc_endpoint": "b:50052", "stage_endpoint": "b:51052"},
        ]

    def test_stock_rpc_contract_is_master_star(self):
        contract = data_plane_contract(LLAMA_RPC, self.topology)
        self.assertEqual(contract["topology"], "master_star")
        self.assertEqual(contract["protocol"], "llama.cpp-rpc")
        self.assertEqual(contract["links"][1]["master"], "llama-server")
        self.assertTrue(contract["available"])

    def test_rpc_selection_imports_only_the_rpc_driver(self):
        from controller import runtimes

        driver = mock.Mock()
        driver.data_plane_contract.return_value = {"mode": LLAMA_RPC}
        with mock.patch.object(runtimes, "import_module", return_value=driver) as imported:
            self.assertEqual(runtimes.data_plane_contract(LLAMA_RPC, [])["mode"], LLAMA_RPC)
        imported.assert_called_once_with("controller.runtimes.llama_rpc")

    def test_ring_contract_is_adjacent_and_fails_closed_until_adapter_exists(self):
        contract = data_plane_contract(RING_PROXY, self.topology)
        self.assertEqual(contract["topology"], "adjacent_ring")
        self.assertEqual(contract["links"][0]["downstream_endpoint"], "b:51052")
        self.assertEqual(contract["links"][1]["downstream_endpoint"], "a:51052")
        self.assertFalse(contract["available"])
        self.assertIn("adapter", contract["blocker"])

    def test_mode_names_are_normalized_but_unknown_modes_are_rejected(self):
        self.assertEqual(normalize_runtime_mode("ring-proxy"), RING_PROXY)
        with self.assertRaises(ValueError):
            normalize_runtime_mode("automatic")


class RuntimeTaskTests(unittest.IsolatedAsyncioTestCase):
    async def test_background_driver_records_failure_without_leaking_task_exception(self):
        from controller import runtimes

        controller = {
            "phase": "loading", "load_cancel": {}, "load_task": object(),
            "pending_load": {},
        }
        with mock.patch.object(
            runtimes, "serve", new=mock.AsyncMock(side_effect=RuntimeError("adapter failed")),
        ):
            result = await runtimes.serve_background(
                RING_PROXY, controller, object(), {}, [],
            )

        self.assertIsNone(result)
        self.assertEqual(controller["phase"], "error")
        self.assertEqual(controller["detail"], "adapter failed")
        self.assertNotIn("load_task", controller)
        self.assertNotIn("pending_load", controller)


if __name__ == "__main__":
    unittest.main()
