#pragma once

#include <string>

// Only named as a pointer here, so the declaration is enough. Including
// llama.cpp's `common.h` would put its whole convenience surface - which
// moves freely between upstream versions - into every translation unit that
// wants to read a request's options (U0 3b).
struct common_params_sampling;
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
