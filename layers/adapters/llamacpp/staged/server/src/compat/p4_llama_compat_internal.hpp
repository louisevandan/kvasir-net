#pragma once

// The plan's contents, for code that still needs them.
//
// Separate from `p4_llama_compat.hpp` because including this one means taking
// on llama.cpp's convenience library, and every file that does is an entry on
// the U0 (3b) debt list. The public header stays free of it so that including
// a P4 type does not drag the library along.

#include "compat/p4_llama_compat.hpp"

#include "common.h"

namespace p4_llama_compat {

common_params & plan_params(LlamaPlan & plan);
const common_params & plan_params(const LlamaPlan & plan);

}  // namespace p4_llama_compat
