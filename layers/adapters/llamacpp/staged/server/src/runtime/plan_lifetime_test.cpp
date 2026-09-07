#include "request_options_test_plan.hpp"

#include <cassert>
#include <iostream>
#include <optional>

int main() {
    using p4_llama_compat::LlamaPlan;
    LlamaPlan plan;
    assert(plan.parse_arguments({"plan-lifetime-test", "--model", "never-loaded.gguf",
        "--batch-size", "17", "--ubatch-size", "8", "--ignore-eos", "--logit-bias", "42+1"}));
    staged::test::RequestOptionsSnapshots snapshots;
    assert(!snapshots.sampler.ignores_end_of_generation());
    std::optional<LlamaPlan> destination;
    unsigned calls = 0;
    assert(staged::test::consume_request_options_plan(std::move(plan),
        [&](LlamaPlan consumed) {
            ++calls;
            // Consume the real opaque owner, not a fake stand-in or a copy.
            destination.emplace(std::move(consumed));
            assert(destination->model_path() == "never-loaded.gguf");
            assert(destination->n_batch() == 17 && destination->n_ubatch() == 8);
            return true;
        }, &snapshots));
    assert(calls == 1 && destination.has_value());
    assert(snapshots.sampler.ignores_end_of_generation());
    assert(snapshots.grammar.ignores_end_of_generation());
    assert(snapshots.sampler.logit_bias_count() == 1);
    assert(snapshots.grammar.logit_bias_count() == 1);
    destination.reset();
    // Snapshots keep owning their data after the plan's consumer dies.
    auto clone = snapshots.sampler.clone();
    assert(clone.ignores_end_of_generation() && clone.logit_bias_count() == 1);
    assert(snapshots.grammar.ignores_end_of_generation());

    LlamaPlan failed_plan;
    assert(failed_plan.parse_arguments({"plan-lifetime-test", "--ignore-eos"}));
    staged::test::RequestOptionsSnapshots unchanged;
    calls = 0;
    assert(!staged::test::consume_request_options_plan(std::move(failed_plan),
        [&](LlamaPlan consumed) {
            ++calls;
            destination.emplace(std::move(consumed));
            return false;
        }, &unchanged));
    assert(calls == 1 && destination.has_value());
    assert(!unchanged.sampler.ignores_end_of_generation());
    assert(!unchanged.grammar.ignores_end_of_generation());
    std::cout << "PLAN_LIFETIME_OK: snapshots precede consuming load\n";
}
