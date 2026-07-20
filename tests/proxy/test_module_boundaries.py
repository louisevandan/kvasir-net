from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ProxyModuleBoundaryTests(unittest.TestCase):
    def source(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_hub_contains_hooks_but_no_proxy_implementation(self):
        source = self.source("controller/hub.py")
        self.assertIn("from controller.proxy import install_hub_api", source)
        self.assertNotIn("from controller.proxy.", source)
        self.assertNotIn("StageStartRequest", source)
        # hub.py may list a proxy model route in the node-token allowlist;
        # route construction itself must remain in controller.proxy.hub_api.
        self.assertNotIn('APIRouter(prefix="/api/proxy"', source)
        self.assertNotIn("LINKCPP_STAGE_BIN", source)

    def test_node_agent_contains_only_lazy_proxy_router_hook(self):
        source = self.source("controller/nodeagent.py")
        self.assertIn("from controller.proxy import install_node_api", source)
        self.assertNotIn("from controller.proxy.", source)
        self.assertNotIn("StageStartRequest", source)
        self.assertNotIn("/control/proxy", source)
        self.assertNotIn("LINKCPP_STAGE_BIN", source)

    def test_stable_protocol_and_rpc_driver_do_not_depend_on_proxy(self):
        protocol = self.source("controller/protocol.py")
        rpc = self.source("controller/runtimes/llama_rpc.py")
        self.assertNotIn("StageStartRequest", protocol)
        self.assertNotIn("stage_manifest", protocol)
        self.assertNotIn("controller.proxy", rpc)

    def test_proxy_http_contract_is_owned_by_proxy_package(self):
        hub_api = self.source("controller/proxy/hub_api.py")
        node_api = self.source("controller/proxy/node_api.py")
        self.assertIn('prefix="/api/proxy"', hub_api)
        self.assertIn('prefix="/control/proxy"', node_api)


if __name__ == "__main__":
    unittest.main()
