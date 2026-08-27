#include "server.hpp"

#include <cstdlib>
#include <iostream>
#include <utility>

#ifndef P4_STAGED_LLAMA_UPSTREAM_COMMIT
#define P4_STAGED_LLAMA_UPSTREAM_COMMIT "none"
#endif

namespace staged::server {

protocol::Frame Session::handle_hello() {
    const auto result = runtime_.hello();
    if (!result.ok()) return error("HELLO rejected: plan is not loaded");
    const bool transaction_capability = transaction_store_.available()
        && capabilities_.kv && capabilities_.llama_runtime;
    const bool atomic_batch_exclusive =
#ifdef P4_STAGED_WITH_LLAMA
        llama_runtime_ != nullptr && capabilities_.llama_options.mtp_requested
        && !llama_runtime_->kv_unified();
#else
        false;
#endif
    const std::string capabilities =
        "READY;llama_runtime=" + std::string(capabilities_.llama_runtime ? "1" : "0") +
        ";hop=" + std::string(capabilities_.hop ? "1" : "0") +
        ";kv=" + std::string(capabilities_.kv ? "1" : "0") +
        ";transactions=" + std::string(transaction_capability ? "1" : "0") +
        ";physical_batch=" + std::string(capabilities_.llama_runtime ? "1" : "0") +
        ";upstream=" P4_STAGED_LLAMA_UPSTREAM_COMMIT +
        ";request_options=1"
        ";request_options_semantics=n_prev,n_probs,samplers,sampler_seq,temperature,top_k,top_p,min_p,min_keep,typical_p,top_n_sigma,dynatemp_range,dynatemp_exponent,adaptive_target,adaptive_decay,ignore_eos,seed,penalty_last_n,penalty_repeat,penalty_freq,penalty_present,dry_multiplier,dry_base,dry_allowed_length,dry_penalty_last_n,dry_sequence_breakers,xtc_probability,xtc_threshold,mirostat,mirostat_tau,mirostat_eta,grammar,grammar_lazy,grammar_triggers,preserved_tokens,generation_prompt,logit_bias,reasoning_budget_tokens,reasoning_budget_start_tag,reasoning_budget_end_tags,reasoning_budget_end_tag,reasoning_budget_message"
#ifdef P4_STAGED_WITH_LLAMA
        + ";n_ctx=" + std::to_string(llama_runtime_ == nullptr ? 0 : llama_runtime_->context_size())
        + ";n_batch=" + std::to_string(llama_runtime_ == nullptr ? 0 : llama_runtime_->batch_size())
        + ";n_ubatch=" + std::to_string(llama_runtime_ == nullptr ? 0 : llama_runtime_->ubatch_size())
        + ";n_seq_max=" + std::to_string(llama_runtime_ == nullptr ? 0 : llama_runtime_->sequence_capacity())
        + ";equal_sequence_ubatch=" + std::string(llama_runtime_ != nullptr
            && llama_runtime_->requires_equal_sequence_ubatch() ? "1" : "0")
        + ";max_atomic_sequences=" + std::to_string(llama_runtime_ == nullptr ? 0
            : atomic_batch_exclusive ? 1 : llama_runtime_->sequence_capacity())
        + ";atomic_batch_exclusive=" + std::string(atomic_batch_exclusive ? "1" : "0")
        + ";" + capabilities_.llama_options.serialize()
#else
        + ";n_ctx=0;n_batch=0;n_ubatch=0;n_seq_max=0;equal_sequence_ubatch=0;max_atomic_sequences=0;atomic_batch_exclusive=0"
#endif
        ;
    std::vector<std::uint8_t> body{
        static_cast<std::uint8_t>(protocol::kProtocolRevision),
        static_cast<std::uint8_t>(protocol::kProtocolRevision >> 8U),
    };
    body.insert(body.end(), capabilities.begin(), capabilities.end());
    if (std::getenv("P4_STAGED_TRACE_HELLO") != nullptr) {
        std::cerr << "P4_STAGED_HELLO body_bytes=" << body.size()
                  << " capabilities=" << capabilities << '\n';
    }
    return protocol::Frame::make(protocol::Operation::Hello, std::move(body));
}

} // namespace staged::server
