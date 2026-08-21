// Putting one decode lap's cut-sets together, and taking the result apart.
//
// Both directions are byte ranges and nothing else. Joining rows means
// writing their bytes one after another; cutting a lap apart means handing
// back equal byte ranges of what came out. Neither names an axis, because
// which axis of a boundary tensor counts tokens is the model's business.
//
// That is also the limit of what byte ranges can express: they merge along
// the token axis only when nothing sits above the token axis in memory.
// llama.cpp says whether anything does — it reports `ggml_n_dims`, so a
// one-token cut is rank 1 exactly when its bytes are that one token and
// nothing else. A shape that is richer interleaves the rows instead, and no
// byte range of it is a row; such a lap is refused here and the per-sequence
// path, which never joins anything, runs it correctly.
//
// This file knows llama.cpp and nothing else. No P4 type appears here, and
// nothing it decides is visible above the adapter.

#include "llama_stage_runtime_hop_shared.hpp"

#include <algorithm>
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
        // One row must be one token's bytes and nothing else, or laying the
        // rows end to end is not a merge. An alias is refused for the same
        // reason: it carries no bytes, so a merged one would have to name a
        // storage this join invented.
        if (merged.dimensions.size() != 1 || merged.strides.size() != 1) return false;
        if (merged.has_alias) return false;
        // Every row has to be the same row already. One whose width, type or
        // layout differs is a different model, not a wider batch.
        std::uint64_t bytes = 0;
        for (std::size_t row = 0; row < bundles.size(); ++row) {
            const auto & descriptor = bundles[row][index];
            if (descriptor.wire_type != merged.wire_type) return false;
            if (descriptor.has_alias) return false;
            if (descriptor.name != merged.name) return false;
            if (descriptor.dimensions != merged.dimensions) return false;
            if (descriptor.strides != merged.strides) return false;
            if (descriptor.nbytes != merged.nbytes) return false;
            bytes += descriptor.nbytes;
        }

        // The axis the rows were laid out on is stated, not derived: as many
        // entries as there are rows, one row's bytes apart. Everything
        // llama.cpp already said about a row — its type, its width, its
        // element stride, its name — is carried through untouched, because
        // its input matcher compares ne and nb against the graph tensor
        // exactly and a descriptor rebuilt from a rule is a descriptor that
        // has to be right about a model this layer does not read.
        const auto row_bytes = merged.nbytes;
        if (row_bytes == 0) return false;
        merged.dimensions.push_back(static_cast<std::uint64_t>(bundles.size()));
        merged.strides.push_back(row_bytes);
        merged.nbytes = bytes;
        merged.view_offset = 0;

        std::vector<std::uint8_t> joined;
        joined.reserve(static_cast<std::size_t>(bytes));
        for (std::size_t row = 0; row < payloads.size(); ++row) {
            const auto * payload = payloads[row][index];
            if (payload == nullptr || payload->size() != row_bytes) return false;
            joined.insert(joined.end(), payload->begin(), payload->end());
        }
        if (joined.size() != bytes) return false;

        llama_linkcpp_tensor_desc llama_descriptor{};
        if (!copy_descriptor(merged, &llama_descriptor, error) ||
            !llama_linkcpp_input_set_tensor(ctx_, &llama_descriptor,
                                            joined.data(), joined.size())) {
            // This runs before execute_decode_batch's llama_decode() call, so
            // nothing has been computed yet: a REFUSAL, not a failure. The
            // per-sequence path binds one row's cut-set at a time instead of
            // this merged bundle and can still succeed where the merge could
            // not -- see the file comment above for the shape this rejects
            // (anything with something above the token axis).
            if (error != nullptr) error->clear();
            return false;
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
        // How far apart two rows are, and therefore how many the tensor
        // holds — both read off the layout llama.cpp reported rather than
        // off an axis this layer would have to name. A lap that was joined
        // by laying rows end to end comes back the same way, so the distance
        // between rows is the stride over the axis it was joined on, and
        // llama.cpp may have padded the ubatch past this lap: the rows in
        // front are still this lap's, in order, which is what the runtime
        // before P4 also relied on.
        if (descriptor.dimensions.size() != 2 || descriptor.strides.size() != 2) {
            return fail_hop("batched decode output is not rows of one token", error);
        }
        const auto row_bytes = static_cast<std::size_t>(descriptor.strides[1]);
        if (row_bytes == 0 || descriptor.nbytes % row_bytes != 0) {
            return fail_hop("batched decode output does not divide by its rows", error);
        }
        const auto produced = static_cast<std::size_t>(descriptor.nbytes / row_bytes);
        if (produced < rows) {
            return fail_hop("batched decode output holds fewer rows than the lap", error);
        }
        if (hop_trace_enabled()) {
            std::fprintf(stderr,
                         "P4_STAGED_DECODE_SPLIT stage=%d-%d rows=%zu produced=%zu row_bytes=%zu bytes=%llu\n",
                         config_.layer_begin, config_.layer_end, rows, produced, row_bytes,
                         static_cast<unsigned long long>(descriptor.nbytes));
        }
        std::vector<std::uint8_t> whole(static_cast<std::size_t>(descriptor.nbytes));
        if (!llama_linkcpp_output_get(ctx_, index, whole.data(), whole.size())) {
            return fail_hop("llama.cpp rejected staged HOP output tensor", error);
        }
        for (std::size_t i = 0; i < rows; ++i) {
            // One row is the tensor without the axis the lap was joined on,
            // and nothing else changes: llama.cpp's own type, width, element
            // stride and name travel on. That is also exactly the descriptor
            // a one-token hop produces, so the stage that receives it cannot
            // tell this lap was batched.
            auto row = descriptor;
            row.dimensions.resize(1);
            row.strides.resize(1);
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
