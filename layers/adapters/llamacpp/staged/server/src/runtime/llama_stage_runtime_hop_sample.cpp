// One sampled token for one row of a batched decode.
//
// Row `i` sat at batch index `i`, which is also its logits index, so the tail
// samples each sequence from its own row. The sampler, its options and the
// detokenised text carry over from the per-sequence path unchanged: what a
// sequence has generated so far is a property of the sequence, not of how it
// was batched.

#include "llama_stage_runtime_hop_shared.hpp"

// The sampler and speculative APIs still take llama.cpp's struct.
#include "compat/p4_llama_compat_internal.hpp"
#include "request_options.hpp"
#include "request_stops.hpp"

#include <chrono>
#include <cstdint>
#include <vector>

namespace staged::llama_runtime {

using namespace hop;

bool StageRuntime::sample_decode_row(
    const protocol::SequencePayload & input,
    int32_t logits_index,
    protocol::SequencePayload * result,
    std::string * error) {
    if (result == nullptr) return fail_hop("invalid batched decode sample", error);
    const auto options_found = sampler_options_.find(input.sequence_id);
    if (options_found != sampler_options_.end() && options_found->second != input.options) {
        samplers_.erase(input.sequence_id);
        sampler_options_.erase(options_found);
        sampled_tokens_.erase(input.sequence_id);
        sampled_texts_.erase(input.sequence_id);
    }
    auto found = samplers_.find(input.sequence_id);
    if (found == samplers_.end()) {
        auto sampling = params_.sampling_options();
        if (!apply_request_options(input.options, model_, &sampling, error)) return false;
        auto sampler = p4_llama_compat::Sampler::create(model_, sampling);
        if (!sampler.valid()) return fail_hop("llama.cpp failed to create staged sampler", error);
        found = samplers_.emplace(input.sequence_id, std::move(sampler)).first;
        sampler_options_[input.sequence_id] = input.options;
    }
    const auto chain_started = std::chrono::steady_clock::now();
    const auto sampled = found->second.sample(ctx_, logits_index);
    sampler_chain_nanos_ += static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - chain_started).count());
    if (sampled == LLAMA_TOKEN_NULL) {
        return fail_hop("llama.cpp staged sampler returned no token", error);
    }
    found->second.accept(sampled, true);
    const auto *vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) return fail_hop("llama.cpp did not expose a sampler vocabulary", error);

    protocol::SequencePayload::OutcomeMetadata metadata;
    metadata.token = static_cast<std::int32_t>(sampled);
    metadata.position = input.position.value_or(0) + 1;
    const bool end_of_generation = llama_vocab_is_eog(vocab, sampled);
    if (!end_of_generation) {
        auto & generated = sampled_tokens_[input.sequence_id];
        generated.push_back(sampled);
        const auto detokenize_started = std::chrono::steady_clock::now();
        const auto detokenized = p4_llama_compat::detokenize(vocab, generated, false);
        detokenize_nanos_ += static_cast<std::uint64_t>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(
                std::chrono::steady_clock::now() - detokenize_started).count());
        auto & emitted = sampled_texts_[input.sequence_id];
        std::vector<std::string> stops;
        if (!parse_request_stops(input.options, &stops, error)) return false;
        const auto filtered = filter_request_stops(
            detokenized, emitted.size(), stops, false);
        metadata.text = filtered.text;
        // Emit only what is whole. What is left over is the beginning of a
        // character whose remainder is in the next token, so it is held back
        // rather than turned into a replacement mark.
        metadata.text.resize(complete_utf8_prefix(metadata.text));
        if (!valid_utf8_text(metadata.text)) metadata.text = "\xEF\xBF\xBD";
        emitted += metadata.text;
        if (filtered.stopped) metadata.stop = "stop";
    } else {
        const auto detokenized = p4_llama_compat::detokenize(
            vocab, sampled_tokens_[input.sequence_id], false);
        auto & emitted = sampled_texts_[input.sequence_id];
        std::vector<std::string> stops;
        if (!parse_request_stops(input.options, &stops, error)) return false;
        metadata.text = filter_request_stops(
            detokenized, emitted.size(), stops, true).text;
        emitted += metadata.text;
    }
    if (end_of_generation) metadata.stop = "eos";
    // The same line the per-sequence path prints, so a batched run and an
    // unbatched one can be counted the same way.
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_SAMPLE stage=%d-%d seq=%s token=%d eog=%d row=%d\n",
                     config_.layer_begin, config_.layer_end, input.sequence_id.c_str(),
                     static_cast<int>(sampled), end_of_generation ? 1 : 0, logits_index);
    }
    result->outcome = std::move(metadata);
    return true;
}

} // namespace staged::llama_runtime
