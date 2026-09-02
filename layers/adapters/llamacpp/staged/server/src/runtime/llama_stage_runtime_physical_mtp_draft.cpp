#include "llama_stage_runtime.hpp"

// Still calls llama.cpp's sampler or speculative API directly.
#include "compat/p4_llama_compat_internal.hpp"
#include "physical_wire.hpp"

#include <algorithm>
#include <unordered_set>

namespace staged::llama_runtime {

namespace {

bool draft_fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

} // namespace

bool StageRuntime::make_mtp_proposal(
        llama_seq_id sequence_id,
        std::uint32_t generated_before,
        std::uint32_t max_tokens,
        llama_token sampled,
        llama_pos sampled_position,
        std::uint32_t generated_now,
        std::vector<llama_token> * proposal,
        std::string * error) {
    if (proposal == nullptr) {
        return draft_fail("invalid MTP proposal output", error);
    }
    std::vector<PhysicalOutcome> outcomes(1);
    const std::vector<MtpDraftRequest> requests{{
        sequence_id,
        generated_before,
        max_tokens,
        sampled,
        sampled_position,
        generated_now,
        0,
    }};
    if (!make_mtp_proposals(requests, &outcomes, error)) return false;
    *proposal = std::move(outcomes.front().proposal);
    return true;
}

bool StageRuntime::make_mtp_proposals(
        const std::vector<MtpDraftRequest> & requests,
        std::vector<PhysicalOutcome> * outcomes,
        std::string * error) {
    if (requests.empty()) return true;
    if (outcomes == nullptr || !mtp_speculative_.valid() || mtp_context() == nullptr) {
        return draft_fail("invalid batched MTP proposal request", error);
    }

    std::unordered_set<llama_seq_id> sequences;
    for (const auto & request : requests) {
        if (request.sequence_id < 0
            || static_cast<std::uint32_t>(request.sequence_id) >= llama_n_seq_max(ctx_)
            || request.sampled == LLAMA_TOKEN_NULL || request.sampled_position < 0
            || request.outcome_index >= outcomes->size()
            || request.generated_before > request.max_tokens
            || request.generated_now > request.max_tokens - request.generated_before
            || !sequences.insert(request.sequence_id).second) {
            return draft_fail("invalid batched MTP proposal identity", error);
        }
        const auto found = mtp_sequences_.find(request.sequence_id);
        if (found == mtp_sequences_.end() || !found->second.begun
            || found->second.pending_proposal.has_value()
            || !(*outcomes)[request.outcome_index].proposal.empty()) {
            return draft_fail(
                "MTP proposal preceded prompt completion or settlement", error);
        }
    }

    struct PreparedDraft final {
        const MtpDraftRequest * request = nullptr;
        std::size_t draft_max = 0;
    };
    std::vector<PreparedDraft> prepared;
    prepared.reserve(requests.size());
    const auto physical_draft_max = llama_n_ubatch(ctx_) > 0
        ? llama_n_ubatch(ctx_) - 1 : 0;
    for (const auto & request : requests) {
        auto & sequence = mtp_sequences_.at(request.sequence_id);
        auto & proposal = (*outcomes)[request.outcome_index].proposal;
        sequence.proposal.clear();
        proposal.push_back(request.sampled);
        const auto remaining = request.max_tokens
            - request.generated_before - request.generated_now;
        const auto request_draft_max = remaining > 0 ? remaining - 1 : 0;
        const auto draft_max = std::min<std::size_t>(
            physical_draft_max, request_draft_max);
        if (draft_max == 0) {
            sequence.proposal = proposal;
            continue;
        }
        auto & params = common_speculative_get_draft_params(
            p4_llama_compat::raw(mtp_speculative_), request.sequence_id);
        params = {
            true,
            static_cast<std::int32_t>(draft_max),
            request.sampled_position,
            request.sampled,
            &sequence.history,
            &sequence.proposal,
        };
        const auto memory = llama_get_memory(mtp_context());
        const auto pos_min = llama_memory_seq_pos_min(memory, request.sequence_id);
        const auto pos_max = llama_memory_seq_pos_max(memory, request.sequence_id);
        sequence.draft_checkpoint.clear();
        sequence.draft_checkpoint.update_positions(
            pos_max >= pos_min ? pos_max - pos_min + 1 : 0, pos_min, pos_max);
        if (draft_seq_rm_type_ == p4_llama_compat::SeqRemoval::FullOnly) {
            sequence.draft_checkpoint.save_draft(
                mtp_context(), request.sequence_id,
                LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
        }
        prepared.push_back(PreparedDraft{&request, draft_max});
    }

    if (prepared.empty()) return true;
    // Upstream llama.cpp consumes every dparams entry whose `drafting` flag is
    // set and constructs one backend-neutral batch for those sequences.
    common_speculative_draft(p4_llama_compat::raw(mtp_speculative_));

    std::string first_error;
    const auto memory = llama_get_memory(mtp_context());
    for (const auto & item : prepared) {
        const auto & request = *item.request;
        auto & sequence = mtp_sequences_.at(request.sequence_id);
        if (sequence.proposal.size() > item.draft_max) {
            sequence.proposal.resize(item.draft_max);
        }
        const auto draft_count = sequence.proposal.size();
        if (draft_seq_rm_type_ == p4_llama_compat::SeqRemoval::FullOnly) {
            sequence.draft_checkpoint.load_draft(
                mtp_context(), request.sequence_id,
                LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
            llama_synchronize(mtp_context());
        }
        if (!llama_memory_seq_rm(
                memory, request.sequence_id,
                sequence.draft_checkpoint.pos_max() + 1, -1)
            && first_error.empty()) {
            first_error = "llama.cpp rejected post-draft rollback";
        }
        auto & proposal = (*outcomes)[request.outcome_index].proposal;
        proposal.insert(
            proposal.end(), sequence.proposal.begin(), sequence.proposal.end());
        if (draft_seq_rm_type_ == p4_llama_compat::SeqRemoval::RecurrentBounded
            && draft_count > llama_n_rs_seq(mtp_context())) {
            sequence.draft_checkpoint.save_draft(
                mtp_context(), request.sequence_id,
                LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
        }
        sequence.proposal = proposal;
    }
    if (!first_error.empty()) {
        if (error != nullptr) *error = first_error;
        return false;
    }
    return true;
}

} // namespace staged::llama_runtime
