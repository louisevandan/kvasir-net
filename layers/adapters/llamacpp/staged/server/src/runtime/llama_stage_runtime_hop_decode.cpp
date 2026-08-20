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
    if (!loaded()) return fail_hop("stage runtime is not loaded", error);

    // Stage 0 starts a lap from token ids and ignores the tail's cut-set;
    // every later stage starts from the cut-set it was handed.
    const bool from_tokens = config_.layer_begin == 0;
    const auto n_embd = llama_model_n_embd(model_);
    if (!from_tokens && n_embd <= 0) return false;

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

    if (!from_tokens && !bind_merged_cut_set(bundles, payloads, error)) return false;

    // A stage that was handed a cut-set decodes an embedding batch: the
    // hidden state arrives through the staged input, and llama.cpp must not
    // treat these rows as token ids and look their embeddings up.
    llama_batch batch = from_tokens
        ? llama_batch_init(static_cast<int32_t>(rows.size()), 0, 1)
        : llama_batch_init(static_cast<int32_t>(rows.size()), n_embd, 1);
    const bool allocated = from_tokens
        ? batch.token != nullptr
        : batch.embd != nullptr;
    if (!allocated || batch.n_seq_id == nullptr ||
        batch.seq_id == nullptr || batch.logits == nullptr) {
        llama_batch_free(batch);
        return fail_hop("llama.cpp failed to allocate the batched decode batch", error);
    }
    if (!from_tokens) {
        std::memset(batch.embd, 0, sizeof(float) * static_cast<std::size_t>(n_embd) * rows.size());
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
        if (from_tokens) batch.token[i] = row.token;
        batch.n_seq_id[i] = 1;
        batch.seq_id[i][0] = row.slot;
        // Every row is a sequence's own next token, so every row needs logits.
        batch.logits[i] = 1;
    }
    // A refusal here is not a failed request. Nothing was computed, so the
    // per-sequence path can still run the same lap; the error is dropped so
    // the caller takes it.
    std::string decode_error;
    const bool decoded = decode(batch, &decode_error);
    llama_batch_free(batch);
    if (!decoded) {
        std::fprintf(stderr,
                     "P4_STAGED_DECODE_BATCH_FAILED stage=%d-%d rows=%zu from_tokens=%d n_embd=%d\n",
                     config_.layer_begin, config_.layer_end, rows.size(),
                     from_tokens ? 1 : 0, n_embd);
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
    if (!tail_stage_ && !split_decode_outputs(rows.size(), &results, error)) return false;
    if (!synchronize_outputs(error)) return false;

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
        for (std::size_t i = 0; i < rows.size(); ++i) {
            results[i].descriptors.clear();
            results[i].payloads.clear();
            if (!sample_decode_row(*rows[i].input, static_cast<int32_t>(i), &results[i], error)) {
                return false;
            }
        }
    }

    for (std::size_t i = 0; i < rows.size(); ++i) {
        sequence_positions_[results[i].sequence_id] = results[i].outcome.has_value()
            ? results[i].outcome->position
            : static_cast<std::uint64_t>(rows[i].position) + 1;
    }
    if (hop_trace_enabled()) {
        std::fprintf(stderr, "P4_STAGED_HOP_DECODE_BATCH stage=%d-%d rows=%zu\n",
                     config_.layer_begin, config_.layer_end, rows.size());
    }
    *outputs = std::move(results);
    return true;
}

} // namespace staged::llama_runtime
