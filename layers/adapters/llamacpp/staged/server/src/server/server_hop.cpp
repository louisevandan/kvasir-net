#include <cstdlib>
#include "server.hpp"

#include <algorithm>
#include <utility>

namespace staged::server {

bool Session::execute_hop(const protocol::SequencePayload &input,
                          protocol::HopPhase phase,
                          protocol::SequencePayload *output,
                          std::string *error) const {
    if (hop_executor_) {
        return hop_executor_(input, phase, output, error);
    }
#ifdef P4_STAGED_WITH_LLAMA
    if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
        if (error != nullptr) {
            *error = "llama stage runtime is not loaded";
        }
        return false;
    }
    return llama_runtime_->execute_hop(input, phase, output, error);
#else
    if (error != nullptr) {
        *error = "descriptor-to-llama_batch bridge is unavailable";
    }
    return false;
#endif
}

protocol::Frame Session::handle_hop(const protocol::Frame &request) {
    protocol::HopPayload input;
    bool enveloped = false;
    try {
        input = protocol::HopPayload::decode(request.body,
                                             protocol::ProtocolLimits{},
                                             &enveloped);
    } catch (const protocol::ProtocolError &exception) {
        return error(exception.what());
    }
    const auto result = runtime_.begin_hop();
    if (!result.ok()) {
        return error("HOP rejected: invalid session state");
    }
    if (!capabilities_.hop) {
        (void)runtime_.cancel();
        return error("CAPABILITY_UNAVAILABLE: staged HOP execution is unavailable");
    }
    if (!hop_executor_
#ifdef P4_STAGED_WITH_LLAMA
        && (llama_runtime_ == nullptr || !llama_runtime_->loaded())
#else
        && true
#endif
    ) {
        (void)runtime_.cancel();
        return error("CAPABILITY_UNAVAILABLE: staged HOP execution is unavailable");
    }
    bool hop_batch_started = false;
#ifdef P4_STAGED_WITH_LLAMA
    if (llama_runtime_ != nullptr) {
        llama_runtime_->begin_hop_batch();
        hop_batch_started = true;
    }
#endif
    auto fail_hop = [&](std::string detail) {
#ifdef P4_STAGED_WITH_LLAMA
        if (hop_batch_started) {
            std::string rollback_error;
            if (!llama_runtime_->rollback_hop_batch(&rollback_error)) {
                detail += "; HOP rollback failed: " + rollback_error;
            }
        }
#endif
        (void)runtime_.cancel();
        return error(detail);
    };
    std::vector<protocol::SequencePayload> outputs;
    outputs.reserve(input.sequences.size());
    // A decode lap is one token per sequence, and running the sequences one
    // at a time reads this stage's weights once for each of them. Ask for the
    // whole lap in one batch first; the runtime refuses shapes it cannot
    // reshape, and the loop below is what a refusal falls back to.
    bool batched = false;
#ifdef P4_STAGED_WITH_LLAMA
    // Off unless asked for. The path itself is right — stage 0 batches a
    // whole lap today — but llama.cpp splits a batch into ubatches of its own
    // choosing and the staged cut-set is bound once per decode, so a split
    // leaves the graph expecting a narrower input than the one handed over,
    // and an output whose token axis is the ubatch rather than the lap. Both
    // were measured. Neither is recoverable once a partial batch has moved a
    // sequence's KV forward, so this stays behind a switch until the cut-set
    // is bound per ubatch in the compatibility series.
    if (llama_runtime_ != nullptr && input.phase == protocol::HopPhase::Decode &&
        input.sequences.size() > 1 && std::getenv("P4_STAGED_DECODE_BATCH") != nullptr) {
        std::string batch_error;
        std::vector<protocol::SequencePayload> batch_outputs;
        if (llama_runtime_->execute_decode_batch(input.sequences, &batch_outputs, &batch_error)) {
            outputs = std::move(batch_outputs);
            batched = true;
        } else if (!batch_error.empty()) {
            return fail_hop("HOP failed: " + batch_error);
        }
    }
#endif
    if (!batched) {
        for (const auto &sequence : input.sequences) {
            protocol::SequencePayload output;
            std::string hop_error;
            if (!execute_hop(sequence, input.phase, &output, &hop_error)) {
                return fail_hop("HOP failed: " + hop_error);
            }
            outputs.push_back(std::move(output));
        }
    }
    std::string encoded_body;
    try {
        const bool has_metadata = std::any_of(outputs.begin(), outputs.end(),
            [](const auto &sequence) { return sequence.outcome.has_value(); });
        const auto body = enveloped || has_metadata
            ? protocol::HopPayload{input.phase, std::move(outputs), false}.encode(protocol::ProtocolLimits{})
            : outputs.front().encode(protocol::ProtocolLimits{});
        encoded_body.assign(body.begin(), body.end());
    } catch (const protocol::ProtocolError &exception) {
        return fail_hop(exception.what());
    }
    const auto finished = runtime_.finish_hop();
    if (!finished.ok()) {
        return fail_hop("HOP state transition failed");
    }
#ifdef P4_STAGED_WITH_LLAMA
    if (hop_batch_started) llama_runtime_->commit_hop_batch();
#endif
    return status(protocol::Operation::HopResult, std::move(encoded_body));
}

} // namespace staged::server
