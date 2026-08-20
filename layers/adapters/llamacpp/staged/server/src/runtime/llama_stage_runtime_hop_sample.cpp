// One sampled token for one row of a batched decode.
//
// Row `i` sat at batch index `i`, which is also its logits index, so the tail
// samples each sequence from its own row. The sampler, its options and the
// detokenised text carry over from the per-sequence path unchanged: what a
// sequence has generated so far is a property of the sequence, not of how it
// was batched.

#include "llama_stage_runtime_hop_shared.hpp"
#include "request_options.hpp"

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
        auto sampling = params_.sampling;
        if (!apply_request_options(input.options, model_, &sampling, error)) return false;
        common_sampler_ptr sampler(common_sampler_init(model_, sampling));
        if (!sampler) return fail_hop("llama.cpp failed to create staged sampler", error);
        found = samplers_.emplace(input.sequence_id, std::move(sampler)).first;
        sampler_options_[input.sequence_id] = input.options;
    }
    const auto sampled = common_sampler_sample(found->second.get(), ctx_, logits_index);
    if (sampled == LLAMA_TOKEN_NULL) {
        return fail_hop("llama.cpp staged sampler returned no token", error);
    }
    common_sampler_accept(found->second.get(), sampled, true);
    const auto *vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) return fail_hop("llama.cpp did not expose a sampler vocabulary", error);

    protocol::SequencePayload::OutcomeMetadata metadata;
    metadata.token = static_cast<std::int32_t>(sampled);
    metadata.position = input.position.value_or(0) + 1;
    const bool end_of_generation = llama_vocab_is_eog(vocab, sampled);
    if (!end_of_generation) {
        auto & generated = sampled_tokens_[input.sequence_id];
        generated.push_back(sampled);
        const auto detokenized = common_detokenize(vocab, generated, false);
        auto & emitted = sampled_texts_[input.sequence_id];
        if (detokenized.size() >= emitted.size() &&
            detokenized.compare(0, emitted.size(), emitted) == 0) {
            metadata.text = detokenized.substr(emitted.size());
        } else {
            metadata.text = detokenized;
        }
        if (!valid_utf8_text(metadata.text)) metadata.text = "\xEF\xBF\xBD";
        emitted = detokenized;
    }
    if (end_of_generation) metadata.stop = "eos";
    result->outcome = std::move(metadata);
    return true;
}

} // namespace staged::llama_runtime
