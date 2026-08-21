#include "llama_stage_runtime.hpp"

#include <cstdio>
#include <cstdlib>

#include <algorithm>
#include <utility>

namespace {

bool checkpoint_sequence(
        std::unordered_map<std::string, llama_seq_id> & ids,
        llama_seq_id & next,
        const std::string & id,
        uint32_t sequence_limit,
        llama_seq_id * result,
        std::string * error) {
    const auto found = ids.find(id);
    if (found != ids.end()) {
        *result = found->second;
        return true;
    }
    if (next < 0 || static_cast<uint64_t>(next) >= sequence_limit) {
        if (error != nullptr) *error = "staged checkpoint sequence table is full";
        return false;
    }
    const auto value = next++;
    ids.emplace(id, value);
    *result = value;
    return true;
}

} // namespace

namespace staged::llama_runtime {

StageRuntime::~StageRuntime() { unload(); }

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
    const bool speculative_requested = mtp_requested ||
        params.speculative.has_dft() || std::any_of(
            params.speculative.types.begin(), params.speculative.types.end(),
            [](const common_speculative_type type) {
                return type != COMMON_SPECULATIVE_TYPE_NONE;
            });
    if (speculative_requested && !config.mtp_ownership_probe) {
        if (mtp_requested) {
            return fail("CAPABILITY_UNAVAILABLE: "
                        "mtp_auxiliary_layers_not_owned_by_stage", error);
        }
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
    backend_initialized_ = true;
    llama_linkcpp_runtime_params runtime_params{};
    runtime_params.model_path = params_.model.path.c_str();
    runtime_params.layer_begin = config_.layer_begin;
    runtime_params.layer_end = config_.layer_end;
    runtime_params.executor = &StageRuntime::stage_executor;
    runtime_params.executor_user_data = this;
    runtime_params.state_executor = &StageRuntime::state_executor;
    runtime_params.state_executor_user_data = this;
    if (!llama_linkcpp_runtime_configure(&runtime_params)) {
        return fail("llama.cpp rejected stage runtime configuration", error);
    }

    auto model_params = common_model_params_to_llama(params_);
    model_params.linkcpp_layer_begin = config_.layer_begin;
    model_params.linkcpp_layer_end = config_.layer_end;
    model_params.linkcpp_kv_gpu_layer_start = config_.kv_gpu_layer_start;
    model_params.linkcpp_kv_gpu_layer_end = config_.kv_gpu_layer_end;
    model_params.linkcpp_stage_executor = &StageRuntime::stage_executor;
    model_params.linkcpp_stage_executor_user_data = this;
    model_params.linkcpp_state_executor = &StageRuntime::state_executor;
    model_params.linkcpp_state_executor_user_data = this;
    model_ = llama_model_load_from_file(params_.model.path.c_str(), model_params);
    if (model_ == nullptr) {
        return fail("llama.cpp failed to load the staged model", error);
    }

    auto context_params = common_context_params_to_llama(params_);
    ctx_ = llama_init_from_model(model_, context_params);
    if (ctx_ == nullptr) {
        return fail("llama.cpp failed to create the staged context", error);
    }

    // This diagnostic path proves that the pinned common speculative driver
    // can create the second MTP context against the staged full-tail target.
    // It deliberately stops before proposal/verification so the production
    // capability remains mtp_execution=0 until HOP rollback is implemented.
    if (mtp_requested && config.mtp_ownership_probe) {
        auto mtp_params = common_base_params_to_speculative(params_);
        mtp_init_ = common_speculative_init_from_params(mtp_params, model_, ctx_);
        if (mtp_init_ == nullptr || mtp_init_->context() == nullptr) {
            return fail("llama.cpp failed to create the staged MTP context", error);
        }
        params_.speculative.draft.ctx_tgt = ctx_;
        params_.speculative.draft.ctx_dft = mtp_init_->context();
        mtp_speculative_.reset(common_speculative_init(params_.speculative, 1));
        if (mtp_speculative_ == nullptr) {
            return fail("llama.cpp failed to initialize the staged MTP driver", error);
        }
    }

    tail_stage_ = config_.layer_end >= llama_model_n_layer(model_);
    return true;
}

void StageRuntime::unload() noexcept {
    mtp_speculative_.reset();
    mtp_init_.reset();
    if (ctx_ != nullptr) {
        llama_free(ctx_);
        ctx_ = nullptr;
    }
    if (model_ != nullptr) {
        llama_model_free(model_);
        model_ = nullptr;
    }
    llama_linkcpp_runtime_clear();
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
    next_sequence_id_ = 0;
    tail_stage_ = false;
    // unload() is the only reset path: load() calls it first on every call,
    // the destructor calls it, and fail() calls it on every load-time error.
    // Clearing here means a fresh load() is the only way hop_memory_dirty_
    // goes back to false, which is the point of it.
    hop_memory_dirty_ = false;
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
        return true;
    }
    (void) llama_memory_seq_rm(llama_get_memory(ctx_), found->second, 0, -1);
    sequence_ids_.erase(found);
    sequence_positions_.erase(sequence_id);
    samplers_.erase(sequence_id);
    sampler_options_.erase(sequence_id);
    sampled_tokens_.erase(sequence_id);
    sampled_texts_.erase(sequence_id);
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_RUNTIME_RELEASE_DONE sequence=%s after=%zu\\n",
                     sequence_id.c_str(), sequence_ids_.size());
    }
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

bool StageRuntime::save_checkpoint(const std::string & sequence_id,
                                   common_prompt_checkpoint * checkpoint,
                                   std::string * error) {
    if (!loaded() || checkpoint == nullptr) {
        return fail("stage runtime is not loaded or checkpoint is null", error);
    }
    llama_seq_id seq = 0;
    if (!checkpoint_sequence(sequence_ids_, next_sequence_id_, sequence_id,
                             llama_n_seq_max(ctx_), &seq, error)) {
        return false;
    }
    const auto memory = llama_get_memory(ctx_);
    const auto pos_min = llama_memory_seq_pos_min(memory, seq);
    const auto pos_max = llama_memory_seq_pos_max(memory, seq);
    checkpoint->clear();
    checkpoint->update_pos(pos_max >= pos_min ? pos_max - pos_min + 1 : 0,
                           pos_min, pos_max);
    checkpoint->update_tgt(ctx_, seq, LLAMA_STATE_SEQ_FLAGS_NONE);
    if (checkpoint->data_tgt.empty()) {
        return fail("llama native checkpoint is empty", error);
    }
    return true;
}

bool StageRuntime::restore_checkpoint(
        const std::string & sequence_id,
        const common_prompt_checkpoint & checkpoint,
        std::string * error) {
    if (!loaded() || checkpoint.data_tgt.empty()) {
        return fail("stage runtime is not loaded or checkpoint is empty", error);
    }
    llama_seq_id seq = 0;
    if (!checkpoint_sequence(sequence_ids_, next_sequence_id_, sequence_id,
                             llama_n_seq_max(ctx_), &seq, error)) {
        return false;
    }
    const auto restored = llama_state_seq_set_data_ext(
        ctx_, checkpoint.data_tgt.data(), checkpoint.data_tgt.size(), seq,
        LLAMA_STATE_SEQ_FLAGS_NONE);
    if (restored != checkpoint.data_tgt.size()) {
        return fail("llama native checkpoint restore failed", error);
    }
    // State import may enqueue asynchronous device copies. The next decode
    // must observe the restored KV before it starts graph execution.
    llama_synchronize(ctx_);
    return true;
}

bool StageRuntime::stage_executor(llama_context *,
                                  const llama_linkcpp_stage_invocation *, void *) {
    // The C API performs the graph cut and host/device copies. The callback is
    // deliberately an acknowledgement hook; transport owns the bytes exposed
    // through llama_linkcpp_output_get().
    return true;
}

bool StageRuntime::state_executor(llama_linkcpp_state_invocation * invocation,
                                  void * user_data) {
    auto * runtime = static_cast<StageRuntime *>(user_data);
    if (runtime == nullptr || invocation == nullptr || invocation->version != 1
        || invocation->ctx != runtime->ctx_
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
