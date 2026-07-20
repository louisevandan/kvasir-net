"""Phase 2 load-adaptive recruitment: a saturated MoE coordinator raises the
effective expert-replica target so the coverage market pulls idle nodes in."""
import time
import unittest

# hub imports fastapi/httpx/gguf; the shared lightweight stubs let the endpoint
# functions be called directly (Query -> plain default). Installing them before
# hub is imported keeps this module order-independent with test_control_protocol,
# which installs the same stubs but only when fastapi is not yet in sys.modules.
from tests.unit.test_control_protocol import (
    install_fastapi_stub,
    install_gguf_stub,
    install_httpx_stub,
)

install_httpx_stub()
install_fastapi_stub()
install_gguf_stub()

from controller import hub


class RecruitmentTests(unittest.TestCase):
    def setUp(self):
        # isolate global state per test
        hub.COORD_LOAD.clear()
        hub.EXPERT_WORKERS.clear()
        self._base = hub.EXPERT_TARGET_REPLICAS
        self._boost = hub.EXPERT_RECRUIT_BOOST
        # a small known model, dims injected so seeding needs no GGUF on disk
        hub._MODEL_DIMS_CACHE["demo.gguf"] = {"n_embd": 128, "n_layer": 4, "n_expert": 8}

    def tearDown(self):
        hub.COORD_LOAD.clear()
        hub.EXPERT_WORKERS.clear()
        hub._MODEL_DIMS_CACHE.pop("demo.gguf", None)

    def _load(self, busy, ts=None):
        hub.COORD_LOAD["demo.gguf"] = {
            "busy": busy, "slots": 4,
            "saturated": busy >= hub.RECRUIT_SATURATE_SLOTS,
            "ts": ts if ts is not None else time.time(), "master_port": 8306, "name": "demo",
        }

    def test_idle_uses_base_target(self):
        self._load(busy=0)
        self.assertFalse(hub._recruiting("demo.gguf"))
        self.assertEqual(hub._effective_target("demo.gguf"), self._base)

    def test_saturated_boosts_target(self):
        self._load(busy=hub.RECRUIT_SATURATE_SLOTS)
        self.assertTrue(hub._recruiting("demo.gguf"))
        self.assertEqual(hub._effective_target("demo.gguf"), self._base + self._boost)

    def test_stale_reading_stops_recruiting(self):
        self._load(busy=4, ts=time.time() - hub._RECRUIT_TTL - 5)
        self.assertFalse(hub._recruiting("demo.gguf"))
        self.assertEqual(hub._effective_target("demo.gguf"), self._base)

    def test_unknown_model_never_recruits(self):
        self.assertFalse(hub._recruiting("other.gguf"))
        self.assertEqual(hub._effective_target("other.gguf"), self._base)

    def test_zero_worker_saturated_model_is_seeded(self):
        # no workers registered, but the coordinator is saturated -> demand appears
        self._load(busy=4)
        cov = hub._expert_coverage("demo.gguf")
        self.assertEqual(len(cov), 1)
        entry = cov[0]
        self.assertTrue(entry["recruiting"])
        self.assertEqual(entry["target_replicas"], self._base + self._boost)
        # layer 0, all 8 experts uncovered -> one scarce segment at full scarcity
        seg = entry["layers"][0]["segments"][0]
        self.assertEqual(seg["experts"], [0, 8])
        self.assertEqual(seg["replicas"], 0)
        self.assertEqual(seg["scarcity"], 1.0)

    def test_idle_zero_worker_model_is_not_seeded(self):
        self._load(busy=0)   # not saturated
        self.assertEqual(hub._expert_coverage("demo.gguf"), [])

    def test_covered_experts_become_scarce_under_load(self):
        # one worker fully covers layer-0 experts [0,8): at base target=2 it is
        # partially covered (replicas=1). Under recruitment target rises to 3, so
        # scarcity increases -> a volunteer is still recommended.
        hub.EXPERT_WORKERS["w1"] = {
            "model": "demo.gguf", "n_layer": 4, "n_expert": 8,
            "segments": [[0, 0, 8]], "url": "relay:expert-w1", "owner": "", "ts": time.time(),
        }
        self._load(busy=0)   # not saturated
        idle = hub._expert_coverage("demo.gguf")[0]
        idle_seg = idle["layers"][0]["segments"][0]
        self._load(busy=4)   # saturated
        hot = hub._expert_coverage("demo.gguf")[0]
        hot_seg = hot["layers"][0]["segments"][0]
        self.assertEqual(idle["target_replicas"], self._base)
        self.assertEqual(hot["target_replicas"], self._base + self._boost)
        self.assertGreater(hot_seg["scarcity"], idle_seg["scarcity"])
        # recommend must now return a segment (was at/above base target before)
        self._load(busy=0)
        self.assertIsNone(hub._recommend_expert_segment("demo.gguf")) if idle_seg["scarcity"] <= 0 else None
        self._load(busy=4)
        self.assertIsNotNone(hub._recommend_expert_segment("demo.gguf"))

    def test_demand_endpoint_exposes_recruiting_flag(self):
        self._load(busy=4)
        resp = hub.api_expert_demand("demo.gguf")
        self.assertTrue(resp["recruiting"])
        self._load(busy=0)
        resp = hub.api_expert_demand("demo.gguf")
        self.assertFalse(resp["recruiting"])

    def test_recruitment_endpoint_reports_state(self):
        self._load(busy=hub.RECRUIT_SATURATE_SLOTS)
        resp = hub.api_moe_recruitment()
        self.assertEqual(resp["saturate_at"], hub.RECRUIT_SATURATE_SLOTS)
        row = next(r for r in resp["models"] if r["model"] == "demo.gguf")
        self.assertTrue(row["saturated"])
        self.assertTrue(row["recruiting"])
        self.assertEqual(row["effective_target"], self._base + self._boost)
        self.assertEqual(row["expert_workers"], 0)


if __name__ == "__main__":
    unittest.main()
