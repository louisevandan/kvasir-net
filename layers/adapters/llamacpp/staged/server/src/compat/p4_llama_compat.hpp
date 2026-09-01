#pragma once

// The only place in the stage server that may reach past llama.cpp's public
// headers.
//
// `src/llama-ext.h` is upstream's own staging surface: it says in its first
// lines that breaking changes are allowed and asks not to be included across
// the codebase. Every pin that moves one of those signatures would otherwise
// break an arbitrary number of stage-server files at once, and the damage a
// pin does is exactly what the layering is supposed to bound. So the include
// lives in one translation unit, the types crossing back are P4's own, and
// `validate-private-headers.mjs` keeps the count at one.
//
// Adding a function here is the deliberate act of taking on a pin risk.
// Adding the include anywhere else is the accident this file exists to stop.

#include "llama.h"
#include "ggml-backend.h"

#include <cstddef>
#include <vector>

namespace p4_llama_compat {

/// One backend buffer type's share of what a context has allocated. Mirrors
/// the staging struct by value so no caller names the upstream type.
struct MemoryBreakdownEntry {
    ggml_backend_buffer_type_t buffer_type = nullptr;
    std::size_t model = 0;
    std::size_t context = 0;
    std::size_t compute = 0;
};

/// Allocation, per backend buffer type, of one context.
std::vector<MemoryBreakdownEntry> memory_breakdown(const llama_context * context);

/// How many backend devices this model is spread over.
std::size_t model_device_count(const llama_model * model);

/// The i-th backend device of this model, or nullptr when out of range.
ggml_backend_dev_t model_device(const llama_model * model, std::size_t index);

}  // namespace p4_llama_compat
