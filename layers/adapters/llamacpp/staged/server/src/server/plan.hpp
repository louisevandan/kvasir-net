#pragma once

#include <string>
#include <vector>

#ifdef P4_STAGED_WITH_LLAMA
#include "compat/p4_llama_compat.hpp"
#include "stage_memory_plan.hpp"

namespace staged::server {

struct ParsedLlamaOptions final {
    // Opaque: sixty-six of this plan's fields are consumed by llama.cpp's
    // own conversion functions and only six are named by P4, so the struct
    // is carried whole rather than mirrored. See LlamaPlan.
    p4_llama_compat::LlamaPlan params;
    std::string model_path;
    std::int32_t layer_begin = 0;
    std::int32_t layer_end = 0;
    std::int32_t kv_layer_begin = 0;
    std::int32_t kv_layer_end = 0;
    std::string kv_root;
    std::string model_identity;
    staged::llama_runtime::MemoryTopology memory_topology;
    std::vector<staged::llama_runtime::LayerDeviceExpectation> layer_device_expectations;
    bool validate_plan = false;
    bool inspect_memory_plan = false;
    bool mtp_requested = false;
    bool speculative_requested = false;
};

struct CapabilityReport final {
    bool mtp_parser = false;
    bool mtp_auxiliary_ownership = false;
    bool mtp_execution = false;
    bool speculative_parser = false;
    bool speculative_execution = false;
    bool normal_decode_execution = false;
    bool mtp_requested = false;
    bool speculative_requested = false;
    // Machine-readable reason why parser-visible speculative options are
    // rejected by the current staged HOP. This is startup telemetry, not a
    // shared P4 wire error code.
    std::string execution_blocker = "none";

    [[nodiscard]] std::string serialize() const;
};

bool parse_plan_tokens(const std::string &plan,
                       std::vector<std::string> *tokens,
                       std::string *error);

bool parse_llama_options(int argc, char **argv,
                         const std::vector<std::string> &plan_tokens,
                         ParsedLlamaOptions *parsed,
                         std::string *error);

[[nodiscard]] CapabilityReport capability_report(const ParsedLlamaOptions &parsed);

} // namespace staged::server
#endif
