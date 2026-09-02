#include "request_options_grammar.hpp"

// Still calls llama.cpp's sampler or speculative API directly.
#include "compat/p4_llama_compat_internal.hpp"
#include "compat/p4_llama_compat.hpp"

// The convenience library stays in the translation unit that needs it. Its
// header no longer forces it on everything downstream (U0 3b).
#include "common.h"

#include <cstdint>

namespace staged::llama_runtime {

namespace {

using json = nlohmann::ordered_json;

bool fail(std::string * error, const std::string & message) {
    if (error != nullptr) *error = message;
    return false;
}

bool integer_value(const json & value, int32_t * result) {
    if (result == nullptr || !value.is_number_integer()) return false;
    const auto number = value.get<int64_t>();
    if (number < INT32_MIN || number > INT32_MAX) return false;
    *result = static_cast<int32_t>(number);
    return true;
}

} // namespace

bool parse_preserved_tokens(const json & value, const llama_vocab * vocab,
                            std::set<llama_token> * output, std::string * error) {
    if (output == nullptr || !value.is_array()) {
        return fail(error, "preserved_tokens must be an array of strings");
    }
    for (const auto & item : value) {
        if (!item.is_string()) {
            return fail(error, "preserved_tokens must contain strings");
        }
        const auto ids = p4_llama_compat::tokenize(vocab, item.get<std::string>(), false, true);
        if (ids.size() != 1) {
            return fail(error, "each preserved_tokens entry must encode to one token");
        }
        output->insert(ids.front());
    }
    return true;
}

bool parse_grammar_triggers(const json & value, const llama_vocab * vocab,
                            const std::set<llama_token> & preserved,
                            std::vector<common_grammar_trigger> * output,
                            std::string * error) {
    if (output == nullptr || !value.is_array()) {
        return fail(error, "grammar_triggers must be an array");
    }
    for (const auto & item : value) {
        common_grammar_trigger trigger;
        if (item.is_string()) {
            trigger.type = COMMON_GRAMMAR_TRIGGER_TYPE_WORD;
            trigger.value = item.get<std::string>();
        } else if (item.is_object() && item.contains("type") && item.contains("value")) {
            int32_t type = 0;
            if (!integer_value(item.at("type"), &type) || type < 0 || type > 3 ||
                !item.at("value").is_string()) {
                return fail(error, "invalid grammar_triggers entry");
            }
            trigger.type = static_cast<common_grammar_trigger_type>(type);
            trigger.value = item.at("value").get<std::string>();
            if (trigger.type == COMMON_GRAMMAR_TRIGGER_TYPE_TOKEN) {
                if (!item.contains("token") || !integer_value(item.at("token"), &type) ||
                    type < 0 || type >= llama_vocab_n_tokens(vocab)) {
                    return fail(error, "grammar token trigger has an invalid token");
                }
                trigger.token = static_cast<llama_token>(type);
            }
        } else {
            return fail(error, "invalid grammar_triggers entry");
        }
        if (trigger.value.empty()) return fail(error, "grammar trigger value is empty");
        if (trigger.type == COMMON_GRAMMAR_TRIGGER_TYPE_WORD) {
            const auto ids = p4_llama_compat::tokenize(vocab, trigger.value, false, true);
            if (ids.size() == 1) {
                if (preserved.find(ids.front()) == preserved.end()) {
                    return fail(error, "grammar word trigger must be preserved_tokens");
                }
                trigger.type = COMMON_GRAMMAR_TRIGGER_TYPE_TOKEN;
                trigger.token = ids.front();
            }
        }
        output->push_back(std::move(trigger));
    }
    return true;
}

} // namespace staged::llama_runtime
