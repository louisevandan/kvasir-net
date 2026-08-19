#pragma once

#include <string>

#include "common.h"
#include "llama.h"

namespace staged::llama_runtime {

// Parses the deliberately small, request-level sampler surface. The base
// values come from the startup plan; a request may override only the fields
// listed in this file.
bool apply_request_options(const std::string & raw,
                           const llama_model * model,
                           common_params_sampling * sampling,
                           std::string * error);

} // namespace staged::llama_runtime
