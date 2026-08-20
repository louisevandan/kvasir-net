// The two numbers a stage does not take from its plan.
//
// A batch wider than a ubatch describes a submission no stage makes, and one
// llama.cpp would be free to split -- which the staged cut-set, bound once per
// decode, cannot survive. A unified cache is what keeps the ubatch meaningful
// past the prefill. Both are properties of a staged lap rather than tuning, so
// a plan that says otherwise is normalised rather than obeyed, and this is the
// test that says so.

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
    // A plan that asks for four times the batch it can submit gets the number
    // it can submit.
    const auto widened = parse({"--model", "not-loaded.gguf",
                                "--batch-size", "2048", "--ubatch-size", "512"});
    assert(widened.params.n_ubatch == 512);
    assert(widened.params.n_batch == widened.params.n_ubatch);

    // A plan that never mentions the cache still gets a unified one.
    const auto quiet = parse({"--model", "not-loaded.gguf"});
    assert(quiet.params.kv_unified);
    assert(quiet.params.n_batch == quiet.params.n_ubatch);

    // And asking for it explicitly is neither required nor refused.
    const auto asked = parse({"--model", "not-loaded.gguf", "--kv-unified",
                              "--batch-size", "512", "--ubatch-size", "512"});
    assert(asked.params.kv_unified);
    assert(asked.params.n_batch == 512);
    assert(asked.params.n_batch == asked.params.n_ubatch);
    return 0;
}
