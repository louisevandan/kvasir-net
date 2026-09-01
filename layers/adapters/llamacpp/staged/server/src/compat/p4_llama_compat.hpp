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
#include <memory>
#include <string>
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

    /// A deep copy. Not a copy constructor, because copying a plan is a
    /// deliberate act - a measurement pass that mutates its own copy - and
    /// not something that should happen by passing one to a function.
    [[nodiscard]] LlamaPlan clone() const;

    /// How many sequences the plan asked for. P4 reports this; llama.cpp
    /// derives its own capacity from the same field.
    [[nodiscard]] int n_parallel() const noexcept;

    /// The one field P4 writes: measurement passes point the plan at the
    /// model the load config names.
    void set_model_path(const std::string & path);

    /// Conversions, which return llama.cpp's public types.
    [[nodiscard]] llama_model_params to_model_params() const;
    [[nodiscard]] llama_context_params to_context_params() const;

    /// The draft model's plan, which is another whole `common_params`.
    [[nodiscard]] LlamaPlan speculative_plan() const;

    /// Whether the plan asks for draft-MTP. A question P4 asks, so it is
    /// asked here rather than by walking a vector of upstream enums.
    [[nodiscard]] bool requests_draft_mtp() const noexcept;

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

}  // namespace p4_llama_compat
