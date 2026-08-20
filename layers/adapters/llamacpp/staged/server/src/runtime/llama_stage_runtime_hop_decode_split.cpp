// Putting one decode lap's cut-sets together, and taking the result apart.
//
// A cut-set is a bundle of tensors rather than one, and they may alias each
// other, so merging is per tensor position across the rows: the token axis
// `ne[1]` adds up and everything else must already agree. Splitting is the
// same in reverse, and it tolerates llama.cpp returning more rows than were
// asked for — a padded ubatch is still correct for the rows in front of it.
//
// This file knows llama.cpp and nothing else. No P4 type appears here, and
// nothing it decides is visible above the adapter.

#include "llama_stage_runtime_hop_shared.hpp"

#include <cstdint>
#include <cstring>
#include <vector>

namespace staged::llama_runtime {

using namespace hop;

bool StageRuntime::bind_merged_cut_set(
    const std::vector<std::vector<protocol::Descriptor>> & bundles,
    const std::vector<std::vector<const std::vector<std::uint8_t> *>> & payloads,
    std::string * error) {
    if (bundles.empty() || bundles.size() != payloads.size()) return false;
    const auto tensors = bundles.front().size();
    if (tensors == 0) return false;
    for (const auto & bundle : bundles) {
        if (bundle.size() != tensors) return false;
    }

    llama_linkcpp_input_clear(ctx_);
    for (std::size_t index = 0; index < tensors; ++index) {
        auto merged = bundles.front()[index];
        // Everything but the token axis has to agree already. A row whose
        // width or type differs is a different model, not a wider batch.
        std::uint64_t token_axis = 0;
        std::uint64_t bytes = 0;
        for (std::size_t row = 0; row < bundles.size(); ++row) {
            const auto & descriptor = bundles[row][index];
            if (descriptor.wire_type != merged.wire_type) return false;
            if (descriptor.has_alias != merged.has_alias) return false;
            if (descriptor.has_alias && descriptor.alias_of != merged.alias_of) return false;
            if (descriptor.name != merged.name) return false;
            if (descriptor.dimensions.empty() || merged.dimensions.empty()) return false;
            if (descriptor.dimensions[0] != merged.dimensions[0]) return false;
            for (std::size_t axis = 2; axis < descriptor.dimensions.size(); ++axis) {
                if (axis >= merged.dimensions.size()) return false;
                if (descriptor.dimensions[axis] != merged.dimensions[axis]) return false;
            }
            // A cut-set squeezes the token axis for a single token, so a
            // missing second dimension counts as one.
            token_axis += descriptor.dimensions.size() > 1 ? descriptor.dimensions[1] : 1;
            bytes += descriptor.nbytes;
        }
        if (merged.has_alias) continue;

        merged.dimensions.resize(std::max<std::size_t>(2, merged.dimensions.size()));
        merged.dimensions[1] = token_axis;
        merged.strides.resize(merged.dimensions.size());
        if (merged.strides.size() > 1 && merged.strides[1] == 0) {
            merged.strides[1] = bundles.front()[index].nbytes;
        }
        merged.nbytes = bytes;
        merged.view_offset = 0;

        std::vector<std::uint8_t> joined;
        joined.reserve(static_cast<std::size_t>(bytes));
        for (std::size_t row = 0; row < payloads.size(); ++row) {
            const auto * payload = payloads[row][index];
            if (payload == nullptr) return false;
            joined.insert(joined.end(), payload->begin(), payload->end());
        }
        if (joined.size() != bytes) return false;

        llama_linkcpp_tensor_desc llama_descriptor{};
        if (!copy_descriptor(merged, &llama_descriptor, error) ||
            !llama_linkcpp_input_set_tensor(ctx_, &llama_descriptor,
                                            joined.data(), joined.size())) {
            return fail_hop("llama.cpp rejected the merged decode cut-set", error);
        }
    }
    return true;
}

bool StageRuntime::split_decode_outputs(
    std::size_t rows,
    std::vector<protocol::SequencePayload> * results,
    std::string * error) {
    if (results == nullptr || results->size() != rows || rows == 0) {
        return fail_hop("invalid batched decode split", error);
    }
    const auto count = llama_linkcpp_output_count(ctx_);
    if (count < 0) return fail_hop("llama.cpp returned an invalid staged output count", error);
    for (int32_t index = 0; index < count; ++index) {
        llama_linkcpp_tensor_desc llama_descriptor{};
        if (!llama_linkcpp_output_desc(ctx_, index, &llama_descriptor)) {
            return fail_hop("llama.cpp could not describe staged HOP output", error);
        }
        auto descriptor = protocol_descriptor(llama_descriptor);
        if (descriptor.has_alias) {
            for (auto & result : *results) {
                auto copy = descriptor;
                copy.alias_of += static_cast<std::uint32_t>(result.descriptors.size());
                result.descriptors.push_back(std::move(copy));
                result.payloads.emplace_back(std::nullopt);
            }
            continue;
        }
        if (descriptor.nbytes > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
            return fail_hop("staged HOP output is too large", error);
        }
        // The token axis is what the batch widened. llama.cpp may hand back
        // more rows than were asked for when it padded the ubatch; the rows
        // in front are still this lap's, in order.
        const auto produced = descriptor.dimensions.size() > 1
            ? static_cast<std::size_t>(descriptor.dimensions[1])
            : 1;
        if (produced < rows || produced == 0) {
            return fail_hop("batched decode output has fewer rows than the lap", error);
        }
        if (descriptor.nbytes % produced != 0) {
            return fail_hop("batched decode output does not divide by its rows", error);
        }
        std::vector<std::uint8_t> whole(static_cast<std::size_t>(descriptor.nbytes));
        if (!llama_linkcpp_output_get(ctx_, index, whole.data(), whole.size())) {
            return fail_hop("llama.cpp rejected staged HOP output tensor", error);
        }
        const auto row_bytes = whole.size() / produced;
        for (std::size_t i = 0; i < rows; ++i) {
            auto row = descriptor;
            row.dimensions.resize(std::max<std::size_t>(1, row.dimensions.size()));
            if (row.dimensions.size() > 1) row.dimensions[1] = 1;
            row.nbytes = static_cast<std::uint64_t>(row_bytes);
            row.view_offset = 0;
            (*results)[i].descriptors.push_back(std::move(row));
            (*results)[i].payloads.emplace_back(std::vector<std::uint8_t>(
                whole.begin() + static_cast<std::ptrdiff_t>(i * row_bytes),
                whole.begin() + static_cast<std::ptrdiff_t>((i + 1) * row_bytes)));
        }
    }
    return true;
}

} // namespace staged::llama_runtime
