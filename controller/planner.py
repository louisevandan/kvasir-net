#!/usr/bin/env python3
"""linkcpp capacity planner.

Reads a GGUF model, computes per-layer weight bytes and per-token KV bytes, then
greedily assigns contiguous layer windows to nodes (pipeline order) within each
node's VRAM/RAM budget — optionally offloading MoE expert FFNs to a node's CPU RAM.

Emits a placement plan (layer window, KV owner, tensor-split, -ot directives per
node) or an INFEASIBLE verdict with the shortfall and suggested knobs.

Usage:
  python planner.py --model /models/foo.gguf \
      --node vram=8 ram=16 cores=6 --node vram=12 ram=24 cores=8 \
      --ctx 8192 --parallel 4 [--kv-bits 16] [--json]
"""
import argparse, json, os, re, sys
from itertools import combinations, permutations
from gguf import GGUFReader, GGUFValueType
from controller.ring_placement import boundary_candidates

GiB = 1024 ** 3
MiB = 1024 ** 2


def _scalar(field):
    # GGUFReader field -> python scalar/string
    if field is None:
        return None
    try:
        if field.types and field.types[0] == GGUFValueType.STRING:
            return str(field.contents())
        return field.contents()
    except Exception:
        # fallback for older gguf: read first part
        try:
            return field.parts[field.data[0]].tolist()
        except Exception:
            return None


def _layer_values(value, n_layer, default):
    """Normalize scalar and per-layer GGUF metadata to one value per block."""
    if isinstance(value, (list, tuple)):
        values = [int(item) for item in value]
        if len(values) == n_layer:
            return values
        if len(values) == 1:
            return values * n_layer
    return [int(value or default)] * n_layer


def read_model(path, shard_paths=None):
    """Read GGUF metadata and tensor tables, including split-model shards.

    Split GGUF part one commonly contains only metadata while the tensor tables
    live in later parts.  Planning from the primary file alone therefore makes
    a large model look like it has zero weights.
    """
    paths = list(dict.fromkeys(shard_paths or [path]))
    if path not in paths:
        paths.insert(0, path)
    readers = [GGUFReader(item) for item in paths]
    f = {k: _scalar(v) for k, v in readers[0].fields.items()}
    arch = f.get("general.architecture", "llama")

    def g(key, default=None):
        return f.get(f"{arch}.{key}", default)

    n_layer_all = int(g("block_count"))
    # llama.cpp excludes auxiliary NextN prediction blocks from the normal
    # decode graph. Ring placement must use that executable layer count too;
    # otherwise the final stage is assigned a block that decode never visits.
    n_layer_nextn = int(g("nextn_predict_layers", 0) or 0)
    n_layer = n_layer_all - n_layer_nextn
    if n_layer <= 0:
        raise ValueError("model has no executable transformer layers")
    n_head_by_layer = _layer_values(g("attention.head_count", 0), n_layer, 0)
    n_head = max(n_head_by_layer, default=0)
    n_head_kv_by_layer = _layer_values(g("attention.head_count_kv", n_head), n_layer, n_head)
    n_head_kv = max(n_head_kv_by_layer, default=n_head)
    n_embd = int(g("embedding_length", 0) or 0)
    key_len = g("attention.key_length")
    val_len = g("attention.value_length")
    head_dim_k = int(key_len) if key_len else (n_embd // n_head if n_head else 0)
    head_dim_v = int(val_len) if val_len else head_dim_k
    n_expert = int(g("expert_count", 0) or 0)

    # per-layer + boundary weight bytes from the tensor table
    weight_layer = [0] * n_layer
    expert_layer = [0] * n_layer
    body_tensor_names = [[] for _ in range(n_layer)]
    expert_tensor_names = [[] for _ in range(n_layer)]
    boundary = 0
    seen_tensors = set()
    for reader in readers:
        for t in reader.tensors:
            nb = int(t.n_bytes)
            name = t.name
            key = (name, nb)
            if key in seen_tensors:
                continue
            seen_tensors.add(key)
            if name.startswith("blk."):
                try:
                    i = int(name.split(".")[1])
                except (IndexError, ValueError):
                    boundary += nb
                    continue
                if 0 <= i < n_layer:
                    weight_layer[i] += nb
                    if "exps" in name:        # MoE expert FFN tensors
                        expert_layer[i] += nb
                        expert_tensor_names[i].append(name)
                    else:
                        body_tensor_names[i].append(name)
                else:
                    boundary += nb
            else:
                boundary += nb                # token_embd / output / output_norm

    return {
        "arch": arch,
        "n_layer": n_layer,
        "n_embd": n_embd,
        "n_head_kv": n_head_kv,
        "n_head_kv_by_layer": n_head_kv_by_layer,
        "head_dim_k": head_dim_k,
        "head_dim_v": head_dim_v,
        "n_expert": n_expert,
        "weight_layer": weight_layer,
        "expert_layer": expert_layer,
        "body_tensor_names": body_tensor_names,
        "expert_tensor_names": expert_tensor_names,
        "boundary_bytes": boundary,
        "total_weight": sum(weight_layer) + boundary,
    }


KV_CACHE_TYPE_BYTES = {
    "f32": 4.0,
    "f16": 2.0,
    "bf16": 2.0,
    "q8_0": 34.0 / 32.0,
    "q4_0": 18.0 / 32.0,
    "q4_1": 20.0 / 32.0,
    "iq4_nl": 18.0 / 32.0,
    "q5_0": 22.0 / 32.0,
    "q5_1": 24.0 / 32.0,
}


def _cache_type_from_bits(kv_bits):
    if kv_bits <= 4:
        return "q4_0"
    if kv_bits <= 5:
        return "q5_0"
    if kv_bits <= 8:
        return "q8_0"
    if kv_bits >= 32:
        return "f32"
    return "f16"


def cache_type_bytes(cache_type):
    key = str(cache_type or "f16").lower()
    if key not in KV_CACHE_TYPE_BYTES:
        raise ValueError(f"unsupported KV cache type: {cache_type}")
    return KV_CACHE_TYPE_BYTES[key]


def kv_bytes_per_layer(m, n_ctx, n_parallel, kv_bits=16, cache_type_k=None, cache_type_v=None):
    # K and V can be independently quantized in llama.cpp.
    type_k = str(cache_type_k or _cache_type_from_bits(kv_bits)).lower()
    type_v = str(cache_type_v or _cache_type_from_bits(kv_bits)).lower()
    heads_by_layer = m.get("n_head_kv_by_layer") or [m["n_head_kv"]] * m["n_layer"]
    return [
        int((
            m["head_dim_k"] * n_head_kv * cache_type_bytes(type_k)
            + m["head_dim_v"] * n_head_kv * cache_type_bytes(type_v)
        ) * n_ctx * n_parallel)
        for n_head_kv in heads_by_layer
    ]


PLACEMENT_STRATEGIES = {"balanced", "vram-weighted", "cpu-minimized", "ring-stage-vram-weighted"}


def plan(m, nodes, n_ctx, n_parallel, kv_bits=16, reserve_mib=1024, cache_type_k=None, cache_type_v=None,
         no_cpu_offload=False, placement_strategy="balanced", optimize_locality=False,
         master_ram_gib=None, _allow_subset_search=True):
    """Plan placement with an explicit distributed layer-boundary strategy.

    A uniform layer count can strand enough VRAM on both sides of a boundary to
    leave another expert block on CPU.  CPU expert execution is the dominant
    decode cost for this mode, so the CPU-minimized strategy evaluates
    contiguous alternatives and retains the smallest CPU-resident footprint.
    """
    if optimize_locality:
        placement_strategy = "cpu-minimized"
    if placement_strategy not in PLACEMENT_STRATEGIES:
        raise ValueError(f"unsupported placement strategy: {placement_strategy}")
    if placement_strategy == "ring-stage-vram-weighted":
        return _plan_ring_stages(
            m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
            cache_type_k, cache_type_v, no_cpu_offload,
        )
    if placement_strategy == "vram-weighted":
        targets = _vram_weighted_layer_targets(m["n_layer"], nodes, reserve_mib)
    else:
        targets = _balanced_layer_targets(m["n_layer"], len(nodes))
    baseline = _plan_with_targets(
        m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib, cache_type_k,
        cache_type_v, no_cpu_offload, targets,
    )
    if (placement_strategy != "cpu-minimized" or no_cpu_offload or len(nodes) < 2
            or not any(m["expert_layer"])):
        if baseline.get("feasible"):
            baseline["layer_split_strategy"] = placement_strategy
        result = _apply_master_ram_limit(baseline, master_ram_gib)
        return _fallback_to_node_subsets(
            result, m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
            cache_type_k, cache_type_v, no_cpu_offload, placement_strategy,
            master_ram_gib, _allow_subset_search,
        )

    # A balanced baseline often fails because the smallest final rank receives
    # too many layers.  Still search alternative contiguous boundaries: a
    # feasible CPU-offload plan may exist even when the initial split does not.
    best = baseline
    best_targets = targets
    # Two nodes are the common interactive topology and can be searched
    # exhaustively.  Larger topologies use coordinate descent over boundaries
    # so the search stays bounded as nodes are added.
    if len(nodes) == 2:
        candidates = ([first, m["n_layer"] - first]
                      for first in range(1, m["n_layer"]))
        for candidate_targets in candidates:
            candidate = _plan_with_targets(
                m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
                cache_type_k, cache_type_v, no_cpu_offload, candidate_targets,
            )
            if _plan_score(candidate) < _plan_score(best):
                best, best_targets = candidate, candidate_targets
    else:
        changed = True
        while changed:
            changed = False
            for boundary in range(len(nodes) - 1):
                left = sum(best_targets[:boundary])
                right = sum(best_targets[boundary + 2:])
                for split_at in range(left + 1, m["n_layer"] - right):
                    candidate_targets = list(best_targets)
                    candidate_targets[boundary] = split_at - left
                    candidate_targets[boundary + 1] = m["n_layer"] - right - split_at
                    candidate = _plan_with_targets(
                        m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
                        cache_type_k, cache_type_v, no_cpu_offload, candidate_targets,
                    )
                    if _plan_score(candidate) < _plan_score(best):
                        best, best_targets = candidate, candidate_targets
                        changed = True

    if best is not baseline:
        best["layer_split_strategy"] = "cpu-offload-optimized"
        best["balanced_tensor_split"] = targets
    else:
        best["layer_split_strategy"] = "balanced"
    result = _apply_master_ram_limit(best, master_ram_gib)
    return _fallback_to_node_subsets(
        result, m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
        cache_type_k, cache_type_v, no_cpu_offload, placement_strategy,
        master_ram_gib, _allow_subset_search,
    )


def _plan_ring_stages(m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
                      cache_type_k, cache_type_v, no_cpu_offload):
    """Find a contiguous all-rank plan for the node-local ring runtime.

    Unlike stock RPC, CPU-resident tensors and KV belong to the rank that owns
    the layer.  The plan therefore must retain every bound rank instead of
    falling back to a master-only or a reduced-node placement.  A model's
    output tensors live on the final rank, so rank order is part of placement.
    """
    if not nodes:
        return _infeasible(m, nodes, n_ctx, n_parallel, kv_bits, 0, 0, cache_type_k, cache_type_v)
    if len(nodes) > m["n_layer"]:
        result = _infeasible(m, nodes, n_ctx, n_parallel, kv_bits, 0, 0, cache_type_k, cache_type_v)
        result["reason"] = "more ring ranks than transformer layers"
        return result

    # linkcpp exposes at most five slots today, so exhaustive order search is
    # bounded (5! = 120) and avoids hard-coding a particular host as master.
    candidates = permutations(range(len(nodes))) if len(nodes) <= 5 else [tuple(range(len(nodes)))]
    best = None
    best_key = None
    best_failure = None
    best_failure_key = None
    search_exhaustive = True
    attempts = 0
    compute_aware = os.environ.get("LINKCPP_RING_COMPUTE_WEIGHTED", "1").strip().lower() \
        not in ("0", "false", "no")
    for order in candidates:
        # The first stage owns layer 0 AND hosts the ring coordinator (a full
        # llama-server), which phones cannot run. Only consider orderings whose
        # first node can coordinate.
        if not _can_coordinate(nodes[order[0]]):
            continue
        ordered_nodes = [nodes[index] for index in order]
        seed_targets = _vram_weighted_layer_targets(
            m["n_layer"], ordered_nodes, reserve_mib, compute_aware=compute_aware,
        )
        target_candidates, order_exhaustive = boundary_candidates(
            m["n_layer"], len(ordered_nodes), seed_targets,
        )
        search_exhaustive = search_exhaustive and order_exhaustive
        for targets in target_candidates:
            attempts += 1
            candidate = _plan_with_targets(
                m, ordered_nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
                cache_type_k, cache_type_v, no_cpu_offload, targets,
            )
            if not candidate.get("feasible"):
                failure_key = (
                    int(candidate.get("stuck_layer") or 0),
                    -int(candidate.get("stuck_node") or 0),
                )
                if best_failure is None or failure_key > best_failure_key:
                    best_failure, best_failure_key = candidate, failure_key
                continue
            if any(not item["n_layers"] for item in candidate["placement"]):
                continue
            for stage_index, placement in enumerate(candidate["placement"]):
                placement["node"] = order[stage_index]
                placement["stage_index"] = stage_index
            cpu_gib = sum(item.get("ram_used_gib", 0.0) for item in candidate["placement"])
            worst_ram_ratio = max(
                item.get("ram_used_gib", 0.0) / max(float(ordered_nodes[index].get("ram", 0.0)), 0.01)
                for index, item in enumerate(candidate["placement"])
            )
            # Feasibility is established before scoring. Prefer less CPU work,
            # then a larger output rank and lower peak RAM pressure.
            key = (cpu_gib, -float(ordered_nodes[-1].get("vram", 0.0)), worst_ram_ratio)
            if best is None or key < best_key:
                best, best_key = candidate, key
                best["stage_node_indexes"] = list(order)
                best["ring_rank_order"] = list(order)

    if best is None:
        kv_total = sum(kv_bytes_per_layer(
            m, n_ctx, n_parallel, kv_bits, cache_type_k, cache_type_v,
        ))
        result = best_failure or _infeasible(
            m, nodes, n_ctx, n_parallel, kv_bits, kv_total, 0,
            cache_type_k, cache_type_v,
        )
        if search_exhaustive:
            result["reason"] = "no all-rank contiguous placement fits the ring-stage budgets"
        else:
            result["reason"] = "no feasible placement found in the bounded ring boundary search"
        result["suggestions"] = list(dict.fromkeys([
            *result.get("suggestions", []),
            "increase the constrained rank's VRAM/RAM budget or lower context/KV precision",
        ]))
        result["ring_search_exhaustive"] = search_exhaustive
        result["ring_search_attempts"] = attempts
        return result
    best["layer_split_strategy"] = "ring-stage-vram-weighted"
    best["ring_search_exhaustive"] = search_exhaustive
    best["ring_search_attempts"] = attempts
    best["requires_stage_runtime"] = True
    best["master_ram_required_gib"] = 0.0
    best["master_mmap_resident_required_gib"] = 0.0
    return best


def _fallback_to_node_subsets(result, m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib,
                              cache_type_k, cache_type_v, no_cpu_offload, placement_strategy,
                              master_ram_gib, allow_subset_search):
    """Retry an infeasible all-node plan without a constrained worker.

    A bound node is available capacity, not a requirement to own a layer. A
    low-VRAM final rank can otherwise make the output boundary fail even when
    an earlier rank pair can hold the complete model.
    """
    if result.get("feasible") or not allow_subset_search or len(nodes) < 2:
        return result

    best = None
    best_key = None
    for count in range(len(nodes) - 1, 0, -1):
        for source_indexes in combinations(range(len(nodes)), count):
            candidate = plan(
                m, [nodes[index] for index in source_indexes], n_ctx, n_parallel,
                kv_bits, reserve_mib, cache_type_k, cache_type_v, no_cpu_offload,
                placement_strategy, master_ram_gib=master_ram_gib,
                _allow_subset_search=False,
            )
            if not candidate.get("feasible"):
                continue
            for placement in candidate.get("placement", []):
                placement["node"] = source_indexes[placement["node"]]
            candidate["active_node_indexes"] = list(source_indexes)
            candidate["layer_split_strategy"] = "active-subset-fallback"
            key = _plan_score(candidate) + (-count,)
            if best is None or key < best_key:
                best, best_key = candidate, key
    return best or result


def _apply_master_ram_limit(result, master_ram_gib):
    """Validate CPU tensors against the process that owns them at runtime.

    Stock llama.cpp RPC keeps CPU-resident tensors and RAM KV in the master
    process.  They cannot be charged to remote worker RAM merely because a
    layer's GPU tensors live on that worker.
    """
    if not result.get("feasible"):
        if master_ram_gib is None:
            return result
        budget = max(float(master_ram_gib), 0.0)
        # Even before a contiguous layer placement has been found, the master
        # must hold every byte that cannot fit in the aggregate GPU budget.
        # This is a lower bound: a real plan can require more RAM because a
        # layer cannot be split arbitrarily across workers.
        cpu_minimum = max(0.0, float(result.get("need_vram_gib") or 0.0)
                          - float(result.get("sum_vram_budget_gib") or 0.0))
        # mmap reads from a Docker bind mount fault model pages into the
        # master process even when their tensors are ultimately copied to GPU.
        # The full GGUF size is therefore a practical lower bound observed on
        # real large-model loads, not merely the CPU-offload tensor count.
        mmap_minimum = float(result.get("total_weight_gib") or 0.0)
        minimum = max(cpu_minimum, mmap_minimum)
        result["master_ram_budget_gib"] = round(budget, 2)
        result["master_cpu_ram_lower_bound_gib"] = round(cpu_minimum, 2)
        result["master_mmap_resident_lower_bound_gib"] = round(mmap_minimum, 2)
        result["master_ram_lower_bound_gib"] = round(minimum, 2)
        if minimum <= budget:
            return result
        result["reason"] = (f"requires at least {minimum:.2f} GiB master RAM for CPU/mmap-resident weights; "
                            f"only {budget:.2f} GiB is available")
        result["suggestions"] = [
            "increase master host RAM or use a stage runtime with node-local CPU weights",
            "add GPU VRAM to reduce the master-RAM lower bound",
        ]
        return result
    cpu_required = float((result.get("resource_totals") or {}).get("planned_ram_gib") or 0.0)
    mmap_required = float(result.get("total_weight_gib") or 0.0)
    required = max(cpu_required, mmap_required)
    result["master_cpu_ram_required_gib"] = round(cpu_required, 2)
    result["master_mmap_resident_required_gib"] = round(mmap_required, 2)
    result["master_ram_required_gib"] = round(required, 2)
    result["gpu_layers"] = int((result.get("model") or {}).get("n_layer") or 0)
    if master_ram_gib is None:
        return result
    budget = max(float(master_ram_gib), 0.0)
    result["master_ram_budget_gib"] = round(budget, 2)
    if required <= budget:
        return result
    result.update(
        feasible=False,
        reason=(f"requires {required:.2f} GiB master RAM for CPU/mmap-resident weights; "
                f"only {budget:.2f} GiB is available"),
        suggestions=[
            "reduce CPU-offloaded tensor footprint",
            "increase master host RAM or use a stage runtime with node-local CPU weights",
        ],
    )
    return result


def _balanced_layer_targets(total_layers, node_count):
    targets = []
    next_layer = 0
    for node_index in range(node_count):
        target = _target_layers(total_layers, next_layer, node_index, node_count)
        targets.append(target)
        next_layer += target
    return targets


# Relative compute throughput priors by backend, used to bias ring-stage layer
# splits toward where compute is fast. A phone with lots of unified memory but a
# slow GPU should not be handed a large layer window just because its VRAM budget
# is high — that makes it the ring bottleneck. Tunable via LINKCPP_RING_BACKEND_TPS
# ("metal=1.0,metal_mobile=0.35,cpu=0.2,...").
_RING_BACKEND_TPS = {
    "cuda": 1.0, "metal": 1.0, "vulkan": 0.6, "opencl": 0.5, "cpu": 0.2,
    "metal_mobile": 0.35, "opencl_mobile": 0.3,
}


def _load_ring_backend_tps():
    table = dict(_RING_BACKEND_TPS)
    raw = os.environ.get("LINKCPP_RING_BACKEND_TPS", "").strip()
    for pair in raw.split(","):
        if "=" in pair:
            key, _, value = pair.partition("=")
            try:
                table[key.strip().lower()] = float(value)
            except ValueError:
                pass
    return table


def _can_coordinate(node):
    """A node can host the ring coordinator (the full llama-server) unless it is
    a phone — mobile hosts run stage windows only, never the coordinator."""
    system = str((node.get("host_platform") or {}).get("system") or "").lower()
    return system not in ("ios", "android")


def _node_compute_factor(node):
    """A node's relative compute throughput prior in [~0.2, 1.0]. Phones (ios /
    android host) get the mobile tier for their backend; a measured perf_tps on
    the node, when present, overrides the prior (normalized to the desktop tier)."""
    table = _load_ring_backend_tps()
    backend = str((node.get("backend") or {}).get("backend_kind")
                  or node.get("backend_kind") or "cpu").lower()
    system = str((node.get("host_platform") or {}).get("system") or "").lower()
    is_mobile = system in ("ios", "android")
    if is_mobile and backend in ("metal", "gpu"):
        key = "metal_mobile"
    elif is_mobile and backend in ("opencl", "vulkan"):
        key = "opencl_mobile"
    else:
        key = backend
    prior = table.get(key, table.get(backend, 0.3))
    measured = node.get("perf_tps")
    if isinstance(measured, (int, float)) and measured > 0:
        # Normalize measured tok/s against a nominal desktop rate so the blend
        # stays in the same range as the priors.
        nominal = float(os.environ.get("LINKCPP_RING_NOMINAL_TPS", "60") or 60)
        return max(0.1, min(1.0, measured / nominal))
    return prior


def _vram_weighted_layer_targets(total_layers, nodes, reserve_mib, compute_aware=False):
    """Give each required node a contiguous window proportional to usable VRAM,
    optionally scaled by a compute-throughput prior (ring stages) so a slow phone
    does not become the pipeline bottleneck."""
    reserve = reserve_mib * MiB
    weights = [max(0.0, node.get("vram", 0.0) * GiB - reserve) for node in nodes]
    if compute_aware:
        weights = [w * _node_compute_factor(node) for w, node in zip(weights, nodes)]
    if not any(weights):
        return _balanced_layer_targets(total_layers, len(nodes))
    # Keep every bound node in the topology.  A node with no usable VRAM will
    # subsequently be rejected by the normal feasibility checks.
    targets = [1] * len(nodes)
    remaining = total_layers - len(nodes)
    if remaining <= 0:
        return targets
    total_weight = sum(weights)
    raw = [remaining * weight / total_weight for weight in weights]
    whole = [int(value) for value in raw]
    for index, value in enumerate(whole):
        targets[index] += value
    leftover = remaining - sum(whole)
    for index in sorted(range(len(nodes)), key=lambda i: (raw[i] - whole[i], weights[i]), reverse=True)[:leftover]:
        targets[index] += 1
    return targets


def _plan_score(result):
    if not result.get("feasible"):
        return (float("inf"),)
    placement = result.get("placement") or []
    cpu_bytes = sum((p.get("ffn_ram_gib", 0.0) + p.get("layer_body_ram_gib", 0.0))
                    for p in placement)
    # Prefer a lower CPU footprint first; then use more GPU expert capacity.
    gpu_expert = sum(p.get("ffn_vram_gib", 0.0) for p in placement)
    return (cpu_bytes, -gpu_expert)


def _plan_with_targets(m, nodes, n_ctx, n_parallel, kv_bits, reserve_mib, cache_type_k, cache_type_v,
                       no_cpu_offload, layer_targets):
    L = m["n_layer"]
    cache_type_k = str(cache_type_k or _cache_type_from_bits(kv_bits)).lower()
    cache_type_v = str(cache_type_v or _cache_type_from_bits(kv_bits)).lower()
    kv_by_layer = kv_bytes_per_layer(m, n_ctx, n_parallel, kv_bits, cache_type_k, cache_type_v)
    kv_total = sum(kv_by_layer)
    reserve = reserve_mib * MiB
    total_vram = sum(n["vram"] for n in nodes)
    total_ram = sum(n["ram"] for n in nodes)
    vram_after_reserve = sum(max(n["vram"] * GiB - reserve, 0) for n in nodes)
    kv_in_vram = kv_total <= vram_after_reserve
    if no_cpu_offload and not kv_in_vram:
        return _infeasible(m, nodes, n_ctx, n_parallel, kv_bits, kv_total, 0, cache_type_k, cache_type_v)
    assign = [None] * L          # node index per layer
    used = [{"vram": reserve, "ram": 0, "kv": 0, "kv_ram": 0, "body": 0, "ffn": 0,
             "body_ram": 0, "ffn_ram": 0, "boundary": 0, "layers": [],
             "ffn_layers": [], "body_layers": [], "ot_rules": []} for _ in nodes]

    ni = 0
    i = 0
    node_target = layer_targets[0]
    used[0]["vram"] += m["boundary_bytes"]          # embeddings on first node
    used[0]["boundary"] += m["boundary_bytes"]
    while i < L:
        budget_v = nodes[ni]["vram"] * GiB
        budget_r = nodes[ni]["ram"] * GiB
        ffn = m["expert_layer"][i]
        body = m["weight_layer"][i] - ffn

        kv_vram_l = kv_by_layer[i] if kv_in_vram else 0
        kv_ram_l = 0 if kv_in_vram else kv_by_layer[i]
        choice = _choose_layer_placement(
            m, i, used[ni], budget_v, budget_r, kv_vram_l, kv_ram_l, body, ffn,
            no_cpu_offload=no_cpu_offload,
        )
        if choice is None:
            ni += 1
            if ni >= len(nodes):
                return _infeasible(
                    m, nodes, n_ctx, n_parallel, kv_bits, kv_total, i,
                    cache_type_k, cache_type_v, stuck_node=ni,
                )
            node_target = layer_targets[ni]
            continue

        used[ni]["vram"] += choice["vram"]
        used[ni]["ram"] += choice["ram"]
        used[ni]["kv"] += kv_vram_l
        used[ni]["kv_ram"] += kv_ram_l
        used[ni]["body"] += choice["body_vram"]
        used[ni]["body_ram"] += choice["body_ram"]
        used[ni]["ffn"] += choice["ffn_vram"]
        used[ni]["ffn_ram"] += choice["ffn_ram"]
        if choice["body_ram"]:
            used[ni]["body_layers"].append(i)
        if choice["ffn_ram"]:
            used[ni]["ffn_layers"].append(i)
        used[ni]["ot_rules"].extend(choice["ot_rules"])
        used[ni]["layers"].append(i); assign[i] = ni
        i += 1
        if ni < len(nodes) - 1 and len(used[ni]["layers"]) >= node_target:
            ni += 1
            node_target = layer_targets[ni]
    last = assign[L - 1]
    used[last]["vram"] += m["boundary_bytes"]        # output/lm_head on last node
    used[last]["boundary"] += m["boundary_bytes"]
    if used[last]["vram"] > nodes[last]["vram"] * GiB:
        return _infeasible(m, nodes, n_ctx, n_parallel, kv_bits, kv_total, L - 1, cache_type_k, cache_type_v)
    # Preserve the requested allocator safety reserve.  Promoting CPU experts
    # after planning must not fill the physical GPU capacity back to 100%.
    _promote_moe_to_vram(m, used, nodes, reserve)

    per_node = []
    for idx, n in enumerate(nodes):
        ls = used[idx]["layers"]
        ffn_cpu = set(used[idx]["ffn_layers"])
        body_cpu = set(used[idx]["body_layers"])
        policies = []
        if any(i in ffn_cpu for i in ls):
            policies.append("ffn=RAM")
        if any(i in body_cpu for i in ls):
            policies.append("body=RAM")
        per_node.append({
            "node": idx,
            "layers": [ls[0], ls[-1] + 1] if ls else None,
            "n_layers": len(ls),
            "vram_used_gib": round(used[idx]["vram"] / GiB, 2),
            "vram_budget_gib": n["vram"],
            "ram_used_gib": round(used[idx]["ram"] / GiB, 2),
            "kv_vram_gib": round(used[idx]["kv"] / GiB, 2),
            "kv_ram_gib": round(used[idx]["kv_ram"] / GiB, 2),
            "layer_body_vram_gib": round(used[idx]["body"] / GiB, 2),
            "layer_body_ram_gib": round(used[idx]["body_ram"] / GiB, 2),
            "ffn_vram_gib": round(used[idx]["ffn"] / GiB, 2),
            "ffn_ram_gib": round(used[idx]["ffn_ram"] / GiB, 2),
            "boundary_vram_gib": round(used[idx]["boundary"] / GiB, 2),
            "offload_ram_gib": round(used[idx]["ram"] / GiB, 2),
            "experts_on_cpu": any(i in ffn_cpu for i in ls),
            "body_on_cpu": any(i in body_cpu for i in ls),
            "offload_policy": ",".join(policies) if policies else "none",
            "ot": ",".join(used[idx]["ot_rules"]) if used[idx]["ot_rules"] else None,
            "confidence": 0.75,
        })
    active = [p for p in per_node if p["n_layers"] > 0]
    split = [p["n_layers"] for p in active]
    planned_vram = sum(p["vram_used_gib"] for p in per_node)
    planned_ram = sum(p["ram_used_gib"] for p in per_node)
    return {
        "feasible": True,
        "model": {k: m.get(k) for k in ("arch", "n_layer", "n_embd", "n_head_kv", "n_expert")},
        "total_weight_gib": round(m["total_weight"] / GiB, 2),
        "is_moe": any(m["expert_layer"]),
        "resource_totals": {
            "vram_budget_gib": round(total_vram, 2),
            "ram_budget_gib": round(total_ram, 2),
            "planned_vram_gib": round(planned_vram, 2),
            "planned_ram_gib": round(planned_ram, 2),
            "reserve_per_node_gib": round(reserve / GiB, 2),
        },
        "kv_total_gib": round(kv_total / GiB, 2),
        "kv_per_layer_gib": round((kv_total / L) / GiB, 4),
        "kv_cache_location": "vram" if kv_in_vram else "ram",
        "kv_offload_enabled": kv_in_vram,
        "cache_type_k": cache_type_k,
        "cache_type_v": cache_type_v,
        "flash_attention": True,
        "calibration": {
            "kv_cache_gib": round(kv_total / GiB, 2),
            "source": "metadata",
            "confidence": 0.75,
        },
        "n_ctx": n_ctx, "n_parallel": n_parallel, "kv_bits": kv_bits,
        "no_cpu_offload": bool(no_cpu_offload),
        "nodes_used": len(active),
        "tensor_split": split,
        "placement": per_node,
    }


def _tensor_pattern(name):
    return re.escape(name)


def _ffn_expert_rules(m, layer):
    names = m.get("expert_tensor_names", [[]])[layer] if m.get("expert_tensor_names") else []
    if not names:
        return []
    return [f"blk\\.{layer}\\.ffn_(up|down|gate)_(ch|)exps=CPU"]


def _body_rules(m, layer):
    names = m.get("body_tensor_names", [[]])[layer] if m.get("body_tensor_names") else []
    return [f"{_tensor_pattern(name)}=CPU" for name in names]


def _target_layers(total_layers, next_layer, node_index, node_count):
    remaining_layers = total_layers - next_layer
    remaining_nodes = max(1, node_count - node_index)
    return max(1, (remaining_layers + remaining_nodes - 1) // remaining_nodes)


def _choose_layer_placement(m, layer, current, budget_v, budget_r, kv_vram_l, kv_ram_l, body, ffn,
                            no_cpu_offload=False):
    # Node-local priority: keep KV in VRAM, then dense layer body, then MoE experts.
    gpu_candidate = {
        "name": "gpu",
        "vram": kv_vram_l + body + ffn,
        "ram": kv_ram_l,
        "body_vram": body,
        "body_ram": 0,
        "ffn_vram": ffn,
        "ffn_ram": 0,
        "ot_rules": [],
    }
    if no_cpu_offload:
        candidates = [gpu_candidate]
    else:
        candidates = []
        if ffn:
            candidates.append({
                "name": "ffn_cpu",
                "vram": kv_vram_l + body,
                "ram": kv_ram_l + ffn,
                "body_vram": body,
                "body_ram": 0,
                "ffn_vram": 0,
                "ffn_ram": ffn,
                "ot_rules": _ffn_expert_rules(m, layer),
            })
        candidates.append(gpu_candidate)
    if not no_cpu_offload and body:
        candidates.append({
            "name": "layer_cpu",
            "vram": kv_vram_l,
            "ram": kv_ram_l + body + ffn,
            "body_vram": 0,
            "body_ram": body,
            "ffn_vram": 0,
            "ffn_ram": ffn,
            "ot_rules": _body_rules(m, layer) + _ffn_expert_rules(m, layer),
        })
    for candidate in candidates:
        if current["vram"] + candidate["vram"] <= budget_v and current["ram"] + candidate["ram"] <= budget_r:
            return candidate
    return None


def _promote_moe_to_vram(m, used, nodes, reserve=0):
    for idx, node_used in enumerate(used):
        budget_v = max(0, nodes[idx]["vram"] * GiB - reserve)
        remaining_ffn_layers = []
        promoted = set()
        body_cpu = set(node_used["body_layers"])
        for layer in node_used["ffn_layers"]:
            ffn = m["expert_layer"][layer]
            if layer not in body_cpu and node_used["vram"] + ffn <= budget_v:
                node_used["vram"] += ffn
                node_used["ram"] -= ffn
                node_used["ffn"] += ffn
                node_used["ffn_ram"] -= ffn
                promoted.add(layer)
            else:
                remaining_ffn_layers.append(layer)
        if promoted:
            promoted_rules = set()
            for layer in promoted:
                promoted_rules.update(_ffn_expert_rules(m, layer))
            node_used["ot_rules"] = [rule for rule in node_used["ot_rules"] if rule not in promoted_rules]
        node_used["ffn_layers"] = remaining_ffn_layers


def _infeasible(m, nodes, n_ctx, n_parallel, kv_bits, kv_total, stuck_layer,
                cache_type_k=None, cache_type_v=None, stuck_node=None):
    need = m["total_weight"] + kv_total
    budget = sum(n["vram"] for n in nodes) * GiB
    ram = sum(n["ram"] for n in nodes) * GiB
    reserve = 1024 * MiB
    vram_after_reserve = sum(max(n["vram"] * GiB - reserve, 0) for n in nodes)
    sug = []
    if kv_total > 0.3 * need:
        sug.append("reduce --parallel or --ctx (KV dominates)")
        sug.append("use --kv-bits 8 (halves KV)")
    if any(m["expert_layer"]):
        sug.append("offload more experts to CPU (needs node RAM)")
    sug.append("add more nodes / increase vram budgets")
    return {
        "feasible": False,
        "reason": f"ran out of node VRAM/RAM at layer {stuck_layer}",
        "stuck_layer": int(stuck_layer),
        "stuck_node": stuck_node,
        "total_weight_gib": round(m["total_weight"] / GiB, 2),
        "need_vram_gib": round(need / GiB, 2),
        "sum_vram_budget_gib": round(budget / GiB, 2),
        "sum_ram_budget_gib": round(ram / GiB, 2),
        "kv_total_gib": round(kv_total / GiB, 2),
        "is_moe": any(m["expert_layer"]),
        "kv_cache_location": "vram" if kv_total <= vram_after_reserve else "ram",
        "cache_type_k": cache_type_k or _cache_type_from_bits(kv_bits),
        "cache_type_v": cache_type_v or _cache_type_from_bits(kv_bits),
        "flash_attention": True,
        "resource_totals": {
            "vram_budget_gib": round(budget / GiB, 2),
            "ram_budget_gib": round(ram / GiB, 2),
            "planned_vram_gib": None,
            "planned_ram_gib": None,
            "reserve_per_node_gib": round(reserve / GiB, 2),
        },
        "suggestions": sug,
    }


def _parse_node(tokens):
    d = {"vram": 0.0, "ram": 0.0, "cores": 0}
    for t in tokens:
        k, _, v = t.partition("=")
        d[k] = float(v) if k in ("vram", "ram") else int(v)
    return d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--node", action="append", nargs="+", required=True,
                    help="per node: vram=8 ram=16 cores=6 (GiB)")
    ap.add_argument("--ctx", type=int, default=4096)
    ap.add_argument("--parallel", type=int, default=1)
    ap.add_argument("--kv-bits", type=int, default=16)
    ap.add_argument("--cache-type-k", default=None)
    ap.add_argument("--cache-type-v", default=None)
    ap.add_argument("--reserve-mib", type=int, default=1024)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--emit-launch", action="store_true",
                    help="print shell env for the launcher (TENSOR_SPLIT, MASTER_OT)")
    a = ap.parse_args()
    nodes = [_parse_node(n) for n in a.node]
    m = read_model(a.model)
    result = plan(m, nodes, a.ctx, a.parallel, a.kv_bits, a.reserve_mib, a.cache_type_k, a.cache_type_v)
    if a.json:
        print(json.dumps(result, indent=2)); return
    if a.emit_launch:
        if not result["feasible"]:
            print(f"FEASIBLE=0\nREASON={result['reason']}"); sys.exit(2)
        print("FEASIBLE=1")
        print("TENSOR_SPLIT=" + ",".join(str(x) for x in result["tensor_split"]))
        # master is node0; its -ot (if any) is applied on the master graph
        m0 = result["placement"][0]
        print("MASTER_OT=" + (m0["ot"] or ""))
        for p in result["placement"]:
            if p["n_layers"]:
                print(f"# node{p['node']} layers {p['layers']} vram {p['vram_used_gib']} GiB"
                      + (f" -ot {p['ot']}" if p["ot"] else ""))
        return
    if not result["feasible"]:
        print(f"INFEASIBLE: {result['reason']}")
        print(f"  need VRAM {result['need_vram_gib']} GiB vs budget {result['sum_vram_budget_gib']} GiB"
              f" (KV {result['kv_total_gib']} GiB)")
        for s in result["suggestions"]:
            print(f"  - {s}")
        sys.exit(2)
    print(f"FEASIBLE  {result['model']['arch']}  L={result['model']['n_layer']}  "
          f"weights {result['total_weight_gib']} GiB  KV {result['kv_total_gib']} GiB "
          f"(ctx {result['n_ctx']} × par {result['n_parallel']}, kv{result['kv_bits']})")
    print(f"  nodes used: {result['nodes_used']}   tensor-split: {result['tensor_split']}")
    for p in result["placement"]:
        if not p["n_layers"]:
            continue
        ot = f"  -ot {p['ot']}" if p["ot"] else ""
        print(f"  node{p['node']}: layers {p['layers'][0]}..{p['layers'][1]}  "
              f"VRAM {p['vram_used_gib']}/{p['vram_budget_gib']} GiB  "
              f"RAM {p['ram_used_gib']} GiB{ot}")


if __name__ == "__main__":
    main()
