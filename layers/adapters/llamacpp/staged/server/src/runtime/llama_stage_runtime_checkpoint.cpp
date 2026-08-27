#include "llama_stage_runtime.hpp"

namespace {

bool checkpoint_sequence(
        std::unordered_map<std::string, llama_seq_id> & ids,
        llama_seq_id & next,
        const std::string & id,
        uint32_t sequence_limit,
        llama_seq_id * result,
        std::string * error) {
    const auto found = ids.find(id);
    if (found != ids.end()) {
        *result = found->second;
        return true;
    }
    if (next < 0 || static_cast<uint64_t>(next) >= sequence_limit) {
        if (error != nullptr) *error = "staged checkpoint sequence table is full";
        return false;
    }
    const auto value = next++;
    ids.emplace(id, value);
    *result = value;
    return true;
}

} // namespace

namespace staged::llama_runtime {

bool StageRuntime::save_checkpoint(const std::string & sequence_id,
                                   common_prompt_checkpoint * checkpoint,
                                   std::string * error) {
    if (!loaded() || checkpoint == nullptr) {
        return fail("stage runtime is not loaded or checkpoint is null", error);
    }
    llama_seq_id seq = 0;
    if (!checkpoint_sequence(sequence_ids_, next_sequence_id_, sequence_id,
                             llama_n_seq_max(ctx_), &seq, error)) {
        return false;
    }
    const auto memory = llama_get_memory(ctx_);
    const auto pos_min = llama_memory_seq_pos_min(memory, seq);
    const auto pos_max = llama_memory_seq_pos_max(memory, seq);
    checkpoint->clear();
    checkpoint->update_pos(pos_max >= pos_min ? pos_max - pos_min + 1 : 0,
                           pos_min, pos_max);
    checkpoint->update_tgt(ctx_, seq, LLAMA_STATE_SEQ_FLAGS_NONE);
    if (checkpoint->data_tgt.empty()) {
        return fail("llama native checkpoint is empty", error);
    }
    return true;
}

bool StageRuntime::restore_checkpoint(
        const std::string & sequence_id,
        const common_prompt_checkpoint & checkpoint,
        std::string * error) {
    if (!loaded() || checkpoint.data_tgt.empty()) {
        return fail("stage runtime is not loaded or checkpoint is empty", error);
    }
    llama_seq_id seq = 0;
    if (!checkpoint_sequence(sequence_ids_, next_sequence_id_, sequence_id,
                             llama_n_seq_max(ctx_), &seq, error)) {
        return false;
    }
    const auto restored = llama_state_seq_set_data_ext(
        ctx_, checkpoint.data_tgt.data(), checkpoint.data_tgt.size(), seq,
        LLAMA_STATE_SEQ_FLAGS_NONE);
    if (restored != checkpoint.data_tgt.size()) {
        return fail("llama native checkpoint restore failed", error);
    }
    // State import may enqueue asynchronous device copies. The next decode
    // must observe the restored KV before it starts graph execution.
    llama_synchronize(ctx_);
    sequence_positions_[sequence_id] = checkpoint.pos_max < 0
        ? 0U : static_cast<std::uint64_t>(checkpoint.pos_max) + 1U;
    return true;
}

} // namespace staged::llama_runtime
