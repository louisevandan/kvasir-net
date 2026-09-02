#include "p4_llama_compat.hpp"

#include <algorithm>
#include <utility>
#include <vector>
#include "p4_llama_compat_internal.hpp"

// The one permitted crossing. See the header for why it is only here.
#include "llama-ext.h"

// The convenience library, on this side of the wall only.
#include "common.h"
#include "sampling.h"
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

struct SamplingOptions::Impl final {
    common_params_sampling options;
};

SamplingOptions::SamplingOptions() : impl_(std::make_unique<Impl>()) {}
SamplingOptions::~SamplingOptions() = default;
SamplingOptions::SamplingOptions(SamplingOptions &&) noexcept = default;
SamplingOptions & SamplingOptions::operator=(SamplingOptions &&) noexcept = default;
bool SamplingOptions::ignores_end_of_generation() const noexcept { return impl_->options.ignore_eos; }
std::size_t SamplingOptions::logit_bias_count() const noexcept { return impl_->options.logit_bias.size(); }

SamplingOptions SamplingOptions::clone() const {
    SamplingOptions copy;
    copy.impl_->options = impl_->options;
    return copy;
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

SamplingOptions LlamaPlan::sampling_options() const {
    SamplingOptions options;
    options.impl().options = impl_->params.sampling;
    return options;
}

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

bool LlamaPlan::requests_unsupported_speculative() const noexcept {
    const auto & speculative = impl_->params.speculative;
    if (speculative.has_dft()) return true;
    return std::any_of(
        speculative.types.begin(), speculative.types.end(),
        [](const common_speculative_type type) {
            // Draft-MTP is the one this stage server implements.
            return type != COMMON_SPECULATIVE_TYPE_NONE
                && type != COMMON_SPECULATIVE_TYPE_DRAFT_MTP;
        });
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

// Held in agreement by the compiler rather than by anyone remembering to
// check. If upstream renumbers or inserts a case, this stops building here
// instead of silently changing what a stage believes it may drop.
static_assert(static_cast<int>(SeqRemoval::Unsupported) == COMMON_CONTEXT_SEQ_RM_TYPE_NO);
static_assert(static_cast<int>(SeqRemoval::Partial) == COMMON_CONTEXT_SEQ_RM_TYPE_PART);
static_assert(static_cast<int>(SeqRemoval::FullOnly) == COMMON_CONTEXT_SEQ_RM_TYPE_FULL);
static_assert(static_cast<int>(SeqRemoval::RecurrentBounded) == COMMON_CONTEXT_SEQ_RM_TYPE_RS);

SeqRemoval sequence_removal(llama_context * context) {
    return static_cast<SeqRemoval>(common_context_can_seq_rm(context));
}

struct PromptCheckpoint::Impl final {
    common_prompt_checkpoint checkpoint;
};

PromptCheckpoint::PromptCheckpoint() : impl_(std::make_unique<Impl>()) {}
PromptCheckpoint::~PromptCheckpoint() = default;
PromptCheckpoint::PromptCheckpoint(PromptCheckpoint &&) noexcept = default;
PromptCheckpoint & PromptCheckpoint::operator=(PromptCheckpoint &&) noexcept = default;

std::int64_t PromptCheckpoint::n_tokens() const noexcept { return impl_->checkpoint.n_tokens; }
llama_pos PromptCheckpoint::pos_max() const noexcept { return impl_->checkpoint.pos_max; }
std::size_t PromptCheckpoint::size() const noexcept { return impl_->checkpoint.size(); }
bool PromptCheckpoint::empty() const noexcept { return impl_->checkpoint.empty(); }
void PromptCheckpoint::clear() { impl_->checkpoint.clear(); }

const std::vector<std::uint8_t> & PromptCheckpoint::target_state() const noexcept {
    return impl_->checkpoint.data_tgt;
}
std::vector<std::uint8_t> & PromptCheckpoint::target_state() noexcept {
    return impl_->checkpoint.data_tgt;
}
const std::vector<std::uint8_t> & PromptCheckpoint::draft_state() const noexcept {
    return impl_->checkpoint.data_dft;
}
std::vector<std::uint8_t> & PromptCheckpoint::draft_state() noexcept {
    return impl_->checkpoint.data_dft;
}

void PromptCheckpoint::update_positions(std::int64_t n_tokens, llama_pos pos_min, llama_pos pos_max) {
    impl_->checkpoint.update_pos(n_tokens, pos_min, pos_max);
}
void PromptCheckpoint::save_target(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags) {
    impl_->checkpoint.update_tgt(context, seq_id, flags);
}
void PromptCheckpoint::save_draft(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags) {
    impl_->checkpoint.update_dft(context, seq_id, flags);
}
void PromptCheckpoint::load_target(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags) {
    impl_->checkpoint.load_tgt(context, seq_id, flags);
}
void PromptCheckpoint::load_draft(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags) {
    impl_->checkpoint.load_dft(context, seq_id, flags);
}

struct Sampler::Impl final {
    common_sampler_ptr sampler;
};

Sampler::Sampler() : impl_(std::make_unique<Impl>()) {}
Sampler::~Sampler() = default;
Sampler::Sampler(Sampler &&) noexcept = default;
Sampler & Sampler::operator=(Sampler &&) noexcept = default;
bool Sampler::valid() const noexcept { return impl_->sampler != nullptr; }
void Sampler::reset() { impl_->sampler.reset(); }

Sampler Sampler::create(const llama_model * model, SamplingOptions & options) {
    Sampler sampler;
    sampler.impl_->sampler.reset(common_sampler_init(model, options.impl().options));
    return sampler;
}

Sampler Sampler::clone() const {
    Sampler copy;
    if (impl_->sampler != nullptr) {
        copy.impl_->sampler.reset(common_sampler_clone(impl_->sampler.get()));
    }
    return copy;
}

void Sampler::accept(llama_token token, bool accept_grammar) {
    common_sampler_accept(impl_->sampler.get(), token, accept_grammar);
}

llama_token Sampler::sample(llama_context * context, int index) {
    return common_sampler_sample(impl_->sampler.get(), context, index);
}

std::vector<llama_token> Sampler::sample_and_accept_n(
        llama_context * context, const std::vector<llama_token> & draft) {
    return common_sampler_sample_and_accept_n(impl_->sampler.get(), context, draft);
}

std::vector<llama_token> Sampler::sample_and_accept_n(
        llama_context * context, const std::vector<int> & indices,
        const std::vector<llama_token> & draft) {
    return common_sampler_sample_and_accept_n(impl_->sampler.get(), context, indices, draft);
}

struct Speculative::Impl final {
    common_speculative_ptr speculative;
};

Speculative::Speculative() : impl_(std::make_unique<Impl>()) {}
Speculative::~Speculative() = default;
Speculative::Speculative(Speculative &&) noexcept = default;
Speculative & Speculative::operator=(Speculative &&) noexcept = default;
bool Speculative::valid() const noexcept { return impl_->speculative != nullptr; }
void Speculative::reset() { impl_->speculative.reset(); }

void Speculative::begin(llama_seq_id seq_id, const std::vector<llama_token> & prompt) {
    common_speculative_begin(impl_->speculative.get(), seq_id, prompt);
}

bool Speculative::process(const llama_batch & batch) {
    return common_speculative_process(impl_->speculative.get(), batch);
}

void Speculative::configure_draft(llama_seq_id seq_id, const DraftRequest & request) {
    auto & params = common_speculative_get_draft_params(impl_->speculative.get(), seq_id);
    params.drafting = request.drafting;
    params.n_max = request.max_tokens;
    params.n_past = request.n_past;
    params.id_last = request.last_token;
    params.prompt = request.prompt;
    params.result = request.result;
}

void Speculative::run_draft() {
    common_speculative_draft(impl_->speculative.get());
}

void Speculative::draft(llama_seq_id seq_id, const DraftRequest & request) {
    configure_draft(seq_id, request);
    run_draft();
}

void Speculative::accept(llama_seq_id seq_id, std::uint16_t accepted) {
    common_speculative_accept(impl_->speculative.get(), seq_id, accepted);
}

void Speculative::end(llama_seq_id seq_id) {
    common_speculative_end(impl_->speculative.get(), seq_id);
}

struct SpeculativeInit::Impl final {
    common_speculative_init_result_ptr init;
};

SpeculativeInit::SpeculativeInit() : impl_(std::make_unique<Impl>()) {}
SpeculativeInit::~SpeculativeInit() = default;
SpeculativeInit::SpeculativeInit(SpeculativeInit &&) noexcept = default;
SpeculativeInit & SpeculativeInit::operator=(SpeculativeInit &&) noexcept = default;
bool SpeculativeInit::valid() const noexcept { return impl_->init != nullptr; }
void SpeculativeInit::reset() { impl_->init.reset(); }

llama_context * SpeculativeInit::context() const noexcept {
    return impl_->init == nullptr ? nullptr : impl_->init->context();
}

common_sampler * raw(Sampler & sampler) { return sampler.impl().sampler.get(); }
void adopt(Sampler & sampler, common_sampler_ptr owned) {
    sampler.impl().sampler = std::move(owned);
}

common_speculative * raw(Speculative & speculative) { return speculative.impl().speculative.get(); }
void adopt(Speculative & speculative, common_speculative_ptr owned) {
    speculative.impl().speculative = std::move(owned);
}

common_speculative_init_result * raw(SpeculativeInit & init) { return init.impl().init.get(); }
void adopt(SpeculativeInit & init, common_speculative_init_result_ptr owned) {
    init.impl().init = std::move(owned);
}

common_params_sampling & sampling_of(SamplingOptions & options) {
    return options.impl().options;
}
const common_params_sampling & sampling_of(const SamplingOptions & options) {
    return options.impl().options;
}

Sampler make_sampler(common_sampler_ptr owned) {
    Sampler sampler;
    adopt(sampler, std::move(owned));
    return sampler;
}

common_prompt_checkpoint & checkpoint_of(PromptCheckpoint & checkpoint) {
    return checkpoint.impl().checkpoint;
}
const common_prompt_checkpoint & checkpoint_of(const PromptCheckpoint & checkpoint) {
    return checkpoint.impl().checkpoint;
}

namespace {

/// Percent escapes anything that would be read as capability-string
/// punctuation, so a backend name cannot end a field or start another.
std::string escape_capability_value(const char * text) {
    if (text == nullptr) return "?";
    static const char * const digits = "0123456789ABCDEF";
    std::string escaped;
    for (const unsigned char ch : std::string(text)) {
        const bool punctuation = ch == ';' || ch == '=' || ch == '|'
            || ch == ',' || ch == '[' || ch == ']' || ch == '%';
        if (punctuation || ch < 0x20) {
            escaped += '%';
            escaped += digits[ch >> 4];
            escaped += digits[ch & 0x0F];
        } else {
            escaped += static_cast<char>(ch);
        }
    }
    return escaped;
}

}  // namespace

std::string backend_inventory() {
    std::vector<std::string> entries;
    const auto registries = ggml_backend_reg_count();
    entries.reserve(registries);
    for (std::size_t index = 0; index < registries; ++index) {
        auto * registry = ggml_backend_reg_get(index);
        if (registry == nullptr) continue;
        std::string entry = escape_capability_value(ggml_backend_reg_name(registry));
        entry += '[';
        const auto devices = ggml_backend_reg_dev_count(registry);
        for (std::size_t device_index = 0; device_index < devices; ++device_index) {
            if (device_index > 0) entry += ',';
            auto * device = ggml_backend_reg_dev_get(registry, device_index);
            entry += escape_capability_value(device == nullptr ? nullptr : ggml_backend_dev_name(device));
        }
        entry += ']';
        entries.push_back(std::move(entry));
    }
    // Sorted, so a different plugin load order is not a different inventory.
    std::sort(entries.begin(), entries.end());
    std::string inventory;
    for (const auto & entry : entries) {
        if (!inventory.empty()) inventory += '|';
        inventory += entry;
    }
    return inventory;
}

SpeculativeSetup bring_up_speculative(
        LlamaPlan & plan, llama_model * model, llama_context * context,
        std::uint32_t sequences) {
    SpeculativeSetup setup;
    auto & params = plan.impl().params;
    auto draft_params = common_base_params_to_speculative(params);
    adopt(setup.init, common_speculative_init_from_params(draft_params, model, context));
    if (!setup.init.valid() || setup.init.context() == nullptr) {
        setup.failure = "llama.cpp failed to create the staged MTP context";
        return setup;
    }
    // The driver reads both contexts out of the plan, so they are written
    // back before it is created.
    params.speculative.draft.ctx_tgt = context;
    params.speculative.draft.ctx_dft = setup.init.context();
    adopt(setup.driver,
          common_speculative_ptr(common_speculative_init(params.speculative, sequences)));
    if (!setup.driver.valid()) {
        setup.failure = "llama.cpp failed to initialize the staged MTP driver";
    }
    return setup;
}

std::size_t find_partial_stop(std::string_view text, std::string_view stop) {
    return string_find_partial_stop(text, stop);
}

}  // namespace p4_llama_compat
