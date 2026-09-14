"""Independent enumeration and real CLI consumption of Qwen planning contracts."""
from copy import deepcopy
import itertools
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import unittest

from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from p4hfadapter.models.qwen3_5_0_8b.planning import LoadingInfeasible, plan_loading
from tests.fixtures.qwen_loading import profiles, request


def enumerate_reference(spec, measured):
    """Brute-force slot subsets and cut combinations; no production helpers."""
    devices = {d["id"]: d for d in spec["devices"]}
    samples = {(p["binding"]["id"], *s["layers"]): s for p in measured for s in p["samples"]}
    active = [s for s in spec["slots"] if devices[s["device_id"]]["enabled"]]
    winners = []
    for size in range(1, min(len(active), len(spec["cuts"]) - 1) + 1):
        for slots in itertools.combinations(active, size):
            if len({devices[s["device_id"]]["host"] for s in slots}) < spec["minimum_hosts"]:
                continue
            for interior in itertools.combinations(spec["cuts"][1:-1], size - 1):
                cuts = (0, *interior, 24)
                cpu, gpu, times = {}, {}, []
                for slot, start, end in zip(slots, cuts, cuts[1:]):
                    key = slot["device_id"]
                    sample, device = samples[key, start, end], devices[key]
                    cpu[device["host"]] = cpu.get(device["host"], 0) + sample["host_peak_bytes"]
                    gpu[key] = gpu.get(key, 0) + sample["device_peak_bytes"]
                    times.append(sample["service_ms"])
                if any(cpu.get(h["id"], 0) > max(0, h["available_bytes"] - h["reserve_bytes"]) for h in spec["hosts"]):
                    continue
                if any(gpu.get(d["id"], 0) > max(0, d["available_bytes"] - d["reserve_bytes"]) for d in spec["devices"]):
                    continue
                winners.append((sum(times), max(times), size))
    return min(winners) if winners else None


class PlanningTests(unittest.TestCase):
    def test_serial_controller_uses_total_service_before_bottleneck(self):
        spec = request(2, (0, 12, 24))
        data = profiles(spec, lambda n, start, end: {"service_ms": 30 if start == 0 and end == 24 else 20})
        result = plan_loading(spec, data)
        self.assertEqual(result["objective"], {"total_stage_service_ms": 30, "maximum_stage_service_ms": 30, "stages": 1})

    def test_machine_count_requires_three_stages(self):
        spec = request(3, (0, 8, 16, 24))
        spec["minimum_hosts"] = 3
        actual = plan_loading(spec, profiles(spec))
        self.assertEqual(actual["objective"], {"maximum_stage_service_ms": 24, "total_stage_service_ms": 48, "stages": 3})

    def test_seeded_independent_reference(self):
        rng = random.Random(9415)
        for case in range(180):
            spec = request(rng.randint(1, 4))
            spec["minimum_hosts"] = rng.randint(1, len(spec["hosts"]))
            for device in spec["devices"]:
                device["available_bytes"] = rng.randint(50, 280)
                device["reserve_bytes"] = rng.randint(0, 20)
                device["enabled"] = rng.random() > .15
            if case % 3 == 0:
                spec["slots"].append({"node_id": "repeat", "device_id": "gpu-0"})
            measured = profiles(spec, lambda *_: {"service_ms": rng.randint(1, 90), "host_peak_bytes": rng.randint(100, 300)})
            for host in spec["hosts"]:
                host["available_bytes"] = rng.randint(100, 700)
            original = deepcopy((spec, measured))
            expected = enumerate_reference(spec, measured)
            if expected is None:
                with self.assertRaises(LoadingInfeasible):
                    plan_loading(spec, measured)
            else:
                result = plan_loading(spec, measured)
                self.assertEqual(tuple(result["objective"].values()), expected, case)
                self.assertEqual(parse_plan(result["plan"]).prefill_chunk, 16)
            self.assertEqual((spec, measured), original)

    def test_repeated_gpu_and_shared_host_are_cumulative(self):
        spec = request(2, (0, 8, 16, 24))
        spec["devices"][1]["host"] = "host-0"
        spec["devices"][1]["device"] = "cuda:1"
        spec["slots"].append({"node_id": "repeat", "device_id": "gpu-0"})
        data = profiles(spec)
        spec["hosts"][0]["available_bytes"] = 99
        with self.assertRaises(LoadingInfeasible):
            plan_loading(spec, data)
        spec["hosts"][0]["available_bytes"] = 10000
        for device in spec["devices"]:
            device["available_bytes"] = 80
        with self.assertRaises(LoadingInfeasible):
            plan_loading(spec, data)

    def test_generated_prefill_limit_is_consumed_by_scenario_admission(self):
        from p4hfadapter.models.qwen3_5_0_8b.scenarios import read_scenario
        from tests.fixtures.qwen_tokenizer import Tokenizer
        spec = request()
        plan = parse_plan(plan_loading(spec, profiles(spec))["plan"])
        scenario = {"name": "oversize", "schedule": "sequential", "requests": [
            {"id": "r", "prompt": "hello", "max_new_tokens": 2, "prefill_chunk": 17, "cancel_after": None}]}
        with tempfile.TemporaryDirectory() as temp:
            file = Path(temp) / "scenario.json"
            file.write_text(json.dumps(scenario))
            with self.assertRaisesRegex(ValueError, "profiled prefill_chunk"):
                read_scenario(file, plan, Tokenizer())

    def test_missing_or_stale_profiles_are_not_infeasibility(self):
        for mutation in ("missing", "source", "workload", "checkpoint", "memory", "release"):
            spec = request()
            data = profiles(spec)
            if mutation == "missing": data[0]["samples"].pop()
            if mutation == "source": data[0]["source_sha256"] = {}
            if mutation == "workload": data[0]["contract"]["limits"]["context"] = 128
            if mutation == "checkpoint": data[0]["checkpoint_sha256"] = "wrong"
            if mutation == "memory": data[0]["samples"][0]["device_peak_bytes"] = 0
            if mutation == "release": data[0]["samples"][0]["active_after_release"] = 1
            with self.assertRaises(ValueError) as error:
                plan_loading(spec, data)
            self.assertNotIsInstance(error.exception, LoadingInfeasible)

    def test_unsupported_dtype_device_model_and_duplicate_resource(self):
        for mutation in ("dtype", "device", "model", "duplicate"):
            spec = request()
            if mutation == "dtype": spec["dtype"] = "bfloat16"
            if mutation == "device": spec["devices"][0]["device"] = "mps"
            if mutation == "model": spec["model_id"] = "arbitrary/HF-model"
            if mutation == "duplicate": spec["devices"][1]["host"] = "host-0"
            with self.assertRaises(ValueError):
                plan_loading(spec, [])

    def test_disabled_device_and_exact_reserve_boundary(self):
        spec = request(1, (0, 24))
        data = profiles(spec)
        spec["devices"][0]["available_bytes"] = data[0]["samples"][0]["device_peak_bytes"] + 20
        spec["devices"][0]["reserve_bytes"] = 20
        plan_loading(spec, data)
        spec["devices"][0]["reserve_bytes"] += 1
        with self.assertRaises(LoadingInfeasible): plan_loading(spec, data)
        spec["devices"][0]["reserve_bytes"] = 0
        spec["devices"][0]["enabled"] = False
        with self.assertRaises(LoadingInfeasible): plan_loading(spec, data)

    def test_cli_emits_consumable_plan_and_refusal_has_no_output(self):
        root = Path(__file__).resolve().parents[4]
        cli = root / "scripts/models/qwen3_5_0_8b/cli/run.py"
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            spec = request()
            (folder / "request.json").write_text(json.dumps(spec))
            (folder / "profiles.json").write_text(json.dumps(profiles(spec)))
            command = [sys.executable, "-B", str(cli), "plan", "--request", str(folder / "request.json"),
                       "--profiles", str(folder / "profiles.json"), "--output", str(folder / "result")]
            run = subprocess.run(command, capture_output=True)
            self.assertEqual(run.returncode, 0, run.stderr)
            inspect = subprocess.run([sys.executable, "-B", str(cli), "inspect", "--plan", str(folder / "result/plan.json")], capture_output=True)
            self.assertEqual(inspect.returncode, 0, inspect.stderr)
            before = (folder / "result/plan.json").read_bytes()
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual((folder / "result/plan.json").read_bytes(), before)
            spec["dtype"] = "bfloat16"
            (folder / "request.json").write_text(json.dumps(spec))
            command[-1] = str(folder / "refused")
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertFalse((folder / "refused").exists())
