#include "p4_llama_compat.hpp"
#include <algorithm>
#include <iterator>
#include <string_view>

namespace p4_llama_compat {

namespace {

bool has_model_metadata(const llama_model * model, const char * key) {
    char value[128]{};
    return model != nullptr && llama_model_meta_val_str(model, key, value, sizeof(value)) >= 0;
}

bool model_metadata_equals(const llama_model * model, const char * key, const char * expected) {
    char value[128]{};
    const auto size = model == nullptr ? -1
        : llama_model_meta_val_str(model, key, value, sizeof(value));
    return size >= 0 && std::string_view(value) == expected;
}

} // namespace

MiniMaxM3AttentionMode minimax_m3_attention_mode(const llama_model * model) {
    if (!model_metadata_equals(model, "general.architecture", "minimax-m3")) {
        return MiniMaxM3AttentionMode::NotMiniMaxM3;
    }
    constexpr const char * required[] = {
        "minimax-m3.attention.indexer.head_count",
        "minimax-m3.attention.indexer.key_length",
        "minimax-m3.attention.indexer.top_k",
        "minimax-m3.attention.indexer.block_size",
        "minimax-m3.attention.indexer.local_blocks",
    };
    return std::all_of(std::begin(required), std::end(required),
                       [model](const char * key) { return has_model_metadata(model, key); })
        ? MiniMaxM3AttentionMode::SparseIndexer
        : MiniMaxM3AttentionMode::MissingSparseIndexer;
}

std::string minimax_m3_attention_rejection(
        MiniMaxM3AttentionMode mode,
        bool flash_attention_enabled,
        bool kv_unified,
        int n_parallel) {
    if (mode == MiniMaxM3AttentionMode::NotMiniMaxM3) return {};
    if (mode == MiniMaxM3AttentionMode::MissingSparseIndexer) {
        return "CAPABILITY_UNAVAILABLE: MiniMax M3 GGUF lacks MSA indexer metadata/tensors";
    }
    if (!flash_attention_enabled) {
        return "CAPABILITY_UNAVAILABLE: MiniMax M3 MSA requires flash attention";
    }
    if (n_parallel > 1 && kv_unified) {
        return "CAPABILITY_UNAVAILABLE: MiniMax M3 MSA with multiple sequences requires --no-kv-unified";
    }
    return {};
}

} // namespace p4_llama_compat
