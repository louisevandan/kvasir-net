#include "llama_stage_runtime.hpp"

namespace staged::llama_runtime {

void StageRuntime::begin_hop_batch() {
    hop_sequence_snapshot_ = sequence_ids_;
    hop_new_sequences_.clear();
    hop_next_sequence_snapshot_ = next_sequence_id_;
    hop_batch_active_ = true;
}

void StageRuntime::commit_hop_batch() {
    hop_sequence_snapshot_.clear();
    hop_new_sequences_.clear();
    hop_batch_active_ = false;
}

bool StageRuntime::rollback_hop_batch(std::string * error) {
    if (!hop_batch_active_) return true;

    bool ok = true;
    std::string failures;
    for (auto it = hop_new_sequences_.rbegin(); it != hop_new_sequences_.rend(); ++it) {
        if (hop_sequence_snapshot_.find(*it) != hop_sequence_snapshot_.end()) continue;
        if (sequence_ids_.find(*it) == sequence_ids_.end()) continue;
        std::string release_error;
        if (!release_sequence(*it, &release_error)) {
            ok = false;
            if (!failures.empty()) failures += "; ";
            failures += *it + ": " +
                (release_error.empty() ? "sequence release failed" : release_error);
        }
    }
    next_sequence_id_ = hop_next_sequence_snapshot_;
    hop_sequence_snapshot_.clear();
    hop_new_sequences_.clear();
    hop_batch_active_ = false;
    if (!ok && error != nullptr) *error = failures;
    return ok;
}

} // namespace staged::llama_runtime
