#include "p4_llama_compat.hpp"

#include <algorithm>
#include <vector>
#include "p4_llama_compat_internal.hpp"

// The one permitted crossing. See the header for why it is only here.
#include "llama-ext.h"

// The convenience library, on this side of the wall only.
#include "common.h"
#include "speculative.h"

namespace p4_llama_compat {

std::vector<MemoryBreakdownEntry> memory_breakdown(const llama_context * context) {
    std::vector<MemoryBreakdownEntry> entries;
    if (context == nullptr) return entries;
    const llama_memory_breakdown breakdown = llama_get_memory_breakdown(context);
    entries.reserve(breakdown.size());
    for (const auto & [buffer_type, memory] : breakdown) {
        entries.push_back(MemoryBreakdownEntry{buffer_type, memory.model, memory.context, memory.compute});
    }
    return entries;
}

std::size_t model_device_count(const llama_model * model) {
    if (model == nullptr) return 0;
    const int32_t count = llama_model_n_devices(model);
    return count > 0 ? static_cast<std::size_t>(count) : 0;
}

ggml_backend_dev_t model_device(const llama_model * model, std::size_t index) {
    if (index >= model_device_count(model)) return nullptr;
    return llama_model_get_device(model, static_cast<int>(index));
}

// The plan, held whole. Everything below is the only place that knows what
// a `common_params` looks like.
struct LlamaPlan::Impl final {
    common_params params;
};

LlamaPlan::LlamaPlan() : impl_(std::make_unique<Impl>()) {}
LlamaPlan::~LlamaPlan() = default;
LlamaPlan::LlamaPlan(LlamaPlan &&) noexcept = default;
LlamaPlan & LlamaPlan::operator=(LlamaPlan &&) noexcept = default;

const std::string & LlamaPlan::model_path() const noexcept { return impl_->params.model.path; }
bool LlamaPlan::kv_unified() const noexcept { return impl_->params.kv_unified; }
ggml_type LlamaPlan::cache_type_k() const noexcept { return impl_->params.cache_type_k; }
ggml_type LlamaPlan::cache_type_v() const noexcept { return impl_->params.cache_type_v; }

LlamaPlan LlamaPlan::clone() const {
    LlamaPlan copy;
    copy.impl_->params = impl_->params;
    return copy;
}

int LlamaPlan::n_parallel() const noexcept { return impl_->params.n_parallel; }

void LlamaPlan::set_model_path(const std::string & path) { impl_->params.model.path = path; }

llama_model_params LlamaPlan::to_model_params() const {
    return common_model_params_to_llama(impl_->params);
}

llama_context_params LlamaPlan::to_context_params() const {
    return common_context_params_to_llama(impl_->params);
}

LlamaPlan LlamaPlan::speculative_plan() const {
    LlamaPlan draft;
    draft.impl_->params = common_base_params_to_speculative(impl_->params);
    return draft;
}

bool LlamaPlan::requests_draft_mtp() const noexcept {
    const auto & types = impl_->params.speculative.types;
    return std::find(types.begin(), types.end(), COMMON_SPECULATIVE_TYPE_DRAFT_MTP)
        != types.end();
}

bool LlamaPlan::has_speculative_model() const noexcept {
    // Upstream owns the answer to "is there a draft model"; asking the field
    // directly is how a rename becomes our problem.
    return impl_->params.speculative.has_dft();
}

/// Hands the underlying plan to the rest of the runtime, which still needs it
/// for the sampler and speculative APIs. Declared in the internal header so
/// the public one stays free of the type; every use is a debt entry.
common_params & plan_params(LlamaPlan & plan) { return plan.impl().params; }
const common_params & plan_params(const LlamaPlan & plan) { return plan.impl().params; }

std::vector<llama_token> tokenize(const llama_vocab * vocab, const std::string & text,
                                  bool add_special, bool parse_special) {
    return common_tokenize(vocab, text, add_special, parse_special);
}

std::string detokenize(const llama_vocab * vocab, const std::vector<llama_token> & tokens,
                       bool special) {
    return common_detokenize(vocab, tokens, special);
}

std::string token_to_piece(const llama_vocab * vocab, llama_token token, bool special) {
    return common_token_to_piece(vocab, token, special);
}

void batch_add(llama_batch & batch, llama_token token, llama_pos pos,
               const std::vector<llama_seq_id> & seq_ids, bool logits) {
    common_batch_add(batch, token, pos, seq_ids, logits);
}

}  // namespace p4_llama_compat
