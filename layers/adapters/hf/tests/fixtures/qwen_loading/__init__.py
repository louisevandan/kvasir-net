"""Synthetic envelopes for solver tests; never measurements or deployment profiles."""
from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION, TORCH_VERSION, TRANSFORMERS_VERSION
from p4hfadapter.models.qwen3_5_0_8b.planning import profile_contract, profile_checkpoint_sha, source_identity


def request(devices=2, cuts=(0, 8, 16, 24)):
    return {"schema": "qwen3.5-0.8b-loading-request-v1", "model_id": MODEL_ID, "revision": REVISION,
            "dtype": "float32", "quantization": "none", "limits": {"context": 64, "max_requests": 2, "max_new_tokens": 8},
            "prefill_chunk": 16, "cuts": list(cuts), "minimum_hosts": 1,
            "hosts": [{"id": f"host-{i}", "available_bytes": 10000, "reserve_bytes": 0} for i in range(devices)],
            "devices": [{"id": f"gpu-{i}", "host": f"host-{i}", "device": "cuda:0", "available_bytes": 200,
                         "reserve_bytes": 0, "enabled": True} for i in range(devices)],
            "slots": [{"node_id": f"node-{i}", "device_id": f"gpu-{i}"} for i in range(devices)]}


def profiles(spec, cost=None):
    result = []
    for number, device in enumerate(spec["devices"]):
        samples = []
        for i, start in enumerate(spec["cuts"][:-1]):
            for end in spec["cuts"][i + 1:]:
                weight = (end - start) * 4 + (40 if start == 0 or end == 24 else 0)
                sample = {"layers": [start, end], "weight_bytes": weight, "cache_bytes": (end - start),
                          "host_peak_bytes": 100, "device_peak_bytes": weight + (end - start),
                          "service_ms": (end - start) * (number + 1), "active_after_release": 0}
                if cost:
                    sample.update(cost(number, start, end))
                samples.append(sample)
        result.append({"schema": "qwen3.5-0.8b-stage-profile-v1", "model_id": MODEL_ID, "model_revision": REVISION,
                       "contract": profile_contract(spec), "source_sha256": source_identity(),
                       "runtime": {"torch": TORCH_VERSION, "transformers": TRANSFORMERS_VERSION},
                       "binding": {key: device[key] for key in ("id", "host", "device")},
                       "checkpoint_sha256": profile_checkpoint_sha(), "measurement_host": "synthetic-test-fixture", "samples": samples})
    return result
