#include "llama_stage_runtime.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>

#include "ggml.h"
#include "llama_stage_runtime_hop_shared.hpp"
#include "request_options.hpp"

namespace staged::llama_runtime {

using namespace hop;



bool StageRuntime::execute_hop(const protocol::SequencePayload & input,
                               protocol::HopPhase phase,
                               protocol::SequencePayload * output,
                               std::string * error) {
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

    // A long prefill may be split by llama.cpp into several ubatches.  The
    // transport must preserve every cut-set chunk; forwarding only the last
    // one loses the earlier tokens from every downstream stage's KV cache.
    // Stage 0 can split token ids directly.  A middle stage receives one
    // tensor per chunk from the current wire representation.
    struct Chunk {
        std::size_t token_count = 0;
        std::size_t descriptor_index = std::numeric_limits<std::size_t>::max();
        std::size_t token_offset = 0;
    };
    std::vector<Chunk> chunks;
    // A decode lap always starts at stage 0.  Its native KV already contains
    // the prompt, and the first stage consumes token ids/placeholders rather
    // than the previous lap's tail cut-set.  Re-injecting that cut-set would
    // replay the whole prefill (and can exceed the stage's sequence window).
    const bool ignore_inbound_cut_set = config_.layer_begin == 0
        && phase == protocol::HopPhase::Decode;
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_CHUNKS_BEGIN stage=%d-%d phase=%s seq=%s ignore=%d descriptors=%zu\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), ignore_inbound_cut_set ? 1 : 0,
                     input.descriptors.size());
    }
    if (!ignore_inbound_cut_set && input_tokens.empty() && !input.descriptors.empty()) {
        // A cut-set can contain one rank-1 hidden tensor per one-token
        // ubatch.  In that representation the token axis is intentionally
        // squeezed, so dimensions[0] is the embedding width rather than the
        // token count.  The explicit logical count is the authoritative
        // grouping signal; reject only when it cannot prove that each
        // descriptor represents exactly one token.
        const bool one_token_per_descriptor =
            input.n_tokens.has_value() &&
            input.n_tokens.value() == input.descriptors.size();
        if (input.descriptors.size() > 1 && !one_token_per_descriptor) {
            // The current GGML transformer cut-set has one tensor per ubatch.
            // Refuse ambiguous multi-tensor grouping instead of silently
            // assigning tensors from different chunks to one graph.
            for (const auto & descriptor : input.descriptors) {
                if (descriptor.dimensions.size() < 2) {
                    return fail_hop("staged HOP multi-chunk descriptor has no token dimension", error);
                }
            }
        }
        for (std::size_t i = 0; i < input.descriptors.size(); ++i) {
            const auto & descriptor = input.descriptors[i];
            if (hop_trace_enabled()) {
                std::fprintf(stderr, "P4_STAGED_HOP_DESCRIPTOR stage=%d-%d phase=%s seq=%s index=%zu dims=%zu token_dim=%llu bytes=%llu\n",
                             config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                             input.sequence_id.c_str(), i, descriptor.dimensions.size(),
                             descriptor.dimensions.size() > 1 ? static_cast<unsigned long long>(descriptor.dimensions[1]) : 0ULL,
                             static_cast<unsigned long long>(descriptor.nbytes));
            }
            // llama.cpp may squeeze the token axis for a one-token decode
            // output, exposing the hidden state as [n_embd] instead of
            // [n_embd, 1].  It is still exactly one executable token.  A
            // prefill descriptor must retain its explicit second dimension.
            if (descriptor.dimensions.size() == 1 &&
                (phase == protocol::HopPhase::Decode || one_token_per_descriptor)) {
                chunks.push_back({1, i, 0});
                continue;
            }
            if (descriptor.dimensions.size() < 2 || descriptor.dimensions[1] == 0) {
                return fail_hop("staged HOP cut-set descriptor has invalid token dimension", error);
            }
            chunks.push_back({static_cast<std::size_t>(descriptor.dimensions[1]), i, 0});
        }
    } else {
        const auto chunk_size = phase == protocol::HopPhase::Prefill
            ? std::max<std::size_t>(1, llama_n_ubatch(ctx_)) : token_count;
        for (std::size_t offset = 0; offset < token_count; offset += chunk_size) {
            chunks.push_back({std::min(chunk_size, token_count - offset),
                              std::numeric_limits<std::size_t>::max(), offset});
        }
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_CHUNKS_DONE stage=%d-%d phase=%s seq=%s chunks=%zu\n",
                     config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                     input.sequence_id.c_str(), chunks.size());
    }
    if (chunks.empty()) return fail_hop("staged HOP has no executable chunks", error);
    const auto chunk_tokens = [&chunks]() {
        std::size_t total = 0;
        for (const auto & chunk : chunks) total += chunk.token_count;
        return total;
    };
    if (chunk_tokens() != token_count) {
        return fail_hop("staged HOP cut-set token count does not match payload", error);
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
            std::fprintf(stderr, "P4_STAGED_HOP_CHUNK stage=%d-%d phase=%s seq=%s tokens=%zu descriptor=%zu\n",
                         config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                         input.sequence_id.c_str(), chunk.token_count, chunk.descriptor_index);
        }
        llama_linkcpp_input_clear(ctx_);
        if (chunk.descriptor_index != std::numeric_limits<std::size_t>::max()) {
            const auto index = chunk.descriptor_index;
            const auto & descriptor = input.descriptors[index];
            if (index >= input.payloads.size()) {
                return fail_hop("staged HOP input descriptor has no payload", error);
            }
            const auto & payload = input.payloads[index];
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
        const bool decoded = decode(batch, error);
        last_batch_tokens = batch.n_tokens;
        llama_batch_free(batch);
        if (!decoded) {
            if (hop_trace_enabled()) {
                std::fprintf(stderr, "P4_STAGED_HOP_DECODE_FAIL stage=%d-%d phase=%s seq=%s error=%s\n",
                             config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                             input.sequence_id.c_str(), error != nullptr ? error->c_str() : "none");
            }
            return false;
        }

        const auto count = llama_linkcpp_output_count(ctx_);
        if (count < 0) return fail_hop("llama.cpp returned an invalid staged output count", error);
        const auto base = result.descriptors.size();
        for (int32_t i = 0; i < count; ++i) {
            llama_linkcpp_tensor_desc llama_descriptor{};
            if (!llama_linkcpp_output_desc(ctx_, i, &llama_descriptor)) {
                return fail_hop("llama.cpp could not describe staged HOP output", error);
            }
            auto descriptor = protocol_descriptor(llama_descriptor);
            if (descriptor.has_alias) descriptor.alias_of += static_cast<std::uint32_t>(base);
            if (descriptor.nbytes > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
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
                    return fail_hop("llama.cpp rejected staged HOP output tensor", error);
                }
            }
        }
        // Each output belongs to this chunk. Synchronize before the next
        // decode so an async device-to-host copy cannot be overwritten.
        if (!synchronize_outputs(error)) return false;
        if (hop_trace_enabled()) {
            std::fprintf(stderr, "P4_STAGED_HOP_CHUNK_DONE stage=%d-%d phase=%s seq=%s outputs=%d\n",
                         config_.layer_begin, config_.layer_end, hop_phase_name(phase),
                         input.sequence_id.c_str(), count);
        }
    }

    std::optional<protocol::SequencePayload::OutcomeMetadata> outcome;
    if (tail_stage_) {
        const auto options_found = sampler_options_.find(input.sequence_id);
        if (options_found != sampler_options_.end() && options_found->second != input.options) {
            samplers_.erase(input.sequence_id);
            sampler_options_.erase(options_found);
            sampled_tokens_.erase(input.sequence_id);
            sampled_texts_.erase(input.sequence_id);
        }
        auto found = samplers_.find(input.sequence_id);
        if (found == samplers_.end()) {
            auto sampling = params_.sampling;
            if (!apply_request_options(input.options, model_, &sampling, error)) return false;
            if (hop_trace_enabled()) {
                std::fprintf(stderr, "P4_STAGED_SAMPLER_OPTIONS stage=%d-%d seq=%s options_bytes=%zu ignore_eos=%d bias_count=%zu\n",
                             config_.layer_begin, config_.layer_end, input.sequence_id.c_str(),
                             input.options.size(), sampling.ignore_eos ? 1 : 0,
                             sampling.logit_bias.size());
            }
            common_sampler_ptr sampler(common_sampler_init(model_, sampling));
            if (!sampler) return fail_hop("llama.cpp failed to create staged sampler", error);
            if (!input_tokens.empty()) {
                for (const auto token : input_tokens) {
                    common_sampler_accept(sampler.get(), token, false);
                }
            }
            found = samplers_.emplace(input.sequence_id, std::move(sampler)).first;
            sampler_options_[input.sequence_id] = input.options;
        }
        if (phase == protocol::HopPhase::Decode) {
            const auto sampled = common_sampler_sample(found->second.get(), ctx_, last_batch_tokens - 1);
            if (sampled == LLAMA_TOKEN_NULL) {
                return fail_hop("llama.cpp staged sampler returned no token", error);
            }
            common_sampler_accept(found->second.get(), sampled, true);
            const auto *vocab = llama_model_get_vocab(model_);
            if (vocab == nullptr) {
                return fail_hop("llama.cpp did not expose a sampler vocabulary", error);
            }
            protocol::SequencePayload::OutcomeMetadata metadata;
            metadata.token = static_cast<std::int32_t>(sampled);
            metadata.position = input.position.value_or(0) + 1;
            const bool end_of_generation = llama_vocab_is_eog(vocab, sampled);
            if (hop_trace_enabled()) {
                std::fprintf(stderr, "P4_STAGED_SAMPLE stage=%d-%d seq=%s token=%d eog=%d ignore_eos=%d\n",
                             config_.layer_begin, config_.layer_end, input.sequence_id.c_str(),
                             static_cast<int>(sampled), end_of_generation ? 1 : 0,
                             input.options.find("ignore_eos") != std::string::npos ? 1 : 0);
            }
            if (!end_of_generation) {
                auto & generated = sampled_tokens_[input.sequence_id];
                generated.push_back(sampled);
                const auto detokenized = common_detokenize(vocab, generated, false);
                auto & emitted = sampled_texts_[input.sequence_id];
                if (detokenized.size() >= emitted.size() &&
                    detokenized.compare(0, emitted.size(), emitted) == 0) {
                    metadata.text = detokenized.substr(emitted.size());
                } else {
                    metadata.text = detokenized;
                }
                if (!valid_utf8_text(metadata.text)) {
                    metadata.text = "\xEF\xBF\xBD";
                }
                emitted = detokenized;
            }
            if (end_of_generation) metadata.stop = "eos";
            outcome = std::move(metadata);
        }
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
            return fail_hop("llama.cpp returned an invalid staged terminal count", error);
        }
        for (int32_t i = 0; i < terminal_count; ++i) {
            llama_linkcpp_tensor_desc llama_descriptor{};
            if (!llama_linkcpp_terminal_desc(ctx_, i, &llama_descriptor)) {
                return fail_hop("llama.cpp could not describe staged terminal output", error);
            }
            auto descriptor = protocol_descriptor(llama_descriptor);
            if (descriptor.has_alias) {
                result.descriptors.push_back(std::move(descriptor));
                result.payloads.emplace_back(std::nullopt);
                continue;
            }
            if (descriptor.nbytes > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
                return fail_hop("staged terminal output is too large", error);
            }
            result.payloads.emplace_back(
                std::vector<std::uint8_t>(static_cast<std::size_t>(descriptor.nbytes)));
            result.descriptors.push_back(std::move(descriptor));
            if (!llama_linkcpp_terminal_get(ctx_, i, result.payloads.back()->data(),
                                            result.payloads.back()->size())) {
                return fail_hop("llama.cpp rejected staged terminal output", error);
            }
        }
    }

    if (!synchronize_outputs(error)) {
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
