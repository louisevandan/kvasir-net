from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import unittest


MODULE_PATH = Path(__file__).parents[1] / "validate_event_runtime_preflight.py"
SPEC = importlib.util.spec_from_file_location("validate_event_runtime_preflight", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class EventRuntimePreflightTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = {
            "ingress_agent": "tcp://ingress:52005",
            "nodes": [{"agent": "tcp://worker:52005", "endpoint": "127.0.0.1:22001"}],
        }
        self.evidence = {
            "schema": 1,
            "ingress_agent": "tcp://ingress:52005",
            "routes": [{
                "agent": "tcp://worker:52005",
                "request_target": [0, "tcp://worker:52005"],
                "reply_source": [0, "tcp://worker:52005"],
                "reply_target": [2, "tcp://ingress:52005", "id", 1],
                "reply_return_route": [2, "tcp://ingress:52005", "id", 1],
                "content": "application/vnd.p4.agent.snapshot-v1+json",
                "nodes": [],
                "transport_failure_count": 0,
                "task_owned_native_processes": 0,
                "task_owned_native_listeners": 0,
                "task_owned_agent_non_listener_connections": 0,
                "dynamic_port_ranges": [{"first": 32768, "last": 60999}],
            }],
        }

    def test_accepts_exact_clean_roundtrip_outside_dynamic_range(self) -> None:
        self.assertEqual(MODULE.validate(self.config, self.evidence), [])

    def test_rejects_wrong_return_route(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        evidence["routes"][0]["reply_return_route"][1] = "tcp://wrong:52005"
        self.assertTrue(any("return route" in item for item in MODULE.validate(self.config, evidence)))

    def test_rejects_native_port_inside_os_dynamic_range(self) -> None:
        config = copy.deepcopy(self.config)
        config["nodes"][0]["endpoint"] = "127.0.0.1:43250"
        self.assertTrue(any("overlaps" in item for item in MODULE.validate(config, self.evidence)))

    def test_rejects_orphan_and_nonempty_agent(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        evidence["routes"][0]["nodes"] = [{"node_id": "leftover"}]
        evidence["routes"][0]["task_owned_native_processes"] = 1
        errors = MODULE.validate(self.config, evidence)
        self.assertTrue(any("not empty" in item for item in errors))
        self.assertTrue(any("native children" in item for item in errors))

    def test_rejects_retained_agent_connection_state(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        evidence["routes"][0]["task_owned_agent_non_listener_connections"] = 1
        self.assertTrue(any("non-listener TCP" in item for item in MODULE.validate(self.config, evidence)))


if __name__ == "__main__":
    unittest.main()
