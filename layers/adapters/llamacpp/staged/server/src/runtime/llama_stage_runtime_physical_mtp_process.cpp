#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"

#include <limits>

namespace staged::llama_runtime {

bool StageRuntime::process_physical_mtp(
        const llama_batch & target_batch,
        const std::vector<PhysicalOwner> & owners,
        std::string * error) {
    if (!tail_stage_ || mtp_speculative_ == nullptr) return true;
    if (owners.empty()
        || owners.size() != static_cast<std::size_t>(target_batch.n_tokens)
        || owners.size() > static_cast<std::size_t>(std::numeric_limits<std::int32_t>::max())) {
        if (error != nullptr) *error = "MTP owner count does not match target batch";
        return false;
    }
    for (const auto & owner : owners) {
        if (owner.sequence_id >= llama_n_seq_max(ctx_)) {
            if (error != nullptr) *error = "MTP owner sequence is outside the loaded context";
            return false;
        }
    }

    // A non-first pipeline stage receives embeddings, while upstream MTP's
    // process() deliberately ignores embedding batches.  The target hidden
    // rows have already been produced in ctx_; provide the original token,
    // position and sequence metadata as the token batch the upstream driver
    // requires to advance its separate draft context.
    llama_batch token_batch = llama_batch_init(
        static_cast<std::int32_t>(owners.size()), 0, 1);
    if (token_batch.token == nullptr || token_batch.pos == nullptr
        || token_batch.n_seq_id == nullptr || token_batch.seq_id == nullptr
        || token_batch.logits == nullptr) {
        llama_batch_free(token_batch);
        if (error != nullptr) *error = "llama.cpp could not allocate MTP token metadata";
        return false;
    }
    token_batch.n_tokens = static_cast<std::int32_t>(owners.size());
    for (std::size_t index = 0; index < owners.size(); ++index) {
        const auto & owner = owners[index];
        token_batch.token[index] = owner.input_token;
        token_batch.pos[index] = static_cast<llama_pos>(owner.position);
        token_batch.n_seq_id[index] = 1;
        token_batch.seq_id[index][0] = static_cast<llama_seq_id>(owner.sequence_id);
        token_batch.logits[index] = 0;
    }
    const bool processed = common_speculative_process(mtp_speculative_.get(), token_batch);
    llama_batch_free(token_batch);
    if (!processed) {
        if (error != nullptr) *error = "llama.cpp rejected target MTP token metadata";
        return false;
    }
    for (const auto & owner : owners) {
        auto & sequence = mtp_sequences_[owner.sequence_id];
        if (owner.phase == PhysicalPhase::Prefill
            || owner.phase == PhysicalPhase::Decode) {
            sequence.history.push_back(owner.input_token);
        }
        if (owner.phase == PhysicalPhase::Prefill && owner.output) {
            if (sequence.begun) {
                if (error != nullptr) *error = "speculative generation began more than once";
                return false;
            }
            common_speculative_begin(
                mtp_speculative_.get(), owner.sequence_id, sequence.history);
            sequence.begun = true;
        } else if (owner.phase != PhysicalPhase::Prefill && !sequence.begun) {
            if (error != nullptr) *error = "speculative work preceded prompt completion";
            return false;
        }
    }
    return true;
}

} // namespace staged::llama_runtime
