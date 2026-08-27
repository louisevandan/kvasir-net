#include "llama_stage_runtime.hpp"
#include "ggml-backend.h"

#include <cstdio>
#include <cstdlib>

#include <algorithm>
#include <iostream>
#include <utility>

namespace staged::llama_runtime {

StageRuntime::~StageRuntime() { unload(); }

bool StageRuntime::tokenize_prompt(
        const std::string & prompt,
        std::vector<std::int32_t> * tokens,
        std::string * error) const {
    if (!loaded() || config_.layer_begin != 0 || tokens == nullptr || prompt.empty()) {
        if (error != nullptr) *error = "invalid first-stage tokenize request";
        return false;
    }
    const auto values = common_tokenize(
        llama_model_get_vocab(model_), prompt, true, true);
    if (values.empty()) {
        if (error != nullptr) *error = "llama.cpp produced an empty prompt";
        return false;
    }
    tokens->assign(values.begin(), values.end());
    return true;
}

bool StageRuntime::fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    unload();
    return false;
}

bool StageRuntime::load(common_params params, const LoadConfig & config,
                        std::string * error) {
    unload();
    if (config.model_path.empty() || config.layer_begin < 0 ||
        config.layer_end <= config.layer_begin) {
        return fail("invalid stage load configuration", error);
    }
    const bool mtp_requested = std::find(
        params.speculative.types.begin(), params.speculative.types.end(),
        COMMON_SPECULATIVE_TYPE_DRAFT_MTP) != params.speculative.types.end();
    const bool unsupported_speculative = params.speculative.has_dft() || std::any_of(
            params.speculative.types.begin(), params.speculative.types.end(),
            [](const common_speculative_type type) {
                return type != COMMON_SPECULATIVE_TYPE_NONE
                    && type != COMMON_SPECULATIVE_TYPE_DRAFT_MTP;
            });
    if (unsupported_speculative) {
        if (params.speculative.has_dft()) {
            return fail("CAPABILITY_UNAVAILABLE: "
                        "draft_context_and_proposal_state_not_in_hop", error);
        }
        return fail("CAPABILITY_UNAVAILABLE: "
                    "proposal_accept_rollback_state_not_in_hop", error);
    }
    params_ = std::move(params);
    config_ = config;
    sequence_ids_.clear();
    sequence_positions_.clear();
    next_sequence_id_ = 0;
    params_.model.path = config_.model_path;

    llama_backend_init();
    // Backend discovery is deliberately delegated to stock ggml. The stage
    // runtime has no CUDA/Vulkan/HIP/Metal/OpenCL branches of its own.
    ggml_backend_load_all();
    backend_initialized_ = true;
    StageMemoryPlan planned_memory;
    std::string memory_error;
    if (!inspect_stage_memory_with_initialized_backend(
            params_, config_, &planned_memory, &memory_error)) {
        if (error != nullptr) *error = memory_error;
        unload();
        return false;
    }
    std::cerr << "MEMORY_PLAN " << serialize_stage_memory_plan(planned_memory) << '\n';
    if (!planned_memory.fits_current_free) {
        if (error != nullptr) *error = "staged memory plan exceeds currently free memory";
        unload();
        return false;
    }
    auto model_params = make_stage_model_params(params_, config_);
    model_params.linkcpp_stage_executor = &StageRuntime::stage_executor;
    model_params.linkcpp_stage_executor_user_data = this;
    model_params.linkcpp_state_executor = &StageRuntime::state_executor;
    model_params.linkcpp_state_executor_user_data = this;
    model_ = llama_model_load_from_file(params_.model.path.c_str(), model_params);
    if (model_ == nullptr) {
        return fail("llama.cpp failed to load the staged model", error);
    }

    auto context_params = make_stage_context_params(params_);
    ctx_ = llama_init_from_model(model_, context_params);
    if (ctx_ == nullptr) {
        return fail("llama.cpp failed to create the staged context", error);
    }
    tail_stage_ = config_.layer_end >= llama_model_n_layer(model_);
    const bool bounded_remove = llama_n_rs_seq(ctx_) > 0;
    const bool full_remove = llama_model_is_recurrent(model_)
        || llama_model_is_hybrid(model_);
    target_seq_rm_type_ = bounded_remove ? COMMON_CONTEXT_SEQ_RM_TYPE_RS
        : (full_remove ? COMMON_CONTEXT_SEQ_RM_TYPE_FULL
                       : COMMON_CONTEXT_SEQ_RM_TYPE_PART);

    // The tail owns the stock llama.cpp MTP context and speculative driver.
    // Non-tail stages never load auxiliary MTP tensors or interpret proposals.
    if (mtp_requested && tail_stage_) {
        auto mtp_params = common_base_params_to_speculative(params_);
        mtp_init_ = common_speculative_init_from_params(mtp_params, model_, ctx_);
        if (mtp_init_ == nullptr || mtp_init_->context() == nullptr) {
            return fail("llama.cpp failed to create the staged MTP context", error);
        }
        params_.speculative.draft.ctx_tgt = ctx_;
        params_.speculative.draft.ctx_dft = mtp_init_->context();
        mtp_speculative_.reset(common_speculative_init(
            params_.speculative, llama_n_seq_max(ctx_)));
        if (mtp_speculative_ == nullptr) {
            return fail("llama.cpp failed to initialize the staged MTP driver", error);
        }
        draft_seq_rm_type_ = llama_n_rs_seq(mtp_init_->context()) > 0
            ? COMMON_CONTEXT_SEQ_RM_TYPE_RS
            : ((llama_model_is_recurrent(model_)
                    || llama_model_is_hybrid(model_))
                ? COMMON_CONTEXT_SEQ_RM_TYPE_FULL
                : COMMON_CONTEXT_SEQ_RM_TYPE_PART);
    }
    StageMemoryPlan actual_memory;
    if (!measure_stage_memory(
            model_, ctx_, mtp_context(), config_.memory_topology,
            params_.kv_unified, &actual_memory, &memory_error)) {
        if (error != nullptr) *error = memory_error;
        unload();
        return false;
    }
    std::cerr << "MEMORY_ACTUAL " << serialize_stage_memory_plan(actual_memory) << '\n';
    if (!same_stage_memory_allocation(planned_memory, actual_memory, &memory_error)) {
        if (error != nullptr) *error = memory_error;
        unload();
        return false;
    }
    return true;
}

void StageRuntime::unload() noexcept {
    mtp_speculative_.reset();
    mtp_init_.reset();
    physical_checkpoints_.clear();
    mtp_sequences_.clear();
    target_seq_rm_type_ = COMMON_CONTEXT_SEQ_RM_TYPE_PART;
    draft_seq_rm_type_ = COMMON_CONTEXT_SEQ_RM_TYPE_PART;
    if (ctx_ != nullptr) {
        llama_free(ctx_);
        ctx_ = nullptr;
    }
    if (model_ != nullptr) {
        llama_model_free(model_);
        model_ = nullptr;
    }
    if (backend_initialized_) {
        llama_backend_free();
        backend_initialized_ = false;
    }
    sequence_ids_.clear();
    sequence_positions_.clear();
    samplers_.clear();
    sampler_options_.clear();
    sampled_tokens_.clear();
    sampled_texts_.clear();
    pending_texts_.clear();
    next_sequence_id_ = 0;
    tail_stage_ = false;
    // unload() is the only reset path: load() calls it first on every call,
    // the destructor calls it, and fail() calls it on every load-time error.
    // Clearing here means a fresh load() is the only way hop_memory_dirty_
    // goes back to false, which is the point of it.
    hop_memory_dirty_ = false;
    captured_executions_.clear();
    capture_error_.clear();
}

bool StageRuntime::release_sequence(const std::string & sequence_id,
                                    std::string * error) {
    if (!loaded()) {
        if (error != nullptr) *error = "stage runtime is not loaded";
        return false;
    }
    const auto found = sequence_ids_.find(sequence_id);
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_RUNTIME_RELEASE sequence=%s found=%d before=%zu\\n",
                     sequence_id.c_str(), found == sequence_ids_.end() ? 0 : 1,
                     sequence_ids_.size());
    }
    if (found == sequence_ids_.end()) {
        // Idempotent release is useful when a terminal response races a
        // cancellation path. There is no native state left to remove.
        sequence_positions_.erase(sequence_id);
        samplers_.erase(sequence_id);
        sampler_options_.erase(sequence_id);
        sampled_tokens_.erase(sequence_id);
        sampled_texts_.erase(sequence_id);
        pending_texts_.erase(sequence_id);
        return true;
    }
    (void) llama_memory_seq_rm(llama_get_memory(ctx_), found->second, 0, -1);
    sequence_ids_.erase(found);
    sequence_positions_.erase(sequence_id);
    samplers_.erase(sequence_id);
    sampler_options_.erase(sequence_id);
    sampled_tokens_.erase(sequence_id);
    sampled_texts_.erase(sequence_id);
    pending_texts_.erase(sequence_id);
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_RUNTIME_RELEASE_DONE sequence=%s after=%zu\\n",
                     sequence_id.c_str(), sequence_ids_.size());
    }
    return true;
}

bool StageRuntime::release_physical_sequence(
        const std::string & sequence_key,
        llama_seq_id sequence_id,
        std::string * error) {
    if (!loaded() || sequence_key.empty() || sequence_id < 0
        || static_cast<std::uint32_t>(sequence_id) >= llama_n_seq_max(ctx_)) {
        if (error != nullptr) *error = "invalid physical sequence release";
        return false;
    }
    if (!llama_memory_seq_rm(llama_get_memory(ctx_), sequence_id, 0, -1)) {
        if (error != nullptr) *error = "llama.cpp rejected target sequence release";
        return false;
    }
    if (mtp_context() != nullptr && !llama_memory_seq_rm(
            llama_get_memory(mtp_context()), sequence_id, 0, -1)) {
        if (error != nullptr) *error = "llama.cpp rejected MTP sequence release";
        return false;
    }
    common_speculative_end(mtp_speculative_.get(), sequence_id);
    samplers_.erase(sequence_key);
    sampler_options_.erase(sequence_key);
    sampled_tokens_.erase(sequence_key);
    sampled_texts_.erase(sequence_key);
    pending_texts_.erase(sequence_key);
    physical_checkpoints_.erase(sequence_id);
    mtp_sequences_.erase(sequence_id);
    return true;
}

DecodeStatus StageRuntime::decode(llama_batch batch, std::string * error) {
    if (!loaded()) {
        fail("stage runtime is not loaded", error);
        // Not a llama_decode() outcome at all -- there is no batch to run.
        // Fatal is the closest fit: fail() already unloaded the runtime, so
        // there is nothing for a caller to retry either way.
        return DecodeStatus::Fatal;
    }
    const auto raw = llama_decode(ctx_, batch);
    const auto status = decode_status_from_raw(raw);
    // The raw status number is kept in the message for every non-success
    // outcome; callers that need to tell REFUSAL from FAILURE do so from the
    // returned DecodeStatus, not by parsing this string.
    if (status != DecodeStatus::Success && error != nullptr) {
        *error = "llama_decode failed with status " + std::to_string(raw);
    }
    return status;
}

bool StageRuntime::set_input(int32_t index, const void * data, std::size_t size,
                             std::string * error) {
    if (!loaded() || !llama_linkcpp_input_set(ctx_, index, data, size)) {
        return fail("llama.cpp rejected stage input", error);
    }
    return true;
}

bool StageRuntime::get_output(int32_t index, void * data, std::size_t size,
                              std::string * error) const {
    if (!loaded() || !llama_linkcpp_output_get(ctx_, index, data, size)) {
        if (error != nullptr) *error = "llama.cpp rejected stage output";
        return false;
    }
    return true;
}

bool StageRuntime::synchronize_outputs(std::string * error) const {
    if (!loaded() || !llama_linkcpp_output_synchronize(ctx_)) {
        if (error != nullptr) *error = "llama.cpp failed to synchronize stage outputs";
        return false;
    }
    return true;
}

bool StageRuntime::stage_executor(llama_context * context,
                                  const llama_linkcpp_stage_invocation * invocation,
                                  void * user_data) {
    auto * runtime = static_cast<StageRuntime *>(user_data);
    return runtime != nullptr && runtime->capture_execution(context, invocation);
}

bool StageRuntime::state_executor(llama_linkcpp_state_invocation * invocation,
                                  void * user_data) {
    auto * runtime = static_cast<StageRuntime *>(user_data);
    if (runtime == nullptr || invocation == nullptr || invocation->version != 1
        || (invocation->ctx != runtime->ctx_
            && invocation->ctx != runtime->mtp_context())
        || (invocation->flags & LLAMA_STATE_SEQ_FLAGS_ON_DEVICE) != 0) {
        return false;
    }
    if (invocation->op == LLAMA_LINKCPP_STATE_SEQ_GET_SIZE ||
        invocation->op == LLAMA_LINKCPP_STATE_SEQ_GET_DATA) {
        invocation->result_size = 0;
        return true;
    }
    if (invocation->op == LLAMA_LINKCPP_STATE_SEQ_SET_DATA) {
        if (invocation->input_size != 0) return false;
        invocation->result_size = 0;
        return true;
    }
    return false;
}

} // namespace staged::llama_runtime
