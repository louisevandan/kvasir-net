#include "physical_wire.hpp"
#include "physical_wire_cursor.hpp"

#include <algorithm>
#include <climits>
#include <cstring>
#include <unordered_set>

namespace staged::llama_runtime {
namespace wire = physical_wire;

namespace {

bool read_owner(wire::Cursor & cursor, PhysicalOwner * owner) {
    std::uint8_t phase = 0;
    std::uint8_t output = 0;
    std::uint16_t reserved = 0;
    if (!cursor.u64(&owner->load_generation) || owner->load_generation == 0
        || !cursor.string(&owner->request_id)
        || !cursor.string(&owner->sequence_key)
        || !cursor.string(&owner->session_id)
        || !cursor.string(&owner->reply)
        || !cursor.u32(&owner->sequence_id)
        || !cursor.byte(&phase) || phase > 3
        || !cursor.byte(&output) || output > 1
        || !cursor.u16(&reserved) || reserved != 0
        || !cursor.u32(&owner->position) || owner->position > INT32_MAX
        || !cursor.u32(&owner->max_tokens) || owner->max_tokens == 0
        || !cursor.u32(&owner->generated_tokens)
        || owner->generated_tokens >= owner->max_tokens
        || !cursor.i32(&owner->input_token)
        || !cursor.u64(&owner->speculative_id)
        || !cursor.u32(&owner->speculative_index)
        || !cursor.u32(&owner->speculative_count)
        || !cursor.string(&owner->options)
        || owner->request_id.empty() || owner->sequence_key.empty()
        || owner->session_id.empty() || owner->reply.empty()) return false;
    owner->phase = static_cast<PhysicalPhase>(phase);
    owner->output = output != 0;
    const bool speculative = owner->phase == PhysicalPhase::Verify
        || owner->phase == PhysicalPhase::Replay;
    return (!speculative && owner->speculative_id == 0
            && owner->speculative_index == 0 && owner->speculative_count == 0)
        || (speculative && owner->speculative_id != 0
            && owner->speculative_count != 0
            && owner->speculative_index < owner->speculative_count
            && ((owner->phase == PhysicalPhase::Verify && owner->output)
                || (owner->phase == PhysicalPhase::Replay && !owner->output)));
}

bool read_tensor(wire::Cursor & cursor, std::size_t index, PhysicalTensor * tensor) {
    std::uint8_t dimensions = 0;
    const std::uint8_t * reserved = nullptr;
    if (!cursor.i32(&tensor->descriptor.type)
        || !cursor.byte(&dimensions) || dimensions == 0 || dimensions > 4
        || !cursor.take(3, &reserved)
        || reserved[0] != 0 || reserved[1] != 0 || reserved[2] != 0) return false;
    tensor->descriptor.n_dims = dimensions;
    for (std::uint8_t dim = 0; dim < dimensions; ++dim) {
        if (!cursor.i64(&tensor->descriptor.ne[dim])
            || tensor->descriptor.ne[dim] <= 0) return false;
    }
    for (std::uint8_t dim = 0; dim < dimensions; ++dim) {
        if (!cursor.u64(&tensor->descriptor.nb[dim])) return false;
    }
    std::int32_t alias = -1;
    std::string name;
    std::uint64_t data_size = 0;
    if (!cursor.u64(&tensor->descriptor.nbytes)
        || !cursor.u64(&tensor->descriptor.view_offset)
        || !cursor.i32(&alias) || alias < -1
        || (alias >= 0 && static_cast<std::size_t>(alias) >= index)
        || !cursor.string(&name) || name.empty() || name.size() >= sizeof(tensor->descriptor.name)
        || !cursor.u64(&data_size) || data_size > SIZE_MAX) return false;
    const bool is_alias = alias >= 0;
    if ((is_alias && data_size != 0)
        || (!is_alias && data_size != tensor->descriptor.nbytes)) return false;
    const std::uint8_t * data = nullptr;
    if (!cursor.take(static_cast<std::size_t>(data_size), &data)) return false;
    tensor->descriptor.alias_of = alias;
    std::memcpy(tensor->descriptor.name, name.data(), name.size());
    tensor->descriptor.name[name.size()] = '\0';
    tensor->data.assign(data, data + data_size);
    return true;
}

bool read_capsule(wire::Cursor & cursor, RoutedPhysicalExecution * result) {
    std::uint32_t capsule_flags = 0;
    auto & execution = result->execution;
    std::uint32_t rows = 0;
    std::uint32_t sequence_total = 0;
    std::uint32_t tensor_count = 0;
    std::uint32_t outcome_count = 0;
    if (!cursor.u64(&result->execution_id) || result->execution_id == 0
        || !cursor.u32(&capsule_flags) || (capsule_flags & ~1U) != 0
        || !cursor.u32(&execution.flags)
        || !cursor.u32(&execution.n_seq_tokens)
        || !cursor.u32(&execution.n_seqs)
        || !cursor.u32(&execution.n_seqs_unq)
        || !cursor.u32(&execution.n_pos) || execution.n_pos == 0 || execution.n_pos > 4
        || !cursor.u32(&rows) || rows == 0 || rows > wire::kMaxRows
        || !cursor.u32(&sequence_total)
        || !cursor.u32(&tensor_count) || tensor_count > wire::kMaxTensors
        || !cursor.u32(&outcome_count)) return false;
    execution.terminal = (capsule_flags & 1U) != 0;
    execution.positions.resize(static_cast<std::size_t>(rows) * execution.n_pos);
    for (auto & value : execution.positions) if (!cursor.i32(&value)) return false;
    execution.sequence_counts.resize(rows);
    std::uint64_t counted_sequences = 0;
    for (auto & value : execution.sequence_counts) {
        std::uint32_t count = 0;
        if (!cursor.u32(&count) || count == 0 || count > INT32_MAX) return false;
        value = static_cast<std::int32_t>(count);
        counted_sequences += count;
    }
    if (counted_sequences != sequence_total) return false;
    execution.sequence_ids.resize(sequence_total);
    for (auto & value : execution.sequence_ids) if (!cursor.i32(&value)) return false;
    execution.output.resize(rows);
    for (auto & value : execution.output) {
        std::uint8_t raw = 0;
        if (!cursor.byte(&raw) || raw > 1) return false;
        value = static_cast<std::int8_t>(raw);
    }
    result->owners.resize(rows);
    for (std::size_t index = 0; index < rows; ++index) {
        if (!read_owner(cursor, &result->owners[index])
            || result->owners[index].output != (execution.output[index] != 0)
            || result->owners[index].position
                != static_cast<std::uint32_t>(execution.positions[index])) {
            return false;
        }
    }
    execution.tensors.resize(tensor_count);
    for (std::size_t index = 0; index < tensor_count; ++index) {
        if (!read_tensor(cursor, index, &execution.tensors[index])) return false;
    }
    result->outcomes.resize(outcome_count);
    std::unordered_set<std::uint32_t> outcome_owners;
    for (auto & outcome : result->outcomes) {
        std::uint32_t generated_count = 0;
        std::uint32_t proposal_count = 0;
        std::uint32_t replay_count = 0;
        std::int32_t retain_from = -1;
        if (!cursor.u32(&outcome.owner_index) || outcome.owner_index >= rows
            || !outcome_owners.insert(outcome.owner_index).second
            || !cursor.u32(&generated_count) || generated_count > wire::kMaxRows
            || !cursor.u32(&proposal_count) || proposal_count > wire::kMaxRows
            || !cursor.u32(&replay_count) || replay_count > wire::kMaxRows
            || (generated_count == 0 && proposal_count == 0 && replay_count == 0)
            || !cursor.i32(&retain_from) || retain_from < -1
            || !cursor.u32(&outcome.replay_position)) return false;
        const auto & owner = result->owners[outcome.owner_index];
        if ((retain_from < 0 && (replay_count != 0 || outcome.replay_position != 0))
            || (retain_from >= 0 && owner.phase != PhysicalPhase::Verify)
            || (retain_from >= 0 && replay_count == 0 && outcome.replay_position != 0)
            || (replay_count != 0
                && static_cast<std::uint64_t>(outcome.replay_position) + replay_count
                    != static_cast<std::uint64_t>(retain_from))) return false;
        outcome.retain_from = retain_from;
        outcome.generated.resize(generated_count);
        for (auto & generated : outcome.generated) {
            if (!cursor.i32(&generated.token) || !cursor.u32(&generated.position)
                || !cursor.string(&generated.text)
                || !cursor.string(&generated.stop)) return false;
        }
        outcome.proposal.resize(proposal_count);
        for (auto & token : outcome.proposal) if (!cursor.i32(&token)) return false;
        outcome.replay_tokens.resize(replay_count);
        for (auto & token : outcome.replay_tokens) if (!cursor.i32(&token)) return false;
    }
    return execution.terminal
        ? tensor_count == 0
        : outcome_count == 0 && tensor_count > 0;
}

} // namespace

bool decode_logical_batch(
        const std::vector<std::uint8_t> & bytes,
        std::vector<LogicalExecutionRow> * rows, std::string * error) {
    if (rows == nullptr) return wire::fail("logical batch result is null", error);
    wire::Cursor cursor(bytes);
    const std::uint8_t * magic = nullptr;
    std::uint16_t version = 0;
    std::uint16_t reserved = 0;
    std::uint32_t count = 0;
    if (!cursor.take(4, &magic) || std::memcmp(magic, "P4LB", 4) != 0
        || !cursor.u16(&version) || version != 3
        || !cursor.u16(&reserved) || reserved != 0
        || !cursor.u32(&count) || count == 0 || count > wire::kMaxRows) {
        return wire::fail("invalid logical batch header", error);
    }
    rows->clear();
    rows->resize(count);
    for (auto & row : *rows) {
        std::uint8_t phase = 0;
        std::uint8_t output = 0;
        std::uint16_t row_reserved = 0;
        std::int32_t token = 0;
        if (!cursor.u64(&row.owner.load_generation) || row.owner.load_generation == 0
            || !cursor.string(&row.owner.request_id)
            || !cursor.string(&row.owner.sequence_key)
            || !cursor.string(&row.owner.session_id)
            || !cursor.string(&row.owner.reply)
            || !cursor.u32(&row.owner.sequence_id)
            || !cursor.byte(&phase) || phase > 3
            || !cursor.byte(&output) || output > 1
            || !cursor.u16(&row_reserved) || row_reserved != 0
            || !cursor.u32(&row.owner.position) || row.owner.position > INT32_MAX
            || !cursor.u32(&row.owner.max_tokens) || row.owner.max_tokens == 0
            || !cursor.u32(&row.owner.generated_tokens)
            || row.owner.generated_tokens >= row.owner.max_tokens
            || !cursor.i32(&token)
            || !cursor.u64(&row.owner.speculative_id)
            || !cursor.u32(&row.owner.speculative_index)
            || !cursor.u32(&row.owner.speculative_count)
            || !cursor.string(&row.owner.options)
            || row.owner.request_id.empty() || row.owner.sequence_key.empty()
            || row.owner.session_id.empty() || row.owner.reply.empty()) {
            return wire::fail("invalid logical batch row", error);
        }
        row.owner.phase = static_cast<PhysicalPhase>(phase);
        row.owner.output = output != 0;
        row.owner.input_token = token;
        row.token = token;
        const bool speculative = phase == 2 || phase == 3;
        if ((!speculative && (row.owner.speculative_id != 0
                || row.owner.speculative_index != 0
                || row.owner.speculative_count != 0))
            || (speculative && (row.owner.speculative_id == 0
                || row.owner.speculative_count == 0
                || row.owner.speculative_index >= row.owner.speculative_count))
            || (phase == 2 && !row.owner.output)
            || (phase == 3 && row.owner.output)) {
            return wire::fail("invalid logical speculation row", error);
        }
    }
    if (!cursor.done()) return wire::fail("logical batch has trailing bytes", error);
    return true;
}

bool decode_physical_set(
        const std::vector<std::uint8_t> & bytes,
        std::vector<RoutedPhysicalExecution> * result, std::string * error) {
    if (result == nullptr) return wire::fail("physical set result is null", error);
    wire::Cursor cursor(bytes);
    const std::uint8_t * magic = nullptr;
    std::uint16_t version = 0;
    std::uint16_t reserved = 0;
    std::uint32_t count = 0;
    if (!cursor.take(4, &magic) || std::memcmp(magic, "P4PB", 4) != 0
        || !cursor.u16(&version) || version != 3
        || !cursor.u16(&reserved) || reserved != 0
        || !cursor.u32(&count) || count == 0 || count > wire::kMaxRows) {
        return wire::fail("invalid physical set header", error);
    }
    result->clear();
    result->resize(count);
    for (auto & capsule : *result) {
        if (!read_capsule(cursor, &capsule)) {
            return wire::fail("invalid physical capsule", error);
        }
    }
    if (!cursor.done()) return wire::fail("physical set has trailing bytes", error);
    return true;
}

} // namespace staged::llama_runtime
