#pragma once

// The plan's contents, for code that still needs them.
//
// Separate from `p4_llama_compat.hpp` because including this one means taking
// on llama.cpp's convenience library, and every file that does is an entry on
// the U0 (3b) debt list. The public header stays free of it so that including
// a P4 type does not drag the library along.

#include "compat/p4_llama_compat.hpp"

#include "common.h"
#include "sampling.h"
#include "speculative.h"

namespace p4_llama_compat {

common_params & plan_params(LlamaPlan & plan);
const common_params & plan_params(const LlamaPlan & plan);

/// The pointers the handles own, for the files that still call llama.cpp's
/// sampler and speculative APIs directly.
common_sampler * raw(Sampler & sampler);
void adopt(Sampler & sampler, common_sampler_ptr owned);

common_speculative * raw(Speculative & speculative);
void adopt(Speculative & speculative, common_speculative_ptr owned);

common_speculative_init_result * raw(SpeculativeInit & init);
void adopt(SpeculativeInit & init, common_speculative_init_result_ptr owned);

/// Wraps an owned sampler so it can be stored where the header may not name
/// llama.cpp's type.
Sampler make_sampler(common_sampler_ptr owned);

/// The checkpoint itself, for the state-store paths that serialise it.
common_prompt_checkpoint & checkpoint_of(PromptCheckpoint & checkpoint);
const common_prompt_checkpoint & checkpoint_of(const PromptCheckpoint & checkpoint);

}  // namespace p4_llama_compat
