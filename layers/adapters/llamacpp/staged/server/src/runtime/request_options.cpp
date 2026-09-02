#include "request_options.hpp"

// Still calls llama.cpp's sampler or speculative API directly.
#include "compat/p4_llama_compat_internal.hpp"
#include "compat/p4_llama_compat.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <limits>
#include <string>

#include <nlohmann/json.hpp>

#include "common.h"
#include "sampling.h"
#include "request_options_grammar.hpp"

namespace staged::llama_runtime {

namespace {

using json = nlohmann::ordered_json;

bool fail(std::string * error, const std::string & message) {
    if (error != nullptr) *error = message;
    return false;
}

bool finite_number(const json & value, float * result) {
    if (!value.is_number() || result == nullptr) return false;
    const auto number = value.get<double>();
    if (!std::isfinite(number) || number < -std::numeric_limits<float>::max() ||
        number > std::numeric_limits<float>::max()) return false;
    *result = static_cast<float>(number);
    return true;
}

bool integer_value(const json & value, int32_t * result) {
    if (result == nullptr || !value.is_number_integer()) return false;
    const auto number = value.get<int64_t>();
    if (number < std::numeric_limits<int32_t>::min() ||
        number > std::numeric_limits<int32_t>::max()) return false;
    *result = static_cast<int32_t>(number);
    return true;
}

bool non_negative_integer(const json & value, int32_t * result) {
    return integer_value(value, result) && *result >= 0;
}

bool probability(const json & value, float * result) {
    return finite_number(value, result) && *result >= 0.0f && *result <= 1.0f;
}

bool string_array(const json & value, std::vector<std::string> * result) {
    if (result == nullptr || !value.is_array()) return false;
    result->clear();
    for (const auto & item : value) {
        if (!item.is_string()) return false;
        result->push_back(item.get<std::string>());
    }
    return true;
}

bool sampler_names(const json & value, std::vector<enum common_sampler_type> * result) {
    if (result == nullptr || !value.is_array()) return false;
    std::vector<std::string> names;
    if (!string_array(value, &names)) return false;
    *result = common_sampler_types_from_names(names);
    return result->size() == names.size();
}

bool parse_bias(const json & value, float * result) {
    if (finite_number(value, result)) return true;
    if (value.is_boolean() && !value.get<bool>()) {
        *result = -INFINITY;
        return true;
    }
    return false;
}

bool append_token_bias(const json & token_value, float bias,
                       const llama_vocab * vocab,
                       std::vector<llama_logit_bias> * output) {
    if (token_value.is_number_integer()) {
        output->push_back({token_value.get<llama_token>(), bias});
        return true;
    }
    if (token_value.is_string()) {
        for (const auto token : p4_llama_compat::tokenize(
                 vocab, token_value.get<std::string>(), false)) {
            output->push_back({token, bias});
        }
        return true;
    }
    return false;
}

bool parse_logit_bias(const json & value, const llama_vocab * vocab,
                      std::vector<llama_logit_bias> * output,
                      std::string * error) {
    if (!value.is_array() && !value.is_object()) {
        return fail(error, "request option logit_bias must be an array or object");
    }
    const auto vocab_size = llama_vocab_n_tokens(vocab);
    auto append = [&](const json & token_value, const json & bias_value) {
        float bias = 0.0f;
        if (!parse_bias(bias_value, &bias)) return false;
        if (token_value.is_number_integer()) {
            const auto token = token_value.get<llama_token>();
            if (token < 0 || token >= vocab_size) return true;
        }
        return append_token_bias(token_value, bias, vocab, output);
    };

    if (value.is_array()) {
        for (const auto & entry : value) {
            if (!entry.is_array() || entry.size() != 2 || !append(entry[0], entry[1])) {
                return fail(error, "invalid request option logit_bias entry");
            }
        }
        return true;
    }
    for (const auto & [key, bias_value] : value.items()) {
        char * end = nullptr;
        const auto token_id = std::strtoll(key.c_str(), &end, 10);
        const json token_value = end != nullptr && *end == '\0'
            ? json(token_id) : json(key);
        if (!append(token_value, bias_value)) {
            return fail(error, "invalid request option logit_bias value");
        }
    }
    return true;
}

} // namespace

bool apply_request_options(const std::string & raw, const llama_model * model,
                           common_params_sampling * sampling,
                           std::string * error) {
    if (raw.empty()) return true;
    if (model == nullptr || sampling == nullptr) {
        return fail(error, "request sampler options require a loaded model");
    }
    json values;
    try {
        values = json::parse(raw);
    } catch (const std::exception & exception) {
        return fail(error, std::string("invalid request sampler options JSON: ") + exception.what());
    }
    if (!values.is_object()) return fail(error, "request sampler options must be a JSON object");

    const auto * vocab = llama_model_get_vocab(model);
    if (vocab == nullptr) return fail(error, "request sampler options require a vocabulary");
    if (values.contains("preserved_tokens") &&
        !parse_preserved_tokens(values.at("preserved_tokens"), vocab,
                                &sampling->preserved_tokens, error)) {
        return false;
    }
    if (values.contains("grammar_triggers") &&
        !parse_grammar_triggers(values.at("grammar_triggers"), vocab,
                                sampling->preserved_tokens,
                                &sampling->grammar_triggers, error)) {
        return false;
    }
    bool reasoning_fields_seen = false;
    bool reasoning_end_fields_seen = false;
    bool reasoning_message_seen = false;
    for (const auto & [key, value] : values.items()) {
        if (key == "stop") {
            // Parsed by request_stops.cpp. It is response framing, not a
            // llama.cpp sampler parameter.
        } else if (key == "request_tag") {
            if (!value.is_string()) return fail(error, "request_tag must be a string");
        } else if (key == "preserved_tokens" || key == "grammar_triggers") {
            // Parsed in a first pass so grammar word triggers can refer to
            // preserved tokens regardless of JSON member order.
        } else if (key == "grammar_lazy") {
            if (!value.is_boolean()) return fail(error, "grammar_lazy must be boolean");
            sampling->grammar_lazy = value.get<bool>();
        } else if (key == "generation_prompt") {
            if (!value.is_string()) return fail(error, "generation_prompt must be a string");
            sampling->generation_prompt = value.get<std::string>();
        } else if (key == "n_prev") {
            if (!non_negative_integer(value, &sampling->n_prev)) return fail(error, "n_prev must be a non-negative integer");
        } else if (key == "n_probs") {
            if (!non_negative_integer(value, &sampling->n_probs)) return fail(error, "n_probs must be a non-negative integer");
        } else if (key == "samplers") {
            if (!sampler_names(value, &sampling->samplers)) return fail(error, "samplers must be an array of known names");
        } else if (key == "sampler_seq" || key == "sampling_seq") {
            if (!value.is_string()) return fail(error, "sampler_seq must be a string");
            sampling->samplers = common_sampler_types_from_chars(value.get<std::string>());
        } else if (key == "top_n_sigma") {
            if (!finite_number(value, &sampling->top_n_sigma)) return fail(error, "top_n_sigma must be finite");
        } else if (key == "dynatemp_range") {
            if (!finite_number(value, &sampling->dynatemp_range) || sampling->dynatemp_range < 0.0f) return fail(error, "dynatemp_range must be finite and non-negative");
        } else if (key == "dynatemp_exponent") {
            if (!finite_number(value, &sampling->dynatemp_exponent) || sampling->dynatemp_exponent <= 0.0f) return fail(error, "dynatemp_exponent must be finite and positive");
        } else if (key == "adaptive_target") {
            if (!finite_number(value, &sampling->adaptive_target) || sampling->adaptive_target > 1.0f) return fail(error, "adaptive_target must be finite and at most 1");
        } else if (key == "adaptive_decay") {
            if (!probability(value, &sampling->adaptive_decay)) return fail(error, "adaptive_decay must be between 0 and 1");
        } else if (key == "ignore_eos") {
            if (!value.is_boolean()) return fail(error, "ignore_eos must be boolean");
            sampling->ignore_eos = value.get<bool>();
        } else if (key == "temperature" || key == "temp") {
            if (!finite_number(value, &sampling->temp) || sampling->temp < 0.0f) {
                return fail(error, "temperature must be a finite non-negative number");
            }
        } else if (key == "top_k") {
            if (!value.is_number_integer()) return fail(error, "top_k must be an integer");
            sampling->top_k = value.get<int32_t>();
        } else if (key == "top_p") {
            if (!finite_number(value, &sampling->top_p) || sampling->top_p < 0.0f || sampling->top_p > 1.0f) {
                return fail(error, "top_p must be a number between 0 and 1");
            }
        } else if (key == "min_p") {
            if (!probability(value, &sampling->min_p)) return fail(error, "min_p must be a number between 0 and 1");
        } else if (key == "min_keep") {
            if (!non_negative_integer(value, &sampling->min_keep)) {
                return fail(error, "min_keep must be a non-negative integer");
            }
        } else if (key == "typical_p" || key == "typ_p") {
            if (!probability(value, &sampling->typ_p)) {
                return fail(error, "typical_p must be a number between 0 and 1");
            }
        } else if (key == "penalty_last_n" || key == "repeat_last_n") {
            if (!non_negative_integer(value, &sampling->penalty_last_n)) {
                return fail(error, "penalty_last_n must be a non-negative integer");
            }
        } else if (key == "penalty_repeat" || key == "repeat_penalty") {
            if (!finite_number(value, &sampling->penalty_repeat) ||
                sampling->penalty_repeat <= 0.0f) {
                return fail(error, "penalty_repeat must be finite and greater than 0");
            }
        } else if (key == "penalty_freq" || key == "frequency_penalty") {
            if (!finite_number(value, &sampling->penalty_freq)) {
                return fail(error, "penalty_freq must be finite");
            }
        } else if (key == "penalty_present" || key == "presence_penalty") {
            if (!finite_number(value, &sampling->penalty_present)) {
                return fail(error, "penalty_present must be finite");
            }
        } else if (key == "dry_multiplier") {
            if (!finite_number(value, &sampling->dry_multiplier) || sampling->dry_multiplier < 0.0f) {
                return fail(error, "dry_multiplier must be finite and non-negative");
            }
        } else if (key == "dry_base") {
            if (!finite_number(value, &sampling->dry_base) || sampling->dry_base <= 0.0f) {
                return fail(error, "dry_base must be finite and greater than 0");
            }
        } else if (key == "dry_allowed_length") {
            if (!non_negative_integer(value, &sampling->dry_allowed_length)) {
                return fail(error, "dry_allowed_length must be a non-negative integer");
            }
        } else if (key == "dry_penalty_last_n") {
            if (!non_negative_integer(value, &sampling->dry_penalty_last_n)) {
                return fail(error, "dry_penalty_last_n must be a non-negative integer");
            }
        } else if (key == "dry_sequence_breakers") {
            if (!string_array(value, &sampling->dry_sequence_breakers)) {
                return fail(error, "dry_sequence_breakers must be an array of strings");
            }
        } else if (key == "xtc_probability") {
            if (!probability(value, &sampling->xtc_probability)) {
                return fail(error, "xtc_probability must be a number between 0 and 1");
            }
        } else if (key == "xtc_threshold") {
            if (!probability(value, &sampling->xtc_threshold)) {
                return fail(error, "xtc_threshold must be a number between 0 and 1");
            }
        } else if (key == "mirostat") {
            if (!integer_value(value, &sampling->mirostat) || sampling->mirostat < 0 || sampling->mirostat > 2) {
                return fail(error, "mirostat must be 0, 1, or 2");
            }
        } else if (key == "mirostat_tau") {
            if (!finite_number(value, &sampling->mirostat_tau) || sampling->mirostat_tau < 0.0f) {
                return fail(error, "mirostat_tau must be finite and non-negative");
            }
        } else if (key == "mirostat_eta") {
            if (!finite_number(value, &sampling->mirostat_eta) || sampling->mirostat_eta < 0.0f) {
                return fail(error, "mirostat_eta must be finite and non-negative");
            }
        } else if (key == "seed") {
            if (!value.is_number_unsigned() && !value.is_number_integer()) {
                return fail(error, "seed must be an integer");
            }
            const auto seed = value.get<int64_t>();
            if (seed < 0) return fail(error, "seed must be non-negative");
            sampling->seed = static_cast<uint32_t>(seed);
        } else if (key == "grammar") {
            if (!value.is_string() || value.get<std::string>().empty()) {
                return fail(error, "grammar must be a non-empty string");
            }
            sampling->grammar = {COMMON_GRAMMAR_TYPE_USER, value.get<std::string>()};
        } else if (key == "logit_bias") {
            sampling->logit_bias.clear();
            if (!parse_logit_bias(value, vocab, &sampling->logit_bias, error)) return false;
        } else if (key == "reasoning_budget_tokens") {
            if (!value.is_number_integer()) return fail(error, "reasoning_budget_tokens must be an integer");
            const auto budget = value.get<int64_t>();
            if (budget < -1 || budget > std::numeric_limits<int32_t>::max()) {
                return fail(error, "reasoning_budget_tokens is out of range");
            }
            sampling->reasoning_budget_tokens = static_cast<int32_t>(budget);
            reasoning_fields_seen = true;
        } else if (key == "reasoning_budget_start_tag") {
            if (!value.is_string() || value.get<std::string>().empty()) {
                return fail(error, "reasoning_budget_start_tag must be a non-empty string");
            }
            sampling->reasoning_budget_start = p4_llama_compat::tokenize(
                vocab, value.get<std::string>(), false, true);
            reasoning_fields_seen = true;
        } else if (key == "reasoning_budget_end_tags" || key == "reasoning_budget_end_tag") {
            sampling->reasoning_budget_end.clear();
            if (key == "reasoning_budget_end_tag") {
                if (!value.is_string() || value.get<std::string>().empty()) {
                    return fail(error, "reasoning_budget_end_tag must be a non-empty string");
                }
                sampling->reasoning_budget_end.push_back(p4_llama_compat::tokenize(
                    vocab, value.get<std::string>(), false, true));
            } else {
                if (!value.is_array()) return fail(error, "reasoning_budget_end_tags must be an array");
                for (const auto & tag : value) {
                    if (!tag.is_string() || tag.get<std::string>().empty()) {
                        return fail(error, "reasoning_budget_end_tags must contain non-empty strings");
                    }
                    sampling->reasoning_budget_end.push_back(p4_llama_compat::tokenize(
                        vocab, tag.get<std::string>(), false, true));
                }
            }
            if (sampling->reasoning_budget_end.empty()) {
                return fail(error, "reasoning budget requires at least one end tag");
            }
            reasoning_fields_seen = true;
            reasoning_end_fields_seen = true;
        } else if (key == "reasoning_budget_message") {
            if (!value.is_string()) return fail(error, "reasoning_budget_message must be a string");
            sampling->reasoning_budget_message = value.get<std::string>();
            reasoning_fields_seen = true;
            reasoning_message_seen = true;
        } else {
            return fail(error, "unsupported request sampler option: " + key);
        }
    }
    if (reasoning_fields_seen && (reasoning_end_fields_seen || reasoning_message_seen)) {
        if (sampling->reasoning_budget_end.empty()) {
            return fail(error, "reasoning budget message/end tag requires end tags");
        }
        sampling->reasoning_budget_forced = sampling->reasoning_budget_end.front();
        if (!sampling->reasoning_budget_message.empty()) {
            const auto message_tokens = p4_llama_compat::tokenize(
                vocab, sampling->reasoning_budget_message, false, true);
            sampling->reasoning_budget_forced.insert(
                sampling->reasoning_budget_forced.begin(),
                message_tokens.begin(), message_tokens.end());
        }
    }
    if (sampling->ignore_eos) {
        // common_init_from_params() prepares this bias for the startup
        // sampler, but request-level samplers are created afresh in the tail.
        // Carry the same EOG suppression into that per-sequence sampler so
        // ignore_eos actually means "continue up to max_tokens" here.
        for (llama_token token = 0; token < llama_vocab_n_tokens(vocab); ++token) {
            if (!llama_vocab_is_eog(vocab, token)) continue;
            const auto exists = std::any_of(
                sampling->logit_bias.begin(), sampling->logit_bias.end(),
                [token](const llama_logit_bias & bias) { return bias.token == token; });
            if (!exists) sampling->logit_bias.push_back({token, -std::numeric_limits<float>::infinity()});
        }
    }
    return true;
}

} // namespace staged::llama_runtime
