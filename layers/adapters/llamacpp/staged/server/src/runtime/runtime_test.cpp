#include "runtime.hpp"
#include "decode_status.hpp"
#include <cassert>
#include <cstdint>
#include <string>
#include <vector>

using staged::runtime::ErrorCode;
using staged::runtime::Operation;
using staged::runtime::Runtime;
using staged::runtime::State;

using staged::llama_runtime::DecodeStatus;
using staged::llama_runtime::decode_status_from_raw;
using staged::llama_runtime::decode_status_is_refusal;
using staged::llama_runtime::decode_status_is_success;
using staged::llama_runtime::decode_status_leaves_memory_dirty;
using staged::llama_runtime::refuse_for_dirty_hop_memory;

namespace {

void feed_plan(Runtime &runtime, const std::string &plan) {
    const auto size = static_cast<std::uint32_t>(plan.size());
    const std::vector<std::uint8_t> header{
        static_cast<std::uint8_t>(size),
        static_cast<std::uint8_t>(size >> 8U),
        static_cast<std::uint8_t>(size >> 16U),
        static_cast<std::uint8_t>(size >> 24U)};
    const std::vector<std::uint8_t> body(plan.begin(), plan.end());
    assert(runtime.stdin_plan().feed(header.data(), header.size()).ok());
    assert(runtime.stdin_plan().feed(body.data(), body.size()).ok());
}

void test_plan_stays_live() {
    Runtime runtime;
    feed_plan(runtime, "model=small");
    assert(runtime.state() == State::AwaitingPlan);
    assert(runtime.hello().ok());
    assert(runtime.state() == State::Ready);
    assert(runtime.stdin_eof().error == ErrorCode::UnexpectedEof);
    assert(runtime.state() == State::Failed);
}

void test_operation_order() {
    Runtime runtime;
    feed_plan(runtime, "plan");
    assert(runtime.hello().ok());
    assert(runtime.begin_hop().ok());
    assert(runtime.begin_hop().error == ErrorCode::OperationInProgress);
    assert(runtime.begin_kv(Operation::KvSave).error
           == ErrorCode::OperationInProgress);
    assert(runtime.unload().error == ErrorCode::OperationInProgress);
    assert(runtime.cancel().ok());
    assert(runtime.begin_kv(Operation::KvRestore).ok());
    assert(runtime.finish_kv().ok());
    assert(runtime.unload().ok());
    assert(runtime.finish_unload().ok());
    assert(runtime.state() == State::Unloaded);
}

void test_invalid_order_is_terminal() {
    Runtime runtime;
    assert(runtime.begin_hop().error == ErrorCode::InvalidState);
    assert(runtime.state() == State::AwaitingPlan);
}

// decode_status.hpp's mapping and quarantine guard, exercised here rather
// than in a llama-gated test file because it is deliberately dependency-free
// (no llama.h) -- everything the real StageRuntime (llama_stage_runtime.cpp,
// only buildable with a llama.cpp checkout) decides from a DecodeStatus is
// exercised through the same free functions its HOP paths call.

// The raw int32_t -> DecodeStatus mapping, one case per line of
// llama_decode()'s documented contract (upstream/include/llama.h).
void test_decode_status_raw_mapping() {
    assert(decode_status_from_raw(0) == DecodeStatus::Success);
    assert(decode_status_from_raw(1) == DecodeStatus::NoKvSlot);
    assert(decode_status_from_raw(2) == DecodeStatus::Aborted);
    assert(decode_status_from_raw(-1) == DecodeStatus::InvalidInput);
    assert(decode_status_from_raw(-2) == DecodeStatus::Fatal);
    // Anything further negative is still Fatal; llama.cpp's contract only
    // promises "< -1", not a specific floor.
    assert(decode_status_from_raw(-1000000) == DecodeStatus::Fatal);
    // A value the documented contract does not name (e.g. some future
    // positive code) still has to land somewhere rather than be undefined;
    // Fatal is the fail-closed choice, not a claim that 3 means what 2 does.
    assert(decode_status_from_raw(3) == DecodeStatus::Fatal);
}

void test_decode_status_is_success() {
    assert(decode_status_is_success(DecodeStatus::Success));
    assert(!decode_status_is_success(DecodeStatus::NoKvSlot));
    assert(!decode_status_is_success(DecodeStatus::Aborted));
    assert(!decode_status_is_success(DecodeStatus::InvalidInput));
    assert(!decode_status_is_success(DecodeStatus::Fatal));
}

// Only NoKvSlot computed nothing and left the cache untouched, so only it is
// a refusal the per-sequence path may legitimately retry into. Every other
// non-success status -- including InvalidInput, which also restores the
// memory state -- is a failure: see decode_status.hpp for why InvalidInput
// specifically must not be treated as a refusal.
void test_decode_status_refusal_classification() {
    assert(decode_status_is_refusal(DecodeStatus::NoKvSlot));
    assert(!decode_status_is_refusal(DecodeStatus::Success));
    assert(!decode_status_is_refusal(DecodeStatus::Aborted));
    assert(!decode_status_is_refusal(DecodeStatus::InvalidInput));
    assert(!decode_status_is_refusal(DecodeStatus::Fatal));
}

// Aborted and Fatal are exactly the two outcomes llama.cpp documents as
// leaving processed ubatches in the memory state; NoKvSlot and InvalidInput
// both restore it, and Success never advanced anything abnormally.
void test_decode_status_memory_dirty_classification() {
    assert(decode_status_leaves_memory_dirty(DecodeStatus::Aborted));
    assert(decode_status_leaves_memory_dirty(DecodeStatus::Fatal));
    assert(!decode_status_leaves_memory_dirty(DecodeStatus::Success));
    assert(!decode_status_leaves_memory_dirty(DecodeStatus::NoKvSlot));
    assert(!decode_status_leaves_memory_dirty(DecodeStatus::InvalidInput));
}

// The entry guard every HOP execution path (execute_hop,
// execute_decode_batch) checks first, before doing anything else.
void test_decode_status_dirty_guard_refuses_with_message() {
    std::string error;
    assert(refuse_for_dirty_hop_memory(true, &error) == true);
    assert(!error.empty());
    // An operator reading only this line should understand why the stage
    // stopped taking HOPs.
    assert(error.find("reload") != std::string::npos);
}

void test_decode_status_clean_guard_lets_the_caller_proceed() {
    std::string error;
    assert(refuse_for_dirty_hop_memory(false, &error) == false);
    // A clean runtime must not have anything written into `error` by the
    // guard itself -- the caller has not failed yet.
    assert(error.empty());
}

void test_decode_status_guard_tolerates_null_error() {
    // Every StageRuntime call site passes a real std::string*, but the
    // guard is a general-purpose helper and must not crash if one does not.
    assert(refuse_for_dirty_hop_memory(true, nullptr) == true);
    assert(refuse_for_dirty_hop_memory(false, nullptr) == false);
}

// StageRuntime keeps hop_memory_dirty_ as a private bool set by
// decode_status_leaves_memory_dirty() (or by a post-decode step failing) and
// cleared only in unload(), which load() always calls first -- see
// llama_stage_runtime.cpp. Building a real StageRuntime needs llama.h, so
// this test stands in for that lifecycle using the same pure functions
// StageRuntime calls, simulating the two transitions unload() and a decode
// failure are each responsible for:
//
//   1. A decode outcome that leaves the memory dirty sets the flag, and the
//      guard then refuses every later HOP.
//   2. Reload (unload(), simulated here by resetting the local bool to
//      false, exactly as llama_stage_runtime.cpp's unload() does to the
//      real field) clears it, and the guard lets HOPs through again.
//
// What this cannot exercise without a real llama_context is that
// StageRuntime::unload() is actually wired to flip the real field -- that is
// a one-line assignment verified by reading llama_stage_runtime.cpp, not by
// running it here.
void test_decode_status_quarantine_lifecycle() {
    bool hop_memory_dirty = false;

    // A clean runtime takes a HOP.
    {
        std::string error;
        assert(!refuse_for_dirty_hop_memory(hop_memory_dirty, &error));
    }

    // That HOP's decode aborts, leaving ubatches processed in the memory
    // state. The runtime marks itself dirty.
    hop_memory_dirty = hop_memory_dirty || decode_status_leaves_memory_dirty(DecodeStatus::Aborted);
    assert(hop_memory_dirty);

    // Every later HOP on this runtime is refused until reload.
    {
        std::string error;
        assert(refuse_for_dirty_hop_memory(hop_memory_dirty, &error));
        assert(!error.empty());
    }
    {
        std::string another_error;
        assert(refuse_for_dirty_hop_memory(hop_memory_dirty, &another_error));
    }

    // Reload: StageRuntime::unload() clears hop_memory_dirty_ unconditionally.
    hop_memory_dirty = false;
    {
        std::string error;
        assert(!refuse_for_dirty_hop_memory(hop_memory_dirty, &error));
        assert(error.empty());
    }
}

} // namespace

int main() {
    test_plan_stays_live();
    test_operation_order();
    test_invalid_order_is_terminal();
    test_decode_status_raw_mapping();
    test_decode_status_is_success();
    test_decode_status_refusal_classification();
    test_decode_status_memory_dirty_classification();
    test_decode_status_dirty_guard_refuses_with_message();
    test_decode_status_clean_guard_lets_the_caller_proceed();
    test_decode_status_guard_tolerates_null_error();
    test_decode_status_quarantine_lifecycle();
    return 0;
}
