#include "llama_stage_runtime.hpp"

#include <algorithm>
#include <iostream>

namespace staged::llama_runtime {

namespace {

bool mtp_fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

} // namespace

bool StageRuntime::execute_mtp_hop(
        const std::vector<std::int32_t> & prompt,
        MtpHopObservation * observation,
        std::string * error) {
    if (!loaded() || mtp_context() == nullptr || mtp_speculative_ == nullptr) {
        return mtp_fail("staged MTP context/driver is not initialized", error);
    }
    if (!tail_stage_ || config_.layer_begin != 0 ||
        config_.layer_end != llama_model_n_layer(model_)) {
        return mtp_fail("staged MTP test requires a full-tail stage", error);
    }
    if (observation == nullptr || prompt.size() < 2) {
        return mtp_fail("staged MTP test requires a prompt of at least two tokens", error);
    }

    constexpr llama_seq_id seq_id = 0;
    const llama_tokens all_tokens(prompt.begin(), prompt.end());
    const llama_tokens prompt_without_last(prompt.begin(), prompt.end() - 1);
    const llama_token id_last = all_tokens.back();
    const int32_t n_past = static_cast<int32_t>(prompt_without_last.size());

    llama_batch prefill = llama_batch_init(n_past, 0, 1);
    if (prefill.token == nullptr || prefill.pos == nullptr ||
        prefill.n_seq_id == nullptr || prefill.seq_id == nullptr ||
        prefill.logits == nullptr) {
        llama_batch_free(prefill);
        return mtp_fail("llama.cpp failed to allocate MTP prefill batch", error);
    }
    for (int32_t i = 0; i < n_past; ++i) {
        common_batch_add(prefill, all_tokens[static_cast<std::size_t>(i)], i,
                         {seq_id}, true);
    }
    std::cerr << "MTP_TEST prefill_decode\n";
    if (llama_decode(ctx_, prefill) != 0) {
        llama_batch_free(prefill);
        return mtp_fail("llama.cpp failed to decode MTP prefill", error);
    }
    std::cerr << "MTP_TEST prefill_process\n";
    if (!common_speculative_process(mtp_speculative_.get(), prefill)) {
        llama_batch_free(prefill);
        return mtp_fail("common_speculative_process failed for MTP prefill", error);
    }
    llama_batch_free(prefill);

    common_speculative_begin(mtp_speculative_.get(), seq_id, prompt_without_last);
    common_sampler_ptr sampler(common_sampler_init(model_, params_.sampling));
    if (!sampler) return mtp_fail("failed to create MTP target sampler", error);
    for (const auto token : prompt_without_last) {
        common_sampler_accept(sampler.get(), token, false);
    }

    llama_tokens draft;
    auto & draft_params = common_speculative_get_draft_params(
        mtp_speculative_.get(), seq_id);
    draft_params = {
        true, 1, n_past, id_last, &prompt_without_last, &draft};
    std::cerr << "MTP_TEST draft\n";
    common_speculative_draft(mtp_speculative_.get());
    observation->drafted_tokens = draft.size();
    if (draft.empty()) return mtp_fail("MTP driver produced no proposal token", error);

    llama_batch verify = llama_batch_init(
        static_cast<int32_t>(draft.size() + 1), 0, 1);
    if (verify.token == nullptr || verify.pos == nullptr ||
        verify.n_seq_id == nullptr || verify.seq_id == nullptr ||
        verify.logits == nullptr) {
        llama_batch_free(verify);
        return mtp_fail("llama.cpp failed to allocate MTP verify batch", error);
    }
    common_batch_add(verify, id_last, n_past, {seq_id}, true);
    for (std::size_t i = 0; i < draft.size(); ++i) {
        common_batch_add(verify, draft[i], n_past + static_cast<int32_t>(i) + 1,
                         {seq_id}, true);
    }
    std::cerr << "MTP_TEST verify_decode\n";
    if (llama_decode(ctx_, verify) != 0) {
        llama_batch_free(verify);
        return mtp_fail("llama.cpp failed to decode MTP verify batch", error);
    }
    std::cerr << "MTP_TEST verify_process\n";
    if (!common_speculative_process(mtp_speculative_.get(), verify)) {
        llama_batch_free(verify);
        return mtp_fail("common_speculative_process failed for MTP verify", error);
    }

    std::cerr << "MTP_TEST sample_accept\n";
    const auto accepted = common_sampler_sample_and_accept_n(
        sampler.get(), ctx_, draft);
    llama_batch_free(verify);
    if (accepted.empty()) return mtp_fail("MTP sampler returned no token", error);

    observation->accepted_tokens = accepted.size() - 1;
    const auto * vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) return mtp_fail("MTP vocabulary is unavailable", error);
    observation->eos = std::any_of(
        accepted.begin(), accepted.end(),
        [vocab](const llama_token token) { return llama_vocab_is_eog(vocab, token); });
    common_speculative_accept(
        mtp_speculative_.get(), seq_id,
        static_cast<uint16_t>(observation->accepted_tokens));
    return true;
}

} // namespace staged::llama_runtime
