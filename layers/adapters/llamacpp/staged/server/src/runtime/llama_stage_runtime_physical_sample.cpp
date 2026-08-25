#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"
#include "request_options.hpp"

#include <limits>

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
    for (const auto & owner : owners) {
        const auto options_found = sampler_options_.find(owner.sequence_key);
        if (options_found != sampler_options_.end()
            && options_found->second != owner.options) {
            samplers_.erase(owner.sequence_key);
            sampler_options_.erase(options_found);
            sampled_tokens_.erase(owner.sequence_key);
            sampled_texts_.erase(owner.sequence_key);
        }
        auto found = samplers_.find(owner.sequence_key);
        if (found == samplers_.end()) {
            auto sampling = params_.sampling;
            if (!apply_request_options(owner.options, model_, &sampling, error)) return false;
            common_sampler_ptr sampler(common_sampler_init(model_, sampling));
            if (!sampler) {
                if (error != nullptr) *error = "llama.cpp failed to create physical sampler";
                return false;
            }
            found = samplers_.emplace(owner.sequence_key, std::move(sampler)).first;
            sampler_options_[owner.sequence_key] = owner.options;
        }
        if (owner.phase == PhysicalPhase::Prefill) {
            common_sampler_accept(found->second.get(), owner.input_token, false);
        }
    }
    for (std::size_t index = 0; index < rows;) {
        if (owners[index].phase == PhysicalPhase::Verify
            || owners[index].phase == PhysicalPhase::Replay) {
            const auto count = static_cast<std::size_t>(owners[index].speculative_count);
            if (owners[index].speculative_index != 0 || count == 0
                || count > rows - index
                || !sample_physical_mtp(
                    input, owners, index, index + count, outcomes, error)) {
                if (error != nullptr && error->empty()) {
                    *error = "invalid mixed MTP physical group";
                }
                return false;
            }
            index += count;
            continue;
        }
        if (input.output[index] == 0) {
            ++index;
            continue;
        }
        auto sampler = samplers_.find(owners[index].sequence_key);
        if (sampler == samplers_.end()) return false;
        const auto token = common_sampler_sample(
            sampler->second.get(), ctx_, static_cast<std::int32_t>(index));
        if (token == LLAMA_TOKEN_NULL) {
            if (error != nullptr) *error = "llama.cpp returned no physical token";
            return false;
        }
        common_sampler_accept(sampler->second.get(), token, true);
        PhysicalOutcome outcome;
        outcome.owner_index = static_cast<std::uint32_t>(index);
        GeneratedToken generated;
        if (owners[index].position == std::numeric_limits<std::uint32_t>::max()) {
            if (error != nullptr) *error = "physical token position overflow";
            return false;
        }
        const auto position = owners[index].position + 1;
        if (!format_generated_token(
                owners[index], token, position, &generated, error)) return false;
        if (generated.stop.empty()
            && owners[index].generated_tokens + 1 >= owners[index].max_tokens) {
            generated.stop = "length";
        }
        if (generated.stop.empty()) {
            if (mtp_speculative_ != nullptr) {
                if (!make_mtp_proposal(
                        static_cast<llama_seq_id>(owners[index].sequence_id),
                        owners[index].generated_tokens,
                        owners[index].max_tokens,
                        token, position,
                        1,
                        &outcome.proposal, error)) return false;
            } else {
                outcome.proposal.push_back(generated.token);
            }
        }
        outcome.generated.push_back(std::move(generated));
        outcomes->push_back(std::move(outcome));
        ++index;
    }
    return true;
}

} // namespace staged::llama_runtime
