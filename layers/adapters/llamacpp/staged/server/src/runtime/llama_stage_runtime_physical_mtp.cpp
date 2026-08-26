#include "llama_stage_runtime.hpp"
#include "llama_stage_runtime_hop_shared.hpp"
#include "physical_wire.hpp"
#include "request_stops.hpp"

#include <algorithm>
#include <limits>

namespace staged::llama_runtime {

using namespace hop;

namespace {

bool mtp_fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

bool speculative_phase(PhysicalPhase phase) {
    return phase == PhysicalPhase::Verify || phase == PhysicalPhase::Replay;
}

} // namespace

bool StageRuntime::prepare_physical_owners(
        const std::vector<PhysicalOwner> & owners, std::string * error) {
    if (owners.empty()) return mtp_fail("physical owners are empty", error);
    for (std::size_t begin = 0; begin < owners.size();) {
        const auto & first = owners[begin];
        if (!speculative_phase(first.phase)) {
            ++begin;
            continue;
        }
        const auto count = static_cast<std::size_t>(first.speculative_count);
        if (first.speculative_id == 0 || count == 0 || count > owners.size() - begin
            || first.sequence_id >= llama_n_seq_max(ctx_)) {
            return mtp_fail("invalid atomic physical group", error);
        }
        for (std::size_t offset = 0; offset < count; ++offset) {
            const auto & owner = owners[begin + offset];
            if (owner.phase != first.phase || owner.sequence_id != first.sequence_id
                || owner.sequence_key != first.sequence_key
                || owner.speculative_id != first.speculative_id
                || owner.speculative_count != first.speculative_count
                || owner.speculative_index != offset) {
                return mtp_fail("atomic physical group is split or reordered", error);
            }
        }
        if (first.phase == PhysicalPhase::Verify
            || (first.phase == PhysicalPhase::Replay && config_.layer_begin == 0)) {
            const bool checkpoint = config_.layer_begin == 0
                || target_seq_rm_type_ == COMMON_CONTEXT_SEQ_RM_TYPE_FULL
                || (target_seq_rm_type_ == COMMON_CONTEXT_SEQ_RM_TYPE_RS
                    && count - 1 > llama_n_rs_seq(ctx_));
            if (checkpoint) {
                auto & value = physical_checkpoints_[first.sequence_id];
                const auto memory = llama_get_memory(ctx_);
                const auto pos_min = llama_memory_seq_pos_min(memory, first.sequence_id);
                const auto pos_max = llama_memory_seq_pos_max(memory, first.sequence_id);
                value.clear();
                value.update_pos(
                    pos_max >= pos_min ? pos_max - pos_min + 1 : 0, pos_min, pos_max);
                value.update_tgt(
                    ctx_, first.sequence_id, LLAMA_STATE_SEQ_FLAGS_PARTIAL_ONLY);
                if (value.data_tgt.empty()) {
                    return mtp_fail("target checkpoint is empty", error);
                }
            }
        }
        begin += count;
    }
    return true;
}

bool StageRuntime::format_generated_token(
        const PhysicalOwner & owner,
        llama_token token,
        std::uint32_t position,
        GeneratedToken * generated,
        std::string * error) {
    if (generated == nullptr) return mtp_fail("generated token output is null", error);
    const auto * vocab = llama_model_get_vocab(model_);
    if (vocab == nullptr) return mtp_fail("llama.cpp vocabulary is unavailable", error);
    generated->token = token;
    generated->position = position;
    const bool eog = llama_vocab_is_eog(vocab, token);
    auto & pending = pending_texts_[owner.sequence_key];
    if (!eog) pending += common_token_to_piece(vocab, token, false);
    std::vector<std::string> stops;
    if (!parse_request_stops(owner.options, &stops, error)) return false;
    const auto filtered = filter_request_stops(pending, 0, stops, eog);
    generated->text = filtered.text;
    if (!eog) generated->text.resize(complete_utf8_prefix(generated->text));
    const auto consumed = generated->text.size();
    if (!valid_utf8_text(generated->text)) generated->text = "\xEF\xBF\xBD";
    pending.erase(0, std::min(pending.size(), consumed));
    if (filtered.stopped) {
        generated->stop = "stop";
        pending.clear();
    }
    if (eog) {
        generated->stop = "eos";
        pending.clear();
    }
    return true;
}

bool StageRuntime::sample_physical_mtp(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::size_t begin,
        std::size_t end,
        std::vector<PhysicalOutcome> * outcomes,
        std::vector<MtpDraftRequest> * draft_requests,
        std::string * error) {
    if (begin >= end || end > owners.size()
        || (owners[begin].phase != PhysicalPhase::Verify
            && owners[begin].phase != PhysicalPhase::Replay)
        || outcomes == nullptr || draft_requests == nullptr
        || input.output.size() != owners.size()
        || end > static_cast<std::size_t>(std::numeric_limits<std::int32_t>::max())) {
        return mtp_fail("invalid MTP verification sample", error);
    }
    const auto & first = owners[begin];
    auto sequence_it = mtp_sequences_.find(first.sequence_id);
    auto sampler_it = samplers_.find(first.sequence_key);
    if (sequence_it == mtp_sequences_.end() || sampler_it == samplers_.end()) {
        return mtp_fail("MTP verification state is missing", error);
    }
    llama_tokens submitted;
    submitted.reserve(end - begin);
    for (auto index = begin; index < end; ++index) {
        submitted.push_back(owners[index].input_token);
    }
    if (sequence_it->second.proposal != submitted || submitted.empty()) {
        return mtp_fail("MTP proposal identity or contents changed in flight", error);
    }
    llama_tokens draft(submitted.begin() + 1, submitted.end());
    const bool replay = first.phase == PhysicalPhase::Replay;
    const auto n_rollback_max = submitted.size() - 1;
    auto sampler_checkpoint = physical_checkpoints_.find(first.sequence_id)
            != physical_checkpoints_.end()
        ? common_sampler_ptr(common_sampler_clone(sampler_it->second.get()))
        : common_sampler_ptr{};
    if (!replay && physical_checkpoints_.find(first.sequence_id)
            != physical_checkpoints_.end() && !sampler_checkpoint) {
        return mtp_fail("MTP sampler checkpoint failed", error);
    }
    std::vector<std::int32_t> logits;
    logits.reserve(end - begin);
    for (auto index = begin; index < end; ++index) {
        logits.push_back(static_cast<std::int32_t>(index));
    }
    auto accepted = common_sampler_sample_and_accept_n(
        sampler_it->second.get(), ctx_, logits, draft);
    if (accepted.empty() || accepted.size() > submitted.size()) {
        return mtp_fail("llama.cpp returned an invalid MTP acceptance", error);
    }
    if (replay && (!sequence_it->second.replay_pending
        || accepted.size() != submitted.size())) {
        return mtp_fail("MTP checkpoint replay diverged", error);
    }
    const auto n_rollback = submitted.size() - accepted.size();
    if (n_rollback > n_rollback_max) {
        return mtp_fail("MTP rollback exceeds the submitted draft", error);
    }
    const bool use_checkpoint = target_seq_rm_type_ == COMMON_CONTEXT_SEQ_RM_TYPE_FULL
        || (target_seq_rm_type_ == COMMON_CONTEXT_SEQ_RM_TYPE_RS
            && n_rollback > llama_n_rs_seq(ctx_));
    const bool checkpoint_replay = !replay && n_rollback > 0 && use_checkpoint;
    if (checkpoint_replay) {
        if (physical_checkpoints_.find(first.sequence_id) == physical_checkpoints_.end()
            || !sampler_checkpoint) {
            return mtp_fail("MTP rollback requires a missing checkpoint", error);
        }
        sampler_it->second = std::move(sampler_checkpoint);
        auto & sequence = sequence_it->second;
        sequence.proposal.clear();
        sequence.proposal.push_back(submitted.front());
        sequence.proposal.insert(
            sequence.proposal.end(), accepted.begin(), accepted.end());
        sequence.replay_pending = true;
        if (first.position > std::numeric_limits<std::uint32_t>::max()
                - sequence.proposal.size()) {
            return mtp_fail("MTP replay position overflow", error);
        }
        PhysicalOutcome outcome;
        outcome.owner_index = static_cast<std::uint32_t>(begin);
        outcome.retain_from = first.position + sequence.proposal.size();
        outcome.replay_position = first.position;
        outcome.replay_tokens = sequence.proposal;
        outcomes->push_back(std::move(outcome));
        return true;
    }
    common_speculative_accept(
        mtp_speculative_.get(), first.sequence_id,
        static_cast<std::uint16_t>(accepted.size() - 1));
    auto & sequence = sequence_it->second;
    if (sequence.pending_proposal.has_value()) {
        return mtp_fail("MTP verification overlapped an unsettled proposal", error);
    }
    sequence.replay_pending = false;
    sequence.history.insert(
        sequence.history.end(), submitted.begin(),
        submitted.begin() + static_cast<std::ptrdiff_t>(accepted.size()));

    PhysicalOutcome outcome;
    outcome.owner_index = static_cast<std::uint32_t>(begin);
    bool stopped = false;
    for (std::size_t index = 0; index < accepted.size(); ++index) {
        if (index > std::numeric_limits<std::uint32_t>::max()) {
            return mtp_fail("MTP output position overflow", error);
        }
        const auto offset = static_cast<std::uint32_t>(index);
        if (first.position > std::numeric_limits<std::uint32_t>::max() - offset - 1) {
            return mtp_fail("MTP output position overflow", error);
        }
        const auto position = first.position + offset + 1;
        GeneratedToken generated;
        if (!format_generated_token(first, accepted[index], position, &generated, error)) {
            return false;
        }
        if (generated.stop.empty()
            && first.generated_tokens + outcome.generated.size() + 1 >= first.max_tokens) {
            generated.stop = "length";
        }
        stopped = stopped || !generated.stop.empty();
        outcome.generated.push_back(std::move(generated));
        if (stopped) break;
    }
    if (!stopped && n_rollback > 0) {
        if (accepted.size() > std::numeric_limits<std::uint32_t>::max()
            || first.position > std::numeric_limits<std::uint32_t>::max()
                - static_cast<std::uint32_t>(accepted.size())) {
            return mtp_fail("MTP settlement position overflow", error);
        }
        const auto retain = first.position + static_cast<std::uint32_t>(accepted.size());
        outcome.retain_from = retain;
        sequence.pending_proposal = MtpPendingProposal{
            accepted.back(),
            static_cast<llama_pos>(outcome.generated.back().position),
            first.generated_tokens,
            static_cast<std::uint32_t>(outcome.generated.size()),
            first.max_tokens,
        };
    } else {
        physical_checkpoints_.erase(first.sequence_id);
    }
    if (!stopped && n_rollback == 0) {
        draft_requests->push_back(MtpDraftRequest{
                static_cast<llama_seq_id>(first.sequence_id),
                first.generated_tokens, first.max_tokens,
                accepted.back(),
                static_cast<llama_pos>(outcome.generated.back().position),
                static_cast<std::uint32_t>(outcome.generated.size()),
                outcomes->size(),
        });
    }
    outcomes->push_back(std::move(outcome));
    return true;
}

} // namespace staged::llama_runtime
