#include "stage_memory_plan.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <string>

void run_stage_memory_plan_tests() {
    staged::llama_runtime::StageMemoryPlan planned;
    planned.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    planned.complete = true;
    planned.fits_current_free = true;
    planned.execution_shape = {12'800, 1'280, 512, 512, 10, false};
    planned.entries.push_back({
        "device", 0, "backend0", "test backend", 4096, 8192, 1024, 512, 256});
    planned.entries.push_back({
        "host", -1, "host", "host memory", 16384, 32768, 2048, 1024, 512});
    auto actual = planned;
    std::string error;
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
}
