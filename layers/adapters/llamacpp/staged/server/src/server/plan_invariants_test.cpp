// The logical batch is admitted once and llama.cpp owns its physical ubatch
// split. A unified cache preserves sequence membership across that split.

#include "plan.hpp"

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
    assert(staged::server::parse_llama_options(5, argv, tokens, &parsed, &error));
    return parsed;
}

} // namespace

int main() {
    // A wider logical batch remains wider; llama.cpp splits it physically.
    const auto widened = parse({"--model", "not-loaded.gguf",
                                "--batch-size", "2048", "--ubatch-size", "512"});
    assert(widened.params.n_ubatch == 512);
    assert(widened.params.n_batch == 2048);

    // A plan that never mentions the cache still gets a unified one.
    const auto quiet = parse({"--model", "not-loaded.gguf"});
    assert(quiet.params.kv_unified);
    assert(quiet.params.n_batch >= quiet.params.n_ubatch);

    // And asking for it explicitly is neither required nor refused.
    const auto asked = parse({"--model", "not-loaded.gguf", "--kv-unified",
                              "--batch-size", "512", "--ubatch-size", "512"});
    assert(asked.params.kv_unified);
    assert(asked.params.n_batch == 512);
    assert(asked.params.n_batch == asked.params.n_ubatch);
    return 0;
}
