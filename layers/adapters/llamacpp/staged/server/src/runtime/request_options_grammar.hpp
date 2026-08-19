#pragma once

#include <set>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "common.h"

namespace staged::llama_runtime {

bool parse_preserved_tokens(const nlohmann::ordered_json & value,
                            const llama_vocab * vocab,
                            std::set<llama_token> * output,
                            std::string * error);

bool parse_grammar_triggers(const nlohmann::ordered_json & value,
                            const llama_vocab * vocab,
                            const std::set<llama_token> & preserved,
                            std::vector<common_grammar_trigger> * output,
                            std::string * error);

} // namespace staged::llama_runtime
