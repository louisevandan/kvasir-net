#include "server.hpp"

#ifdef P4_STAGED_WITH_LLAMA
#include "physical_wire.hpp"

#include <algorithm>
#include <limits>
#include <utility>
#endif

namespace staged::server {

#ifdef P4_STAGED_WITH_LLAMA
namespace {

bool physical_row_matches(
        const llama_runtime::PhysicalExecution & execution,
        std::size_t row,
        const llama_runtime::LogicalExecutionRow & logical) {
    const auto position_index = row * execution.n_pos;
    if (position_index >= execution.positions.size()
        || execution.positions[position_index] != static_cast<llama_pos>(logical.owner.position)) {
        return false;
    }
    std::size_t sequence_offset = 0;
    for (std::size_t index = 0; index < row; ++index) {
        sequence_offset += static_cast<std::size_t>(execution.sequence_counts[index]);
    }
    const auto count = static_cast<std::size_t>(execution.sequence_counts[row]);
    const auto begin = execution.sequence_ids.begin()
        + static_cast<std::ptrdiff_t>(sequence_offset);
    const auto end = begin + static_cast<std::ptrdiff_t>(count);
    return std::find(begin, end, static_cast<llama_seq_id>(logical.owner.sequence_id)) != end;
}

protocol::Frame physical_error(
        const Session & session, const std::string & detail) {
    // Kept local only to make the two handlers' failure path visibly
    // identical; Session::error is private, so the callers construct it.
    (void) session;
    return protocol::Frame::make(protocol::Operation::Error,
        std::vector<std::uint8_t>(detail.begin(), detail.end()));
}

} // namespace
#endif

protocol::Frame Session::handle_logical_batch(const protocol::Frame & request) {
#ifndef P4_STAGED_WITH_LLAMA
    (void) request;
    return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
#else
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
    }
    const auto active = runtime_.begin_hop();
    if (!active.ok()) return error("LOGICAL_BATCH rejected: invalid session state");
    auto fail = [&](const std::string & detail) {
        (void) runtime_.cancel();
        return physical_error(*this, "LOGICAL_BATCH failed: " + detail);
    };
    std::vector<llama_runtime::LogicalExecutionRow> input;
    std::string detail;
    if (!llama_runtime::decode_logical_batch(request.body, &input, &detail)) {
        return fail(detail);
    }
    std::vector<llama_runtime::LogicalRow> rows;
    rows.reserve(input.size());
    for (const auto & value : input) {
        if (value.owner.sequence_id > static_cast<std::uint32_t>(
                std::numeric_limits<llama_seq_id>::max())) {
            return fail("logical sequence id is out of range");
        }
        rows.push_back({value.token, static_cast<llama_pos>(value.owner.position),
                        static_cast<llama_seq_id>(value.owner.sequence_id),
                        value.owner.output});
    }
    std::vector<llama_runtime::PhysicalExecution> captured;
    if (!llama_runtime_->execute_first_batch(rows, &captured, &detail)) {
        return fail(detail);
    }
    std::vector<bool> used(input.size(), false);
    std::vector<llama_runtime::RoutedPhysicalExecution> output;
    output.reserve(captured.size());
    for (auto & execution : captured) {
        llama_runtime::RoutedPhysicalExecution capsule;
        capsule.execution_id = next_physical_execution_id_++;
        if (capsule.execution_id == 0) capsule.execution_id = next_physical_execution_id_++;
        capsule.execution = std::move(execution);
        const auto physical_rows = capsule.execution.sequence_counts.size();
        capsule.owners.reserve(physical_rows);
        for (std::size_t row = 0; row < physical_rows; ++row) {
            auto found = input.size();
            for (std::size_t logical = 0; logical < input.size(); ++logical) {
                if (!used[logical]
                    && physical_row_matches(capsule.execution, row, input[logical])) {
                    found = logical;
                    break;
                }
            }
            if (found == input.size()) return fail("physical row has no logical owner");
            used[found] = true;
            capsule.owners.push_back(input[found].owner);
            // The compatibility layer may force one cut-set output on an
            // otherwise output-free ubatch. Restore the logical mask before
            // the capsule leaves this process.
            capsule.execution.output[row] = input[found].owner.output ? 1 : 0;
        }
        output.push_back(std::move(capsule));
    }
    if (std::find(used.begin(), used.end(), false) != used.end()) {
        return fail("logical rows were omitted from physical ubatches");
    }
    std::vector<std::uint8_t> body;
    if (!llama_runtime::encode_physical_set(output, &body, &detail)) return fail(detail);
    if (!runtime_.finish_hop().ok()) return fail("session transition failed");
    return protocol::Frame::make(protocol::Operation::PhysicalResult, std::move(body));
#endif
}

protocol::Frame Session::handle_tokenize(const protocol::Frame & request) {
#ifndef P4_STAGED_WITH_LLAMA
    (void) request;
    return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
#else
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
    }
    const std::string prompt(request.body.begin(), request.body.end());
    std::vector<std::int32_t> tokens;
    std::string detail;
    if (!llama_runtime_->tokenize_prompt(prompt, &tokens, &detail)) {
        return error("TOKENIZE failed: " + detail);
    }
    if (tokens.size() > std::numeric_limits<std::uint32_t>::max()) {
        return error("TOKENIZE failed: token count is out of range");
    }
    std::vector<std::uint8_t> body;
    body.reserve(4 + tokens.size() * 4);
    const auto count = static_cast<std::uint32_t>(tokens.size());
    for (unsigned shift = 0; shift < 32; shift += 8) {
        body.push_back(static_cast<std::uint8_t>(count >> shift));
    }
    for (const auto token : tokens) {
        const auto value = static_cast<std::uint32_t>(token);
        for (unsigned shift = 0; shift < 32; shift += 8) {
            body.push_back(static_cast<std::uint8_t>(value >> shift));
        }
    }
    return protocol::Frame::make(protocol::Operation::Tokenized, std::move(body));
#endif
}

protocol::Frame Session::handle_physical_batch(const protocol::Frame & request) {
#ifndef P4_STAGED_WITH_LLAMA
    (void) request;
    return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
#else
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        return error("CAPABILITY_UNAVAILABLE: physical llama runtime is unavailable");
    }
    const auto active = runtime_.begin_hop();
    if (!active.ok()) return error("PHYSICAL_BATCH rejected: invalid session state");
    auto fail = [&](const std::string & detail) {
        (void) runtime_.cancel();
        return physical_error(*this, "PHYSICAL_BATCH failed: " + detail);
    };
    std::vector<llama_runtime::RoutedPhysicalExecution> input;
    std::string detail;
    if (!llama_runtime::decode_physical_set(request.body, &input, &detail)) {
        return fail(detail);
    }
    std::vector<llama_runtime::RoutedPhysicalExecution> output;
    output.reserve(input.size());
    for (auto & capsule : input) {
        llama_runtime::RoutedPhysicalExecution result;
        result.execution_id = capsule.execution_id;
        result.owners = std::move(capsule.owners);
        if (!llama_runtime_->execute_physical(
                capsule.execution, &result.execution, &detail)) return fail(detail);
        if (result.execution.terminal
            && !llama_runtime_->sample_physical_outputs(
                result.execution, result.owners, &result.outcomes, &detail)) return fail(detail);
        output.push_back(std::move(result));
    }
    std::vector<std::uint8_t> body;
    if (!llama_runtime::encode_physical_set(output, &body, &detail)) return fail(detail);
    if (!runtime_.finish_hop().ok()) return fail("session transition failed");
    return protocol::Frame::make(protocol::Operation::PhysicalResult, std::move(body));
#endif
}

} // namespace staged::server
