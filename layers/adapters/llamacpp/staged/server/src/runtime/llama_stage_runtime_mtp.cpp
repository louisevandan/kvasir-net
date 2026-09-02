#include "llama_stage_runtime.hpp"

// The sampler and speculative APIs still take llama.cpp's struct.
#include "compat/p4_llama_compat_internal.hpp"

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
    // Same entry guard as execute_hop/execute_decode_batch (see
    // hop_memory_dirty() in llama_stage_runtime.hpp). This test-only path is
    // not reachable from server_hop.cpp or the wire dispatch today, but a
    // future caller should not have to rediscover that a quarantined runtime
    // must refuse every decode path, not just the two production ones.
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || mtp_context() == nullptr || !mtp_speculative_.valid()) {
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
    const std::vector<llama_token> all_tokens(prompt.begin(), prompt.end());
    const std::vector<llama_token> prompt_without_last(prompt.begin(), prompt.end() - 1);
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
        p4_llama_compat::batch_add(prefill, all_tokens[static_cast<std::size_t>(i)], i,
                         {seq_id}, true);
    }
    std::cerr << "MTP_TEST prefill_decode\n";
    const auto prefill_status = decode_status_from_raw(llama_decode(ctx_, prefill));
    if (prefill_status != DecodeStatus::Success) {
        llama_batch_free(prefill);
        // This test-only path bypasses execute_hop()/execute_decode_batch()
        // entirely, so it has no hop_memory_dirty_ guard to trip on a later
        // call -- but a decode that leaves ubatches processed in the memory
        // state (Aborted/Fatal) still makes this StageRuntime unsafe for any
        // ordinary HOP that might run on it afterwards, so it is marked the
        // same way the ordinary paths do.
        if (decode_status_leaves_memory_dirty(prefill_status)) hop_memory_dirty_ = true;
        return mtp_fail("llama.cpp failed to decode MTP prefill", error);
    }
    std::cerr << "MTP_TEST prefill_process\n";
    if (!mtp_speculative_.process(prefill)) {
        llama_batch_free(prefill);
        return mtp_fail("common_speculative_process failed for MTP prefill", error);
    }
    llama_batch_free(prefill);

    mtp_speculative_.begin(seq_id, prompt_without_last);
    auto sampling = params_.sampling_options();
    auto sampler = p4_llama_compat::Sampler::create(model_, sampling);
    if (!sampler.valid()) return mtp_fail("failed to create MTP target sampler", error);
    for (const auto token : prompt_without_last) {
        sampler.accept(token, false);
    }

    std::vector<llama_token> draft;
    p4_llama_compat::PromptCheckpoint draft_checkpoint;
    const auto draft_memory = llama_get_memory(mtp_context());
    const auto draft_pos_min = llama_memory_seq_pos_min(draft_memory, seq_id);
    const auto draft_pos_max = llama_memory_seq_pos_max(draft_memory, seq_id);
    draft_checkpoint.update_positions(
        draft_pos_max >= draft_pos_min ? draft_pos_max - draft_pos_min + 1 : 0,
        draft_pos_min,
        draft_pos_max);
    if (draft_seq_rm_type_ == p4_llama_compat::SeqRemoval::FullOnly) {
        draft_checkpoint.save_draft(
            mtp_context(), seq_id, LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
    }
    mtp_speculative_.draft(seq_id, p4_llama_compat::DraftRequest{
        true, 1, n_past, id_last, &prompt_without_last, &draft});
    std::cerr << "MTP_TEST draft\n";
    
    observation->drafted_tokens = draft.size();
    if (draft.empty()) return mtp_fail("MTP driver produced no proposal token", error);
    if (draft_seq_rm_type_ == p4_llama_compat::SeqRemoval::FullOnly) {
        draft_checkpoint.load_draft(
            mtp_context(), seq_id, LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
        llama_synchronize(mtp_context());
    }
    if (!llama_memory_seq_rm(
            draft_memory, seq_id, draft_checkpoint.pos_max() + 1, -1)) {
        return mtp_fail("llama.cpp rejected post-draft rollback", error);
    }

    llama_batch verify = llama_batch_init(
        static_cast<int32_t>(draft.size() + 1), 0, 1);
    if (verify.token == nullptr || verify.pos == nullptr ||
        verify.n_seq_id == nullptr || verify.seq_id == nullptr ||
        verify.logits == nullptr) {
        llama_batch_free(verify);
        return mtp_fail("llama.cpp failed to allocate MTP verify batch", error);
    }
    p4_llama_compat::batch_add(verify, id_last, n_past, {seq_id}, true);
    for (std::size_t i = 0; i < draft.size(); ++i) {
        p4_llama_compat::batch_add(verify, draft[i], n_past + static_cast<int32_t>(i) + 1,
                         {seq_id}, true);
    }
    std::cerr << "MTP_TEST verify_decode\n";
    const auto verify_status = decode_status_from_raw(llama_decode(ctx_, verify));
    if (verify_status != DecodeStatus::Success) {
        llama_batch_free(verify);
        if (decode_status_leaves_memory_dirty(verify_status)) hop_memory_dirty_ = true;
        return mtp_fail("llama.cpp failed to decode MTP verify batch", error);
    }
    std::cerr << "MTP_TEST verify_process\n";
    if (!mtp_speculative_.process(verify)) {
        llama_batch_free(verify);
        return mtp_fail("common_speculative_process failed for MTP verify", error);
    }

    std::cerr << "MTP_TEST sample_accept\n";
    const auto accepted = sampler.sample_and_accept_n(ctx_, draft);
    llama_batch_free(verify);
    if (accepted.empty()) return mtp_fail("MTP sampler returned no token", error);

    observation->accepted_tokens = accepted.size() - 1;
    const auto * vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) return mtp_fail("MTP vocabulary is unavailable", error);
    observation->eos = std::any_of(
        accepted.begin(), accepted.end(),
        [vocab](const llama_token token) { return llama_vocab_is_eog(vocab, token); });
    mtp_speculative_.accept(seq_id,
        static_cast<uint16_t>(observation->accepted_tokens));
    return true;
}

} // namespace staged::llama_runtime
