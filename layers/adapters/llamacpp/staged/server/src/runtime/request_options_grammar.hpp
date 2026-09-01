#pragma once

#include <set>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

// llama.cpp's public header, which `common.h` used to drag in behind it -
// the vocabulary and token types below come from here, not from the
// convenience library.
#include "llama.h"

// Named only inside a vector this header declares but never sizes, so the
// declaration is enough; see request_options.hpp (U0 3b).
struct common_grammar_trigger;

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
