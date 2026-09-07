#pragma once

// Test-only preparation shared by the real options E2E and its model-free
// lifetime regression. No production parser or upstream field mirror here.
#include "compat/p4_llama_compat.hpp"

#include <utility>

namespace staged::test {
struct RequestOptionsSnapshots {
    p4_llama_compat::SamplingOptions sampler;
    p4_llama_compat::SamplingOptions grammar;
};

template<class Consumer>
bool consume_request_options_plan(p4_llama_compat::LlamaPlan plan,
        Consumer && consume, RequestOptionsSnapshots * snapshots) {
    // LlamaPlan owns a unique_ptr. Both snapshots must be taken before the
    // consumer takes that ownership; successful load does not revive plan.
    RequestOptionsSnapshots prepared{plan.sampling_options(), plan.sampling_options()};
    if (!std::forward<Consumer>(consume)(std::move(plan))) return false;
    *snapshots = std::move(prepared);
    return true;
}
} // namespace staged::test
