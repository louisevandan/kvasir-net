#include "llama_stage_runtime.hpp"

namespace staged::llama_runtime {

namespace {

bool settlement_fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

} // namespace

bool StageRuntime::settle_physical_sequence(
        llama_seq_id sequence_id,
        llama_pos rollback_from,
        bool restore_checkpoint,
        std::vector<llama_token> * proposal,
        std::string * error) {
    if (!loaded() || sequence_id < 0 || rollback_from < 0 || proposal == nullptr
        || static_cast<std::uint32_t>(sequence_id) >= llama_n_seq_max(ctx_)) {
        return settlement_fail("invalid physical settlement", error);
    }
    proposal->clear();
    auto checkpoint = physical_checkpoints_.find(sequence_id);
    if (restore_checkpoint && checkpoint != physical_checkpoints_.end()) {
        checkpoint->second.load_target(
            ctx_, sequence_id, LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
        llama_synchronize(ctx_);
    } else if (!llama_memory_seq_rm(
            llama_get_memory(ctx_), sequence_id, rollback_from, -1)) {
        return settlement_fail("llama.cpp rejected physical settlement", error);
    }
    if (checkpoint != physical_checkpoints_.end()) {
        physical_checkpoints_.erase(checkpoint);
    }
    if (auto found = mtp_sequences_.find(sequence_id); found != mtp_sequences_.end()) {
        auto & sequence = found->second;
        if (restore_checkpoint && !sequence.draft_checkpoint.draft_state().empty()) {
            sequence.draft_checkpoint.load_draft(
                mtp_context(), sequence_id, LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
            llama_synchronize(mtp_context());
        } else if (mtp_context() != nullptr && !llama_memory_seq_rm(
                llama_get_memory(mtp_context()), sequence_id, rollback_from, -1)) {
            return settlement_fail("llama.cpp rejected draft settlement", error);
        }
        if (restore_checkpoint) {
            sequence.pending_proposal.reset();
        } else {
            const auto keep = static_cast<std::size_t>(rollback_from);
            if (sequence.history.size() > keep) sequence.history.resize(keep);
            if (tail_stage_) {
                const auto pending = sequence.pending_proposal;
                if (!pending.has_value()) {
                    return settlement_fail(
                        "direct MTP settlement lost its pending proposal", error);
                }
                sequence.pending_proposal.reset();
                if (!make_mtp_proposal(
                        sequence_id,
                        pending->generated_before,
                        pending->max_tokens,
                        pending->sampled,
                        pending->sampled_position,
                        pending->generated_now,
                        proposal,
                        error)) return false;
            } else if (sequence.pending_proposal.has_value()) {
                return settlement_fail(
                    "non-terminal MTP stage owns a pending proposal", error);
            }
        }
    }
    return true;
}

} // namespace staged::llama_runtime
