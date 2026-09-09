#include "llama_stage_runtime.hpp"
#include "compat/p4_llama_compat_internal.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string>

#ifdef _WIN32
#include <process.h>
#else
#include <unistd.h>
#endif

namespace staged::llama_runtime {

// Defines the friend declared in llama_stage_runtime.hpp. See that
// declaration for why this exists instead of a public setter: it is the
// narrowest way to observe hop_memory_dirty_'s wiring (the entry guards in
// execute_hop/execute_decode_batch, and unload() clearing it) without a real
// llama_context, since every production path that sets the flag requires
// one.
void test_force_hop_memory_dirty(StageRuntime & runtime, bool value) noexcept {
    runtime.hop_memory_dirty_ = value;
}

} // namespace staged::llama_runtime

void run_stage_memory_plan_tests();

namespace {

unsigned long long process_id() {
#ifdef _WIN32
    return static_cast<unsigned long long>(::_getpid());
#else
    return static_cast<unsigned long long>(::getpid());
#endif
}

const char * environment_value(const char * name) {
    const auto * value = std::getenv(name);
    return value != nullptr && *value != '\0' ? value : nullptr;
}

// Real coverage for the StageRuntime wiring around hop_memory_dirty_, not
// just the pure decode_status.hpp functions runtime_test.cpp already
// exercises without llama.h. These three run on a never-loaded StageRuntime
// with the flag forced true through the test-only friend above -- the entry
// guards sit before any llama.cpp call, so no real model is required. What
// is NOT covered here: the tail-split check in
// llama_stage_runtime_hop_decode_split.cpp reverting from a refusal back to
// fail_hop() after a successful llama_decode(). That path only exists once a
// real llama_context has produced logits for a partial-stage cut-set; there
// is no way to reach it without one, and building a fake context would
// verify the fake rather than the real wiring. Accepted as uncovered by this
// suite; the SKIP-gated regressions below are the closest thing to it and
// need P4_STAGED_LLAMA_MODEL to run at all.
void execute_decode_batch_refuses_when_hop_memory_dirty() {
    staged::llama_runtime::StageRuntime runtime;
    assert(!runtime.loaded());
    staged::llama_runtime::test_force_hop_memory_dirty(runtime, true);

    const std::vector<staged::protocol::SequencePayload> inputs(2);
    std::vector<staged::protocol::SequencePayload> outputs;
    std::string error;
    assert(!runtime.execute_decode_batch(inputs, &outputs, &error));
    assert(error.find("reload") != std::string::npos);
}

void execute_hop_refuses_when_hop_memory_dirty() {
    staged::llama_runtime::StageRuntime runtime;
    assert(!runtime.loaded());
    staged::llama_runtime::test_force_hop_memory_dirty(runtime, true);

    staged::protocol::SequencePayload input;
    staged::protocol::SequencePayload output;
    std::string error;
    assert(!runtime.execute_hop(
        input, staged::protocol::HopPhase::Prefill, &output, &error));
    assert(error.find("reload") != std::string::npos);
}

void unload_clears_hop_memory_dirty() {
    staged::llama_runtime::StageRuntime runtime;
    staged::llama_runtime::test_force_hop_memory_dirty(runtime, true);
    assert(runtime.hop_memory_dirty());
    runtime.unload();
    assert(!runtime.hop_memory_dirty());
}

void quarantine_marks_hop_memory_dirty() {
    staged::llama_runtime::StageRuntime runtime;
    assert(!runtime.hop_memory_dirty());
    runtime.quarantine_physical_memory();
    assert(runtime.hop_memory_dirty());
}

// A quarantined runtime must not let KvSave/KvRestore/KvDrop through either
// (llama_stage_runtime_kv.cpp): a decode failure gives no way to tell which
// sequence's KV content is suspect, so persisting or reloading any of them
// while dirty could round-trip torn state through disk. Same unloaded +
// forced-dirty seam as the HOP guards above; each of save/restore/drop
// checks hop_memory_dirty_ before its own loaded() check, so no real model
// is needed here either.
void kv_operations_refuse_when_hop_memory_dirty() {
    staged::llama_runtime::StageRuntime runtime;
    assert(!runtime.loaded());
    staged::llama_runtime::test_force_hop_memory_dirty(runtime, true);

    const staged::protocol::KvPayload request;
    staged::protocol::KvResult result;
    std::string error;

    error.clear();
    assert(!runtime.save(request, &result, &error));
    assert(error.find("reload") != std::string::npos);

    error.clear();
    assert(!runtime.restore(request, &result, &error));
    assert(error.find("reload") != std::string::npos);

    error.clear();
    assert(!runtime.drop(request, &result, &error));
    assert(error.find("reload") != std::string::npos);
}

void real_decode_after_restore_regression() {
    const auto * model_path = environment_value("P4_STAGED_LLAMA_MODEL");
    if (model_path == nullptr) {
        std::cout << "SKIP: set P4_STAGED_LLAMA_MODEL for real KV restore regression\n";
        return;
    }

    staged::llama_runtime::LoadConfig config;
    config.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    config.model_path = model_path;
    config.model_identity = model_path;
    config.layer_begin = 0;
    config.layer_end = 28;
    const auto root = std::filesystem::temp_directory_path() /
        ("p4-staged-kv-restore-regression-" + std::to_string(process_id()));
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root);
    config.kv_root = root.string();

    p4_llama_compat::LlamaPlan plan;
    common_params & params = p4_llama_compat::plan_params(plan);
    params.n_ctx = 512;
    params.n_batch = 512;
    params.n_ubatch = 512;
    params.n_parallel = 1;
    params.n_sequences = 1;
    params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    params.sampling.temp = 0.0f;

    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    assert(runtime.load(std::move(plan), config, &error) && error.empty());

    staged::protocol::SequencePayload prefill;
    prefill.sequence_id = "kv-restore-regression";
    prefill.prompt = "Reply with one short word: hello";
    prefill.position = 0;
    staged::protocol::SequencePayload cut;
    assert(runtime.execute_hop(prefill, staged::protocol::HopPhase::Prefill,
                                &cut, &error) && error.empty());
    assert(cut.n_tokens.has_value() && *cut.n_tokens > 0);
    const auto sequence = static_cast<llama_seq_id>(0);
    const auto memory = llama_get_memory(runtime.context());
    const auto prefill_max = llama_memory_seq_pos_max(memory, sequence);
    assert(prefill_max == static_cast<llama_pos>(*cut.n_tokens) - 1);

    p4_llama_compat::PromptCheckpoint native_checkpoint;
    assert(runtime.save_checkpoint(prefill.sequence_id, &native_checkpoint, &error));
    assert(!native_checkpoint.empty());
    assert(native_checkpoint.n_tokens() == static_cast<int64_t>(*cut.n_tokens));

    staged::protocol::SequencePayload checkpoint_decode = cut;
    checkpoint_decode.descriptors.clear();
    checkpoint_decode.payloads.clear();
    checkpoint_decode.prompt.reset();
    checkpoint_decode.initial_tokens.reset();
    checkpoint_decode.n_tokens = 1;
    checkpoint_decode.position = static_cast<std::uint32_t>(*cut.n_tokens);
    staged::protocol::SequencePayload checkpoint_result;
    assert(runtime.execute_hop(checkpoint_decode, staged::protocol::HopPhase::Decode,
                                &checkpoint_result, &error) && error.empty());
    assert(runtime.restore_checkpoint(prefill.sequence_id, native_checkpoint, &error));
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max);
    staged::protocol::SequencePayload restored_checkpoint_result;
    assert(runtime.execute_hop(checkpoint_decode, staged::protocol::HopPhase::Decode,
                                &restored_checkpoint_result, &error) && error.empty());
    assert(restored_checkpoint_result.outcome.has_value());
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max + 1);
    assert(runtime.restore_checkpoint(prefill.sequence_id, native_checkpoint, &error));
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max);
    std::cout << "NATIVE_CONTEXT_CHECKPOINT_RESTORE_OK"
              << " bytes=" << native_checkpoint.size()
              << " n_tokens=" << native_checkpoint.n_tokens() << "\n";

    staged::protocol::KvPayload request;
    request.sequence_id = prefill.sequence_id;
    request.cache_key = "kv-restore-regression";
    request.model_identity = config.model_identity;
    request.stage_begin = config.layer_begin;
    request.stage_end = config.layer_end;
    staged::protocol::KvResult saved;
    assert(runtime.save(request, &saved, &error) && error.empty());
    assert(llama_memory_seq_pos_max(memory, sequence) == -1);

    if (!runtime.restore(request, &saved, &error) || !error.empty()) {
        std::cerr << "KV_RESTORE_FAILED error=" << error << "\n";
        std::abort();
    }
    const auto restored_max = llama_memory_seq_pos_max(memory, sequence);
    assert(restored_max == prefill_max);

    staged::protocol::SequencePayload decode = cut;
    // This regression uses a full-range tail stage, so it has no incoming
    // hidden-state cut-set. The prefill result may contain an output
    // descriptor for a partial stage; do not feed that descriptor back into
    // the full model during the next-token check.
    decode.descriptors.clear();
    decode.payloads.clear();
    decode.prompt.reset();
    decode.initial_tokens.reset();
    decode.n_tokens = 1;
    decode.position = static_cast<std::uint32_t>(*cut.n_tokens);
    staged::protocol::SequencePayload baseline;
    assert(runtime.execute_hop(decode, staged::protocol::HopPhase::Decode,
                                &baseline, &error) && error.empty());
    assert(baseline.outcome.has_value());
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max + 1);

    assert(runtime.restore(request, &saved, &error) && error.empty());
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max);
    staged::protocol::SequencePayload restored;
    assert(runtime.execute_hop(decode, staged::protocol::HopPhase::Decode,
                                &restored, &error) && error.empty());
    assert(restored.outcome->token == baseline.outcome->token);
    assert(restored.outcome->text == baseline.outcome->text);
    assert(restored.outcome->position == baseline.outcome->position);
    assert(restored.outcome->stop == baseline.outcome->stop);
    assert(llama_memory_seq_pos_max(memory, sequence) == prefill_max + 1);

    std::cout << "KV_RESTORE_NEXT_TOKEN_EQUIVALENT"
              << " token=" << *baseline.outcome->token
              << " position=" << baseline.outcome->position
              << " prefill_max=" << prefill_max
              << " restored_max=" << restored_max << "\n";
    staged::protocol::KvResult dropped;
    assert(runtime.drop(request, &dropped, &error) && error.empty());
    runtime.unload();
    std::filesystem::remove_all(root);
}

void hop_batch_rolls_back_only_new_sequences() {
    const auto * model_path = environment_value("P4_STAGED_LLAMA_MODEL");
    if (model_path == nullptr) {
        std::cout << "SKIP: set P4_STAGED_LLAMA_MODEL for HOP batch rollback regression\n";
        return;
    }

    staged::llama_runtime::LoadConfig config;
    config.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    config.model_path = model_path;
    config.model_identity = model_path;
    config.layer_begin = 0;
    config.layer_end = 28;

    p4_llama_compat::LlamaPlan plan;
    common_params & params = p4_llama_compat::plan_params(plan);
    params.n_ctx = 512;
    params.n_batch = 512;
    params.n_ubatch = 512;
    params.n_parallel = 2;
    params.n_sequences = 2;
    params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    params.sampling.temp = 0.0f;

    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    assert(runtime.load(std::move(plan), config, &error) && error.empty());
    assert(llama_n_seq_max(runtime.context()) == 2);

    staged::protocol::SequencePayload existing;
    existing.sequence_id = "existing-sequence";
    existing.prompt = "Reply with one short word: baseline";
    existing.position = 0;
    staged::protocol::SequencePayload existing_output;
    assert(runtime.execute_hop(existing, staged::protocol::HopPhase::Prefill,
                                &existing_output, &error) && error.empty());

    runtime.begin_hop_batch();
    staged::protocol::SequencePayload added;
    added.sequence_id = "new-sequence";
    added.prompt = "Reply with one short word: added";
    added.position = 0;
    staged::protocol::SequencePayload output;
    assert(runtime.execute_hop(added, staged::protocol::HopPhase::Prefill,
                               &output, &error) && error.empty());

    staged::protocol::SequencePayload over_capacity;
    over_capacity.sequence_id = "over-capacity";
    over_capacity.prompt = "Reply with one short word: overflow";
    over_capacity.position = 0;
    assert(!runtime.execute_hop(over_capacity, staged::protocol::HopPhase::Prefill,
                                &output, &error));
    assert(error.find("sequence table is full") != std::string::npos);
    error.clear();

    std::string rollback_error;
    assert(runtime.rollback_hop_batch(&rollback_error));
    assert(rollback_error.empty());

    // The pre-existing mapping remains usable after rollback.
    staged::protocol::SequencePayload existing_decode;
    existing_decode.sequence_id = existing.sequence_id;
    existing_decode.n_tokens = 1;
    existing_decode.position = existing_output.position;
    assert(runtime.execute_hop(existing_decode, staged::protocol::HopPhase::Decode,
                                &output, &error) && error.empty());

    // The newly allocated mapping was released and can be allocated again.
    added.position = 0;
    assert(runtime.execute_hop(added, staged::protocol::HopPhase::Prefill,
                               &output, &error) && error.empty());
    runtime.unload();
}

} // namespace

int main() {
    staged::llama_runtime::StageRuntime runtime;
    assert(!runtime.loaded());
    p4_llama_compat::LlamaPlan plan;
    common_params & params = p4_llama_compat::plan_params(plan);
    params.speculative.types = {COMMON_SPECULATIVE_TYPE_DRAFT_MTP};
    staged::llama_runtime::LoadConfig config;
    config.memory_topology.kind =
        staged::llama_runtime::MemoryTopologyKind::Discrete;
    config.model_path = "not-loaded.gguf";
    config.layer_begin = 0;
    config.layer_end = 1;
    std::string error;
    assert(!runtime.load(std::move(plan), config, &error));
    assert(error == "llama.cpp failed to create the no-alloc model plan");
    assert(!runtime.loaded());
    execute_decode_batch_refuses_when_hop_memory_dirty();
    execute_hop_refuses_when_hop_memory_dirty();
    unload_clears_hop_memory_dirty();
    quarantine_marks_hop_memory_dirty();
    run_stage_memory_plan_tests();
    kv_operations_refuse_when_hop_memory_dirty();
    real_decode_after_restore_regression();
    hop_batch_rolls_back_only_new_sequences();
    return 0;
}
