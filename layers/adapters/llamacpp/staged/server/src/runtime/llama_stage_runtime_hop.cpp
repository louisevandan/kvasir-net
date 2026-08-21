#include "llama_stage_runtime.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>

#include "ggml.h"
#include "llama_stage_runtime_hop_shared.hpp"

namespace staged::llama_runtime {

using namespace hop;



bool StageRuntime::execute_hop(const protocol::SequencePayload & input,
                               protocol::HopPhase phase,
                               protocol::SequencePayload * output,
                               std::string * error) {
    // See execute_decode_batch's entry guard and hop_memory_dirty() in
    // llama_stage_runtime.hpp: a prior decode on this runtime left processed
    // ubatches in the memory state with no way to roll them back, so every
    // HOP after that one -- batched or per-sequence -- is refused until a
    // fresh load() clears it. Checked before loaded() (matching
    // execute_decode_batch and decode_status.hpp's "every HOP execution path
    // checks first" comment) rather than after: hop_memory_dirty_ can only be
    // true while a real load is (or was) in effect, since unload() is the
    // only place that clears it and it always sets loaded()==false in the
    // same call, so the two checks never actually compete over a real
    // runtime state. Putting the guard first is what lets an unloaded
    // StageRuntime exercise it in a test without a llama_context.
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded()) {
        return fail_hop("stage runtime is not loaded", error);
    }
    if (output == nullptr || input.descriptors.size() != input.payloads.size()) {
        return fail_hop("invalid staged HOP payload", error);
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_BEGIN stage=%d-%d phase=%s seq=%s descriptors=%zu n_tokens=%u\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), input.descriptors.size(),
                     input.n_tokens.value_or(0));
    }

    std::vector<llama_token> input_tokens;
    if (input.initial_tokens.has_value()) {
        input_tokens.reserve(input.initial_tokens->size());
        for (const auto token : *input.initial_tokens) {
            input_tokens.push_back(static_cast<llama_token>(token));
        }
    } else if (input.prompt.has_value()) {
        const auto *vocab = llama_model_get_vocab(model_);
        if (vocab == nullptr) {
            return fail_hop("llama.cpp did not expose a tokenizer vocabulary", error);
        }
        input_tokens = common_tokenize(vocab, *input.prompt, true, true);
        if (input_tokens.empty()) {
            return fail_hop("llama.cpp tokenizer produced no input tokens", error);
        }
    }
    const auto token_count = input_tokens.empty()
        ? static_cast<std::size_t>(input.n_tokens.value_or(1))
        : input_tokens.size();
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_TOKENS stage=%d-%d phase=%s seq=%s input_tokens=%zu token_count=%zu\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), input_tokens.size(), token_count);
    }
    if (token_count == 0 || token_count > static_cast<std::size_t>(std::numeric_limits<int32_t>::max())) {
        return fail_hop("invalid staged HOP token count", error);
    }

    // A long prefill does not cross the wire in one piece. llama.cpp splits a
    // batch wider than `n_ubatch` into that many graph executions, and a
    // staged cut-set is bound once per execution, so the producing stage ran
    // `ceil(n_tokens / n_ubatch)` graphs and sent one cut-set per graph. The
    // transport must preserve every one of them; forwarding only the last
    // loses the earlier tokens from every downstream stage's KV cache.
    //
    // How the tokens divide is therefore arithmetic on the stated count and
    // this stage's own ubatch width -- `plan.cpp` makes `n_batch == n_ubatch`
    // on every stage, so the two stages divide it identically. Nothing about
    // the division is read out of a tensor axis: which axis of a boundary
    // tensor counts tokens is the model's business, not this layer's.
    struct Chunk {
        std::size_t token_count = 0;
        std::size_t descriptor_begin = 0;
        std::size_t descriptor_count = 0;
        std::size_t token_offset = 0;
    };
    // A decode lap always starts at stage 0.  Its native KV already contains
    // the prompt, and the first stage consumes token ids/placeholders rather
    // than the previous lap's tail cut-set.  Re-injecting that cut-set would
    // replay the whole prefill (and can exceed the stage's sequence window).
    const bool ignore_inbound_cut_set = config_.layer_begin == 0
        && phase == protocol::HopPhase::Decode;
    std::vector<Chunk> chunks;
    const auto chunk_size = phase == protocol::HopPhase::Prefill
        ? std::max<std::size_t>(1, llama_n_ubatch(ctx_)) : token_count;
    for (std::size_t offset = 0; offset < token_count; offset += chunk_size) {
        chunks.push_back({std::min(chunk_size, token_count - offset), 0, 0, offset});
    }
    if (chunks.empty()) return fail_hop("staged HOP has no executable chunks", error);
    if (!ignore_inbound_cut_set && input_tokens.empty() && !input.descriptors.empty()) {
        // A cut-set is a bundle of tensors rather than one. `cut_at` in the
        // graph patch collects every tensor produced below the boundary and
        // consumed above it, and how many that is belongs to the model:
        // gemma4 carries a per-layer input embedding beside the hidden
        // state. llama.cpp's input matcher wants all of them for one graph,
        // so a bundle is bound whole, and the bundles arrive in chunk order.
        if (input.descriptors.size() % chunks.size() != 0) {
            return fail_hop("staged HOP cut-set is not one bundle per chunk", error);
        }
        const auto bundle = input.descriptors.size() / chunks.size();
        for (std::size_t i = 0; i < chunks.size(); ++i) {
            chunks[i].descriptor_begin = i * bundle;
            chunks[i].descriptor_count = bundle;
        }
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_CHUNKS stage=%d-%d phase=%s seq=%s ignore=%d descriptors=%zu chunks=%zu bundle=%zu\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), ignore_inbound_cut_set ? 1 : 0,
                     input.descriptors.size(), chunks.size(), chunks.front().descriptor_count);
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_SEQ_LIMIT_BEGIN stage=%d-%d phase=%s seq=%s\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str());
    }
    const auto sequence_limit = llama_n_seq_max(ctx_);
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_SEQ_LIMIT_DONE stage=%d-%d phase=%s seq=%s limit=%u\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), sequence_limit);
    }
    if (sequence_limit == 0 || sequence_limit >
            static_cast<uint32_t>(std::numeric_limits<int32_t>::max())) {
        return fail_hop("llama.cpp returned an invalid sequence limit", error);
    }
    llama_seq_id sequence_id = 0;
    const bool was_present = sequence_ids_.find(input.sequence_id) != sequence_ids_.end();
    if (!local_sequence(sequence_ids_, next_sequence_id_, input.sequence_id,
                        sequence_limit, &sequence_id, error)) {
        return false;
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_SEQUENCE_DONE stage=%d-%d phase=%s seq=%s slot=%lld\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), static_cast<long long>(sequence_id));
    }
    if (hop_batch_active_ && !was_present) {
        hop_new_sequences_.push_back(input.sequence_id);
    }

    protocol::SequencePayload result;
    result.sequence_id = input.sequence_id;
    result.n_tokens = static_cast<std::uint32_t>(token_count);
    result.options = input.options;
    int32_t last_batch_tokens = 0;
    for (const auto & chunk : chunks) {
        if (hop_trace_enabled()) {
            std::fprintf(stderr, "P4_STAGED_HOP_CHUNK stage=%d-%d phase=%s seq=%s tokens=%zu descriptors=%zu\n",
                         config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                         input.sequence_id.c_str(), chunk.token_count, chunk.descriptor_count);
        }
        llama_linkcpp_input_clear(ctx_);
        for (std::size_t offset = 0; offset < chunk.descriptor_count; ++offset) {
            const auto index = chunk.descriptor_begin + offset;
            const auto & descriptor = input.descriptors[index];
            // An alias names an earlier descriptor's storage instead of
            // carrying bytes of its own; `linkcpp_tensor_desc` reports one
            // whenever two cut tensors view the same allocation. Resolve it
            // by the index it states and hand llama.cpp those bytes again,
            // the way `set_boundary` did in the runtime that came before P4.
            const auto source = descriptor.has_alias
                ? static_cast<std::size_t>(descriptor.alias_of) : index;
            if (source >= input.payloads.size()) {
                return fail_hop("staged HOP input descriptor has no payload", error);
            }
            const auto & payload = input.payloads[source];
            if (!payload.has_value() || payload->size() != descriptor.nbytes) {
                return fail_hop("staged HOP input payload length does not match descriptor", error);
            }
            llama_linkcpp_tensor_desc llama_descriptor{};
            if (!copy_descriptor(descriptor, &llama_descriptor, error) ||
                !llama_linkcpp_input_set_tensor(ctx_, &llama_descriptor,
                                                payload->data(), payload->size())) {
                return fail_hop("llama.cpp rejected staged HOP input tensor", error);
            }
        }

        llama_batch batch = llama_batch_init(
            static_cast<int32_t>(chunk.token_count), 0, static_cast<int32_t>(sequence_limit));
        if (batch.token == nullptr || batch.pos == nullptr || batch.n_seq_id == nullptr ||
            batch.seq_id == nullptr || batch.logits == nullptr) {
            llama_batch_free(batch);
            return fail_hop("llama.cpp failed to allocate staged HOP batch", error);
        }
        // Later stages consume the cut-set, not token ids, but llama_batch
        // still requires initialized token storage when embd is null.
        if (input_tokens.empty()) {
            std::fill(batch.token, batch.token + chunk.token_count, llama_token(0));
        } else {
            std::copy_n(input_tokens.begin() + static_cast<std::ptrdiff_t>(chunk.token_offset),
                        static_cast<std::ptrdiff_t>(chunk.token_count), batch.token);
        }
        std::free(batch.pos);
        batch.pos = nullptr;
        batch.n_tokens = static_cast<int32_t>(chunk.token_count);
        for (int32_t i = 0; i < batch.n_tokens; ++i) {
            batch.n_seq_id[i] = 1;
            batch.seq_id[i][0] = sequence_id;
            batch.logits[i] = i == batch.n_tokens - 1;
        }
        const auto decode_status = decode(batch, error);
        last_batch_tokens = batch.n_tokens;
        llama_batch_free(batch);
        if (decode_status != DecodeStatus::Success) {
            // Aborted/Fatal leave processed ubatches in the memory state,
            // same as the batched decode path -- see decode_status.hpp.
            // NoKvSlot/InvalidInput restore it, but this per-sequence path
            // has no fallback beneath it for the caller to retry into, so
            // the REFUSAL/FAILURE split that matters for the batched path
            // does not apply here: any non-success ends this HOP either way.
            if (decode_status_leaves_memory_dirty(decode_status)) {
                hop_memory_dirty_ = true;
            }
            if (hop_trace_enabled()) {
                std::fprintf(stderr, "P4_STAGED_HOP_DECODE_FAIL stage=%d-%d phase=%s seq=%s status=%d error=%s\n",
                             config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                             input.sequence_id.c_str(), static_cast<int>(decode_status),
                             error != nullptr ? error->c_str() : "none");
            }
            return false;
        }
        // Everything below this point in the function runs only after a
        // decode succeeded, so any failure from here on means the KV cache
        // has already moved past what rollback_hop_batch can undo -- this
        // runtime must refuse every later HOP until it is reloaded.
        const auto count = llama_linkcpp_output_count(ctx_);
        if (count < 0) {
            hop_memory_dirty_ = true;
            return fail_hop("llama.cpp returned an invalid staged output count", error);
        }
        const auto base = result.descriptors.size();
        for (int32_t i = 0; i < count; ++i) {
            llama_linkcpp_tensor_desc llama_descriptor{};
            if (!llama_linkcpp_output_desc(ctx_, i, &llama_descriptor)) {
                hop_memory_dirty_ = true;
                return fail_hop("llama.cpp could not describe staged HOP output", error);
            }
            auto descriptor = protocol_descriptor(llama_descriptor);
            if (descriptor.has_alias) descriptor.alias_of += static_cast<std::uint32_t>(base);
            if (descriptor.nbytes > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
                hop_memory_dirty_ = true;
                return fail_hop("staged HOP output is too large", error);
            }
            result.descriptors.push_back(std::move(descriptor));
            if (result.descriptors.back().has_alias) {
                result.payloads.emplace_back(std::nullopt);
            } else {
                result.payloads.emplace_back(
                    std::vector<std::uint8_t>(static_cast<std::size_t>(result.descriptors.back().nbytes)));
                if (!llama_linkcpp_output_get(ctx_, i, result.payloads.back()->data(),
                                              result.payloads.back()->size())) {
                    hop_memory_dirty_ = true;
                    return fail_hop("llama.cpp rejected staged HOP output tensor", error);
                }
            }
        }
        // Each output belongs to this chunk. Synchronize before the next
        // decode so an async device-to-host copy cannot be overwritten.
        if (!synchronize_outputs(error)) {
            hop_memory_dirty_ = true;
            return false;
        }
        if (hop_trace_enabled()) {
            std::fprintf(stderr, "P4_STAGED_HOP_CHUNK_DONE stage=%d-%d phase=%s seq=%s outputs=%d\n",
                         config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                         input.sequence_id.c_str(), count);
        }
    }

    // The sampler step itself is split into sample_hop_outcome() -- see its
    // doc comment in llama_stage_runtime.hpp -- to keep this file under the
    // repository's line limit.
    std::optional<protocol::SequencePayload::OutcomeMetadata> outcome;
    if (tail_stage_ &&
        !sample_hop_outcome(input, phase, input_tokens, last_batch_tokens, &outcome, error)) {
        return false;
    }

    result.outcome = std::move(outcome);

    // Terminal tensors are final-stage results, not cut-set inputs. Expose
    // them only from the tail so an intermediate stage never forwards logits
    // into the next stage's input table.
    if (tail_stage_) {
        // The cut-set collected above is an internal carry between stages.
        // Once the tail has consumed the final prefill/decode chunk, it must
        // not send those activations back to the outer node.  Stage 0 starts
        // the next decode from its token input and ignores the previous tail
        // cut-set; returning all prefill chunks here would only inflate the
        // response and make the next lap ambiguous.
        result.descriptors.clear();
        result.payloads.clear();

        const auto terminal_count = llama_linkcpp_terminal_count(ctx_);
        if (terminal_count < 0) {
            hop_memory_dirty_ = true;
            return fail_hop("llama.cpp returned an invalid staged terminal count", error);
        }
        for (int32_t i = 0; i < terminal_count; ++i) {
            llama_linkcpp_tensor_desc llama_descriptor{};
            if (!llama_linkcpp_terminal_desc(ctx_, i, &llama_descriptor)) {
                hop_memory_dirty_ = true;
                return fail_hop("llama.cpp could not describe staged terminal output", error);
            }
            auto descriptor = protocol_descriptor(llama_descriptor);
            if (descriptor.has_alias) {
                result.descriptors.push_back(std::move(descriptor));
                result.payloads.emplace_back(std::nullopt);
                continue;
            }
            if (descriptor.nbytes > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
                hop_memory_dirty_ = true;
                return fail_hop("staged terminal output is too large", error);
            }
            result.payloads.emplace_back(
                std::vector<std::uint8_t>(static_cast<std::size_t>(descriptor.nbytes)));
            result.descriptors.push_back(std::move(descriptor));
            if (!llama_linkcpp_terminal_get(ctx_, i, result.payloads.back()->data(),
                                            result.payloads.back()->size())) {
                hop_memory_dirty_ = true;
                return fail_hop("llama.cpp rejected staged terminal output", error);
            }
        }
    }

    if (!synchronize_outputs(error)) {
        hop_memory_dirty_ = true;
        return false;
    }
    sequence_positions_[input.sequence_id] = result.outcome.has_value()
        ? result.outcome->position
        : input.position.value_or(0) + static_cast<std::uint64_t>(token_count);
    *output = std::move(result);
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_END stage=%d-%d phase=%s seq=%s descriptors=%zu outcome=%d\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), output->descriptors.size(),
                     output->outcome.has_value() ? 1 : 0);
    }
    return true;
}

} // namespace staged::llama_runtime
