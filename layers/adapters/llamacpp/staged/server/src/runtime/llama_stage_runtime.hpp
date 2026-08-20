#pragma once

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

#include "common.h"
#include "speculative.h"
#include "sampling.h"
#include "llama.h"
#include "kv_bridge.hpp"
#include "state_store.hpp"
#include "protocol.hpp"

namespace staged::llama_runtime {

struct LoadConfig {
    std::string model_path;
    int32_t layer_begin = 0;
    int32_t layer_end = 0;
    int32_t kv_gpu_layer_start = 0;
    int32_t kv_gpu_layer_end = 0;
    std::string model_identity;
    std::string kv_root;
    // Test-only compatibility probe: load the MTP auxiliary weights in the
    // normal tail stage without claiming proposal/accept execution support.
    bool mtp_ownership_probe = false;
};

struct MtpHopObservation final {
    std::size_t drafted_tokens = 0;
    std::size_t accepted_tokens = 0;
    bool eos = false;
};

class StageRuntime final : public runtime::KvBridge {
public:
    StageRuntime() = default;
    StageRuntime(const StageRuntime &) = delete;
    StageRuntime & operator=(const StageRuntime &) = delete;
    ~StageRuntime();

    [[nodiscard]] bool load(common_params params, const LoadConfig & config,
                            std::string * error = nullptr);
    void unload() noexcept;

    [[nodiscard]] bool loaded() const noexcept { return model_ != nullptr && ctx_ != nullptr; }
    [[nodiscard]] const common_params & params() const noexcept { return params_; }
    [[nodiscard]] const LoadConfig & config() const noexcept { return config_; }
    [[nodiscard]] llama_context * context() const noexcept { return ctx_; }
    [[nodiscard]] const llama_model * model() const noexcept { return model_; }
    [[nodiscard]] llama_context * mtp_context() const noexcept {
        return mtp_init_ == nullptr ? nullptr : mtp_init_->context();
    }
    // Test-only full-tail path. The ordinary HOP remains unchanged.
    [[nodiscard]] bool execute_mtp_hop(
        const std::vector<std::int32_t> & prompt,
        MtpHopObservation * observation,
        std::string * error = nullptr);

    [[nodiscard]] bool decode(llama_batch batch, std::string * error = nullptr);
    [[nodiscard]] bool set_input(int32_t index, const void * data, std::size_t size,
                                 std::string * error = nullptr);
    [[nodiscard]] bool get_output(int32_t index, void * data, std::size_t size,
                                  std::string * error = nullptr) const;
    [[nodiscard]] bool synchronize_outputs(std::string * error = nullptr) const;

    // In-memory native context checkpoint. This reuses llama.cpp's common
    // checkpoint container for one target sequence; sampler state and any
    // speculative/draft context are intentionally outside this slice.
    [[nodiscard]] bool save_checkpoint(
        const std::string & sequence_id,
        common_prompt_checkpoint * checkpoint,
        std::string * error = nullptr);
    [[nodiscard]] bool restore_checkpoint(
        const std::string & sequence_id,
        const common_prompt_checkpoint & checkpoint,
        std::string * error = nullptr);

    [[nodiscard]] bool save(const protocol::KvPayload &, protocol::KvResult *,
                            std::string *) override;
    [[nodiscard]] bool restore(const protocol::KvPayload &, protocol::KvResult *,
                               std::string *) override;
    [[nodiscard]] bool drop(const protocol::KvPayload &, protocol::KvResult *,
                            std::string *) override;
    [[nodiscard]] bool reconcile_transaction(
        const protocol::KvPayload &, const protocol::KvReceipt &, std::string *) const;

    // Execute the smallest transport unit supported by staged today: one
    // sequence, with its input cut-set already materialized by the caller.
    // The runtime owns the llama_batch construction and returns the output
    // cut-set in the same descriptor/payload representation used on the wire.
    [[nodiscard]] bool execute_hop(
        const protocol::SequencePayload & input,
        protocol::HopPhase phase,
        protocol::SequencePayload * output,
            std::string * error = nullptr);

    // Every sequence of one decode lap, in a single llama_batch.
    //
    // A decode lap is one token per sequence, so running them one at a time
    // reads this stage's weights once per sequence and the device does the
    // work of a batch of one however many are waiting. Measured on two cards:
    // 6,000 decode laps against 6,208 graph executions, one sequence each,
    // with the cards mostly idle. Batching them is the same weights read once
    // for all of them, and llama.cpp keeps the caches apart by `seq_id`.
    //
    // Returns false without touching `outputs` when the batch is not one this
    // path can take — several tokens for one sequence, a missing cut-set, a
    // shape it cannot slice. The caller then runs the per-sequence path,
    // which is always correct and never faster.
    [[nodiscard]] bool execute_decode_batch(
        const std::vector<protocol::SequencePayload> & inputs,
        std::vector<protocol::SequencePayload> * outputs,
        std::string * error = nullptr);

    // Taking that batch apart again. Row `i` is the sequence that sat at
    // batch index `i`, in both the cut-set and the logits.
    // Every row's cut-set as one tensor bundle, bound to the graph. A
    // cut-set is several tensors that may alias one another, so the merge is
    // per tensor position across the rows.
    [[nodiscard]] bool bind_merged_cut_set(
        const std::vector<std::vector<protocol::Descriptor>> & bundles,
        const std::vector<std::vector<const std::vector<std::uint8_t> *>> & payloads,
        std::string * error);
    [[nodiscard]] bool split_decode_outputs(
        std::size_t rows,
        std::vector<protocol::SequencePayload> * results,
        std::string * error);
    [[nodiscard]] bool sample_decode_row(
        const protocol::SequencePayload & input,
        int32_t logits_index,
        protocol::SequencePayload * result,
        std::string * error);

    // A server HOP may contain several sequence executions. The batch boundary
    // is kept here so a later sequence failure can release only slots created
    // by this HOP while preserving mappings that existed at its start.
    void begin_hop_batch();
    void commit_hop_batch();
    [[nodiscard]] bool rollback_hop_batch(std::string * error = nullptr);

    // Releases one completed request's native KV sequence without touching
    // persisted state. This is distinct from KV_DROP, which removes an SSD
    // checkpoint as well.
    [[nodiscard]] bool release_sequence(
        const std::string & sequence_id,
        std::string * error = nullptr);

private:
    static bool stage_executor(llama_context *,
                               const llama_linkcpp_stage_invocation *, void * user_data);
    static bool state_executor(llama_linkcpp_state_invocation *, void * user_data);

    bool fail(const char * message, std::string * error);
    [[nodiscard]] protocol::KvPayload manifest_request(
        const protocol::KvPayload &, std::uint64_t token_position) const;
    [[nodiscard]] std::uint64_t sequence_token_position(
        const std::string & sequence_id) const;

    common_params params_;
    LoadConfig config_;
    llama_model * model_ = nullptr;
    llama_context * ctx_ = nullptr;
    common_speculative_init_result_ptr mtp_init_;
    common_speculative_ptr mtp_speculative_;
    bool backend_initialized_ = false;
    std::unordered_map<std::string, llama_seq_id> sequence_ids_;
    std::unordered_map<std::string, common_sampler_ptr> samplers_;
    std::unordered_map<std::string, std::string> sampler_options_;
    // Keep generated token history so detokenization happens over the token
    // stream, not one piece at a time. A single llama token may contain an
    // incomplete UTF-8 byte sequence, which cannot cross the wire as String.
    std::unordered_map<std::string, std::vector<llama_token>> sampled_tokens_;
    std::unordered_map<std::string, std::string> sampled_texts_;
    std::unordered_map<std::string, std::uint64_t> sequence_positions_;
    std::unordered_map<std::string, llama_seq_id> hop_sequence_snapshot_;
    std::vector<std::string> hop_new_sequences_;
    llama_seq_id hop_next_sequence_snapshot_ = 0;
    bool hop_batch_active_ = false;
    llama_seq_id next_sequence_id_ = 0;
    bool tail_stage_ = false;
};

} // namespace staged::llama_runtime
