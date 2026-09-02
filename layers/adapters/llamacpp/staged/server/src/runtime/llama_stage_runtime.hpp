#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include "compat/p4_llama_compat.hpp"
#include "llama.h"
#include "decode_status.hpp"
#include "kv_bridge.hpp"
#include "state_store.hpp"
#include "protocol.hpp"
#include "physical_execution.hpp"
#include "stage_memory_plan.hpp"

namespace staged::llama_runtime {

struct PhysicalOwner;
struct PhysicalOutcome;
struct GeneratedToken;

struct MtpHopObservation final {
    std::size_t drafted_tokens = 0;
    std::size_t accepted_tokens = 0;
    bool eos = false;
};

struct MtpPendingProposal final {
    llama_token sampled = LLAMA_TOKEN_NULL;
    llama_pos sampled_position = -1;
    std::uint32_t generated_before = 0;
    std::uint32_t generated_now = 0;
    std::uint32_t max_tokens = 0;
};

struct MtpPhysicalSequence final {
    std::vector<llama_token> history;
    std::vector<llama_token> proposal;
    p4_llama_compat::PromptCheckpoint draft_checkpoint;
    std::optional<MtpPendingProposal> pending_proposal;
    bool replay_pending = false;
    bool begun = false;
};

struct MtpDraftRequest final {
    llama_seq_id sequence_id = -1;
    std::uint32_t generated_before = 0;
    std::uint32_t max_tokens = 0;
    llama_token sampled = LLAMA_TOKEN_NULL;
    llama_pos sampled_position = -1;
    std::uint32_t generated_now = 0;
    std::size_t outcome_index = 0;
};

class StageRuntime final : public runtime::KvBridge {
public:
    StageRuntime() = default;
    StageRuntime(const StageRuntime &) = delete;
    StageRuntime & operator=(const StageRuntime &) = delete;
    ~StageRuntime();

    [[nodiscard]] bool load(p4_llama_compat::LlamaPlan params, const LoadConfig & config,
                            std::string * error = nullptr);
    void unload() noexcept;

    [[nodiscard]] bool loaded() const noexcept { return model_ != nullptr && ctx_ != nullptr; }
    [[nodiscard]] const LoadConfig & config() const noexcept { return config_; }
    [[nodiscard]] llama_context * context() const noexcept { return ctx_; }
    [[nodiscard]] const llama_model * model() const noexcept { return model_; }
    [[nodiscard]] std::uint32_t context_size() const noexcept {
        return ctx_ == nullptr ? 0 : llama_n_ctx(ctx_);
    }
    [[nodiscard]] std::uint32_t batch_size() const noexcept {
        return ctx_ == nullptr ? 0 : llama_n_batch(ctx_);
    }
    [[nodiscard]] std::uint32_t ubatch_size() const noexcept {
        return ctx_ == nullptr ? 0 : llama_n_ubatch(ctx_);
    }
    [[nodiscard]] std::uint32_t sequence_capacity() const noexcept {
        return ctx_ == nullptr ? 0 : llama_n_seq_max(ctx_);
    }
    [[nodiscard]] bool kv_unified() const noexcept {
        return params_.kv_unified();
    }
    [[nodiscard]] bool requires_equal_sequence_ubatch() const noexcept {
        return model_ != nullptr
            && (llama_model_is_recurrent(model_) || llama_model_is_hybrid(model_));
    }
    [[nodiscard]] llama_context * mtp_context() const noexcept {
        return !mtp_init_.valid() ? nullptr : mtp_init_.context();
    }
    // Test-only full-tail path. The ordinary HOP remains unchanged.
    [[nodiscard]] bool execute_mtp_hop(
        const std::vector<std::int32_t> & prompt,
        MtpHopObservation * observation,
        std::string * error = nullptr);

    [[nodiscard]] DecodeStatus decode(llama_batch batch, std::string * error = nullptr);

    // Stage zero submits one logical mixed Prefill/Decode window. llama.cpp
    // remains authoritative for splitting it into physical ubatches; every
    // callback invocation and its complete cut-set is returned intact.
    [[nodiscard]] bool execute_first_batch(
        const std::vector<LogicalRow> & rows,
        const std::vector<PhysicalOwner> & owners,
        std::vector<PhysicalExecution> * executions,
        std::string * error = nullptr);
    [[nodiscard]] bool tokenize_prompt(
        const std::string & prompt,
        std::vector<std::int32_t> * tokens,
        std::string * error = nullptr) const;

    // A downstream stage replays exactly one physical invocation and binds
    // the complete cut-set produced by its predecessor.
    [[nodiscard]] bool execute_physical(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        PhysicalExecution * output,
        std::string * error = nullptr);
    [[nodiscard]] bool prepare_physical_execution(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::string * error = nullptr);
    [[nodiscard]] bool sample_physical_outputs(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::vector<PhysicalOutcome> * outcomes,
        std::string * error = nullptr);
    [[nodiscard]] bool release_physical_sequence(
        const std::string & sequence_key,
        llama_seq_id sequence_id,
        std::string * error = nullptr);
    [[nodiscard]] bool settle_physical_sequence(
        llama_seq_id sequence_id,
        llama_pos rollback_from,
        bool restore_checkpoint,
        std::vector<llama_token> * proposal,
        std::string * error = nullptr);

    // Set by any HOP path when a decode (or a step that runs after a
    // successful one) leaves the KV cache somewhere rollback cannot restore.
    // See decode_status.hpp: rollback_hop_batch() only releases sequences
    // the current HOP newly created, so once this is true the only way back
    // to a clean state is a fresh load(), which starts by calling unload().
    [[nodiscard]] bool hop_memory_dirty() const noexcept { return hop_memory_dirty_; }
    void quarantine_physical_memory() noexcept { hop_memory_dirty_ = true; }
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
        p4_llama_compat::PromptCheckpoint * checkpoint,
        std::string * error = nullptr);
    [[nodiscard]] bool restore_checkpoint(
        const std::string & sequence_id,
        const p4_llama_compat::PromptCheckpoint & checkpoint,
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
    //
    // This does NOT undo a decode. It releases sequence-id slots this HOP
    // newly allocated (see hop_new_sequences_) and restores the round-robin
    // allocator's cursor; it never restores the KV position of a sequence
    // that already existed before the HOP started, because llama.cpp does
    // not expose a way to rewind a sequence's cache to an arbitrary earlier
    // position after llama_decode() has advanced it. A decode outcome that
    // leaves processed ubatches in the memory state (DecodeStatus::Aborted,
    // DecodeStatus::Fatal, or any failure detected after a successful
    // decode) is therefore not something a rollback can paper over -- see
    // hop_memory_dirty() above.
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
    [[nodiscard]] bool capture_execution(
        llama_context *, const llama_linkcpp_stage_invocation *);
    [[nodiscard]] bool collect_physical_tensors(
        bool terminal, std::vector<PhysicalTensor> *, std::string * error);
    [[nodiscard]] protocol::KvPayload manifest_request(
        const protocol::KvPayload &, std::uint64_t token_position) const;
    [[nodiscard]] std::uint64_t sequence_token_position(
        const std::string & sequence_id) const;

    // The tail-stage sampler step of the per-sequence HOP path (execute_hop),
    // split out to keep that file under the repository's line limit. Every
    // llama.cpp call in here runs after execute_hop's own decode() already
    // returned success, so every failure path sets hop_memory_dirty_.
    [[nodiscard]] bool sample_hop_outcome(
        const protocol::SequencePayload & input,
        protocol::HopPhase phase,
        const std::vector<llama_token> & input_tokens,
        int32_t last_batch_tokens,
        std::optional<protocol::SequencePayload::OutcomeMetadata> * outcome,
        std::string * error);
    [[nodiscard]] bool process_physical_mtp(
        const llama_batch &, const std::vector<PhysicalOwner> &, std::string *);
    [[nodiscard]] bool sample_physical_mtp(
        const PhysicalExecution &, const std::vector<PhysicalOwner> &,
        std::size_t, std::size_t,
        std::vector<PhysicalOutcome> *, std::vector<MtpDraftRequest> *, std::string *);
    [[nodiscard]] bool make_mtp_proposal(
        llama_seq_id, std::uint32_t, std::uint32_t,
        llama_token, llama_pos, std::uint32_t,
        std::vector<llama_token> *, std::string *);
    [[nodiscard]] bool make_mtp_proposals(
        const std::vector<MtpDraftRequest> &,
        std::vector<PhysicalOutcome> *, std::string *);
    [[nodiscard]] bool prepare_physical_owners(
        const std::vector<PhysicalOwner> &, std::string *);
    [[nodiscard]] bool validate_physical_atomic_round(
        const std::vector<PhysicalOwner> &, std::string *);
    [[nodiscard]] bool format_generated_token(
        const PhysicalOwner &, llama_token, std::uint32_t,
        bool terminal_after_token, GeneratedToken *, std::string *);

    p4_llama_compat::LlamaPlan params_;
    LoadConfig config_;
    llama_model * model_ = nullptr;
    llama_context * ctx_ = nullptr;
    p4_llama_compat::SpeculativeInit mtp_init_;
    p4_llama_compat::Speculative mtp_speculative_;
    p4_llama_compat::SeqRemoval target_seq_rm_type_ = p4_llama_compat::SeqRemoval::Partial;
    p4_llama_compat::SeqRemoval draft_seq_rm_type_ = p4_llama_compat::SeqRemoval::Partial;
    std::unordered_map<llama_seq_id, p4_llama_compat::PromptCheckpoint> physical_checkpoints_;
    std::unordered_map<llama_seq_id, MtpPhysicalSequence> mtp_sequences_;
    bool backend_initialized_ = false;
    std::unordered_map<std::string, llama_seq_id> sequence_ids_;
    // Where a lap's sampling time goes, split so the two halves are not
    // guessed at: the sampler chain over the whole vocabulary, and the
    // detokenisation of what it picked.
    std::uint64_t sampler_chain_nanos_ = 0;
    std::uint64_t detokenize_nanos_ = 0;
    std::unordered_map<std::string, p4_llama_compat::Sampler> samplers_;
    std::unordered_map<std::string, std::string> sampler_options_;
    // Keep generated token history so detokenization happens over the token
    // stream, not one piece at a time. A single llama token may contain an
    // incomplete UTF-8 byte sequence, which cannot cross the wire as String.
    std::unordered_map<std::string, std::vector<llama_token>> sampled_tokens_;
    std::unordered_map<std::string, std::string> sampled_texts_;
    std::unordered_map<std::string, std::string> pending_texts_;
    std::unordered_map<std::string, std::uint64_t> sequence_positions_;
    std::unordered_map<std::string, llama_seq_id> hop_sequence_snapshot_;
    std::vector<std::string> hop_new_sequences_;
    llama_seq_id hop_next_sequence_snapshot_ = 0;
    bool hop_batch_active_ = false;
    llama_seq_id next_sequence_id_ = 0;
    bool tail_stage_ = false;
    // See hop_memory_dirty() above. Cleared only in unload(), which load()
    // always calls first, so a fresh load() is the only way back to false.
    bool hop_memory_dirty_ = false;
    std::vector<PhysicalExecution> captured_executions_;
    std::string capture_error_;

    // Test-only seam: the mutation coverage in
    // llama_stage_runtime_compile_test.cpp for execute_hop's and
    // execute_decode_batch's entry guards, and for unload() clearing the
    // flag, needs to observe hop_memory_dirty_ without running a real decode
    // (an unloaded StageRuntime is enough for the guard tests -- see that
    // file). A single named friend function keeps the write path out of the
    // class's real API surface, unlike a public setter any caller could
    // reach by accident.
    friend void test_force_hop_memory_dirty(StageRuntime & runtime, bool value) noexcept;
};

} // namespace staged::llama_runtime
