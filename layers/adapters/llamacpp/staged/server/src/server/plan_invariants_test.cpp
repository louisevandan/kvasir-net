// The logical batch is admitted once and llama.cpp owns its physical ubatch
// split. P4 preserves the selected KV topology and captures the exact physical
// sequence membership instead of changing llama.cpp policy.

#include "plan.hpp"

#include <algorithm>
#include <cassert>
#include <string>
#include <vector>

namespace {

staged::server::ParsedLlamaOptions parse(const std::vector<std::string> & tokens) {
    char program[] = "p4_staged_plan_invariants_test";
    char port[] = "--port";
    char port_value[] = "1";
    char bind[] = "--bind";
    char bind_value[] = "127.0.0.1";
    char * argv[] = {program, port, port_value, bind, bind_value, nullptr};
    staged::server::ParsedLlamaOptions parsed;
    std::string error;
    auto configured = tokens;
    if (std::find(configured.begin(), configured.end(), "--memory-topology") ==
        configured.end()) {
        configured.insert(configured.end(), {"--memory-topology", "discrete"});
    }
    assert(staged::server::parse_llama_options(
        5, argv, configured, &parsed, &error));
    return parsed;
}

} // namespace

int main() {
    char program[] = "p4_staged_plan_invariants_test";
    char port[] = "--port";
    char port_value[] = "1";
    char bind[] = "--bind";
    char bind_value[] = "127.0.0.1";
    char * argv[] = {program, port, port_value, bind, bind_value, nullptr};
    staged::server::ParsedLlamaOptions missing_topology;
    std::string error;
    assert(!staged::server::parse_llama_options(
        5, argv, {"--model", "not-loaded.gguf"}, &missing_topology, &error));
    assert(error.find("explicit --memory-topology") != std::string::npos);
    const auto shared = parse({"--model", "not-loaded.gguf",
                               "--memory-topology", "host-shared:0,2"});
    assert(shared.memory_topology.kind ==
           staged::llama_runtime::MemoryTopologyKind::HostShared);
    assert((shared.memory_topology.host_shared_devices ==
            std::vector<std::int32_t>{0, 2}));

    // A wider logical batch remains wider; llama.cpp splits it physically.
    const auto widened = parse({"--model", "not-loaded.gguf",
                                "--batch-size", "2048", "--ubatch-size", "512"});
    assert(widened.params.n_ubatch == 512);
    assert(widened.params.n_batch == 2048);

    // Keep llama.cpp's stock per-sequence cache default.
    const auto quiet = parse({"--model", "not-loaded.gguf"});
    assert(!quiet.params.kv_unified);
    assert(quiet.params.n_batch >= quiet.params.n_ubatch);

    // Both stock switches remain plan-owned; the adapter must not override
    // either one after parsing.
    const auto asked = parse({"--model", "not-loaded.gguf", "--kv-unified",
                              "--batch-size", "512", "--ubatch-size", "512"});
    assert(asked.params.kv_unified);
    assert(asked.params.n_batch == 512);
    assert(asked.params.n_batch == asked.params.n_ubatch);
    const auto separated = parse({"--model", "not-loaded.gguf", "--no-kv-unified",
                                  "--n-seq-max", "10"});
    assert(!separated.params.kv_unified);
    assert(separated.params.n_parallel == 10);

    // llama.cpp cannot create a context with a quantized V cache when flash
    // attention is explicitly disabled. Catch the invalid opaque plan before
    // loading a multi-shard model on every pipeline node.
    staged::server::ParsedLlamaOptions invalid;
    assert(!staged::server::parse_llama_options(
        5, argv,
        {"--model", "not-loaded.gguf", "--cache-type-v", "q8_0",
         "--flash-attn", "off", "--memory-topology", "discrete"},
        &invalid, &error));
    assert(error.find("quantized V cache requires flash attention") != std::string::npos);
    const auto portable = parse({"--model", "not-loaded.gguf",
                                 "--cache-type-v", "f16",
                                 "--flash-attn", "off"});
    assert(portable.params.cache_type_v == GGML_TYPE_F16);
    const auto compact = parse({"--model", "not-loaded.gguf",
                                "--cache-type-v", "q8_0",
                                "--flash-attn", "on"});
    assert(compact.params.cache_type_v == GGML_TYPE_Q8_0);
    const auto memory_plan = parse({"--model", "not-loaded.gguf",
                                    "--inspect-memory-plan"});
    assert(memory_plan.inspect_memory_plan);
    assert(!memory_plan.validate_plan);
    return 0;
}
