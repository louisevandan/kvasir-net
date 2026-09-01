#include "llama_stage_runtime.hpp"

#include <filesystem>
#include <cstdio>
#include <cstdlib>
#include <limits>

namespace staged::llama_runtime {

namespace {

constexpr const char * kRuntimeIdentity = "staged-llama-kv-manifest-v1";

#ifndef P4_STAGED_LLAMA_UPSTREAM_COMMIT
#define P4_STAGED_LLAMA_UPSTREAM_COMMIT "unknown"
#endif

std::string model_identity(const LoadConfig & config) {
    if (!config.model_identity.empty()) return config.model_identity;
    std::error_code error;
    const auto canonical = std::filesystem::weakly_canonical(config.model_path, error);
    return error ? config.model_path : canonical.string();
}

bool matches(const LoadConfig & config, const protocol::KvPayload & request,
             std::string * error) {
    if (request.stage_begin != config.layer_begin || request.stage_end != config.layer_end) {
        if (error != nullptr) *error = "KV stage range does not match loaded stage";
        return false;
    }
    if (request.model_identity != model_identity(config)) {
        if (error != nullptr) *error = "KV model identity does not match loaded model";
        return false;
    }
    if (request.flags != 0) {
        if (error != nullptr) *error = "KV flags are unsupported by the host bridge";
        return false;
    }
    return true;
}

bool local_sequence(std::unordered_map<std::string, llama_seq_id> & ids,
                    llama_seq_id & next, const std::string & id,
                    uint32_t sequence_limit, llama_seq_id * result,
                    std::string * error) {
    const auto found = ids.find(id);
    if (found != ids.end()) {
        *result = found->second;
        return true;
    }
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_ALLOC_REQUEST sequence=%s limit=%u next=%lld active=%zu\\n",
                     id.c_str(), sequence_limit, static_cast<long long>(next), ids.size());
    }
    if (sequence_limit == 0) {
        if (error != nullptr) *error = "staged HOP sequence table is full";
        return false;
    }
    const auto start = next < 0
        ? 0U
        : static_cast<uint32_t>(next) % sequence_limit;
    for (uint32_t offset = 0; offset < sequence_limit; ++offset) {
        const auto candidate = (start + offset) % sequence_limit;
        const auto used = std::any_of(ids.begin(), ids.end(),
            [candidate](const auto & entry) {
                return entry.second == static_cast<llama_seq_id>(candidate);
            });
        if (used) continue;
        const auto value = static_cast<llama_seq_id>(candidate);
        ids.emplace(id, value);
        next = static_cast<llama_seq_id>((candidate + 1U) % sequence_limit);
        *result = value;
        return true;
    }
    if (error != nullptr) *error = "staged HOP sequence table is full";
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_ALLOC_FULL sequence=%s limit=%u active=%zu\\n",
                     id.c_str(), sequence_limit, ids.size());
    }
    return false;
}

} // namespace

protocol::KvPayload StageRuntime::manifest_request(
        const protocol::KvPayload & request, std::uint64_t token_position) const {
    auto manifest = request;
    const auto * system = llama_print_system_info();
    manifest.build_identity = std::string("upstream=")
        + P4_STAGED_LLAMA_UPSTREAM_COMMIT + ";system="
        + (system == nullptr || *system == '\0'
            ? "llama.cpp-system-info-unavailable" : system);
    manifest.runtime_identity = kRuntimeIdentity;
    manifest.context_identity = "n_ctx=" + std::to_string(llama_n_ctx(ctx_))
        + ";n_ctx_seq=" + std::to_string(llama_n_ctx_seq(ctx_))
        + ";n_batch=" + std::to_string(llama_n_batch(ctx_))
        + ";n_ubatch=" + std::to_string(llama_n_ubatch(ctx_))
        + ";n_seq_max=" + std::to_string(llama_n_seq_max(ctx_))
        + ";kv_unified=" + (params_.kv_unified() ? "1" : "0");
    manifest.kv_format = "K=" + std::to_string(static_cast<int>(params_.cache_type_k()))
        + ";V=" + std::to_string(static_cast<int>(params_.cache_type_v()))
        + ";flags=" + std::to_string(request.flags);
    manifest.token_position = token_position;
    return manifest;
}

std::uint64_t StageRuntime::sequence_token_position(const std::string & sequence_id) const {
    const auto found = sequence_positions_.find(sequence_id);
    return found == sequence_positions_.end() ? 0 : found->second;
}

bool StageRuntime::save(const protocol::KvPayload & request, protocol::KvResult * result,
                        std::string * error) {
    // A decode failure that left processed ubatches in the memory state
    // (hop_memory_dirty(), see llama_stage_runtime.hpp) makes every
    // sequence's KV content on this runtime unverifiable, not just the one
    // that failed -- llama.cpp does not report which sequence a fatal/abort
    // status actually touched. Persisting anything to disk while quarantined
    // would let a client KvSave a possibly-torn state and later KvRestore it
    // as if it were good.
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || result == nullptr || !matches(config_, request, error)) {
        if (error != nullptr && error->empty()) *error = "stage runtime is not loaded";
        return false;
    }
    if (config_.kv_root.empty()) {
        if (error != nullptr) *error = "KV SSD root is not configured";
        return false;
    }
    llama_seq_id seq = 0;
    if (!local_sequence(sequence_ids_, next_sequence_id_, request.sequence_id,
                        llama_n_seq_max(ctx_), &seq, error)) return false;
    const auto size = llama_state_seq_get_size_ext(ctx_, seq, request.flags);
    if (size == 0 || size > 128U * 1024U * 1024U) {
        if (error != nullptr) *error = "llama state size is unavailable or too large";
        return false;
    }
    std::vector<std::uint8_t> state(size);
    if (llama_state_seq_get_data_ext(ctx_, state.data(), state.size(), seq, request.flags) != size) {
        if (error != nullptr) *error = "llama state export failed";
        return false;
    }
    runtime::StateFileInfo info;
    const auto manifest = manifest_request(request, sequence_token_position(request.sequence_id));
    if (!runtime::StateStore(config_.kv_root).save(manifest, state, &info, error)) return false;
    (void)llama_memory_seq_rm(llama_get_memory(ctx_), seq, 0, -1);
    *result = {request.sequence_id, request.cache_key, info.bytes, info.checksum};
    return true;
}

bool StageRuntime::restore(const protocol::KvPayload & request, protocol::KvResult * result,
                           std::string * error) {
    // See save() above: a quarantined runtime's sequence table and local
    // position bookkeeping are as suspect as its KV content, so KvRestore is
    // refused along with KvSave/KvDrop until a fresh load() clears it.
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || result == nullptr || !matches(config_, request, error)) {
        if (error != nullptr && error->empty()) *error = "stage runtime is not loaded";
        return false;
    }
    std::vector<std::uint8_t> state;
    runtime::StateFileInfo info;
    const auto manifest = manifest_request(request, std::numeric_limits<std::uint64_t>::max());
    if (!runtime::StateStore(config_.kv_root).load(manifest, &state, &info, error)) return false;
    llama_seq_id seq = 0;
    if (!local_sequence(sequence_ids_, next_sequence_id_, request.sequence_id,
                        llama_n_seq_max(ctx_), &seq, error)) return false;
    // llama_state_seq_set_data_ext() restores the sequence metadata inline, but
    // the host IO reader uploads KV tensor ranges through the backend after
    // parsing the blob.  The public API synchronizes before the import, not
    // after it.  A following decode must not race those uploads (CUDA reports
    // this as the opaque llama_decode() status -3).
    if (llama_state_seq_set_data_ext(ctx_, state.data(), state.size(), seq, request.flags)
        != state.size()) {
        if (error != nullptr) *error = "llama state import failed";
        return false;
    }
    llama_synchronize(ctx_);
    const auto restored_position = info.token_position;
    const auto memory = llama_get_memory(ctx_);
    const auto max_position = llama_memory_seq_pos_max(memory, seq);
    const auto actual_position = max_position < 0
        ? 0U : static_cast<std::uint64_t>(max_position) + 1U;
    if (actual_position != restored_position) {
        if (error != nullptr) {
            *error = "KV restored token position does not match manifest: expected="
                + std::to_string(restored_position)
                + " actual=" + std::to_string(actual_position);
        }
        return false;
    }
    sequence_positions_[request.sequence_id] = restored_position;
    *result = {request.sequence_id, request.cache_key, info.bytes, info.checksum};
    return true;
}

bool StageRuntime::drop(const protocol::KvPayload & request, protocol::KvResult * result,
                        std::string * error) {
    // See save() above.
    if (refuse_for_dirty_hop_memory(hop_memory_dirty_, error)) return false;
    if (!loaded() || result == nullptr || !matches(config_, request, error)) {
        if (error != nullptr && error->empty()) *error = "stage runtime is not loaded";
        return false;
    }
    runtime::StateFileInfo info;
    const auto manifest = manifest_request(request, std::numeric_limits<std::uint64_t>::max());
    if (!runtime::StateStore(config_.kv_root).drop(manifest, &info, error)) return false;
    const auto found = sequence_ids_.find(request.sequence_id);
    if (found != sequence_ids_.end()) {
        (void)llama_memory_seq_rm(llama_get_memory(ctx_), found->second, 0, -1);
        sequence_ids_.erase(found);
    }
    sequence_positions_.erase(request.sequence_id);
    samplers_.erase(request.sequence_id);
    sampler_options_.erase(request.sequence_id);
    sampled_tokens_.erase(request.sequence_id);
    sampled_texts_.erase(request.sequence_id);
    *result = {request.sequence_id, request.cache_key, 0, info.checksum};
    return true;
}

bool StageRuntime::reconcile_transaction(
        const protocol::KvPayload & request, const protocol::KvReceipt & receipt,
        std::string * error) const {
    if (!loaded()) {
        if (error != nullptr) *error = "stage runtime is not loaded";
        return false;
    }
    if (receipt.state != protocol::KvReceiptState::Committed) return true;
    if (config_.kv_root.empty()) {
        if (error != nullptr) *error = "KV SSD root is not configured";
        return false;
    }
    auto manifest = manifest_request(request, std::numeric_limits<std::uint64_t>::max());
    manifest.operation_id.clear();
    manifest.flags = 0;
    std::string path_error;
    const auto path = runtime::StateStore(config_.kv_root).path_for(
        request.cache_key, &path_error);
    if (path.empty()) {
        if (error != nullptr) *error = path_error;
        return false;
    }
    if (receipt.kind == protocol::kKvDiscard) {
        if (std::filesystem::exists(path)) {
            if (error != nullptr) *error = "committed KV discard still has a state file";
            return false;
        }
        return true;
    }
    std::vector<std::uint8_t> state;
    runtime::StateFileInfo info;
    if (!runtime::StateStore(config_.kv_root).load(manifest, &state, &info, error)) {
        return false;
    }
    if (info.checksum != receipt.checksum || info.bytes != receipt.bytes) {
        if (error != nullptr) *error = "committed KV receipt does not match state file";
        return false;
    }
    return true;
}

} // namespace staged::llama_runtime
