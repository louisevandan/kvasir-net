#include "llama_stage_runtime.hpp"

#include "physical_wire.hpp"
#include "request_options.hpp"

#include <algorithm>
#include <atomic>
#include <cstdlib>
#include <limits>
#include <map>
#include <utility>
#include <thread>
#include <unordered_set>
#include <vector>

namespace staged::llama_runtime {
namespace {

/// How many output rows make threading the sampler worth its setup.
///
/// Measured on a tail stage holding 22 layers over a vocabulary above 240k:
/// sampling costs about 0.29 ms per row against 0.11 ms per row for the
/// layers themselves, so a wide decode lap spends more time choosing tokens
/// than computing them. The work is per row and each row has its own sampler,
/// so it parallelises; below a handful of rows the threads cost more than
/// they save. The figures are in the batching evidence, not here - a runtime
/// source names tensor contracts, not the model that produced a measurement.
constexpr std::size_t PARALLEL_SAMPLE_THRESHOLD = 8;

/// Threads to sample with. **Serial unless asked for, and asking is unsafe.**
///
/// Threading this loop measured +10% on a 2B model and the arms did not
/// overlap, but the measurement does not establish that it is correct. Each
/// row has its own sampler, yet every worker calls
/// `common_sampler_sample(ctx_, ...)` on the *same* context, and upstream
/// enters it through `llama_synchronize()`, which updates `t_eval_us`,
/// `n_eval` and `n_queued_tokens` without a lock, and through
/// `get_logits_ith()`, which calls `output_reorder()` and swaps rows of
/// `logits.data` in place. Two workers doing that at once is a data race on
/// the buffer the answer is read from. A run passing its judge is not
/// evidence of its absence.
///
/// So the default is 1. `P4_STAGED_SAMPLE_THREADS=N` turns it on for the
/// experiment that has to come first: synchronise the context once on one
/// thread, take an immutable copy of the logits, and only then let
/// independent samplers run on it - with a fixed seed, fixed batch
/// membership, and a thread sanitiser over the shared context.
std::size_t sample_thread_limit() {
    static const std::size_t limit = [] {
        if (const char * requested = std::getenv("P4_STAGED_SAMPLE_THREADS")) {
            const auto value = std::strtoul(requested, nullptr, 10);
            if (value >= 1) return static_cast<std::size_t>(value);
        }
        return std::size_t{1};
    }();
    return limit;
}

/// Runs `work(slot)` for every slot below `count` across the sampler threads.
///
/// The caller guarantees the slots are independent; nothing here checks that.
template <typename Work>
void across_sample_threads(std::size_t count, Work && work) {
    const auto workers = std::min(sample_thread_limit(), count);
    std::atomic<std::size_t> next{0};
    const auto claim = [&] {
        for (;;) {
            const auto slot = next.fetch_add(1, std::memory_order_relaxed);
            if (slot >= count) return;
            work(slot);
        }
    };
    std::vector<std::thread> pool;
    pool.reserve(workers > 0 ? workers - 1 : 0);
    for (std::size_t worker = 1; worker < workers; ++worker) pool.emplace_back(claim);
    claim();
    for (auto & thread : pool) thread.join();
}

} // namespace

bool StageRuntime::sample_physical_outputs(
        const PhysicalExecution & input,
        const std::vector<PhysicalOwner> & owners,
        std::vector<PhysicalOutcome> * outcomes,
        std::string * error) {
    const auto rows = input.output.size();
    if (!loaded() || !tail_stage_ || outcomes == nullptr
        || owners.size() != rows || input.positions.size() != rows * input.n_pos) {
        if (error != nullptr) *error = "invalid physical sampling request";
        return false;
    }
    outcomes->clear();
    outcomes->reserve(rows);
    std::vector<MtpDraftRequest> draft_requests;
    // Prompt tokens to fold into each sequence's sampler, in row order.
    std::map<std::string, std::vector<llama_token>> prefill_accepts;
    for (const auto & owner : owners) {
        const auto options_found = sampler_options_.find(owner.sequence_key);
        if (options_found != sampler_options_.end()
            && options_found->second != owner.options) {
            samplers_.erase(owner.sequence_key);
            sampler_options_.erase(options_found);
            sampled_tokens_.erase(owner.sequence_key);
            sampled_texts_.erase(owner.sequence_key);
        }
        auto found = samplers_.find(owner.sequence_key);
        if (found == samplers_.end()) {
            auto sampling = params_.sampling_options();
            if (!apply_request_options(owner.options, model_, &sampling, error)) return false;
            auto sampler = p4_llama_compat::Sampler::create(model_, sampling);
            if (!sampler.valid()) {
                if (error != nullptr) *error = "llama.cpp failed to create physical sampler";
                return false;
            }
            found = samplers_.emplace(owner.sequence_key, std::move(sampler)).first;
            sampler_options_[owner.sequence_key] = owner.options;
        }
        if (owner.phase == PhysicalPhase::Prefill) {
            prefill_accepts[owner.sequence_key].push_back(owner.input_token);
        }
    }
    // A prompt's tokens enter the sampler's history before its first real
    // sample, and that is a walk of the sampler chain per token rather than a
    // push. Grouping the tokens by sequence and running the groups in parallel
    // was tried and reverted: the sampling time of prefill-dominated batches
    // was 165.1 and 161.2 ms serial against 166.2 and 156.7 ms threaded, which
    // is no difference. One request can contribute over a thousand prompt rows
    // to a batch, so the longest group sets the parallel time and there is
    // nothing to win. Kept serial.
    for (const auto & entry : prefill_accepts) {
        auto sampler = samplers_.find(entry.first);
        if (sampler == samplers_.end()) return false;
        for (const auto token : entry.second) sampler->second.accept(token, false);
    }
    // Choose the tokens first, in parallel, then run the bookkeeping loop
    // below unchanged.
    //
    // Only ordinary output rows are pre-sampled: Verify and Replay go through
    // the MTP path, which samples a whole speculative group against one
    // sampler and must stay sequential. Two rows sharing a sampler would also
    // have to stay sequential - one row's `accept` is the next row's state -
    // so a repeated sequence key abandons the pass rather than racing. Within
    // one physical batch a request contributes at most one output row, so the
    // repeat is not expected; it is refused rather than assumed away.
    std::vector<std::size_t> parallel_rows;
    std::vector<llama_token> parallel_tokens;
    {
        std::unordered_set<std::string> keys;
        bool eligible = true;
        for (std::size_t index = 0; index < rows && eligible; ++index) {
            if (owners[index].phase == PhysicalPhase::Verify
                || owners[index].phase == PhysicalPhase::Replay) {
                eligible = false;
                break;
            }
            if (input.output[index] == 0) continue;
            if (!keys.insert(owners[index].sequence_key).second) eligible = false;
            parallel_rows.push_back(index);
        }
        const auto threads = sample_thread_limit();
        if (!eligible || threads < 2 || parallel_rows.size() < PARALLEL_SAMPLE_THRESHOLD) {
            parallel_rows.clear();
        }
    }
    if (!parallel_rows.empty()) {
        parallel_tokens.assign(parallel_rows.size(), LLAMA_TOKEN_NULL);
        std::atomic<bool> missing_sampler{false};
        across_sample_threads(parallel_rows.size(), [&](std::size_t slot) {
            const auto index = parallel_rows[slot];
            auto sampler = samplers_.find(owners[index].sequence_key);
            if (sampler == samplers_.end()) {
                missing_sampler.store(true, std::memory_order_relaxed);
                return;
            }
            // Each row reads its own logits slice and mutates only its own
            // sampler; `samplers_` itself is not written here, every entry
            // having been created in the loop above.
            parallel_tokens[slot] =
                sampler->second.sample(ctx_, static_cast<std::int32_t>(index));
        });
        if (missing_sampler.load(std::memory_order_relaxed)) return false;
    }
    std::size_t parallel_cursor = 0;
    for (std::size_t index = 0; index < rows;) {
        if (owners[index].phase == PhysicalPhase::Verify
            || owners[index].phase == PhysicalPhase::Replay) {
            const auto count = static_cast<std::size_t>(owners[index].speculative_count);
            if (owners[index].speculative_index != 0 || count == 0
                || count > rows - index
                || !sample_physical_mtp(
                    input, owners, index, index + count,
                    outcomes, &draft_requests, error)) {
                if (error != nullptr && error->empty()) {
                    *error = "invalid mixed MTP physical group";
                }
                return false;
            }
            index += count;
            continue;
        }
        if (input.output[index] == 0) {
            ++index;
            continue;
        }
        auto sampler = samplers_.find(owners[index].sequence_key);
        if (sampler == samplers_.end()) return false;
        llama_token token = LLAMA_TOKEN_NULL;
        if (parallel_cursor < parallel_rows.size()
            && parallel_rows[parallel_cursor] == index) {
            token = parallel_tokens[parallel_cursor];
            ++parallel_cursor;
        } else {
            token = sampler->second.sample(ctx_, static_cast<std::int32_t>(index));
        }
        if (token == LLAMA_TOKEN_NULL) {
            if (error != nullptr) *error = "llama.cpp returned no physical token";
            return false;
        }
        sampler->second.accept(token, true);
        PhysicalOutcome outcome;
        outcome.owner_index = static_cast<std::uint32_t>(index);
        GeneratedToken generated;
        if (owners[index].position == std::numeric_limits<std::uint32_t>::max()) {
            if (error != nullptr) *error = "physical token position overflow";
            return false;
        }
        const auto position = owners[index].position + 1;
        const bool length_stop =
            owners[index].generated_tokens + 1 >= owners[index].max_tokens;
        if (!format_generated_token(
                owners[index], token, position, length_stop,
                &generated, error)) return false;
        if (generated.stop.empty() && length_stop) {
            generated.stop = "length";
        }
        if (generated.stop.empty()) {
            if (mtp_speculative_.valid()) {
                draft_requests.push_back(MtpDraftRequest{
                        static_cast<llama_seq_id>(owners[index].sequence_id),
                        owners[index].generated_tokens,
                        owners[index].max_tokens,
                        token, static_cast<llama_pos>(position),
                        1,
                        outcomes->size(),
                });
            } else {
                outcome.proposal.push_back(generated.token);
            }
        }
        outcome.generated.push_back(std::move(generated));
        outcomes->push_back(std::move(outcome));
        ++index;
    }
    return make_mtp_proposals(draft_requests, outcomes, error);
}

} // namespace staged::llama_runtime
