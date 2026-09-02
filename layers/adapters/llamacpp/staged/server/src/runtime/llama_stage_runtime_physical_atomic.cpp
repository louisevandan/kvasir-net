#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"

#include <algorithm>
#include <utility>

namespace staged::llama_runtime {

namespace {

bool execution_row_matches_owner(
        const PhysicalExecution & execution,
        std::size_t row,
        const PhysicalOwner & owner) {
    if (row >= execution.sequence_counts.size() || execution.n_pos == 0) return false;
    // llama_ubatch positions are dimension-major: pos[j*n_tokens + row].
    if (row >= execution.positions.size()
        || execution.positions[row] != static_cast<llama_pos>(owner.position)) {
        return false;
    }
    std::size_t sequence_offset = 0;
    for (std::size_t index = 0; index < row; ++index) {
        sequence_offset += static_cast<std::size_t>(execution.sequence_counts[index]);
    }
    const auto count = static_cast<std::size_t>(execution.sequence_counts[row]);
    if (sequence_offset > execution.sequence_ids.size()
        || count > execution.sequence_ids.size() - sequence_offset) return false;
    const auto begin = execution.sequence_ids.begin()
        + static_cast<std::ptrdiff_t>(sequence_offset);
    const auto end = begin + static_cast<std::ptrdiff_t>(count);
    return std::find(
        begin, end, static_cast<llama_seq_id>(owner.sequence_id)) != end;
}

} // namespace

bool StageRuntime::validate_physical_atomic_round(
        const std::vector<PhysicalOwner> & owners,
        std::string * error) {
    const auto atomic = std::find_if(
        owners.begin(), owners.end(), [](const PhysicalOwner & owner) {
            return owner.phase == PhysicalPhase::Verify
                || owner.phase == PhysicalPhase::Replay;
        });
    if (atomic == owners.end()) return true;

    std::vector<std::pair<std::size_t, std::size_t>> groups;
    bool groups_valid = true;
    std::size_t atomic_rows = 0;
    for (std::size_t begin = 0; begin < owners.size();) {
        const auto & first = owners[begin];
        if (first.phase != PhysicalPhase::Verify
            && first.phase != PhysicalPhase::Replay) {
            ++begin;
            continue;
        }
        const auto count = static_cast<std::size_t>(first.speculative_count);
        if (count == 0 || count > owners.size() - begin) {
            groups_valid = false;
            break;
        }
        groups.emplace_back(begin, count);
        atomic_rows += count;
        begin += count;
    }

    // Rollback and sampling happen after llama_decode returns. Therefore an
    // atomic group may share a physical UBATCH with ordinary rows, but no
    // later UBATCH may overwrite its recurrent snapshots first. Prove the
    // whole logical round is exactly one captured physical invocation and
    // that llama.cpp preserved every row owner before accepting the round.
    bool terminal = groups_valid && !groups.empty() && captured_executions_.size() == 1;
    if (terminal) {
        const auto & execution = captured_executions_.front();
        const auto physical_rows = execution.sequence_counts.size();
        terminal = physical_rows == owners.size();
        for (std::size_t row = 0; terminal && row < owners.size(); ++row) {
            terminal = execution_row_matches_owner(
                execution, row, owners[row]);
        }
    }
    if (!terminal) {
        std::string rollback_error;
        for (const auto & [begin, count] : groups) {
            (void) count;
            const auto & first = owners[begin];
            std::vector<llama_token> ignored_proposal;
            std::string detail;
            if (!settle_physical_sequence(
                    static_cast<llama_seq_id>(first.sequence_id),
                    static_cast<llama_pos>(first.position), true,
                    &ignored_proposal, &detail) && rollback_error.empty()) {
                rollback_error = detail;
            }
        }
        hop_memory_dirty_ = true;
        captured_executions_.clear();
        if (error != nullptr) {
            *error = "atomic verification/replay round was not one exact llama.cpp physical UBATCH"
                ";atomic_groups=" + std::to_string(groups.size())
                + ";atomic_rows=" + std::to_string(atomic_rows)
                + ";memory_dirty=1;action=reload";
            if (!rollback_error.empty()) *error += ";rollback_error=" + rollback_error;
        }
        return false;
    }

    for (const auto & [begin, count] : groups) {
        const auto & first = owners[begin];
        if (config_.layer_begin == 0
            && (first.phase == PhysicalPhase::Replay
            || (first.phase == PhysicalPhase::Verify
                && target_seq_rm_type_ != p4_llama_compat::SeqRemoval::FullOnly
                && !(target_seq_rm_type_ == p4_llama_compat::SeqRemoval::RecurrentBounded
                    && count - 1 > llama_n_rs_seq(ctx_))))) {
            // Stage zero's unconditional checkpoint only guards against a
            // physical split. Normal PART/RS rollback owns intact groups.
            physical_checkpoints_.erase(
                static_cast<llama_seq_id>(first.sequence_id));
        }
    }
    return true;
}

} // namespace staged::llama_runtime
