// The tail-stage sampler step of the per-sequence HOP path.
//
// Split out of llama_stage_runtime_hop.cpp to keep that file under this
// repository's 400-line limit -- execute_hop's own chunk loop already fills
// most of it, and this is the one self-contained piece downstream of it: by
// the time it runs, execute_hop's decode() has already returned Success for
// the sequence's last chunk, so every failure here means the KV cache has
// already advanced and hop_memory_dirty_ must be set (see decode_status.hpp
// and hop_memory_dirty() in llama_stage_runtime.hpp).

#include "llama_stage_runtime_hop_shared.hpp"

#include "request_options.hpp"
#include "request_stops.hpp"

namespace staged::llama_runtime {

using namespace hop;

bool StageRuntime::sample_hop_outcome(
        const protocol::SequencePayload & input,
        protocol::HopPhase phase,
        const std::vector<llama_token> & input_tokens,
        int32_t last_batch_tokens,
        std::optional<protocol::SequencePayload::OutcomeMetadata> * outcome,
        std::string * error) {
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
        if (!apply_request_options(input.options, model_, &sampling, error)) {
            hop_memory_dirty_ = true;
            return false;
        }
        if (hop_trace_enabled()) {
            std::fprintf(stderr, "P4_STAGED_SAMPLER_OPTIONS stage=%d-%d seq=%s options_bytes=%zu ignore_eos=%d bias_count=%zu\n",
                         config_.layer_begin, config_.layer_end, input.sequence_id.c_str(),
                         input.options.size(), sampling.ignores_end_of_generation() ? 1 : 0,
                         sampling.logit_bias_count());
        }
        auto sampler = p4_llama_compat::Sampler::create(model_, sampling);
        if (!sampler.valid()) {
            hop_memory_dirty_ = true;
            return fail_hop("llama.cpp failed to create staged sampler", error);
        }
        if (!input_tokens.empty()) {
            for (const auto token : input_tokens) {
                sampler.accept(token, false);
            }
        }
        found = samplers_.emplace(input.sequence_id, std::move(sampler)).first;
        sampler_options_[input.sequence_id] = input.options;
    }
    if (phase != protocol::HopPhase::Decode) return true;

    const auto sampled = found->second.sample(ctx_, last_batch_tokens - 1);
    if (sampled == LLAMA_TOKEN_NULL) {
        hop_memory_dirty_ = true;
        return fail_hop("llama.cpp staged sampler returned no token", error);
    }
    found->second.accept(sampled, true);
    const auto *vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) {
        hop_memory_dirty_ = true;
        return fail_hop("llama.cpp did not expose a sampler vocabulary", error);
    }
    protocol::SequencePayload::OutcomeMetadata metadata;
    metadata.token = static_cast<std::int32_t>(sampled);
    metadata.position = input.position.value_or(0) + 1;
    const bool end_of_generation = llama_vocab_is_eog(vocab, sampled);
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_SAMPLE stage=%d-%d seq=%s token=%d eog=%d ignore_eos=%d\n",
                     config_.layer_begin, config_.layer_end, input.sequence_id.c_str(),
                     static_cast<int>(sampled), end_of_generation ? 1 : 0,
                     input.options.find("ignore_eos") != std::string::npos ? 1 : 0);
    }
    if (!end_of_generation) {
        auto & generated = sampled_tokens_[input.sequence_id];
        generated.push_back(sampled);
        const auto detokenized = p4_llama_compat::detokenize(vocab, generated, false);
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
        // Anything still invalid after that trim is corruption, not an
        // unfinished character - and replacing the whole text with U+FFFD both
        // hides it and emits the exact mark the acceptance judge uses to catch
        // a token split across a stage boundary. Refuse instead.
        if (!valid_utf8_text(metadata.text)) {
            return fail_hop("detokenised text is not valid UTF-8 after trimming", error);
        }
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
    *outcome = std::move(metadata);
    return true;
}

} // namespace staged::llama_runtime
