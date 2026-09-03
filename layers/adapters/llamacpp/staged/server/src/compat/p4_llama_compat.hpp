#pragma once

// The only place in the stage server that may reach past llama.cpp's public
// headers.
//
// `src/llama-ext.h` is upstream's own staging surface: it says in its first
// lines that breaking changes are allowed and asks not to be included across
// the codebase. Every pin that moves one of those signatures would otherwise
// break an arbitrary number of stage-server files at once, and the damage a
// pin does is exactly what the layering is supposed to bound. So the include
// lives in one translation unit, the types crossing back are P4's own, and
// `validate-private-headers.mjs` keeps the count at one.
//
// Adding a function here is the deliberate act of taking on a pin risk.
// Adding the include anywhere else is the accident this file exists to stop.

#include "llama.h"
#include "ggml-backend.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <vector>
#include <vector>

namespace p4_llama_compat {

/// One backend buffer type's share of what a context has allocated. Mirrors
/// the staging struct by value so no caller names the upstream type.
struct MemoryBreakdownEntry {
    ggml_backend_buffer_type_t buffer_type = nullptr;
    std::size_t model = 0;
    std::size_t context = 0;
    std::size_t compute = 0;
};

/// Allocation, per backend buffer type, of one context.
std::vector<MemoryBreakdownEntry> memory_breakdown(const llama_context * context);

/// How many backend devices this model is spread over.
std::size_t model_device_count(const llama_model * model);

/// The i-th backend device of this model, or nullptr when out of range.
ggml_backend_dev_t model_device(const llama_model * model, std::size_t index);

/// Sampling options, held whole for the same reason the plan is: upstream
/// owns the field set and grows it.
class SamplingOptions final {
public:
    SamplingOptions();
    ~SamplingOptions();
    SamplingOptions(SamplingOptions &&) noexcept;
    SamplingOptions & operator=(SamplingOptions &&) noexcept;
    SamplingOptions(const SamplingOptions &) = delete;
    SamplingOptions & operator=(const SamplingOptions &) = delete;
    [[nodiscard]] SamplingOptions clone() const;

    /// Two values a stage trace reports, so a trace need not open the struct.
    [[nodiscard]] bool ignores_end_of_generation() const noexcept;
    [[nodiscard]] std::size_t logit_bias_count() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;

public:
    [[nodiscard]] Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const Impl & impl() const noexcept { return *impl_; }
};

/// A llama.cpp startup plan, held whole and never mirrored.
///
/// The measurement that decided this shape: the stage server reads six
/// `common_params` fields by name, and hands the whole struct to three
/// conversion functions that read sixty-six between them - sixty of which no
/// P4 code ever names. A P4-owned struct carrying the six would have silently
/// dropped context size, batch and ubatch, sequence capacity, GPU layer count,
/// device split, flash attention, rope and yarn scaling, tensor overrides and
/// fifty more, and would have to grow every time upstream adds a knob -
/// turning a boundary meant to reduce the cost of following upstream into a
/// second copy of the thing being followed.
///
/// So the plan is opaque. P4 names the operations it needs, not the fields,
/// and the struct stays on one side of the wall where an upstream change to
/// its shape reaches exactly one translation unit.
class LlamaPlan final {
public:
    LlamaPlan();
    ~LlamaPlan();
    LlamaPlan(LlamaPlan &&) noexcept;
    LlamaPlan & operator=(LlamaPlan &&) noexcept;
    LlamaPlan(const LlamaPlan &) = delete;
    LlamaPlan & operator=(const LlamaPlan &) = delete;

    /// The four values P4 reads for its own decisions rather than to hand
    /// straight back to llama.cpp.
    [[nodiscard]] const std::string & model_path() const noexcept;
    [[nodiscard]] bool kv_unified() const noexcept;
    [[nodiscard]] ggml_type cache_type_k() const noexcept;
    [[nodiscard]] ggml_type cache_type_v() const noexcept;

    /// Fills this plan from llama.cpp's own argument grammar.
    ///
    /// The grammar stays llama.cpp's - P4 hands it the arguments and takes
    /// the result - but the call lives here, so a change to the parser's
    /// signature or to what it fills reaches one translation unit. Deciding
    /// that P4 should own a grammar of its own and translate is a larger
    /// step and a separate one; this is only about where the call is made.
    ///
    /// `false` when llama.cpp rejected the arguments.
    [[nodiscard]] bool parse_arguments(const std::vector<std::string> & arguments);

    /// A deep copy. Not a copy constructor, because copying a plan is a
    /// deliberate act - a measurement pass that mutates its own copy - and
    /// not something that should happen by passing one to a function.
    [[nodiscard]] LlamaPlan clone() const;

    /// The batch widths the plan asked for. P4 reports both in HELLO and
    /// refuses a load whose stage cannot meet them, so they are its values
    /// to read - not a mirror of upstream's field set.
    [[nodiscard]] int n_batch() const noexcept;
    [[nodiscard]] int n_ubatch() const noexcept;

    /// How many sequences the plan asked for. P4 reports this; llama.cpp
    /// derives its own capacity from the same field.
    [[nodiscard]] int n_parallel() const noexcept;

    /// The one field P4 writes: measurement passes point the plan at the
    /// model the load config names.
    void set_model_path(const std::string & path);

    /// A copy of this plan's sampling options, which a request then adjusts.
    [[nodiscard]] SamplingOptions sampling_options() const;

    /// Conversions, which return llama.cpp's public types.
    [[nodiscard]] llama_model_params to_model_params() const;
    [[nodiscard]] llama_context_params to_context_params() const;

    /// The draft model's plan, which is another whole `common_params`.
    [[nodiscard]] LlamaPlan speculative_plan() const;

    /// Whether the plan pairs a quantized V cache with flash attention off.
    /// llama.cpp rejects that combination only after the model and every
    /// selected tensor have been loaded, so P4 asks before paying for the
    /// load - and asks here, because both halves of the question are
    /// upstream's.
    [[nodiscard]] bool quantized_v_without_flash_attention() const noexcept;

    /// Whether the plan asks for any speculative method at all.
    [[nodiscard]] bool requests_any_speculative() const noexcept;

    /// Whether the plan asks for a draft-model method other than MTP.
    ///
    /// Distinguishes "needs a draft context and proposal state" from
    /// "needs an ngram cache", which is what P4 reports as the blocker. Asked
    /// here so the list of draft-family enumerators lives beside the enum it
    /// belongs to; upstream adding one is then a compile-time visit to this
    /// function rather than a silent misclassification at the call site.
    [[nodiscard]] bool requests_draft_family() const noexcept;

    /// Whether the plan asks for draft-MTP. A question P4 asks, so it is
    /// asked here rather than by walking a vector of upstream enums.
    [[nodiscard]] bool requests_draft_mtp() const noexcept;

    /// Whether the plan asks for a speculative method this stage server does
    /// not implement. Asked here so the set of methods P4 supports is stated
    /// in one place, rather than as a walk over upstream's enum at the call
    /// site - which is how a new enumerator becomes silently supported.
    [[nodiscard]] bool requests_unsupported_speculative() const noexcept;

    /// Whether a draft model was asked for at all.
    [[nodiscard]] bool has_speculative_model() const noexcept;

    /// The plan itself, for the compat implementation only. Declared here
    /// because C++ has no other way to let one translation unit past a
    /// pimpl; `p4_llama_compat_internal.hpp` is what actually names the type.
    struct Impl;
    [[nodiscard]] struct Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const struct Impl & impl() const noexcept { return *impl_; }

private:
    std::unique_ptr<Impl> impl_;
};

/// Tokenisation, routed through here rather than called directly.
///
/// These four are thin conveniences over `llama.h`'s own tokenise and
/// detokenise entry points - the ones that size a buffer, call again, and
/// return a container. Wrapping them confines the dependency today;
/// reimplementing them over the public API would remove it entirely, and
/// that step needs a golden comparison rather than an assumption that two
/// buffer-sizing loops agree on every edge.
std::vector<llama_token> tokenize(const llama_vocab * vocab, const std::string & text,
                                  bool add_special, bool parse_special = false);

std::string detokenize(const llama_vocab * vocab, const std::vector<llama_token> & tokens,
                       bool special = true);

std::string token_to_piece(const llama_vocab * vocab, llama_token token, bool special = true);

/// Appends one token to a batch, with its position, sequences and whether
/// its logits are wanted.
void batch_add(llama_batch & batch, llama_token token, llama_pos pos,
               const std::vector<llama_seq_id> & seq_ids, bool logits);

/// What a context can do when asked to drop part of a sequence.
///
/// P4's own, mirroring upstream's four cases. An enum is the one shape worth
/// mirroring: it is closed, it is small, and a static assertion can hold the
/// two in agreement - which is not true of a struct with sixty-six fields.
enum class SeqRemoval {
    Unsupported,  ///< no memory module; seq_rm is not available at all
    Partial,      ///< an arbitrary range of a sequence can be dropped
    FullOnly,     ///< only a whole sequence can be dropped
    RecurrentBounded,  ///< partial, but bounded by n_rs_seq
};

/// What this context supports, asked of llama.cpp.
SeqRemoval sequence_removal(llama_context * context);

/// A prompt checkpoint: the saved state a sequence can be rewound to.
///
/// Opaque for the same reason the plan is - it carries serialised context
/// state whose layout is upstream's business. P4 stores one per sequence and
/// names only what it does with it.
class PromptCheckpoint final {
public:
    PromptCheckpoint();
    ~PromptCheckpoint();
    PromptCheckpoint(PromptCheckpoint &&) noexcept;
    PromptCheckpoint & operator=(PromptCheckpoint &&) noexcept;
    PromptCheckpoint(const PromptCheckpoint &) = delete;
    PromptCheckpoint & operator=(const PromptCheckpoint &) = delete;

    [[nodiscard]] std::int64_t n_tokens() const noexcept;
    [[nodiscard]] llama_pos pos_max() const noexcept;
    [[nodiscard]] std::size_t size() const noexcept;
    [[nodiscard]] bool empty() const noexcept;
    void clear();

    /// The serialised target and draft state. P4 moves these bytes across
    /// the wire; it does not interpret them.
    [[nodiscard]] const std::vector<std::uint8_t> & target_state() const noexcept;
    [[nodiscard]] std::vector<std::uint8_t> & target_state() noexcept;
    [[nodiscard]] const std::vector<std::uint8_t> & draft_state() const noexcept;
    [[nodiscard]] std::vector<std::uint8_t> & draft_state() noexcept;

    void update_positions(std::int64_t n_tokens, llama_pos pos_min, llama_pos pos_max);
    void save_target(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags);
    void save_draft(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags);
    void load_target(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags);
    void load_draft(llama_context * context, llama_seq_id seq_id, llama_state_seq_flags flags);

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;

public:
    [[nodiscard]] Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const Impl & impl() const noexcept { return *impl_; }
};

/// A sampler, a speculative driver, and the result of initialising one.
///
/// Owning handles rather than the upstream smart pointers, so that a header
/// naming one of these does not name llama.cpp's convenience library. The
/// underlying pointer is reached through `p4_llama_compat_internal.hpp` by
/// the files that still call those APIs directly - each such file is a debt
/// entry, which is the point: the list is the work remaining.
class Sampler final {
public:
    /// Creates a sampler for this model from these options, or an invalid
    /// handle if llama.cpp refuses.
    /// Non-const because llama.cpp mutates the options while initialising -
    /// it resolves defaults and caches derived state into them.
    [[nodiscard]] static Sampler create(const llama_model * model, SamplingOptions & options);

    /// An independent copy, used to checkpoint a sequence's sampler state.
    [[nodiscard]] Sampler clone() const;

    /// Feeds a token back in. `accept_grammar` is false while replaying
    /// tokens the sampler did not choose.
    void accept(llama_token token, bool accept_grammar);

    /// Samples one token from the logits at `index`.
    [[nodiscard]] llama_token sample(llama_context * context, int index);

    /// Samples and accepts across a draft, returning what was accepted.
    /// The same over a draft whose logits are the batch's own order.
    [[nodiscard]] std::vector<llama_token> sample_and_accept_n(
        llama_context * context, const std::vector<llama_token> & draft);

    [[nodiscard]] std::vector<llama_token> sample_and_accept_n(
        llama_context * context, const std::vector<int> & indices,
        const std::vector<llama_token> & draft);

    Sampler();
    ~Sampler();
    Sampler(Sampler &&) noexcept;
    Sampler & operator=(Sampler &&) noexcept;
    Sampler(const Sampler &) = delete;
    Sampler & operator=(const Sampler &) = delete;
    [[nodiscard]] bool valid() const noexcept;
    void reset();

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;

public:
    [[nodiscard]] Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const Impl & impl() const noexcept { return *impl_; }
};

/// What P4 asks of one draft round.
///
/// Mirrored rather than carried, because unlike a plan this is closed and
/// P4 owns the meaning of every field: it decides how far to draft, from
/// which position, after which token, over which prompt, and where the
/// result goes.
struct DraftRequest final {
    bool drafting = true;
    std::int32_t max_tokens = -1;
    llama_pos n_past = 0;
    llama_token last_token = 0;
    const std::vector<llama_token> * prompt = nullptr;
    std::vector<llama_token> * result = nullptr;
};

class Speculative final {
public:
    /// Starts a sequence's speculative history.
    void begin(llama_seq_id seq_id, const std::vector<llama_token> & prompt);

    /// Feeds a decoded batch in. False when the driver rejected it.
    [[nodiscard]] bool process(const llama_batch & batch);

    /// Configures one sequence for the next draft round without running it.
    /// Upstream drafts every configured sequence in a single batch, so a
    /// caller with several sequences configures each and then runs once -
    /// running per sequence would be a different computation.
    void configure_draft(llama_seq_id seq_id, const DraftRequest & request);

    /// Runs one draft round over everything configured.
    void run_draft();

    /// Configures this one sequence and runs immediately.
    void draft(llama_seq_id seq_id, const DraftRequest & request);

    /// Reports how many of the drafted tokens were accepted.
    void accept(llama_seq_id seq_id, std::uint16_t accepted);

    /// Drops a sequence's speculative state.
    void end(llama_seq_id seq_id);

    Speculative();
    ~Speculative();
    Speculative(Speculative &&) noexcept;
    Speculative & operator=(Speculative &&) noexcept;
    Speculative(const Speculative &) = delete;
    Speculative & operator=(const Speculative &) = delete;
    [[nodiscard]] bool valid() const noexcept;
    void reset();

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;

public:
    [[nodiscard]] Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const Impl & impl() const noexcept { return *impl_; }
};

/// What initialising a speculative driver produced, including the draft
/// context it owns.
class SpeculativeInit final {
public:
    SpeculativeInit();
    ~SpeculativeInit();
    SpeculativeInit(SpeculativeInit &&) noexcept;
    SpeculativeInit & operator=(SpeculativeInit &&) noexcept;
    SpeculativeInit(const SpeculativeInit &) = delete;
    SpeculativeInit & operator=(const SpeculativeInit &) = delete;
    [[nodiscard]] bool valid() const noexcept;
    void reset();
    /// The draft model's context, which P4 measures and drives.
    [[nodiscard]] llama_context * context() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;

public:
    [[nodiscard]] Impl & impl() noexcept { return *impl_; }
    [[nodiscard]] const Impl & impl() const noexcept { return *impl_; }
};

/// Which ggml backends and devices are registered in this process.
///
/// An inventory, not a layout: it enumerates what the registry offers, not
/// where the model's tensors, the KV cache and the compute buffers actually
/// ended up. A run that placed everything on CUDA and a run that fell back
/// to host buffers report the same inventory. Binding persisted state to
/// this value would be binding it to the wrong thing; that needs a separate
/// identity taken from the placement itself.
///
/// What it does separate is a CPU build from a CUDA one, and a process that
/// registered two devices from one that registered one.
///
/// Format: `reg[dev,dev]|reg[dev]`, registries sorted by name so a different
/// plugin load order is not a different inventory. Every name is percent
/// escaped, because this value travels inside a `;`-delimited, `=`-keyed
/// capability string and a registry named with either would otherwise
/// become a field of its own - which is exactly what happened on
/// 2026-09-02, when `CUDA[CUDA0];CPU[CPU]` reached the adapter as
/// `CUDA[CUDA0]` and the CPU registry became a nameless capability.
std::string backend_inventory();

/// What bringing up a draft model produced.
struct SpeculativeSetup final {
    SpeculativeInit init;
    Speculative driver;
    /// Empty when it came up; otherwise why it did not.
    std::string failure;
};

/// Brings up the draft model and its speculative driver for this plan.
///
/// The whole sequence lives here because every step of it is llama.cpp's:
/// deriving the draft plan from the target's, creating the draft context,
/// writing the two contexts back into the plan, and initialising the driver
/// over the result. Leaving any of it outside meant an upstream change to
/// the speculative structures still reached the runtime.
[[nodiscard]] SpeculativeSetup bring_up_speculative(
    LlamaPlan & plan, llama_model * model, llama_context * context, std::uint32_t sequences);

/// Where `text` starts a prefix of `stop`, or npos.
///
/// A one-line helper from the convenience library, wrapped rather than
/// reimplemented so the two cannot disagree about what counts as a partial
/// match. It is here because it was the last thing keeping request_stops.cpp
/// on the debt list, and unlike the CLI and option grammars around it, it
/// needed no contract decision to move.
[[nodiscard]] std::size_t find_partial_stop(std::string_view text, std::string_view stop);

}  // namespace p4_llama_compat
