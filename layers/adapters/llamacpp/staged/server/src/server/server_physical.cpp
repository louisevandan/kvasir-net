#include "server.hpp"

#ifdef P4_STAGED_WITH_LLAMA
#include "physical_wire.hpp"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <utility>
#endif

namespace staged::server {

protocol::Frame Session::handle_bind_load(const protocol::Frame & request) {
    if (request.body.size() != 8 || runtime_.state() != runtime::State::Ready) {
        return error("BIND_LOAD rejected: invalid state or payload");
    }
    std::uint64_t generation = 0;
    for (unsigned i = 0; i < 8; ++i) generation |= std::uint64_t(request.body[i]) << (i * 8U);
    std::uint32_t capacity = 1;
#ifdef P4_STAGED_WITH_LLAMA
    if (llama_runtime_ != nullptr) capacity = llama_runtime_->sequence_capacity();
#endif
    std::string detail;
    if (!physical_authority_.bind(generation, capacity, &detail)) return error(detail);
    return protocol::Frame::make(protocol::Operation::BindLoad, request.body);
}

#ifdef P4_STAGED_WITH_LLAMA
namespace {

// A step's parts, on stderr, so the 34 ms per batch that dominates a
// narrow-batch run can be attributed.
//
// The adapter measures a batch's whole stage call and a fit across widths
// puts it at 34.2 ms per batch plus 1.051 ms per row, which is why a wide
// batch is 3.4x more efficient per row. That fit says nothing about which
// of decoding the request, running the graph, matching owners and encoding
// the reply the 34 ms is. These four timers do.
//
// Off unless P4_STAGED_TRACE_STEP is set. Written to the stderr the stage
// servers already share with the agent, which the harness collects.
bool step_trace_enabled() {
    static const bool enabled = std::getenv("P4_STAGED_TRACE_STEP") != nullptr;
    return enabled;
}

using step_clock = std::chrono::steady_clock;

std::int64_t step_us(step_clock::time_point from, step_clock::time_point to) {
    return std::chrono::duration_cast<std::chrono::microseconds>(to - from).count();
}

bool physical_row_matches(
        const llama_runtime::PhysicalExecution & execution,
        std::size_t row,
        const llama_runtime::LogicalExecutionRow & logical) {
    const auto position_index = row;
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

runtime::PhysicalAdmission admission(const llama_runtime::PhysicalOwner & owner) {
    return {{owner.load_generation, owner.incarnation, owner.sequence_id,
        owner.session_id, owner.sequence_key},
        owner.phase == llama_runtime::PhysicalPhase::Prefill && owner.position == 0};
}

} // namespace
#endif

protocol::Frame Session::handle_physical_settle(
        const protocol::Frame & request) {
#ifndef P4_STAGED_WITH_LLAMA
    (void) request;
    return error("CAPABILITY_UNAVAILABLE: llama runtime is unavailable");
#else
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        return error("PHYSICAL_SETTLE rejected: invalid runtime or payload");
    }
    runtime::PhysicalControlIdentity control;
    std::string detail;
    if (!runtime::decode_physical_control_identity(request.body, &control, &detail)) return error(detail);
    const auto prefix = control.prefix_bytes;
    if (request.body.size() - prefix < 12 || (request.body.size() - prefix - 12) % 4 != 0) {
        return error("PHYSICAL_SETTLE rejected: truncated identity-bound payload");
    }
    auto read_u32 = [&](std::size_t offset) {
        return static_cast<std::uint32_t>(request.body[offset])
            | static_cast<std::uint32_t>(request.body[offset + 1]) << 8U
            | static_cast<std::uint32_t>(request.body[offset + 2]) << 16U
            | static_cast<std::uint32_t>(request.body[offset + 3]) << 24U;
    };
    const auto id = control.identity.slot;
    const auto retain_from = read_u32(prefix);
    const auto replay_position = read_u32(prefix + 4);
    const auto replay_count = read_u32(prefix + 8);
    if (request.body.size() != prefix + 12ULL + 4ULL * replay_count
        || retain_from > std::uint32_t(std::numeric_limits<llama_pos>::max())
        || (replay_count == 0 && replay_position != 0)
        || (replay_count != 0
            && static_cast<std::uint64_t>(replay_position) + replay_count != retain_from)
        || id > static_cast<std::uint32_t>(std::numeric_limits<llama_seq_id>::max())) {
        return error("PHYSICAL_SETTLE rejected: inconsistent settlement");
    }
    std::vector<std::uint8_t> cached;
    const auto decision = physical_authority_.prepare_control(control, false, request.body, &cached, &detail);
    if (decision == runtime::PhysicalAuthority::ControlDecision::Rejected) return error(detail);
    if (decision == runtime::PhysicalAuthority::ControlDecision::Replay) {
        return protocol::Frame::make(protocol::Operation::PhysicalSettle, std::move(cached));
    }
    std::vector<llama_token> proposal;
    const auto rollback_from = replay_count == 0 ? retain_from : replay_position;
    if (!llama_runtime_->settle_physical_sequence(
            static_cast<llama_seq_id>(id), static_cast<llama_pos>(rollback_from),
            replay_count != 0, &proposal, &detail)) {
        physical_authority_.fence();
        return error("PHYSICAL_SETTLE failed: " + detail);
    }
    if (proposal.size() > std::numeric_limits<std::uint32_t>::max()
        || proposal.size() > (runtime::PhysicalAuthority::max_control_bytes - prefix - 4) / 4) {
        physical_authority_.fence();
        return error("PHYSICAL_SETTLE failed: proposal is too large");
    }
    std::vector<std::uint8_t> body(request.body.begin(), request.body.begin() + prefix);
    body.reserve(prefix + 4 + proposal.size() * 4);
    const auto count = static_cast<std::uint32_t>(proposal.size());
    for (unsigned shift = 0; shift < 32; shift += 8) {
        body.push_back(static_cast<std::uint8_t>(count >> shift));
    }
    for (const auto token : proposal) {
        const auto value = static_cast<std::uint32_t>(token);
        for (unsigned shift = 0; shift < 32; shift += 8) {
            body.push_back(static_cast<std::uint8_t>(value >> shift));
        }
    }
    physical_authority_.commit_control(control, false, request.body, body);
    return protocol::Frame::make(protocol::Operation::PhysicalSettle, std::move(body));
#endif
}

protocol::Frame Session::handle_physical_release(
        const protocol::Frame & request) {
#ifndef P4_STAGED_WITH_LLAMA
    (void) request;
    return error("CAPABILITY_UNAVAILABLE: llama runtime is unavailable");
#else
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        return error("PHYSICAL_RELEASE rejected: invalid runtime or payload");
    }
    std::string detail;
    runtime::PhysicalControlIdentity control;
    if (!runtime::decode_physical_control_identity(request.body, &control, &detail)
        || control.prefix_bytes != request.body.size()) return error("PHYSICAL_RELEASE rejected: invalid identity-bound payload");
    const auto id = control.identity.slot;
    const auto & key = control.identity.key;
    std::vector<std::uint8_t> cached;
    const auto decision = physical_authority_.prepare_control(control, true, request.body, &cached, &detail);
    if (decision == runtime::PhysicalAuthority::ControlDecision::Rejected) return error(detail);
    if (decision == runtime::PhysicalAuthority::ControlDecision::Replay) {
        return protocol::Frame::make(protocol::Operation::PhysicalRelease, std::move(cached));
    }
    if (id > static_cast<std::uint32_t>(std::numeric_limits<llama_seq_id>::max())
        || !llama_runtime_->release_physical_sequence(
            key, static_cast<llama_seq_id>(id), &detail)) {
        physical_authority_.fence();
        return error("PHYSICAL_RELEASE failed: " + detail);
    }
    physical_authority_.commit_control(control, true, request.body, request.body);
    return protocol::Frame::make(protocol::Operation::PhysicalRelease, request.body);
#endif
}

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
    bool executed = false;
    auto fail = [&](const std::string & detail) {
        auto failure = detail;
        if (executed) {
            physical_authority_.fence();
            llama_runtime_->quarantine_physical_memory();
            failure += ";memory_dirty=1;action=reload";
        }
        (void) runtime_.cancel();
        return physical_error(*this, "LOGICAL_BATCH failed: " + failure);
    };
    const auto step_began = step_clock::now();
    std::vector<llama_runtime::LogicalExecutionRow> input;
    std::string detail;
    if (!llama_runtime::decode_logical_batch(request.body, &input, &detail)) {
        return fail(detail);
    }
    const auto step_parsed = step_clock::now();
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
    std::vector<llama_runtime::PhysicalOwner> owners;
    owners.reserve(input.size());
    for (const auto & value : input) owners.push_back(value.owner);
    std::vector<runtime::PhysicalAdmission> identities;
    identities.reserve(owners.size());
    for (const auto & owner : owners) identities.push_back(admission(owner));
    runtime::PhysicalAuthority::RowsPlan owner_plan;
    if (!physical_authority_.prepare_rows(identities, &owner_plan, &detail)) return fail(detail);
    // Once a native attempt starts, even a failed response may follow KV or
    // checkpoint side effects. Never silently return to an unowned empty slot.
    executed = true;
    if (!llama_runtime_->execute_first_batch(rows, owners, &captured, &detail)) {
        return fail(detail);
    }
    const auto step_executed = step_clock::now();
    executed = true;
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
    const auto step_matched = step_clock::now();
    std::vector<std::uint8_t> body;
    if (!llama_runtime::encode_physical_set(output, &body, &detail)) return fail(detail);
    const auto step_encoded = step_clock::now();
    if (step_trace_enabled()) {
        std::fprintf(stderr,
            "P4_STAGED_STEP role=first rows=%zu parse_us=%lld decode_us=%lld"
            " match_us=%lld encode_us=%lld bytes=%zu\n",
            input.size(),
            static_cast<long long>(step_us(step_began, step_parsed)),
            static_cast<long long>(step_us(step_parsed, step_executed)),
            static_cast<long long>(step_us(step_executed, step_matched)),
            static_cast<long long>(step_us(step_matched, step_encoded)),
            body.size());
    }
    if (!runtime_.finish_hop().ok()) return fail("session transition failed");
    physical_authority_.commit_rows(owner_plan);
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
    bool executed = false;
    auto fail = [&](const std::string & detail) {
        auto failure = detail;
        if (executed) {
            physical_authority_.fence();
            llama_runtime_->quarantine_physical_memory();
            failure += ";memory_dirty=1;action=reload";
        }
        (void) runtime_.cancel();
        return physical_error(*this, "PHYSICAL_BATCH failed: " + failure);
    };
    const auto step_began = step_clock::now();
    std::vector<llama_runtime::RoutedPhysicalExecution> input;
    std::string detail;
    if (!llama_runtime::decode_physical_set(request.body, &input, &detail)) {
        return fail(detail);
    }
    std::vector<runtime::PhysicalAdmission> identities;
    for (const auto & capsule : input) {
        for (const auto & owner : capsule.owners) identities.push_back(admission(owner));
    }
    runtime::PhysicalAuthority::RowsPlan owner_plan;
    if (!physical_authority_.prepare_rows(identities, &owner_plan, &detail)) return fail(detail);
    const auto step_parsed = step_clock::now();
    std::int64_t decode_us = 0;
    std::int64_t sample_us = 0;
    std::size_t step_rows = 0;
    std::vector<llama_runtime::RoutedPhysicalExecution> output;
    output.reserve(input.size());
    for (auto & capsule : input) {
        llama_runtime::RoutedPhysicalExecution result;
        result.execution_id = capsule.execution_id;
        result.owners = std::move(capsule.owners);
        step_rows += result.owners.size();
        const auto capsule_began = step_clock::now();
        executed = true;
        if (!llama_runtime_->execute_physical(
                capsule.execution, result.owners, &result.execution, &detail)) return fail(detail);
        const auto capsule_executed = step_clock::now();
        decode_us += step_us(capsule_began, capsule_executed);
        executed = true;
        if (result.execution.terminal
            && !llama_runtime_->sample_physical_outputs(
                result.execution, result.owners, &result.outcomes, &detail)) return fail(detail);
        sample_us += step_us(capsule_executed, step_clock::now());
        if (result.execution.terminal) result.execution.tensors.clear();
        output.push_back(std::move(result));
    }
    const auto step_ready = step_clock::now();
    std::vector<std::uint8_t> body;
    if (!llama_runtime::encode_physical_set(output, &body, &detail)) return fail(detail);
    if (step_trace_enabled()) {
        std::fprintf(stderr,
            "P4_STAGED_STEP role=downstream rows=%zu parse_us=%lld decode_us=%lld"
            " sample_us=%lld encode_us=%lld bytes=%zu\n",
            step_rows,
            static_cast<long long>(step_us(step_began, step_parsed)),
            static_cast<long long>(decode_us),
            static_cast<long long>(sample_us),
            static_cast<long long>(step_us(step_ready, step_clock::now())),
            body.size());
    }
    if (!runtime_.finish_hop().ok()) return fail("session transition failed");
    physical_authority_.commit_rows(owner_plan);
    return protocol::Frame::make(protocol::Operation::PhysicalResult, std::move(body));
#endif
}

} // namespace staged::server
