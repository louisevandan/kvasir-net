#include "llama_stage_runtime.hpp"
#include "compat/p4_llama_compat_internal.hpp"
#include "request_options.hpp"
#include "request_options_test_plan.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <cstdlib>
#include <iostream>
#include <string>

#include <nlohmann/json.hpp>

namespace {

const char * env_value(const char * name) {
    const auto * value = std::getenv(name);
    return value != nullptr && *value != '\0' ? value : nullptr;
}

staged::protocol::SequencePayload decode_request(
    staged::llama_runtime::StageRuntime & runtime,
    const std::string & id,
    const std::optional<std::string> & options,
    std::uint32_t position,
    std::string * error) {
    staged::protocol::SequencePayload request;
    request.sequence_id = id;
    request.n_tokens = 1;
    request.position = position;
    request.options = options.value_or(std::string{});
    staged::protocol::SequencePayload result;
    assert(runtime.execute_hop(request, staged::protocol::HopPhase::Decode,
                               &result, error));
    assert(result.outcome.has_value());
    return result;
}

staged::protocol::SequencePayload prefill(
    staged::llama_runtime::StageRuntime & runtime,
    const std::string & id,
    std::string * error,
    const std::string & prompt = "Reply with one short word: hello",
    const std::string & options = {}) {
    staged::protocol::SequencePayload request;
    request.sequence_id = id;
    request.prompt = prompt;
    request.options = options;
    request.position = 0;
    staged::protocol::SequencePayload result;
    if (!runtime.execute_hop(request, staged::protocol::HopPhase::Prefill,
                             &result, error)) {
        std::cerr << "PREFILL_ERROR " << *error << "\n";
        std::abort();
    }
    assert(result.n_tokens.has_value() && *result.n_tokens > 0);
    return result;
}

} // namespace

int main() {
    const auto * model_path = env_value("P4_STAGED_LLAMA_MODEL");
    if (model_path == nullptr) {
        std::cout << "SKIP: set P4_STAGED_LLAMA_MODEL for request options E2E\n";
        return 0;
    }

    staged::llama_runtime::LoadConfig config;
    config.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    config.model_path = model_path;
    config.model_identity = model_path;
    config.layer_begin = 0;
    config.layer_end = 28;
    p4_llama_compat::LlamaPlan plan;
    common_params & params = p4_llama_compat::plan_params(plan);
    params.n_ctx = 512;
    params.n_batch = 512;
    params.n_ubatch = 512;
    params.n_parallel = 3;
    params.n_sequences = 3;
    params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    params.sampling.temp = 0.0f;
    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    staged::test::RequestOptionsSnapshots snapshots;
    assert(staged::test::consume_request_options_plan(std::move(plan),
        [&](p4_llama_compat::LlamaPlan consumed) {
            return runtime.load(std::move(consumed), config, &error);
        }, &snapshots) && error.empty());

    auto & sampler_options = snapshots.sampler;
    // The test reads the fields the parser wrote, so it opens the handle.
    common_params_sampling & sampled = p4_llama_compat::sampling_of(sampler_options);
    const std::string extended_options = R"({
        "n_prev": 20,
        "n_probs": 3,
        "sampler_seq": "ktp",
        "min_p": 0.04,
        "min_keep": 2,
        "typical_p": 0.8,
        "top_n_sigma": 2.0,
        "dynatemp_range": 0.2,
        "dynatemp_exponent": 1.1,
        "adaptive_target": 0.4,
        "adaptive_decay": 0.8,
        "ignore_eos": true,
        "penalty_last_n": 12,
        "penalty_repeat": 1.15,
        "penalty_freq": 0.2,
        "penalty_present": 0.1,
        "dry_multiplier": 0.4,
        "dry_base": 1.75,
        "dry_allowed_length": 3,
        "dry_penalty_last_n": 24,
        "dry_sequence_breakers": ["\\n", ":", "|"],
        "xtc_probability": 0.25,
        "xtc_threshold": 0.15,
        "mirostat": 2,
        "mirostat_tau": 4.0,
        "mirostat_eta": 0.2
    })";
    assert(staged::llama_runtime::apply_request_options(
        extended_options, runtime.model(), &sampler_options, &error));
    assert(sampled.n_prev == 20);
    assert(sampled.n_probs == 3);
    assert(sampled.samplers.size() == 3);
    assert(sampled.min_p == 0.04f);
    assert(sampled.min_keep == 2);
    assert(sampled.typ_p == 0.8f);
    assert(sampled.top_n_sigma == 2.0f);
    assert(sampled.dynatemp_range == 0.2f);
    assert(sampled.dynatemp_exponent == 1.1f);
    assert(sampled.adaptive_target == 0.4f);
    assert(sampled.adaptive_decay == 0.8f);
    assert(sampled.ignore_eos);
    assert(sampled.penalty_last_n == 12);
    assert(sampled.penalty_repeat == 1.15f);
    assert(sampled.penalty_freq == 0.2f);
    assert(sampled.penalty_present == 0.1f);
    assert(sampled.dry_allowed_length == 3);
    assert(sampled.dry_penalty_last_n == 24);
    assert(sampled.dry_sequence_breakers.size() == 3);
    assert(sampled.xtc_probability == 0.25f);
    assert(sampled.mirostat == 2);
    assert(sampled.mirostat_tau == 4.0f);
    assert(sampled.mirostat_eta == 0.2f);
    assert(!staged::llama_runtime::apply_request_options(
        R"({"mirostat":3})", runtime.model(), &sampler_options, &error));
    std::cout << "REQUEST_OPTIONS_EXTENDED_PARSE_OK\n";

    const auto *vocab = llama_model_get_vocab(runtime.model());
    llama_token preserved_token = LLAMA_TOKEN_NULL;
    std::string preserved_piece;
    for (llama_token token = 0; token < llama_vocab_n_tokens(vocab); ++token) {
        const auto piece = p4_llama_compat::token_to_piece(vocab, token, true);
        if (piece.empty()) continue;
        if (p4_llama_compat::tokenize(vocab, piece, false, true).size() == 1) {
            preserved_token = token;
            preserved_piece = piece;
            break;
        }
    }
    assert(preserved_token != LLAMA_TOKEN_NULL);
    nlohmann::ordered_json grammar_options = nlohmann::ordered_json::object();
    grammar_options["grammar_lazy"] = true;
    grammar_options["generation_prompt"] = "assistant";
    grammar_options["preserved_tokens"] = nlohmann::ordered_json::array({preserved_piece});
    grammar_options["grammar_triggers"] = nlohmann::ordered_json::array({
        nlohmann::ordered_json{{"type", 2}, {"value", "^<tool>"}}
    });
    auto & grammar_sampling = snapshots.grammar;
    common_params_sampling & grammar_sampled = p4_llama_compat::sampling_of(grammar_sampling);
    assert(staged::llama_runtime::apply_request_options(
        grammar_options.dump(), runtime.model(), &grammar_sampling, &error));
    assert(grammar_sampled.grammar_lazy);
    assert(grammar_sampled.generation_prompt == "assistant");
    assert(grammar_sampled.preserved_tokens.count(preserved_token) == 1);
    assert(grammar_sampled.grammar_triggers.size() == 1);
    assert(grammar_sampled.grammar_triggers[0].type == COMMON_GRAMMAR_TRIGGER_TYPE_PATTERN);
    std::cout << "REQUEST_OPTIONS_GRAMMAR_TRIGGER_PARSE_OK"
              << " preserved_token=" << preserved_token << "\n";

    const auto baseline_prefill = prefill(runtime, "request-options-baseline", &error);
    const auto forced_prefill = prefill(runtime, "request-options-forced", &error);
    const auto baseline = decode_request(runtime, "request-options-baseline", std::nullopt,
                                         *baseline_prefill.n_tokens, &error);
    // This uses the same object form accepted by llama.cpp's server schema.
    const auto forced = decode_request(
        runtime, "request-options-forced",
        std::string(R"({"logit_bias":{"198":1000},"temperature":0,"top_k":1,"top_p":1,"seed":1})"),
        *forced_prefill.n_tokens, &error);
    assert(baseline.outcome->token != forced.outcome->token);
    std::cout << "REQUEST_OPTIONS_TOKEN_DIFF baseline=" << *baseline.outcome->token
              << " forced=" << *forced.outcome->token
              << " forced_text=" << forced.outcome->text << "\n";

    const std::string reasoning_options =
        R"({"reasoning_budget_tokens":0,"reasoning_budget_start_tag":" Hello","reasoning_budget_end_tags":["</think>"],"reasoning_budget_message":""})";
    const auto reasoning_prefill = prefill(
        runtime, "request-options-reasoning", &error,
        "Reply with one short word: hello", reasoning_options);
    const auto reasoning_start = decode_request(
        runtime, "request-options-reasoning", reasoning_options,
        *reasoning_prefill.n_tokens, &error);
    assert(*reasoning_start.outcome->token == *baseline.outcome->token);
    const auto reasoning = decode_request(
        runtime, "request-options-reasoning", reasoning_options,
        *reasoning_prefill.n_tokens + 1, &error);
    const auto expected_forced = p4_llama_compat::tokenize(
        llama_model_get_vocab(runtime.model()), "</think>", false, true);
    assert(!expected_forced.empty());
    std::cerr << "REQUEST_OPTIONS_REASONING_BUDGET0 forced="
              << *reasoning.outcome->token << " text=" << reasoning.outcome->text
              << " expected_first_end_tag=" << expected_forced.front() << "\n";
    assert(*reasoning.outcome->token == expected_forced.front());
    runtime.unload();
    return 0;
}
