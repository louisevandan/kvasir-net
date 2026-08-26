#pragma once

// The five outcomes of llama_decode(), and the two questions P4 actually
// needs answered about each of them.
//
// This header knows nothing about llama.cpp -- no llama.h, no llama_context,
// not even a forward declaration. `decode()` in llama_stage_runtime.cpp is
// the one and only place the raw int32_t crosses into a DecodeStatus, and
// everything downstream of that (the batched decode lap and the per-sequence
// path) works with the enum. Kept dependency-free on purpose: the mapping
// and the classification below are what a defect lived in for a long time --
// see llama_stage_runtime_hop_decode.cpp for the history -- and a mapping
// that can only be checked by running a real model is a mapping nobody
// checks. This one is exercised by decode_status_test.cpp without a
// llama.cpp checkout.
//
// The authoritative contract, quoted from `llama_decode`'s declaration in
// upstream/include/llama.h (the comment immediately above it, as of the
// commit this repository clones):
//
//   Positive return values does not mean a fatal error, but rather a warning.
//   Upon fatal-error or abort, the ubatches that managed to be been processed
//   will remain in the memory state of the context
//     To handle this correctly, query the memory state using
//     llama_memory_seq_pos_min() and llama_memory_seq_pos_max()
//   Upon other return values, the memory state is restored to the state
//   before this call
//      0 - success
//      1 - could not find a KV slot for the batch (try reducing the size of
//          the batch or increase the context)
//      2 - aborted     (processed ubatches will remain in the context's
//          memory)
//     -1 - invalid input batch
//    < -1 - fatal error (processed ubatches will remain in the context's
//          memory)
//
// So two of the five leave the memory state advanced past what the caller
// believes happened (Aborted, Fatal), two leave it exactly as it was
// (NoKvSlot, InvalidInput), and only one is success. Collapsing all of this
// into a bool -- what this file replaces -- cannot tell a batch that
// computed nothing from one that computed half of itself and stopped.

#include <cstdint>
#include <string>

namespace staged::llama_runtime {

enum class DecodeStatus {
    Success,       //    0
    NoKvSlot,      //    1  memory state restored to the state before the call
    Aborted,       //    2  processed ubatches REMAIN in the memory state
    InvalidInput,  //   -1  memory state restored
    Fatal,         // < -1  processed ubatches REMAIN in the memory state
};

// The one and only place the raw llama_decode() return value is interpreted.
inline DecodeStatus decode_status_from_raw(std::int32_t raw) {
    if (raw == 0) return DecodeStatus::Success;
    if (raw == 1) return DecodeStatus::NoKvSlot;
    if (raw == 2) return DecodeStatus::Aborted;
    if (raw == -1) return DecodeStatus::InvalidInput;
    return DecodeStatus::Fatal;
}

inline bool decode_status_is_success(DecodeStatus status) {
    return status == DecodeStatus::Success;
}

inline const char * decode_status_name(DecodeStatus status) {
    switch (status) {
    case DecodeStatus::Success: return "success";
    case DecodeStatus::NoKvSlot: return "no_kv_slot";
    case DecodeStatus::Aborted: return "aborted";
    case DecodeStatus::InvalidInput: return "invalid_input";
    case DecodeStatus::Fatal: return "fatal";
    }
    return "unknown";
}

// NoKvSlot is the only non-success outcome that computed nothing and left
// the cache exactly as it was, so it is the only one where the per-sequence
// path may legitimately redo the identical lap. InvalidInput also restores
// the memory state, but the batch itself was rejected as malformed --
// resubmitting the same rows one at a time cannot succeed either, it would
// just fail again more slowly, so it is not a refusal. Aborted and Fatal
// both leave processed ubatches in the memory state: redoing any of that
// work double-decodes a token that already advanced the KV cache.
inline bool decode_status_is_refusal(DecodeStatus status) {
    return status == DecodeStatus::NoKvSlot;
}

// Whether this outcome leaves the KV cache somewhere rollback cannot undo.
// `rollback_hop_batch` only releases sequences the current HOP newly
// created; it never restores the position of a sequence that already
// existed, so once the cache has advanced past what the caller believes
// happened, nothing in this process can put it back. The stage must refuse
// every later HOP until it is reloaded.
inline bool decode_status_leaves_memory_dirty(DecodeStatus status) {
    return status == DecodeStatus::Aborted || status == DecodeStatus::Fatal;
}

// The line an operator needs when a stage stops taking HOPs after a decode
// failure: what happened, and what to do about it. Shared verbatim between
// the point a failure sets the quarantine and the point a later HOP is
// refused because of it, so the two messages are recognizably the same
// event rather than two things to cross-reference.
inline const char * hop_memory_dirty_message() {
    return "stage KV memory state may be inconsistent after a decode failure; "
           "the deployment must be reloaded before this stage can run another HOP";
}

// The entry guard every HOP execution path checks first. `dirty` is the
// runtime's own hop_memory_dirty_ flag; this function makes the guard itself
// a pure, unit-testable decision instead of an inline `if` repeated at two
// call sites and drifting apart. Returns true (and fills `error`, if given)
// when the caller must refuse without doing anything else; false when it is
// safe to proceed.
inline bool refuse_for_dirty_hop_memory(bool dirty, std::string * error) {
    if (!dirty) return false;
    if (error != nullptr) *error = hop_memory_dirty_message();
    return true;
}

} // namespace staged::llama_runtime
