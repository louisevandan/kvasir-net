#include "p4_llama_compat.hpp"

// The one permitted crossing. See the header for why it is only here.
#include "llama-ext.h"

namespace p4_llama_compat {

std::vector<MemoryBreakdownEntry> memory_breakdown(const llama_context * context) {
    std::vector<MemoryBreakdownEntry> entries;
    if (context == nullptr) return entries;
    const llama_memory_breakdown breakdown = llama_get_memory_breakdown(context);
    entries.reserve(breakdown.size());
    for (const auto & [buffer_type, memory] : breakdown) {
        entries.push_back(MemoryBreakdownEntry{buffer_type, memory.model, memory.context, memory.compute});
    }
    return entries;
}

std::size_t model_device_count(const llama_model * model) {
    if (model == nullptr) return 0;
    const int32_t count = llama_model_n_devices(model);
    return count > 0 ? static_cast<std::size_t>(count) : 0;
}

ggml_backend_dev_t model_device(const llama_model * model, std::size_t index) {
    if (index >= model_device_count(model)) return nullptr;
    return llama_model_get_device(model, static_cast<int>(index));
}

}  // namespace p4_llama_compat
