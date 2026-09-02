#pragma once

#include <string>

// The options a request adjusts, as a P4 handle: llama.cpp owns the field
// set and grows it, so it is carried whole rather than named here.
#include "compat/p4_llama_compat.hpp"
#include "llama.h"

namespace staged::llama_runtime {

// Parses the deliberately small, request-level sampler surface. The base
// values come from the startup plan; a request may override only the fields
// listed in this file.
bool apply_request_options(const std::string & raw,
                           const llama_model * model,
                           p4_llama_compat::SamplingOptions * sampling,
                           std::string * error);

} // namespace staged::llama_runtime
