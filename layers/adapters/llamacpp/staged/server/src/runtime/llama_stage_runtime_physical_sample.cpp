#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"

namespace staged::llama_runtime {

bool StageRuntime::sample_physical_outputs(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::vector<PhysicalOutcome> * outcomes,
        std::string * error) {
    const auto rows = input.output.size();
    if (!loaded() || !tail_stage_ || outcomes == nullptr
        || owners.size() != rows || input.positions.size() != rows * input.n_pos) {
        if (error != nullptr) *error = "invalid physical sampling request";
        return false;
    }
    outcomes->clear();
    for (std::size_t index = 0; index < rows; ++index) {
        if (input.output[index] == 0) continue;
        protocol::SequencePayload request;
        request.sequence_id = owners[index].sequence_key;
        request.position = owners[index].position;
        request.options = owners[index].options;
        protocol::SequencePayload result;
        if (!sample_decode_row(request, static_cast<std::int32_t>(index),
                               &result, error)
            || !result.outcome.has_value() || !result.outcome->token.has_value()) {
            return false;
        }
        PhysicalOutcome outcome;
        outcome.owner_index = static_cast<std::uint32_t>(index);
        outcome.token = *result.outcome->token;
        outcome.text = std::move(result.outcome->text);
        outcome.position = result.outcome->position;
        outcome.stop = result.outcome->stop.value_or("");
        if (outcome.stop.empty()
            && owners[index].generated_tokens + 1 >= owners[index].max_tokens) {
            outcome.stop = "length";
        }
        outcomes->push_back(std::move(outcome));
    }
    return true;
}

} // namespace staged::llama_runtime
