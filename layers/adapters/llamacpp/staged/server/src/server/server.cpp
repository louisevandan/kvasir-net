#include "server.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <limits>
#include <utility>

namespace staged::server {

namespace {

protocol::KvReceipt receipt_from(const protocol::KvPayload &payload,
                                 protocol::KvReceiptState state,
                                 std::string detail) {
    return protocol::KvReceipt{
        payload.operation_id, payload.sequence_id, payload.cache_key,
        payload.model_identity, payload.stage_begin, payload.stage_end,
        payload.flags, state, 0, std::string(64, '0'), std::move(detail)};
}

} // namespace

protocol::Frame Session::handle(const protocol::Frame &request, bool *close_after) {
    if (close_after != nullptr) {
        *close_after = false;
    }

    using protocol::Operation;
    switch (request.header.operation) {
    case Operation::Hello: {
        const auto result = runtime_.hello();
        if (!result.ok()) {
            return error("HELLO rejected: plan is not loaded");
        }
        const bool transaction_capability = transaction_store_.available()
            && capabilities_.kv && capabilities_.llama_runtime;
        const std::string capabilities =
            "READY;llama_runtime=" + std::string(capabilities_.llama_runtime ? "1" : "0") +
            ";hop=" + std::string(capabilities_.hop ? "1" : "0") +
            ";kv=" + std::string(capabilities_.kv ? "1" : "0") +
            ";transactions=" + std::string(transaction_capability ? "1" : "0") +
            ";physical_batch=" + std::string(capabilities_.llama_runtime ? "1" : "0") +
            ";request_options=1"
            ";request_options_semantics=n_prev,n_probs,samplers,sampler_seq,temperature,top_k,top_p,min_p,min_keep,typical_p,top_n_sigma,dynatemp_range,dynatemp_exponent,adaptive_target,adaptive_decay,ignore_eos,seed,penalty_last_n,penalty_repeat,penalty_freq,penalty_present,dry_multiplier,dry_base,dry_allowed_length,dry_penalty_last_n,dry_sequence_breakers,xtc_probability,xtc_threshold,mirostat,mirostat_tau,mirostat_eta,grammar,grammar_lazy,grammar_triggers,preserved_tokens,generation_prompt,logit_bias,reasoning_budget_tokens,reasoning_budget_start_tag,reasoning_budget_end_tags,reasoning_budget_end_tag,reasoning_budget_message"
#ifdef P4_STAGED_WITH_LLAMA
            + ";" + capabilities_.llama_options.serialize()
#endif
            ;
        std::vector<std::uint8_t> body{
            static_cast<std::uint8_t>(protocol::kProtocolRevision),
            static_cast<std::uint8_t>(protocol::kProtocolRevision >> 8U),
        };
        body.insert(body.end(), capabilities.begin(), capabilities.end());
        return protocol::Frame::make(Operation::Hello, std::move(body));
    }
    case Operation::Hop: {
        return handle_hop(request);
    }
    case Operation::LogicalBatch:
        return handle_logical_batch(request);
    case Operation::PhysicalBatch:
        return handle_physical_batch(request);
    case Operation::Tokenize:
        return handle_tokenize(request);
    case Operation::PhysicalRelease: {
#ifdef P4_STAGED_WITH_LLAMA
        if (llama_runtime_ == nullptr || !llama_runtime_->loaded()
            || request.body.size() <= 4) {
            return error("PHYSICAL_RELEASE rejected: invalid runtime or payload");
        }
        const std::uint32_t id = static_cast<std::uint32_t>(request.body[0])
            | static_cast<std::uint32_t>(request.body[1]) << 8U
            | static_cast<std::uint32_t>(request.body[2]) << 16U
            | static_cast<std::uint32_t>(request.body[3]) << 24U;
        const std::string key(request.body.begin() + 4, request.body.end());
        std::string detail;
        if (id > static_cast<std::uint32_t>(std::numeric_limits<llama_seq_id>::max())
            || !llama_runtime_->release_physical_sequence(
                key, static_cast<llama_seq_id>(id), &detail)) {
            return error("PHYSICAL_RELEASE failed: " + detail);
        }
        return status(Operation::PhysicalRelease, "SEQUENCE_RELEASED");
#else
        return error("CAPABILITY_UNAVAILABLE: llama runtime is unavailable");
#endif
    }
    case Operation::Cancel: {
        // An empty CANCEL retains the original meaning: cancel an active HOP.
        // A sequence id body is the post-completion cleanup path used by the
        // staged adapter. It only releases in-memory llama state; persisted
        // KV checkpoints remain untouched and are managed by KV_DROP.
        if (!request.body.empty()) {
            if (runtime_.state() != runtime::State::Ready) {
                return error("SEQUENCE_RELEASE rejected: invalid session state");
            }
#ifdef P4_STAGED_WITH_LLAMA
            if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
                return error("CAPABILITY_UNAVAILABLE: llama runtime is unavailable");
            }
            const std::string sequence_id(request.body.begin(), request.body.end());
            if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
                std::fprintf(stderr, "P4_STAGED_SERVER_RELEASE_REQUEST sequence=%s\\n", sequence_id.c_str());
            }
            std::string release_error;
            if (!llama_runtime_->release_sequence(sequence_id, &release_error)) {
                return error(release_error.empty() ? "SEQUENCE_RELEASE failed" : release_error);
            }
            if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
                std::fprintf(stderr, "P4_STAGED_SERVER_RELEASED sequence=%s\\n", sequence_id.c_str());
            }
            return status(Operation::Cancel, "SEQUENCE_RELEASED");
#else
            return error("CAPABILITY_UNAVAILABLE: llama runtime is unavailable");
#endif
        }
        const auto result = runtime_.cancel();
        if (!result.ok()) {
            // A terminal response can race an outer cancellation request.
            // Once the HOP has already completed there is no work left to
            // cancel; acknowledge the request so cleanup remains idempotent.
            // Keep rejecting cancellation while the session is in another
            // invalid state (for example during unload or KV work).
            if (runtime_.state() == runtime::State::Ready) {
                return status(Operation::Cancel, "HOP_ALREADY_COMPLETE");
            }
            return error("CANCEL rejected: no active HOP");
        }
        return status(Operation::Cancel, "HOP_CANCELLED");
    }
    case Operation::KvPrepare:
    case Operation::KvCommit:
    case Operation::KvAbort:
    case Operation::KvReconcile: {
        const auto operation = request.header.operation;
        const auto trace_transaction = [] {
            return std::getenv("P4_STAGED_TRACE_KV_TRANSACTION") != nullptr;
        };
        if (trace_transaction()) {
            std::cerr << "KV_TRANSACTION_BEGIN operation="
                      << static_cast<int>(operation) << '\n';
        }
        const auto internal = operation == Operation::KvPrepare
            ? runtime::Operation::KvPrepare
            : operation == Operation::KvCommit
                ? runtime::Operation::KvCommit
                : operation == Operation::KvAbort
                    ? runtime::Operation::KvAbort
                    : runtime::Operation::KvReconcile;
        const auto active = runtime_.begin_kv(internal);
        if (!active.ok()) return error("KV transaction rejected: invalid session state");
        protocol::KvPayload payload;
        try {
            payload = protocol::KvPayload::decode(request.body, protocol::ProtocolLimits{});
        } catch (const protocol::ProtocolError &exception) {
            (void)runtime_.finish_kv();
            return error(exception.what());
        }
        if (trace_transaction()) {
            std::cerr << "KV_TRANSACTION_PAYLOAD op=" << payload.operation_id
                      << " sequence=" << payload.sequence_id
                      << " flags=" << payload.flags << '\n';
        }
        if (!transaction_store_.available()) {
            (void)runtime_.finish_kv();
            return error("CAPABILITY_UNAVAILABLE: durable KV transaction store is unavailable");
        }
        if (!capabilities_.kv) {
            (void)runtime_.finish_kv();
            return error("CAPABILITY_UNAVAILABLE: KV persistence is unavailable");
        }
        if (payload.operation_id.empty()
            || (operation == Operation::KvPrepare
                && (payload.flags < protocol::kKvPersist || payload.flags > protocol::kKvDiscard))
            || (operation != Operation::KvPrepare && payload.flags != protocol::kKvDirect)) {
            (void)runtime_.finish_kv();
            return error("invalid KV transaction identity or kind");
        }

        std::string lease_error;
        auto lease = transaction_store_.acquire(payload.operation_id, &lease_error);
        if (!lease.held()) {
            (void)runtime_.finish_kv();
            return error("KV transaction lease unavailable: " + lease_error);
        }

        protocol::KvReceipt receipt;
        std::string store_error;
        if (operation == Operation::KvPrepare) {
            if (!transaction_store_.prepare(payload, &receipt, &store_error)) {
                (void)runtime_.finish_kv();
                return error("KV prepare failed: " + store_error);
            }
            if (trace_transaction()) std::cerr << "KV_TRANSACTION_PREPARED\n";
        } else if (operation == Operation::KvReconcile) {
            if (!transaction_store_.read(payload, &receipt, &store_error)) {
                const auto absent = store_error == "transaction receipt absent";
                receipt = receipt_from(payload,
                    absent ? protocol::KvReceiptState::Absent
                           : protocol::KvReceiptState::Inconsistent,
                    absent ? "absent" : "receipt-corrupt");
            } else if (receipt.state == protocol::KvReceiptState::Committed) {
#ifdef P4_STAGED_WITH_LLAMA
                if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
                    receipt.state = protocol::KvReceiptState::Inconsistent;
                    receipt.detail = "committed receipt cannot be verified without llama runtime";
                } else {
                    auto verify_payload = payload;
                    verify_payload.flags = 0;
                    verify_payload.operation_id.clear();
                    std::string verify_error;
                    if (!llama_runtime_->reconcile_transaction(
                            verify_payload, receipt, &verify_error)) {
                        receipt.state = protocol::KvReceiptState::Inconsistent;
                        receipt.detail = verify_error.empty()
                            ? "committed receipt does not match KV state"
                            : verify_error;
                    }
                }
#else
                receipt.state = protocol::KvReceiptState::Inconsistent;
                receipt.detail = "committed receipt cannot be verified without llama runtime";
#endif
            }
        } else if (operation == Operation::KvAbort) {
            if (!transaction_store_.read(payload, &receipt, &store_error)) {
                (void)runtime_.finish_kv();
                return error("KV abort failed: " + store_error);
            }
            if (receipt.state == protocol::KvReceiptState::Aborted) {
                // Idempotent abort.
            } else if (receipt.state == protocol::KvReceiptState::Prepared) {
                receipt.state = protocol::KvReceiptState::Aborted;
                receipt.detail = "aborted";
                if (!transaction_store_.write(receipt, &store_error)) {
                    (void)runtime_.finish_kv();
                    return error("KV abort receipt failed: " + store_error);
                }
            } else {
                receipt.state = protocol::KvReceiptState::Inconsistent;
                receipt.detail = "abort-after-commit-or-ambiguous";
            }
        } else {
            if (!transaction_store_.read(payload, &receipt, &store_error)) {
                (void)runtime_.finish_kv();
                return error("KV commit failed: " + store_error);
            }
            if (receipt.state == protocol::KvReceiptState::Committed) {
                // Idempotent commit: return the durable result without rerunning llama.
            } else if (receipt.state != protocol::KvReceiptState::Prepared) {
                (void)runtime_.finish_kv();
                return error("KV commit rejected: receipt is not prepared");
            } else {
                receipt.state = protocol::KvReceiptState::Committing;
                receipt.detail = "committing";
                if (!transaction_store_.write(receipt, &store_error)) {
                    (void)runtime_.finish_kv();
                    return error("KV commit fence failed: " + store_error);
                }
#ifdef P4_STAGED_WITH_LLAMA
                if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
                    receipt.state = protocol::KvReceiptState::Inconsistent;
                    receipt.detail = "llama KV bridge unavailable after commit fence";
                    (void)transaction_store_.write(receipt, nullptr);
                    (void)runtime_.finish_kv();
                    return error("CAPABILITY_UNAVAILABLE: llama KV bridge is unavailable");
                }
                auto bridge_payload = payload;
                bridge_payload.flags = protocol::kKvDirect;
                bridge_payload.operation_id.clear();
                protocol::KvResult result_value;
                std::string bridge_error;
                const bool ok = receipt.kind == protocol::kKvPersist
                    ? llama_runtime_->save(bridge_payload, &result_value, &bridge_error)
                    : receipt.kind == protocol::kKvRestore
                        ? llama_runtime_->restore(bridge_payload, &result_value, &bridge_error)
                        : llama_runtime_->drop(bridge_payload, &result_value, &bridge_error);
                if (!ok) {
                    receipt.state = protocol::KvReceiptState::Inconsistent;
                    receipt.detail = bridge_error.empty() ? "KV bridge rejected operation" : bridge_error;
                    (void)transaction_store_.write(receipt, nullptr);
                    (void)runtime_.finish_kv();
                    return error(receipt.detail);
                }
                receipt.state = protocol::KvReceiptState::Committed;
                receipt.bytes = result_value.bytes;
                receipt.checksum = result_value.checksum;
                receipt.detail = "committed";
                if (!transaction_store_.write(receipt, &store_error)) {
                    (void)runtime_.finish_kv();
                    return error("KV commit receipt finalization failed: " + store_error);
                }
#else
                receipt.state = protocol::KvReceiptState::Inconsistent;
                receipt.detail = "llama KV bridge unavailable";
                (void)transaction_store_.write(receipt, nullptr);
                (void)runtime_.finish_kv();
                return error("CAPABILITY_UNAVAILABLE: llama KV bridge is unavailable");
#endif
            }
        }
        (void)runtime_.finish_kv();
        if (trace_transaction()) {
            std::cerr << "KV_TRANSACTION_REPLY state="
                      << static_cast<int>(receipt.state) << '\n';
        }
        try {
            const auto encoded = receipt.encode(protocol::ProtocolLimits{});
            return status(Operation::KvReceipt, std::string(encoded.begin(), encoded.end()));
        } catch (const protocol::ProtocolError &exception) {
            return error(exception.what());
        }
    }
    case Operation::KvSave:
    case Operation::KvRestore:
    case Operation::KvDrop: {
        const auto operation = request.header.operation;
        const auto internal = operation == Operation::KvSave
            ? runtime::Operation::KvSave
            : operation == Operation::KvRestore
                ? runtime::Operation::KvRestore
                : runtime::Operation::KvDrop;
        const auto result = runtime_.begin_kv(internal);
        if (!result.ok()) {
            return error("KV operation rejected: invalid session state");
        }
        if (!capabilities_.kv) {
            (void)runtime_.finish_kv();
            return error("CAPABILITY_UNAVAILABLE: KV persistence is unavailable");
        }
#ifdef P4_STAGED_WITH_LLAMA
        if (llama_runtime_ == nullptr || !llama_runtime_->loaded()) {
            (void)runtime_.finish_kv();
            return error("CAPABILITY_UNAVAILABLE: llama KV bridge is unavailable");
        }
        protocol::KvPayload payload;
        try {
            payload = protocol::KvPayload::decode(request.body,
                                                  protocol::ProtocolLimits{});
        } catch (const protocol::ProtocolError &exception) {
            (void)runtime_.finish_kv();
            return error(exception.what());
        }
        protocol::KvResult result_value;
        std::string bridge_error;
        const bool ok = operation == Operation::KvSave
            ? llama_runtime_->save(payload, &result_value, &bridge_error)
            : operation == Operation::KvRestore
                ? llama_runtime_->restore(payload, &result_value, &bridge_error)
                : llama_runtime_->drop(payload, &result_value, &bridge_error);
        (void)runtime_.finish_kv();
        if (!ok) {
            return error(bridge_error.empty() ? "KV bridge rejected operation" : bridge_error);
        }
        try {
            const auto encoded = result_value.encode(protocol::ProtocolLimits{});
            return status(Operation::KvResult,
                          std::string(encoded.begin(), encoded.end()));
        } catch (const protocol::ProtocolError &exception) {
            return error(exception.what());
        }
#else
        (void)runtime_.finish_kv();
        return error("CAPABILITY_UNAVAILABLE: llama KV bridge is unavailable");
#endif
    }
    case Operation::Unload: {
        const auto result = runtime_.unload();
        if (!result.ok()) {
            return error("UNLOAD rejected: active operation or invalid session state");
        }
        (void)runtime_.finish_unload();
        if (close_after != nullptr) {
            *close_after = true;
        }
        return status(Operation::Unload, "UNLOADED");
    }
    case Operation::HopResult:
    case Operation::KvResult:
    case Operation::KvReceipt:
    case Operation::PhysicalResult:
    case Operation::Tokenized:
    case Operation::Error:
        return error("unsupported client operation");
    }
    return error("unsupported client operation");
}

} // namespace staged::server
