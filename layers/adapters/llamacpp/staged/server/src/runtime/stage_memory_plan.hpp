#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "common.h"
#include "llama.h"

namespace staged::llama_runtime {

enum class MemoryTopologyKind {
    Unspecified,
    Discrete,
    HostShared,
};

struct MemoryTopology final {
    MemoryTopologyKind kind = MemoryTopologyKind::Unspecified;
    // Backend-neutral llama_model device indices whose allocations consume
    // the same physical pool as the host entry. This supports Metal and
    // integrated Vulkan/SYCL/OpenCL without inferring a CUDA-shaped topology.
    std::vector<std::int32_t> host_shared_devices;

    [[nodiscard]] bool operator==(const MemoryTopology & other) const noexcept {
        return kind == other.kind && host_shared_devices == other.host_shared_devices;
    }
};

struct LoadConfig {
    std::string model_path;
    int32_t layer_begin = 0;
    int32_t layer_end = 0;
    int32_t kv_gpu_layer_start = 0;
    int32_t kv_gpu_layer_end = 0;
    std::string model_identity;
    std::string kv_root;
    MemoryTopology memory_topology;
    bool mtp_ownership_probe = false;
};

struct StageMemoryEntry final {
    std::string scope;
    int32_t index = -1;
    std::string name;
    std::string description;
    std::int64_t free = 0;
    std::int64_t total = 0;
    std::uint64_t model = 0;
    std::uint64_t context = 0;
    std::uint64_t compute = 0;

    [[nodiscard]] std::uint64_t required() const noexcept {
        return model + context + compute;
    }
};

struct StageExecutionShape final {
    std::uint32_t n_ctx = 0;
    std::uint32_t n_ctx_seq = 0;
    std::uint32_t n_batch = 0;
    std::uint32_t n_ubatch = 0;
    std::uint32_t n_seq_max = 0;
    bool kv_unified = false;

    [[nodiscard]] bool operator==(const StageExecutionShape & other) const noexcept {
        return n_ctx == other.n_ctx && n_ctx_seq == other.n_ctx_seq &&
            n_batch == other.n_batch && n_ubatch == other.n_ubatch &&
            n_seq_max == other.n_seq_max && kv_unified == other.kv_unified;
    }
};

struct StageMemoryPlan final {
    MemoryTopology memory_topology;
    StageExecutionShape execution_shape;
    std::vector<StageMemoryEntry> entries;
    bool complete = false;
    bool fits_current_free = false;
};

[[nodiscard]] llama_model_params make_stage_model_params(
    common_params & params,
    const LoadConfig & config);

[[nodiscard]] llama_context_params make_stage_context_params(
    const common_params & params);

// This is the one private llama.cpp compatibility boundary for allocation
// planning. It deliberately uses backend buffer/device identities rather than
// naming CUDA, Vulkan, HIP, Metal, OpenCL, or any other concrete backend.
[[nodiscard]] bool inspect_stage_memory(
    common_params params,
    const LoadConfig & config,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool inspect_stage_memory_with_initialized_backend(
    common_params params,
    const LoadConfig & config,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool measure_stage_memory(
    const llama_model * model,
    const llama_context * context,
    const llama_context * speculative_context,
    const MemoryTopology & memory_topology,
    bool kv_unified,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool stage_memory_plan_fits_current_free(
    const StageMemoryPlan & plan);

[[nodiscard]] bool same_stage_memory_allocation(
    const StageMemoryPlan & planned,
    const StageMemoryPlan & actual,
    std::string * error = nullptr);

[[nodiscard]] std::string serialize_stage_memory_plan(const StageMemoryPlan & plan);

} // namespace staged::llama_runtime
