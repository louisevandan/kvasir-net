#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "compat/p4_llama_compat.hpp"
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

struct LayerDeviceExpectation final {
    std::int32_t begin = 0;
    std::int32_t end = 0;
    std::string device;
};

struct LayerDefaultDevice final {
    std::int32_t layer = 0;
    std::string device;
    [[nodiscard]] bool operator==(const LayerDefaultDevice & other) const noexcept {
        return layer == other.layer && device == other.device;
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
    std::vector<LayerDeviceExpectation> layer_device_expectations;
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
    // Default repeating-layer assignment, not tensor overrides, KV placement,
    // output/embedding placement or proof of where every operator executes.
    bool layer_device_query_supported = false;
    bool layer_device_expectations_checked = false;
    std::vector<LayerDefaultDevice> layer_default_devices;
    std::uint64_t physical_result_payload_bytes = 0;
    std::uint32_t physical_result_tensor_count = 0;
    std::uint64_t max_physical_result_bytes = 0;
    bool complete = false;
    bool fits_current_free = false;
};

[[nodiscard]] bool validate_layer_device_expectations(
    const std::vector<LayerDeviceExpectation> & expectations,
    std::int32_t begin, std::int32_t end, std::string * error = nullptr);

[[nodiscard]] bool validate_stage_layer_devices(
    const LoadConfig & config, const StageMemoryPlan & measured,
    std::string * error = nullptr);

[[nodiscard]] bool measure_stage_layer_devices(
    const llama_model * model, const LoadConfig & config,
    StageMemoryPlan * result, std::string * error = nullptr);

[[nodiscard]] llama_model_params make_stage_model_params(
    p4_llama_compat::LlamaPlan & params,
    const LoadConfig & config);

[[nodiscard]] llama_context_params make_stage_context_params(
    const p4_llama_compat::LlamaPlan & params);

// This is the one private llama.cpp compatibility boundary for allocation
// planning. It deliberately uses backend buffer/device identities rather than
// naming CUDA, Vulkan, HIP, Metal, OpenCL, or any other concrete backend.
[[nodiscard]] bool inspect_stage_memory(
    const p4_llama_compat::LlamaPlan & params,
    const LoadConfig & config,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool inspect_stage_memory_with_initialized_backend(
    const p4_llama_compat::LlamaPlan & params,
    const LoadConfig & config,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool measure_stage_memory(
    const llama_model * model,
    const llama_context * context,
    const llama_context * speculative_context,
    const MemoryTopology & memory_topology,
    bool kv_unified,
    bool speculative,
    StageMemoryPlan * result,
    std::string * error = nullptr);

[[nodiscard]] bool stage_memory_plan_fits_current_free(
    const StageMemoryPlan & plan);

// Physical allocation ownership differs from direct CPU access to a buffer's
// tensor representation (for example a CPU-owned repacked tensor).
[[nodiscard]] bool stage_buffer_uses_host_memory(ggml_backend_buffer_type_t buffer_type);

[[nodiscard]] bool same_stage_memory_allocation(
    const StageMemoryPlan & planned,
    const StageMemoryPlan & actual,
    std::string * error = nullptr);

[[nodiscard]] bool derive_max_physical_result_bytes(
    const StageExecutionShape & execution_shape,
    std::uint64_t payload_bytes_per_capsule,
    std::uint32_t tensors_per_capsule,
    bool speculative,
    std::uint64_t * result,
    std::string * error = nullptr);

[[nodiscard]] std::string serialize_stage_memory_plan(const StageMemoryPlan & plan);

} // namespace staged::llama_runtime
