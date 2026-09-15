#include "stage_memory_plan.hpp"
#include "ggml-backend.h"
#include <iostream>

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <limits>
#include <string>

void run_stage_memory_plan_tests() {
    ggml_backend_load_all();
    auto * cpu = ggml_backend_dev_by_type(GGML_BACKEND_DEVICE_TYPE_CPU);
    assert(cpu != nullptr);
    assert(staged::llama_runtime::stage_buffer_uses_host_memory(ggml_backend_dev_buffer_type(cpu)));
    const auto extras = reinterpret_cast<ggml_backend_dev_get_extra_bufts_t>(
        ggml_backend_reg_get_proc_address(ggml_backend_dev_backend_reg(cpu), "ggml_backend_dev_get_extra_bufts"));
    std::size_t special_count = 0;
    if (extras != nullptr) {
        for (auto * types = extras(cpu); types != nullptr && *types != nullptr; ++types) {
            assert(ggml_backend_dev_type(ggml_backend_buft_get_device(*types)) == GGML_BACKEND_DEVICE_TYPE_CPU);
            assert(staged::llama_runtime::stage_buffer_uses_host_memory(*types));
            if (!ggml_backend_buft_is_host(*types)) {
                ++special_count;
                std::cout << "HOST_MEMORY_EXTRA_BUFFER " << ggml_backend_buft_name(*types) << '\n';
            }
        }
    }
    std::cout << "HOST_MEMORY_EXTRA_BUFFER_COUNT " << special_count << '\n';
    staged::llama_runtime::StageMemoryPlan planned;
    planned.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    planned.complete = true;
    planned.fits_current_free = true;
    planned.execution_shape = {12'800, 1'280, 512, 512, 10, false};
    planned.physical_result_payload_bytes = 1'048'576;
    planned.physical_result_tensor_count = 2;
    std::string error;
    assert(staged::llama_runtime::derive_max_physical_result_bytes(
        planned.execution_shape, planned.physical_result_payload_bytes,
        planned.physical_result_tensor_count, false,
        &planned.max_physical_result_bytes, &error));
    assert(planned.max_physical_result_bytes == 551'745'036);
    planned.entries.push_back({
        "device", 0, "backend0", "test backend", 4096, 8192, 1024, 512, 256});
    planned.entries.push_back({
        "host", -1, "host", "host memory", 16384, 32768, 2048, 1024, 512});
    auto actual = planned;
    assert(staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.empty());

    actual.entries[0].compute += 1;
    assert(!staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.find("entry 0") != std::string::npos);

    const auto json = staged::llama_runtime::serialize_stage_memory_plan(planned);
    assert(json.find("\"required\":1792") != std::string::npos);
    assert(json.find("\"fits_current_free\":true") != std::string::npos);
    assert(json.find("\"n_ctx_seq\":1280") != std::string::npos);
    assert(json.find("\"kv_unified\":false") != std::string::npos);
    assert(json.find("\"physical_result_payload_bytes\":1048576") != std::string::npos);
    assert(json.find("\"physical_result_tensor_count\":2") != std::string::npos);
    assert(json.find("\"max_physical_result_bytes\":551745036") != std::string::npos);

    actual = planned;
    actual.physical_result_payload_bytes += 1;
    assert(!staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.find("physical result bounds") != std::string::npos);

    actual = planned;
    actual.max_physical_result_bytes += 1;
    assert(!staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.find("physical result bounds") != std::string::npos);

    std::uint64_t speculative_bound = 0;
    assert(staged::llama_runtime::derive_max_physical_result_bytes(
        planned.execution_shape, planned.physical_result_payload_bytes,
        planned.physical_result_tensor_count, true, &speculative_bound, &error));
    assert(speculative_bound == 2'163'314'188);
    auto invalid_shape = planned.execution_shape;
    invalid_shape.n_ubatch = 0;
    assert(!staged::llama_runtime::derive_max_physical_result_bytes(
        invalid_shape, 0, 0, false, &speculative_bound, &error));
    assert(error == "invalid physical result bound inputs");
    assert(!staged::llama_runtime::derive_max_physical_result_bytes(
        planned.execution_shape, std::numeric_limits<std::uint64_t>::max(),
        planned.physical_result_tensor_count, false, &speculative_bound, &error));
    assert(error == "physical result bound overflows uint64");

    actual = planned;
    actual.execution_shape.n_ctx_seq += 256;
    assert(!staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.find("execution shapes") != std::string::npos);

    // Host-shared accelerators consume their device working-set ceiling and
    // the same physical pool as CPU allocations.
    planned.entries[1].free = 4000;
    assert(staged::llama_runtime::stage_memory_plan_fits_current_free(planned));
    planned.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::HostShared;
    planned.memory_topology.host_shared_devices = {0};
    assert(!staged::llama_runtime::stage_memory_plan_fits_current_free(planned));
    planned.entries[1].free = 6000;
    assert(staged::llama_runtime::stage_memory_plan_fits_current_free(planned));

    actual = planned;
    actual.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    actual.memory_topology.host_shared_devices.clear();
    assert(!staged::llama_runtime::same_stage_memory_allocation(
        planned, actual, &error));
    assert(error.find("topologies") != std::string::npos);

    using namespace staged::llama_runtime;
    LoadConfig config;
    config.layer_begin = 2;
    config.layer_end = 5;
    config.layer_device_expectations = {{3, 5, "backend0"}, {2, 3, "CPU"}};
    StageMemoryPlan placement;
    placement.layer_device_query_supported = true;
    placement.layer_default_devices = {{2, "CPU"}, {3, "backend0"}, {4, "backend0"}};
    assert(validate_stage_layer_devices(config, placement, &error));
    const auto before = serialize_stage_memory_plan(placement);
    config.layer_device_expectations = {{2, 5, "backend0"}};
    assert(!validate_stage_layer_devices(config, placement, &error));
    assert(error.find("layer=2 expected=backend0 actual=CPU") != std::string::npos);
    assert(serialize_stage_memory_plan(placement) == before);
    assert(before.find("\"layer\":2,\"device\":\"CPU\"") != std::string::npos);
    auto changed = placement;
    changed.layer_default_devices[0].device = "backend0";
    assert(!same_stage_memory_allocation(placement, changed, &error));
    assert(error.find("default layer devices") != std::string::npos);
    changed = placement;
    changed.layer_default_devices.pop_back();
    assert(!validate_stage_layer_devices(config, changed, &error));
    changed = placement;
    changed.layer_default_devices[1].layer = 2;
    config.layer_device_expectations = {{2, 3, "CPU"}, {3, 5, "backend0"}};
    assert(!validate_stage_layer_devices(config, changed, &error));
    changed = placement;
    changed.layer_device_query_supported = false;
    assert(!validate_stage_layer_devices(config, changed, &error));
    assert(error.find("query support") != std::string::npos);
    // An unasserted legacy plan remains explicit about missing query support.
    config.layer_device_expectations.clear();
    assert(validate_stage_layer_devices(config, changed, &error));
    for (const std::vector<LayerDeviceExpectation> bad : {
            std::vector<LayerDeviceExpectation>{{2, 4, "CPU"}},
            {{2, 4, "CPU"}, {3, 5, "backend0"}},
            {{2, 3, "CPU"}, {4, 5, "backend0"}},
            {{2, 6, "CPU"}}, {{2, 5, ""}}}) {
        assert(!validate_layer_device_expectations(bad, 2, 5, &error));
    }
}
