#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"

#include <algorithm>
#include <limits>
#include <utility>

namespace staged::llama_runtime {

namespace {

bool physical_fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

bool valid_execution(const PhysicalExecution & execution) {
    const auto rows = execution.sequence_counts.size();
    if (rows == 0 || execution.n_pos == 0 || execution.n_pos > 4
        || execution.positions.size() != rows * execution.n_pos
        || execution.output.size() != rows) return false;
    std::size_t total = 0;
    for (const auto count : execution.sequence_counts) {
        if (count <= 0 || total > std::numeric_limits<std::size_t>::max()
                                  - static_cast<std::size_t>(count)) return false;
        total += static_cast<std::size_t>(count);
    }
    return total == execution.sequence_ids.size();
}

bool execution_row_matches_owner(
        const PhysicalExecution & execution,
        std::size_t row,
        const PhysicalOwner & owner) {
    if (row >= execution.sequence_counts.size()
        || execution.n_pos == 0) {
        return false;
    }
    // llama_ubatch positions are dimension-major: pos[j*n_tokens + row].
    // Dimension zero therefore starts at exactly `row`, even when n_pos=4.
    const auto position_index = row;
    if (position_index >= execution.positions.size()
        || execution.positions[position_index] != static_cast<llama_pos>(owner.position)) {
        return false;
    }
    std::size_t sequence_offset = 0;
    for (std::size_t index = 0; index < row; ++index) {
        sequence_offset += static_cast<std::size_t>(execution.sequence_counts[index]);
    }
    const auto count = static_cast<std::size_t>(execution.sequence_counts[row]);
    if (sequence_offset > execution.sequence_ids.size()
        || count > execution.sequence_ids.size() - sequence_offset) {
        return false;
    }
    const auto begin = execution.sequence_ids.begin()
        + static_cast<std::ptrdiff_t>(sequence_offset);
    const auto end = begin + static_cast<std::ptrdiff_t>(count);
    return std::find(begin, end, static_cast<llama_seq_id>(owner.sequence_id)) != end;
}

} // namespace

bool StageRuntime::collect_physical_tensors(
        bool terminal, std::vector<PhysicalTensor> * result, std::string * error) {
    if (result == nullptr || !loaded()) {
        return physical_fail("invalid physical tensor collection", error);
    }
    const auto count = terminal
        ? llama_linkcpp_terminal_count(ctx_)
        : llama_linkcpp_output_count(ctx_);
    if (count < 0) return physical_fail("llama.cpp returned an invalid tensor count", error);
    result->clear();
    result->reserve(static_cast<std::size_t>(count));
    for (std::int32_t index = 0; index < count; ++index) {
        PhysicalTensor tensor;
        const bool described = terminal
            ? llama_linkcpp_terminal_desc(ctx_, index, &tensor.descriptor)
            : llama_linkcpp_output_desc(ctx_, index, &tensor.descriptor);
        if (!described || tensor.descriptor.n_dims <= 0
            || tensor.descriptor.n_dims > 4
            || tensor.descriptor.nbytes > std::numeric_limits<std::size_t>::max()) {
            return physical_fail("llama.cpp returned an invalid tensor descriptor", error);
        }
        if (tensor.descriptor.alias_of < 0) {
            tensor.data.resize(static_cast<std::size_t>(tensor.descriptor.nbytes));
            const bool copied = terminal
                ? llama_linkcpp_terminal_get(ctx_, index, tensor.data.data(), tensor.data.size())
                : llama_linkcpp_output_get(ctx_, index, tensor.data.data(), tensor.data.size());
            if (!copied) return physical_fail("llama.cpp rejected physical tensor copy", error);
        }
        result->push_back(std::move(tensor));
    }
    if (!terminal && !llama_linkcpp_output_synchronize(ctx_)) {
        return physical_fail("llama.cpp failed to synchronize physical cut-set", error);
    }
    return true;
}

bool StageRuntime::capture_execution(
        llama_context * context, const llama_linkcpp_stage_invocation * invocation) {
    if (context == nullptr || context != ctx_ || invocation == nullptr
        || invocation->version != 1 || invocation->reserved != 0
        || invocation->n_tokens == 0 || invocation->n_pos == 0
        || invocation->n_pos > 4 || invocation->pos == nullptr
        || invocation->n_seq_id == nullptr || invocation->seq_id == nullptr
        || invocation->output == nullptr) {
        capture_error_ = "invalid llama.cpp physical invocation";
        return false;
    }
    PhysicalExecution captured;
    captured.flags = invocation->flags;
    captured.n_seq_tokens = invocation->n_seq_tokens;
    captured.n_seqs = invocation->n_seqs;
    captured.n_seqs_unq = invocation->n_seqs_unq;
    captured.n_pos = invocation->n_pos;
    captured.positions.assign(
        invocation->pos,
        invocation->pos + static_cast<std::size_t>(invocation->n_tokens)
                            * invocation->n_pos);
    captured.sequence_counts.assign(
        invocation->n_seq_id, invocation->n_seq_id + invocation->n_tokens);
    captured.output.assign(
        invocation->output, invocation->output + invocation->n_tokens);
    for (std::uint32_t row = 0; row < invocation->n_tokens; ++row) {
        const auto count = invocation->n_seq_id[row];
        if (count <= 0 || invocation->seq_id[row] == nullptr) {
            capture_error_ = "invalid llama.cpp physical sequence membership";
            return false;
        }
        captured.sequence_ids.insert(
            captured.sequence_ids.end(), invocation->seq_id[row],
            invocation->seq_id[row] + count);
    }
    if (!collect_physical_tensors(false, &captured.tensors, &capture_error_)
        || captured.tensors.empty()) return false;
    captured_executions_.push_back(std::move(captured));
    return true;
}

bool StageRuntime::execute_first_batch(
        const std::vector<LogicalRow> & rows,
        const std::vector<PhysicalOwner> & owners,
        std::vector<PhysicalExecution> * executions,
        std::string * error) {
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || config_.layer_begin != 0 || tail_stage_
        || rows.empty() || owners.size() != rows.size() || executions == nullptr
        || rows.size() > llama_n_batch(ctx_)) {
        return physical_fail("invalid first-stage logical batch", error);
    }
    if (!prepare_physical_owners(owners, error)) return false;
    captured_executions_.clear();
    capture_error_.clear();
    llama_batch batch = llama_batch_init(
        static_cast<std::int32_t>(rows.size()), 0, 1);
    if (batch.token == nullptr || batch.pos == nullptr
        || batch.n_seq_id == nullptr || batch.seq_id == nullptr
        || batch.logits == nullptr) {
        llama_batch_free(batch);
        return physical_fail("llama.cpp could not allocate logical batch", error);
    }
    batch.n_tokens = static_cast<std::int32_t>(rows.size());
    for (std::size_t index = 0; index < rows.size(); ++index) {
        const auto & row = rows[index];
        if (row.sequence_id < 0
            || static_cast<std::uint32_t>(row.sequence_id) >= llama_n_seq_max(ctx_)) {
            llama_batch_free(batch);
            return physical_fail("logical row sequence is outside the loaded context", error);
        }
        batch.token[index] = row.token;
        batch.pos[index] = row.position;
        batch.n_seq_id[index] = 1;
        batch.seq_id[index][0] = row.sequence_id;
        batch.logits[index] = row.output ? 1 : 0;
    }
    const bool encoder = llama_model_has_encoder(model_);
    const auto raw = encoder ? llama_encode(ctx_, batch) : llama_decode(ctx_, batch);
    llama_batch_free(batch);
    if (raw != 0 || captured_executions_.empty()) {
        const auto status = decode_status_from_raw(raw);
        const bool capture_missing = captured_executions_.empty();
        const bool memory_dirty = (!encoder && decode_status_leaves_memory_dirty(status))
            || (raw == 0 && capture_missing);
        hop_memory_dirty_ = hop_memory_dirty_ || memory_dirty;
        if (error != nullptr) {
            *error = "llama.cpp first-stage logical batch failed"
                ";operation=" + std::string(encoder ? "encode" : "decode")
                + ";raw_status=" + std::to_string(raw)
                + ";status=" + std::string(
                    encoder ? (raw == 0 ? "success" : "error")
                            : decode_status_name(status))
                + ";captured_ubatches=" + std::to_string(captured_executions_.size())
                + ";capture_missing=" + std::string(capture_missing ? "1" : "0")
                + ";memory_dirty=" + std::string(memory_dirty ? "1" : "0")
                + ";action=" + std::string(memory_dirty ? "reload" : "do_not_retry_blindly");
            if (!capture_error_.empty()) *error += ";capture_error=" + capture_error_;
        }
        captured_executions_.clear();
        return false;
    }
    const auto atomic = std::find_if(
        owners.begin(), owners.end(), [](const PhysicalOwner & owner) {
            return owner.phase == PhysicalPhase::Verify
                || owner.phase == PhysicalPhase::Replay;
        });
    const auto atomic_begin = static_cast<std::size_t>(
        std::distance(owners.begin(), atomic));
    const auto atomic_count = atomic == owners.end()
        ? 0U : static_cast<std::size_t>(atomic->speculative_count);
    bool atomic_is_terminal = atomic_count == 0;
    if (atomic_count > 0 && atomic_count <= owners.size() - atomic_begin
        && atomic_begin + atomic_count == owners.size()
        && !captured_executions_.empty()) {
        const auto & execution = captured_executions_.back();
        const auto physical_rows = execution.sequence_counts.size();
        atomic_is_terminal = atomic_count <= physical_rows;
        for (std::size_t offset = 0; atomic_is_terminal && offset < atomic_count; ++offset) {
            atomic_is_terminal = execution_row_matches_owner(
                execution, physical_rows - atomic_count + offset,
                owners[atomic_begin + offset]);
        }
    }
    if (!atomic_is_terminal) {
        const auto sequence_id = static_cast<llama_seq_id>(atomic->sequence_id);
        std::vector<llama_token> ignored_proposal;
        if (!settle_physical_sequence(
                sequence_id, static_cast<llama_pos>(atomic->position), true,
                &ignored_proposal, error)) {
            hop_memory_dirty_ = true;
            if (error != nullptr) *error += ";memory_dirty=1;action=reload";
            captured_executions_.clear();
            return false;
        }
        hop_memory_dirty_ = true;
        captured_executions_.clear();
        if (error != nullptr) {
            *error = "atomic verification/replay group was not the final llama.cpp physical group: "
                + atomic->sequence_key + ";memory_dirty=1;action=reload";
        }
        return false;
    }
    if (atomic_count > 0 && config_.layer_begin == 0
        && (atomic->phase == PhysicalPhase::Replay
            || (atomic->phase == PhysicalPhase::Verify
                && target_seq_rm_type_ != COMMON_CONTEXT_SEQ_RM_TYPE_FULL
                && !(target_seq_rm_type_ == COMMON_CONTEXT_SEQ_RM_TYPE_RS
                    && atomic_count - 1 > llama_n_rs_seq(ctx_))))) {
        // Stage zero takes an unconditional checkpoint only so a physical
        // ubatch split can be aborted before a cut-set leaves this process.
        // Once the group is intact, normal PART/RS rollback is sufficient.
        physical_checkpoints_.erase(
            static_cast<llama_seq_id>(atomic->sequence_id));
    }
    std::size_t captured_rows = 0;
    for (const auto & execution : captured_executions_) {
        captured_rows += execution.sequence_counts.size();
    }
    if (captured_rows != rows.size()) {
        hop_memory_dirty_ = true;
        captured_executions_.clear();
        return physical_fail(
            "llama.cpp physical ubatches omitted logical rows;memory_dirty=1;action=reload",
            error);
    }
    *executions = std::move(captured_executions_);
    captured_executions_.clear();
    return true;
}

bool StageRuntime::execute_physical(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        PhysicalExecution * output,
        std::string * error) {
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || config_.layer_begin == 0 || output == nullptr
        || !valid_execution(input) || input.tensors.empty()
        || owners.size() != input.sequence_counts.size()) {
        return physical_fail("invalid downstream physical execution", error);
    }
    if (!prepare_physical_execution(input, owners, error)) return false;
    llama_linkcpp_input_clear(ctx_);
    for (std::size_t index = 0; index < input.tensors.size(); ++index) {
        const auto & tensor = input.tensors[index];
        const bool alias = tensor.descriptor.alias_of >= 0;
        const auto source = alias
            ? static_cast<std::size_t>(tensor.descriptor.alias_of) : index;
        if (source >= input.tensors.size()) {
            llama_linkcpp_input_clear(ctx_);
            return physical_fail("physical cut-set alias is out of range", error);
        }
        const auto & source_data = input.tensors[source].data;
        if ((!alias && tensor.data.size() != tensor.descriptor.nbytes)
            || (alias && !tensor.data.empty())
            || source_data.size() != tensor.descriptor.nbytes
            || !llama_linkcpp_input_set_tensor(
                ctx_, &tensor.descriptor,
                source_data.data(), source_data.size())) {
            llama_linkcpp_input_clear(ctx_);
            return physical_fail("llama.cpp rejected the physical cut-set", error);
        }
    }
    std::vector<llama_token> tokens;
    tokens.reserve(owners.size());
    for (const auto & owner : owners) tokens.push_back(owner.input_token);
    std::vector<llama_seq_id *> sequence_rows(input.sequence_counts.size());
    std::size_t sequence_offset = 0;
    for (std::size_t row = 0; row < sequence_rows.size(); ++row) {
        sequence_rows[row] = const_cast<llama_seq_id *>(
            input.sequence_ids.data() + sequence_offset);
        sequence_offset += static_cast<std::size_t>(input.sequence_counts[row]);
    }
    llama_batch batch{
        static_cast<std::int32_t>(input.sequence_counts.size()),
        tokens.data(), nullptr,
        const_cast<llama_pos *>(input.positions.data()),
        const_cast<std::int32_t *>(input.sequence_counts.data()),
        sequence_rows.data(), const_cast<std::int8_t *>(input.output.data())};
    const bool encoder = (input.flags & LLAMA_LINKCPP_STAGE_FLAG_ENCODER) != 0;
    if ((input.flags & ~LLAMA_LINKCPP_STAGE_FLAG_ENCODER) != 0
        || (encoder && !llama_model_has_encoder(model_))) {
        llama_linkcpp_input_clear(ctx_);
        return physical_fail("unsupported physical invocation flags", error);
    }
    const auto raw = encoder ? llama_encode(ctx_, batch) : llama_decode(ctx_, batch);
    const bool input_mismatch = llama_linkcpp_input_count(ctx_)
        != static_cast<std::int32_t>(input.tensors.size());
    if (raw != 0 || input_mismatch) {
        const auto status = decode_status_from_raw(raw);
        const bool memory_dirty = (!encoder && decode_status_leaves_memory_dirty(status))
            || (raw == 0 && input_mismatch);
        hop_memory_dirty_ = hop_memory_dirty_ || memory_dirty;
        llama_linkcpp_input_clear(ctx_);
        if (error != nullptr) {
            *error = "llama.cpp downstream physical invocation failed"
                ";operation=" + std::string(encoder ? "encode" : "decode")
                + ";raw_status=" + std::to_string(raw)
                + ";status=" + std::string(
                    encoder ? (raw == 0 ? "success" : "error")
                            : decode_status_name(status))
                + ";input_mismatch=" + std::string(input_mismatch ? "1" : "0")
                + ";memory_dirty=" + std::string(memory_dirty ? "1" : "0")
                + ";action=" + std::string(memory_dirty ? "reload" : "do_not_retry_blindly");
        }
        return false;
    }
    if (!encoder && !process_physical_mtp(batch, owners, error)) {
        hop_memory_dirty_ = true;
        if (error != nullptr) *error += ";memory_dirty=1;action=reload";
        return false;
    }
    *output = input;
    output->tensors.clear();
    output->terminal = tail_stage_;
    // Terminal logits and h_nextn remain inside llama.cpp. Sampling and MTP
    // consume them in this process; only compact token decisions cross P4.
    if (tail_stage_) return true;
    if (!collect_physical_tensors(false, &output->tensors, error)) {
        hop_memory_dirty_ = true;
        if (error != nullptr) *error += ";memory_dirty=1;action=reload";
        return false;
    }
    if (output->tensors.empty()) {
        hop_memory_dirty_ = true;
        return physical_fail(
            "llama.cpp produced no physical result tensors;memory_dirty=1;action=reload",
            error);
    }
    return true;
}

bool StageRuntime::prepare_physical_execution(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::string * error) {
    if (!valid_execution(input) || owners.size() != input.sequence_counts.size()) {
        return physical_fail("invalid physical preparation", error);
    }
    return prepare_physical_owners(owners, error);
}

} // namespace staged::llama_runtime
