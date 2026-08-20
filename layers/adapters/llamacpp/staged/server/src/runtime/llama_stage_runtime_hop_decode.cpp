// One decode lap, every sequence of it, in a single llama_batch.
//
// The per-sequence path is correct and was the only one for a long time. What
// it costs is a weights read per sequence: this stage's layers are streamed
// from VRAM once for each token it produces, so ten sequences decoding cost
// ten times one. Measured on four cards, 35,000 decode laps ran as 35,500
// graph executions with one sequence in each, and the cards sat at 14-28%.
//
// The shape of this is not invented here. The runtime that came before P4
// batched a pipeline the same way — merge the boundary bundles along the
// token axis, decode once, split the result back — and the three things it
// got right that a first attempt does not are kept:
//
//   * a stage that consumes a cut-set takes an **embedding** batch, not a
//     token batch, so llama.cpp does not go looking for token embeddings;
//   * positions are stated per row rather than left to be inferred;
//   * a cut-set is a bundle of tensors, and the split tolerates llama.cpp
//     returning more rows than were asked for.

#include "llama_stage_runtime_hop_shared.hpp"
#include "request_options.hpp"

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <vector>

namespace staged::llama_runtime {

using namespace hop;

namespace {

// What one sequence contributes to the batch.
struct Row {
    const protocol::SequencePayload * input = nullptr;
    llama_seq_id slot = 0;
    llama_token token = 0;
    llama_pos position = 0;
};

} // namespace

bool StageRuntime::execute_decode_batch(
    const std::vector<protocol::SequencePayload> & inputs,
    std::vector<protocol::SequencePayload> * outputs,
    std::string * error) {
    if (outputs == nullptr || inputs.size() < 2) return false;
    // A hop that costs 18 ms on a stage whose arithmetic is under 2 ms is not
    // a compute problem, and the phases have to be told apart before anything
    // is optimised. Decode is asynchronous on CUDA, so the submit and the wait
    // for it are timed separately.
    const auto hop_started = std::chrono::steady_clock::now();
    sampler_chain_nanos_ = 0;
    detokenize_nanos_ = 0;
    std::chrono::steady_clock::duration bind_taken{};
    std::chrono::steady_clock::duration split_taken{};
    std::chrono::steady_clock::duration sample_taken{};
    // Every row comes back where it went in, and that is read off llama.cpp
    // rather than hoped for:
    //
    //   * `llama_memory_hybrid::init_batch` calls `split_equal` with
    //     `sequential = !unified`, so a unified cache removes the
    //     increasing-sequence-id filter entirely;
    //   * `split_equal` builds its sequence sets by scanning the batch
    //     upwards, and a decode gives each sequence exactly one token, so
    //     every per-sequence list holds a single index and the final concat
    //     is `[0 .. n)` -- the identity. `n_keep_tail` defers nothing
    //     either, because a sequence with one token has none remaining;
    //   * `ubatch_add` records `out_ids[i] = idxs[i]`, so `out_ids` is the
    //     identity too, and `llama_context::decode` finds `out_id == i` on
    //     every row, leaves `sorted_output` true and never fills
    //     `output_swaps`. `output_reorder` is then a loop over nothing.
    //
    // So no row moves, for any model, and the identity of a row is its
    // position. What stood here instead was a refusal for recurrent and
    // hybrid models, inferred from runs that predate the unified cache:
    // without it `sequential` is true, scattered slots are accepted only as
    // a leading run and the rest deferred to another ubatch -- which splits
    // the cut-set and makes `out_ids` non-identity at the same time. That is
    // why batching looked right at widths of two and three and wrong from
    // four up: a narrow window happened to hold consecutive slots. The model
    // was never the problem.
    //
    // Both things this rests on are the stage's own invariants, checked
    // below: a unified cache, and a lap that fits one ubatch.
    if (!loaded()) return fail_hop("stage runtime is not loaded", error);

    // llama.cpp splits a batch into ubatches of its own choosing, and the
    // staged cut-set is bound once per decode -- a split would leave the
    // graph expecting a narrower input than the one handed over, and an
    // output whose token axis is the ubatch rather than the lap. Both were
    // measured, and neither is recoverable once a partial batch has moved a
    // sequence's cache forward.
    //
    // A stage's plan is normalised so that neither can happen: `n_batch`
    // equals `n_ubatch` and the cache is unified. These two lines are the
    // reading of that, not a second opinion about it -- a stage that
    // somehow reaches here configured otherwise keeps the per-sequence
    // path rather than binding a cut-set it cannot honour.
    if (!params_.kv_unified) return false;
    if (inputs.size() > static_cast<std::size_t>(llama_n_ubatch(ctx_))) return false;

    // Stage 0 starts a lap from token ids and ignores the tail's cut-set;
    // every later stage starts from the cut-set it was handed.
    const bool from_tokens = config_.layer_begin == 0;
    const auto n_embd = llama_model_n_embd(model_);
    if (!from_tokens && n_embd <= 0) return false;

    // Whether this stage's cut-set can be cut back into rows by copying byte
    // ranges is a property of the model and the boundary, not of the lap, so
    // it is read off the graph this stage has already run rather than found
    // out after a decode that cannot be taken back. A cut tensor with an
    // axis above the token axis interleaves the rows — gemma4's per-layer
    // embedding is [n_embd_per_layer, n_tokens, n_layer] and is rank 3 even
    // for one token — and no byte range of it is a row. `ggml_n_dims` is
    // what says so, and llama.cpp reports it in every descriptor.
    if (!tail_stage_) {
        const auto previous = llama_linkcpp_output_count(ctx_);
        for (int32_t i = 0; i < previous; ++i) {
            llama_linkcpp_tensor_desc descriptor{};
            if (!llama_linkcpp_output_desc(ctx_, i, &descriptor)) return false;
            if (descriptor.alias_of < 0 && descriptor.n_dims > 2) return false;
        }
    }

    std::vector<Row> rows;
    rows.reserve(inputs.size());
    std::vector<std::vector<protocol::Descriptor>> bundles;
    std::vector<std::vector<const std::vector<std::uint8_t> *>> payloads;
    for (const auto & input : inputs) {
        if (input.descriptors.size() != input.payloads.size()) return false;
        // One token per sequence is what makes this a decode lap.
        if (input.n_tokens.value_or(1) != 1) return false;
        if (input.prompt.has_value()) return false;
        Row row;
        row.input = &input;
        row.position = static_cast<llama_pos>(input.position.value_or(0));
        if (from_tokens) {
            if (input.initial_tokens.has_value()) {
                if (input.initial_tokens->size() != 1) return false;
                row.token = static_cast<llama_token>(input.initial_tokens->front());
            }
        } else {
            if (input.descriptors.empty()) return false;
            std::vector<const std::vector<std::uint8_t> *> row_payloads;
            row_payloads.reserve(input.payloads.size());
            for (std::size_t i = 0; i < input.descriptors.size(); ++i) {
                const auto & descriptor = input.descriptors[i];
                if (descriptor.has_alias) {
                    row_payloads.push_back(nullptr);
                    continue;
                }
                if (!input.payloads[i].has_value()) return false;
                if (input.payloads[i]->size() != descriptor.nbytes) return false;
                row_payloads.push_back(&input.payloads[i].value());
            }
            bundles.push_back(input.descriptors);
            payloads.push_back(std::move(row_payloads));
        }
        rows.push_back(row);
    }

    const auto sequence_limit = llama_n_seq_max(ctx_);
    if (sequence_limit == 0) return fail_hop("llama.cpp returned an invalid sequence limit", error);
    if (rows.size() > sequence_limit) return false;
    for (auto & row : rows) {
        const bool was_present =
            sequence_ids_.find(row.input->sequence_id) != sequence_ids_.end();
        if (!local_sequence(sequence_ids_, next_sequence_id_, row.input->sequence_id,
                            sequence_limit, &row.slot, error)) {
            return false;
        }
        if (hop_batch_active_ && !was_present) {
            hop_new_sequences_.push_back(row.input->sequence_id);
        }
    }

    if (!from_tokens) {
        const auto bind_started = std::chrono::steady_clock::now();
        if (!bind_merged_cut_set(bundles, payloads, error)) return false;
        bind_taken = std::chrono::steady_clock::now() - bind_started;
    }

    // A token batch, exactly as the per-sequence path builds one, including
    // for a stage that consumes a cut-set: the hidden state arrives through
    // the staged input and the token storage is only what `llama_batch`
    // requires when `embd` is null. The runtime that came before P4 used an
    // embedding batch here, but its graph was arranged differently, and the
    // path this one has to agree with is the one beside it.
    llama_batch batch = llama_batch_init(
        static_cast<int32_t>(rows.size()), 0, static_cast<int32_t>(sequence_limit));
    if (batch.token == nullptr || batch.n_seq_id == nullptr ||
        batch.seq_id == nullptr || batch.logits == nullptr) {
        llama_batch_free(batch);
        return fail_hop("llama.cpp failed to allocate the batched decode batch", error);
    }
    // Positions are llama.cpp's to assign, per sequence, from what each
    // sequence's cache already holds — which is what the per-sequence path
    // has always relied on. Stating them here instead means stating an
    // absolute position, and a lap only knows its own index: a lap 22 of a
    // sequence whose prompt was 5,005 tokens is position 5,027, and handing
    // over 22 is refused as a cache that ran backwards.
    std::free(batch.pos);
    batch.pos = nullptr;
    batch.n_tokens = static_cast<int32_t>(rows.size());
    for (int32_t i = 0; i < batch.n_tokens; ++i) {
        const auto & row = rows[static_cast<std::size_t>(i)];
        batch.token[i] = row.token;
        batch.n_seq_id[i] = 1;
        batch.seq_id[i][0] = row.slot;
        // Every row is a sequence's own next token, so every row needs logits.
        batch.logits[i] = 1;
    }
    // A refusal here is not a failed request. Nothing was computed, so the
    // per-sequence path can still run the same lap; the error is dropped so
    // the caller takes it.
    std::string decode_error;
    const auto decode_started = std::chrono::steady_clock::now();
    const bool decoded = decode(batch, &decode_error);
    const auto decode_ended = std::chrono::steady_clock::now();
    llama_batch_free(batch);
    if (!decoded) {
        std::fprintf(stderr,
                     "P4_STAGED_DECODE_BATCH_FAILED stage=%d-%d rows=%zu from_tokens=%d n_embd=%d\n",
                     config_.layer_begin, config_.layer_end, rows.size(),
                     from_tokens ? 1 : 0, 0);
        if (error != nullptr) error->clear();
        return false;
    }

    std::vector<protocol::SequencePayload> results(rows.size());
    for (std::size_t i = 0; i < rows.size(); ++i) {
        results[i].sequence_id = rows[i].input->sequence_id;
        results[i].n_tokens = 1;
        results[i].options = rows[i].input->options;
    }
    // The tail forwards no cut-set: stage 0 starts the next lap from its own
    // token input, so returning these activations would only inflate the
    // reply and make the next lap ambiguous.
    if (!tail_stage_) {
        const auto split_started = std::chrono::steady_clock::now();
        if (!split_decode_outputs(rows.size(), &results, error)) return false;
        split_taken = std::chrono::steady_clock::now() - split_started;
    }
    const auto sync_started = std::chrono::steady_clock::now();
    if (!synchronize_outputs(error)) return false;
    const auto sync_ended = std::chrono::steady_clock::now();

    if (tail_stage_) {
        // Every row must have a logits row of its own. If llama.cpp split the
        // batch into ubatches, only the last one's outputs survive and the
        // rows in front would be sampled from logits that are not theirs —
        // which does not fail, it just answers the wrong sequence. Checked
        // rather than assumed, because a wrong token is invisible until a
        // stream stops making sense.
        if (llama_get_logits_ith(ctx_, static_cast<int32_t>(rows.size()) - 1) == nullptr) {
            std::fprintf(stderr,
                         "P4_STAGED_DECODE_BATCH_SPLIT stage=%d-%d rows=%zu\n",
                         config_.layer_begin, config_.layer_end, rows.size());
            if (error != nullptr) error->clear();
            return false;
        }
        const auto sample_started = std::chrono::steady_clock::now();
        for (std::size_t i = 0; i < rows.size(); ++i) {
            results[i].descriptors.clear();
            results[i].payloads.clear();
            if (!sample_decode_row(*rows[i].input, static_cast<int32_t>(i), &results[i], error)) {
                return false;
            }
        }
        sample_taken = std::chrono::steady_clock::now() - sample_started;
    }

    for (std::size_t i = 0; i < rows.size(); ++i) {
        sequence_positions_[results[i].sequence_id] = results[i].outcome.has_value()
            ? results[i].outcome->position
            : static_cast<std::uint64_t>(rows[i].position) + 1;
    }
    if (hop_trace_enabled()) {
        const auto micros = [](auto from, auto to) {
            return static_cast<long long>(
                std::chrono::duration_cast<std::chrono::microseconds>(to - from).count());
        };
        std::fprintf(stderr,
                     "P4_STAGED_HOP_DECODE_BATCH stage=%d-%d rows=%zu "
                     "submit_us=%lld sync_us=%lld bind_us=%lld split_us=%lld "
                     "sample_us=%lld chain_us=%lld detok_us=%lld total_us=%lld\n",
                     config_.layer_begin, config_.layer_end, rows.size(),
                     micros(decode_started, decode_ended), micros(sync_started, sync_ended),
                     micros(hop_started, hop_started + bind_taken),
                     micros(hop_started, hop_started + split_taken),
                     micros(hop_started, hop_started + sample_taken),
                     static_cast<long long>(sampler_chain_nanos_ / 1000),
                     static_cast<long long>(detokenize_nanos_ / 1000),
                     micros(hop_started, std::chrono::steady_clock::now()));
    }
    *outputs = std::move(results);
    return true;
}

} // namespace staged::llama_runtime
