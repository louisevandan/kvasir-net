#include "llama_stage_runtime.hpp"
#include "compat/p4_llama_compat_internal.hpp"

#include <cassert>
#include <cstdlib>
#include <iostream>
#include <string>
#include <utility>

int main() {
    const char * model = std::getenv("P4_STAGED_MTP_MODEL");
    if (model == nullptr || *model == '\0') {
        std::cout << "SKIP: P4_STAGED_MTP_MODEL is not set\n";
        return 0;
    }

    const char * end_value = std::getenv("P4_STAGED_MTP_LAYER_END");
    const int layer_end = end_value == nullptr ? 24 : std::atoi(end_value);
    assert(layer_end > 0);

    p4_llama_compat::LlamaPlan plan;
    common_params & params = p4_llama_compat::plan_params(plan);
    params.model.path = model;
    params.n_ctx = 512;
    params.n_batch = 64;
    params.speculative.types = {COMMON_SPECULATIVE_TYPE_DRAFT_MTP};

    staged::llama_runtime::LoadConfig config;
    config.model_path = model;
    config.layer_begin = 0;
    config.layer_end = layer_end;
    config.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    config.mtp_ownership_probe = true;

    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    if (!runtime.load(std::move(plan), config, &error)) {
        std::cerr << "FAIL: MTP auxiliary ownership probe could not load: "
                  << error << '\n';
        return 1;
    }

    assert(runtime.loaded());
    assert(runtime.mtp_context() != nullptr);
    if (std::getenv("P4_STAGED_MTP_EXECUTE") != nullptr) {
        staged::llama_runtime::MtpHopObservation observation;
        if (!runtime.execute_mtp_hop({11, 872, 198, 220}, &observation, &error)) {
            std::cerr << "FAIL: MTP draft/verify/accept round failed: " << error
                      << '\n';
            return 1;
        }
        assert(observation.drafted_tokens >= 1);
        assert(observation.accepted_tokens <= observation.drafted_tokens);
        std::cout << "PASS: MTP round drafted=" << observation.drafted_tokens
                  << " accepted=" << observation.accepted_tokens
                  << " eos=" << (observation.eos ? 1 : 0) << '\n';
    }
    runtime.unload();
    std::cout << "PASS: MTP auxiliary ownership probe loaded and unloaded the "
                 "tail stage\n";
    return 0;
}
