#include "llama_stage_runtime.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <process.h>
#include <string>

namespace {

const char * environment_value(const char * name) {
    const auto * value = std::getenv(name);
    return value != nullptr && *value != '\0' ? value : nullptr;
}

void real_decode_after_restore_regression() {
    const auto * model_path = environment_value("P4_STAGED_LLAMA_MODEL");
    if (model_path == nullptr) {
        std::cout << "SKIP: set P4_STAGED_LLAMA_MODEL for real KV restore regression\n";
        return;
    }

    staged::llama_runtime::LoadConfig config;
    config.model_path = model_path;
    config.model_identity = model_path;
    config.layer_begin = 0;
    config.layer_end = 28;
    const auto root = std::filesystem::temp_directory_path() /
        ("p4-staged-kv-restore-regression-" + std::to_string(
            static_cast<unsigned long long>(::_getpid())));
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root);
    config.kv_root = root.string();

    common_params params;
    params.n_ctx = 512;
    params.n_batch = 512;
    params.n_ubatch = 512;
    params.n_parallel = 1;
    params.n_sequences = 1;
    params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    params.sampling.temp = 0.0f;

    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    assert(runtime.load(params, config, &error) && error.empty());

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

    common_prompt_checkpoint native_checkpoint;
    assert(runtime.save_checkpoint(prefill.sequence_id, &native_checkpoint, &error));
    assert(!native_checkpoint.empty());
    assert(native_checkpoint.n_tokens == static_cast<int64_t>(*cut.n_tokens));

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
              << " n_tokens=" << native_checkpoint.n_tokens << "\n";

    staged::protocol::KvPayload request;
    request.sequence_id = prefill.sequence_id;
    request.cache_key = "kv-restore-regression";
    request.model_identity = config.model_identity;
    request.stage_begin = config.layer_begin;
    request.stage_end = config.layer_end;
    staged::protocol::KvResult saved;
    assert(runtime.save(request, &saved, &error) && error.empty());
    assert(llama_memory_seq_pos_max(memory, sequence) == -1);

    assert(runtime.restore(request, &saved, &error) && error.empty());
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
    config.model_path = model_path;
    config.model_identity = model_path;
    config.layer_begin = 0;
    config.layer_end = 28;

    common_params params;
    params.n_ctx = 512;
    params.n_batch = 512;
    params.n_ubatch = 512;
    params.n_parallel = 2;
    params.n_sequences = 2;
    params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    params.sampling.temp = 0.0f;

    staged::llama_runtime::StageRuntime runtime;
    std::string error;
    assert(runtime.load(params, config, &error) && error.empty());
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
    common_params params;
    params.speculative.types = {COMMON_SPECULATIVE_TYPE_DRAFT_MTP};
    staged::llama_runtime::LoadConfig config;
    config.model_path = "not-loaded.gguf";
    config.layer_begin = 0;
    config.layer_end = 1;
    std::string error;
    assert(!runtime.load(params, config, &error));
    assert(error.find("CAPABILITY_UNAVAILABLE") != std::string::npos);
    assert(error.find("mtp_auxiliary_layers_not_owned_by_stage") !=
           std::string::npos);
    assert(!runtime.loaded());
    real_decode_after_restore_regression();
    hop_batch_rolls_back_only_new_sequences();
    return 0;
}
