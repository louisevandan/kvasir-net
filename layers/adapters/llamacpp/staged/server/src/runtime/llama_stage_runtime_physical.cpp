#include "llama_stage_runtime.hpp"

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
        std::vector<PhysicalExecution> * executions,
        std::string * error) {
    if (!loaded() || config_.layer_begin != 0 || tail_stage_
        || rows.empty() || executions == nullptr
        || rows.size() > llama_n_batch(ctx_)) {
        return physical_fail("invalid first-stage logical batch", error);
    }
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
    const auto raw = llama_model_has_encoder(model_)
        ? llama_encode(ctx_, batch) : llama_decode(ctx_, batch);
    llama_batch_free(batch);
    if (raw != 0 || captured_executions_.empty()) {
        if (error != nullptr) {
            *error = capture_error_.empty()
                ? "llama.cpp failed the first-stage logical batch"
                : capture_error_;
        }
        captured_executions_.clear();
        return false;
    }
    std::size_t captured_rows = 0;
    for (const auto & execution : captured_executions_) {
        captured_rows += execution.sequence_counts.size();
    }
    if (captured_rows != rows.size()) {
        captured_executions_.clear();
        return physical_fail("llama.cpp physical ubatches omitted logical rows", error);
    }
    *executions = std::move(captured_executions_);
    captured_executions_.clear();
    return true;
}

bool StageRuntime::execute_physical(
        const PhysicalExecution & input,
        PhysicalExecution * output,
        std::string * error) {
    if (!loaded() || config_.layer_begin == 0 || output == nullptr
        || !valid_execution(input) || input.tensors.empty()) {
        return physical_fail("invalid downstream physical execution", error);
    }
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
    std::vector<llama_token> tokens(input.sequence_counts.size(), 0);
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
    if (raw != 0 || llama_linkcpp_input_count(ctx_)
        != static_cast<std::int32_t>(input.tensors.size())) {
        llama_linkcpp_input_clear(ctx_);
        return physical_fail("llama.cpp failed the downstream physical invocation", error);
    }
    *output = input;
    output->tensors.clear();
    output->terminal = tail_stage_;
    const auto wants_output = std::any_of(input.output.begin(), input.output.end(),
                                          [](std::int8_t value) { return value != 0; });
    if (tail_stage_ && !wants_output) return true;
    if (!collect_physical_tensors(tail_stage_, &output->tensors, error)) return false;
    if (output->tensors.empty()) {
        return physical_fail("llama.cpp produced no physical result tensors", error);
    }
    return true;
}

} // namespace staged::llama_runtime
