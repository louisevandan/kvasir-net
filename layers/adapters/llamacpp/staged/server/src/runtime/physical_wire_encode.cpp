#include "physical_wire.hpp"
#include "physical_wire_cursor.hpp"
#include "physical_authority.hpp"

#include <algorithm>
#include <cstring>
#include <limits>
#include <unordered_set>

namespace staged::llama_runtime {
namespace wire = physical_wire;

namespace {

bool add_size(std::uint64_t * total, std::uint64_t value) {
    if (total == nullptr || value > std::numeric_limits<std::uint64_t>::max() - *total) {
        return false;
    }
    *total += value;
    return true;
}

bool add_string_size(std::uint64_t * total, const std::string & value) {
    return value.size() <= wire::kMaxString
        && value.size() <= std::numeric_limits<std::uint16_t>::max()
        && add_size(total, 2 + static_cast<std::uint64_t>(value.size()));
}

bool owner_encoded_size(const PhysicalOwner & owner, std::uint64_t * size) {
    const bool speculative = owner.phase == PhysicalPhase::Verify
        || owner.phase == PhysicalPhase::Replay;
    if (size == nullptr || owner.load_generation == 0 || owner.incarnation == 0
        || owner.position > INT32_MAX
        || (!speculative && (owner.speculative_id != 0
            || owner.speculative_index != 0 || owner.speculative_count != 0))
        || (speculative && (owner.speculative_id == 0
            || owner.speculative_count == 0
            || owner.speculative_index >= owner.speculative_count))
        || (owner.phase == PhysicalPhase::Verify && !owner.output)
        || (owner.phase == PhysicalPhase::Replay && owner.output)
        || !runtime::canonical_physical_request_identity(owner.session_id,
            owner.sequence_key, owner.request_id)
        || owner.reply.empty() || owner.max_tokens == 0
        || owner.generated_tokens >= owner.max_tokens) return false;
    *size = 56;
    return add_string_size(size, owner.request_id)
        && add_string_size(size, owner.sequence_key)
        && add_string_size(size, owner.session_id)
        && add_string_size(size, owner.reply)
        && add_string_size(size, owner.options);
}

bool tensor_encoded_size(
        const PhysicalTensor & tensor, std::size_t index, std::uint64_t * size) {
    const auto & descriptor = tensor.descriptor;
    const bool alias = descriptor.alias_of >= 0;
    const auto name_end = std::find(
        descriptor.name, descriptor.name + sizeof(descriptor.name), '\0');
    const std::string name(descriptor.name, name_end);
    if (size == nullptr || descriptor.n_dims <= 0 || descriptor.n_dims > 4
        || name.empty() || name_end == descriptor.name + sizeof(descriptor.name)
        || (alias && (static_cast<std::size_t>(descriptor.alias_of) >= index
                      || !tensor.data.empty()))
        || (!alias && tensor.data.size() != descriptor.nbytes)) return false;
    *size = 36 + 16 * static_cast<std::uint64_t>(descriptor.n_dims);
    return add_string_size(size, name)
        && add_size(size, static_cast<std::uint64_t>(tensor.data.size()));
}

bool outcome_encoded_size(
        const RoutedPhysicalExecution & capsule, const PhysicalOutcome & outcome,
        std::uint64_t * size) {
    const auto rows = capsule.execution.sequence_counts.size();
    if (size == nullptr || outcome.owner_index >= rows
        || (outcome.generated.empty() && outcome.proposal.empty()
            && outcome.replay_tokens.empty())
        || outcome.generated.size() > wire::kMaxRows
        || outcome.proposal.size() > wire::kMaxRows
        || outcome.replay_tokens.size() > wire::kMaxRows
        || outcome.retain_from > INT32_MAX || outcome.retain_from < -1
        || (outcome.retain_from < 0
            && (!outcome.replay_tokens.empty() || outcome.replay_position != 0))
        || (outcome.retain_from >= 0
            && capsule.owners[outcome.owner_index].phase != PhysicalPhase::Verify)
        || (outcome.retain_from >= 0 && outcome.replay_tokens.empty()
            && outcome.replay_position != 0)
        || (!outcome.replay_tokens.empty()
            && static_cast<std::uint64_t>(outcome.replay_position)
                + outcome.replay_tokens.size()
                != static_cast<std::uint64_t>(outcome.retain_from))) return false;
    *size = 24;
    for (const auto & generated : outcome.generated) {
        if (!add_size(size, 8)
            || !add_string_size(size, generated.text)
            || !add_string_size(size, generated.stop)) return false;
    }
    std::uint64_t token_bytes = 0;
    const auto token_count = static_cast<std::uint64_t>(outcome.proposal.size())
        + static_cast<std::uint64_t>(outcome.replay_tokens.size());
    if (token_count > std::numeric_limits<std::uint64_t>::max() / 4) return false;
    token_bytes = token_count * 4;
    return add_size(size, token_bytes);
}

bool capsule_encoded_size(
        const RoutedPhysicalExecution & capsule, std::uint64_t * size) {
    const auto & execution = capsule.execution;
    const auto rows = execution.sequence_counts.size();
    if (size == nullptr || capsule.execution_id == 0 || rows == 0
        || rows > wire::kMaxRows || execution.n_pos == 0 || execution.n_pos > 4
        || execution.positions.size() != rows * execution.n_pos
        || execution.output.size() != rows || capsule.owners.size() != rows
        || execution.tensors.size() > wire::kMaxTensors
        || capsule.outcomes.size() > rows) return false;
    std::uint64_t sequence_total = 0;
    for (const auto count : execution.sequence_counts) {
        if (count <= 0 || !add_size(&sequence_total, static_cast<std::uint32_t>(count))) {
            return false;
        }
    }
    if (sequence_total != execution.sequence_ids.size()
        || sequence_total > std::numeric_limits<std::uint32_t>::max()) return false;
    for (std::size_t index = 0; index < rows; ++index) {
        if (capsule.owners[index].output != (execution.output[index] != 0)
            || capsule.owners[index].position != static_cast<std::uint32_t>(
                execution.positions[index])) return false;
    }
    if ((!execution.terminal && (!capsule.outcomes.empty() || execution.tensors.empty()))
        || (execution.terminal && !execution.tensors.empty())) return false;

    *size = 48;
    const std::uint64_t scalar_items = static_cast<std::uint64_t>(
        execution.positions.size() + execution.sequence_counts.size()
        + execution.sequence_ids.size());
    if (scalar_items > std::numeric_limits<std::uint64_t>::max() / 4
        || !add_size(size, scalar_items * 4)
        || !add_size(size, static_cast<std::uint64_t>(execution.output.size()))) return false;
    for (const auto & owner : capsule.owners) {
        std::uint64_t owner_size = 0;
        if (!owner_encoded_size(owner, &owner_size) || !add_size(size, owner_size)) return false;
    }
    for (std::size_t index = 0; index < execution.tensors.size(); ++index) {
        std::uint64_t tensor_size = 0;
        if (!tensor_encoded_size(execution.tensors[index], index, &tensor_size)
            || !add_size(size, tensor_size)) return false;
    }
    std::unordered_set<std::uint32_t> outcome_owners;
    for (const auto & outcome : capsule.outcomes) {
        if (!outcome_owners.insert(outcome.owner_index).second) return false;
        std::uint64_t outcome_size = 0;
        if (!outcome_encoded_size(capsule, outcome, &outcome_size)
            || !add_size(size, outcome_size)) return false;
    }
    return true;
}

bool write_owner(std::vector<std::uint8_t> * output, const PhysicalOwner & owner) {
    const bool speculative = owner.phase == PhysicalPhase::Verify
        || owner.phase == PhysicalPhase::Replay;
    if (owner.load_generation == 0 || owner.incarnation == 0 || owner.position > INT32_MAX
        || (!speculative && (owner.speculative_id != 0
            || owner.speculative_index != 0 || owner.speculative_count != 0))
        || (speculative && (owner.speculative_id == 0
            || owner.speculative_count == 0
            || owner.speculative_index >= owner.speculative_count))
        || (owner.phase == PhysicalPhase::Verify && !owner.output)
        || (owner.phase == PhysicalPhase::Replay && owner.output)) return false;
    wire::put_u64(output, owner.load_generation);
    wire::put_u64(output, owner.incarnation);
    return runtime::canonical_physical_request_identity(owner.session_id,
            owner.sequence_key, owner.request_id)
        && !owner.reply.empty()
        && wire::put_string(output, owner.request_id)
        && wire::put_string(output, owner.sequence_key)
        && wire::put_string(output, owner.session_id)
        && wire::put_string(output, owner.reply)
        && (wire::put_u32(output, owner.sequence_id), true)
        && (output->push_back(static_cast<std::uint8_t>(owner.phase)), true)
        && (output->push_back(owner.output ? 1 : 0), true)
        && (wire::put_u16(output, 0), true)
        && (wire::put_u32(output, owner.position), true)
        && owner.max_tokens > 0 && owner.generated_tokens < owner.max_tokens
        && (wire::put_u32(output, owner.max_tokens), true)
        && (wire::put_u32(output, owner.generated_tokens), true)
        && (wire::put_i32(output, owner.input_token), true)
        && (wire::put_u64(output, owner.speculative_id), true)
        && (wire::put_u32(output, owner.speculative_index), true)
        && (wire::put_u32(output, owner.speculative_count), true)
        && wire::put_string(output, owner.options);
}

bool write_tensor(
        std::vector<std::uint8_t> * output, const PhysicalTensor & tensor,
        std::size_t index) {
    const auto & descriptor = tensor.descriptor;
    const bool alias = descriptor.alias_of >= 0;
    if (descriptor.n_dims <= 0 || descriptor.n_dims > 4
        || descriptor.name[0] == '\0'
        || (alias && (static_cast<std::size_t>(descriptor.alias_of) >= index
                      || !tensor.data.empty()))
        || (!alias && tensor.data.size() != descriptor.nbytes)) return false;
    wire::put_i32(output, descriptor.type);
    output->push_back(static_cast<std::uint8_t>(descriptor.n_dims));
    output->insert(output->end(), 3, 0);
    for (std::int32_t dim = 0; dim < descriptor.n_dims; ++dim) {
        wire::put_i64(output, descriptor.ne[dim]);
    }
    for (std::int32_t dim = 0; dim < descriptor.n_dims; ++dim) {
        wire::put_u64(output, descriptor.nb[dim]);
    }
    wire::put_u64(output, descriptor.nbytes);
    wire::put_u64(output, descriptor.view_offset);
    wire::put_i32(output, descriptor.alias_of);
    if (!wire::put_string(output, descriptor.name)) return false;
    wire::put_u64(output, tensor.data.size());
    output->insert(output->end(), tensor.data.begin(), tensor.data.end());
    return true;
}

bool write_capsule(
        std::vector<std::uint8_t> * output,
        const RoutedPhysicalExecution & capsule) {
    const auto & execution = capsule.execution;
    const auto rows = execution.sequence_counts.size();
    if (capsule.execution_id == 0 || rows == 0 || rows > wire::kMaxRows
        || execution.n_pos == 0 || execution.n_pos > 4
        || execution.positions.size() != rows * execution.n_pos
        || execution.output.size() != rows || capsule.owners.size() != rows
        || execution.tensors.size() > wire::kMaxTensors) return false;
    std::uint64_t sequence_total = 0;
    for (const auto count : execution.sequence_counts) {
        if (count <= 0) return false;
        sequence_total += static_cast<std::uint32_t>(count);
    }
    if (sequence_total != execution.sequence_ids.size()
        || sequence_total > std::numeric_limits<std::uint32_t>::max()) return false;
    for (std::size_t index = 0; index < rows; ++index) {
        if (capsule.owners[index].output != (execution.output[index] != 0)
            || capsule.owners[index].position != static_cast<std::uint32_t>(
                execution.positions[index])) return false;
    }
    if ((!execution.terminal && (!capsule.outcomes.empty() || execution.tensors.empty()))
        || (execution.terminal && !execution.tensors.empty())) return false;

    wire::put_u64(output, capsule.execution_id);
    wire::put_u32(output, execution.terminal ? 1 : 0);
    wire::put_u32(output, execution.flags);
    wire::put_u32(output, execution.n_seq_tokens);
    wire::put_u32(output, execution.n_seqs);
    wire::put_u32(output, execution.n_seqs_unq);
    wire::put_u32(output, execution.n_pos);
    wire::put_u32(output, static_cast<std::uint32_t>(rows));
    wire::put_u32(output, static_cast<std::uint32_t>(sequence_total));
    wire::put_u32(output, static_cast<std::uint32_t>(execution.tensors.size()));
    wire::put_u32(output, static_cast<std::uint32_t>(capsule.outcomes.size()));
    for (const auto value : execution.positions) wire::put_i32(output, value);
    for (const auto value : execution.sequence_counts) {
        wire::put_u32(output, static_cast<std::uint32_t>(value));
    }
    for (const auto value : execution.sequence_ids) wire::put_i32(output, value);
    for (const auto value : execution.output) output->push_back(value != 0 ? 1 : 0);
    for (const auto & owner : capsule.owners) if (!write_owner(output, owner)) return false;
    for (std::size_t index = 0; index < execution.tensors.size(); ++index) {
        if (!write_tensor(output, execution.tensors[index], index)) return false;
    }
    for (const auto & outcome : capsule.outcomes) {
        if (outcome.owner_index >= rows
            || (outcome.generated.empty() && outcome.proposal.empty()
                && outcome.replay_tokens.empty())
            || outcome.generated.size() > wire::kMaxRows
            || outcome.proposal.size() > wire::kMaxRows
            || outcome.replay_tokens.size() > wire::kMaxRows
            || outcome.retain_from > INT32_MAX
            || outcome.retain_from < -1
            || (outcome.retain_from < 0
                && (!outcome.replay_tokens.empty() || outcome.replay_position != 0))
            || (outcome.retain_from >= 0
                && capsule.owners[outcome.owner_index].phase != PhysicalPhase::Verify)
            || (outcome.retain_from >= 0 && outcome.replay_tokens.empty()
                && outcome.replay_position != 0)
            || (!outcome.replay_tokens.empty()
                && static_cast<std::uint64_t>(outcome.replay_position)
                    + outcome.replay_tokens.size()
                    != static_cast<std::uint64_t>(outcome.retain_from))) return false;
        wire::put_u32(output, outcome.owner_index);
        wire::put_u32(output, static_cast<std::uint32_t>(outcome.generated.size()));
        wire::put_u32(output, static_cast<std::uint32_t>(outcome.proposal.size()));
        wire::put_u32(output, static_cast<std::uint32_t>(outcome.replay_tokens.size()));
        wire::put_i32(output, static_cast<std::int32_t>(outcome.retain_from));
        wire::put_u32(output, outcome.replay_position);
        for (const auto & generated : outcome.generated) {
            if (generated.text.size() > wire::kMaxString
                || generated.stop.size() > wire::kMaxString) return false;
            wire::put_i32(output, generated.token);
            wire::put_u32(output, generated.position);
            if (!wire::put_string(output, generated.text)
                || !wire::put_string(output, generated.stop)) return false;
        }
        for (const auto token : outcome.proposal) wire::put_i32(output, token);
        for (const auto token : outcome.replay_tokens) wire::put_i32(output, token);
    }
    return true;
}

} // namespace

bool encode_physical_set(
        const std::vector<RoutedPhysicalExecution> & capsules,
        std::vector<std::uint8_t> * output, std::string * error) {
    return encode_physical_set_bounded(
        capsules, std::numeric_limits<std::uint64_t>::max(), output, error);
}

bool physical_set_encoded_size(
        const std::vector<RoutedPhysicalExecution> & capsules,
        std::uint64_t * result, std::string * error) {
    if (result == nullptr || capsules.empty()
        || capsules.size() > std::numeric_limits<std::uint32_t>::max()) {
        return wire::fail("invalid physical set", error);
    }
    std::uint64_t total = 12;
    for (const auto & capsule : capsules) {
        std::uint64_t capsule_size = 0;
        if (!capsule_encoded_size(capsule, &capsule_size)
            || !add_size(&total, capsule_size)) {
            return wire::fail("invalid physical capsule result", error);
        }
    }
    *result = total;
    return true;
}

bool encode_physical_set_bounded(
        const std::vector<RoutedPhysicalExecution> & capsules,
        std::uint64_t max_bytes, std::vector<std::uint8_t> * output,
        std::string * error) {
    if (output == nullptr || capsules.empty()
        || capsules.size() > std::numeric_limits<std::uint32_t>::max()) {
        return wire::fail("invalid physical set", error);
    }
    std::uint64_t encoded_size = 0;
    if (!physical_set_encoded_size(capsules, &encoded_size, error)) return false;
    if (encoded_size > max_bytes || encoded_size > std::numeric_limits<std::size_t>::max()
        || encoded_size > output->max_size()) {
        return wire::fail("physical result exceeds planned byte bound", error);
    }
    output->clear();
    output->reserve(static_cast<std::size_t>(encoded_size));
    output->insert(output->end(), {'P', '4', 'P', 'B'});
    wire::put_u16(output, 4);
    wire::put_u16(output, 0);
    wire::put_u32(output, static_cast<std::uint32_t>(capsules.size()));
    for (const auto & capsule : capsules) {
        const auto before = output->size();
        if (!write_capsule(output, capsule)) {
            output->resize(before);
            return wire::fail("invalid physical capsule result", error);
        }
    }
    if (output->size() != encoded_size) {
        output->clear();
        return wire::fail("physical result encoded size changed", error);
    }
    return true;
}

} // namespace staged::llama_runtime
