#include "plan.hpp"
#include "compat/p4_llama_compat_internal.hpp"

#include <cassert>
#include <string>
#include <vector>

int main() {
    char program[] = "p4_staged_capability_test";
    char port[] = "--port";
    char port_value[] = "1";
    char bind[] = "--bind";
    char bind_value[] = "127.0.0.1";
    char *argv[] = {program, port, port_value, bind, bind_value, nullptr};
    staged::server::ParsedLlamaOptions parsed;
    std::string error;
    const std::vector<std::string> speculative_types{
        "draft-simple", "draft-mtp", "ngram-simple", "ngram-map-k", "ngram-map-k4v",
        "ngram-mod", "ngram-cache"};
    for (const auto & type : speculative_types) {
        parsed = {};
        error.clear();
        const std::vector<std::string> tokens{
            "--model", "not-loaded.gguf", "--bind", "127.0.0.1",
            "--spec-type", type, "--memory-topology", "discrete"};
        assert(staged::server::parse_llama_options(5, argv, tokens, &parsed, &error));
        const auto report = staged::server::capability_report(parsed);
        assert(report.speculative_parser);
        assert(report.speculative_requested);
        assert(report.serialize().find("execution_blocker=") != std::string::npos);
        if (type == "draft-mtp") {
            assert(report.mtp_parser);
            assert(report.mtp_auxiliary_ownership);
            assert(report.mtp_execution);
            assert(report.speculative_execution);
            assert(report.mtp_requested);
            assert(report.serialize().find("mtp_execution=1") != std::string::npos);
            assert(report.execution_blocker == "none");
        } else if (type.rfind("draft-", 0) == 0) {
            assert(!report.speculative_execution);
            assert(report.execution_blocker ==
                   "draft_context_and_proposal_state_not_in_hop");
        } else {
            assert(!report.speculative_execution);
            assert(report.execution_blocker ==
                   "proposal_accept_rollback_state_not_in_hop");
        }
    }

    parsed = {};
    error.clear();
    const std::vector<std::string> ordinary_tokens{
        "--model", "not-loaded.gguf", "--bind", "127.0.0.1",
        "--spec-type", "none", "--memory-topology", "discrete"};
    assert(staged::server::parse_llama_options(
        5, argv, ordinary_tokens, &parsed, &error));
    const auto ordinary = staged::server::capability_report(parsed);
    assert(ordinary.normal_decode_execution);
    assert(!ordinary.speculative_requested);
    assert(ordinary.execution_blocker == "none");
    assert(ordinary.serialize().find("execution_blocker=none") !=
           std::string::npos);

    // The synthetic stdin plan can have exactly the same token count as the
    // Windows process command line.  In that case common_params_parse must
    // not replace it with the server's --bind/--port argv.
    parsed = {};
    error.clear();
    const std::vector<std::string> unified_tokens{
        "--model", "not-loaded.gguf", "--kv-unified",
        "--memory-topology", "discrete"};
    assert(staged::server::parse_llama_options(
        5, argv, unified_tokens, &parsed, &error));
    // What actually distinguishes "the plan tokens reached common_params_parse"
    // from "the Windows argv-reconstruction bug silently substituted the real
    // process argv" is whether --model's value made it into params.model.path:
    // the real process argv here carries only --port/--bind, no --model at
    // all, so a collision would leave this empty. n_ctx is not a usable signal
    // for that: it defaults to 0 ("use the model's trained context") for any
    // plan that omits --ctx-size, collision or not, since llama.cpp only
    // resolves it once a real model is loaded -- which this test, using
    // "not-loaded.gguf", never does. The previous `n_ctx > 0` assertion here
    // could not have passed for this input regardless of the code under test;
    // NDEBUG had stripped it since the test was first written.
    assert(p4_llama_compat::plan_params(parsed.params).model.path == "not-loaded.gguf");
    return 0;
}
