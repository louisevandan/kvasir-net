#include "plan.hpp"

#ifdef P4_STAGED_WITH_LLAMA

#include <cerrno>
#include <cstdlib>
#include <limits>
#include <algorithm>
#include <utility>

namespace staged::server {
namespace {

bool valid_utf8(const std::string &value) {
    for (std::size_t i = 0; i < value.size();) {
        const auto byte = static_cast<unsigned char>(value[i]);
        std::size_t width = 0;
        if (byte <= 0x7f) width = 1;
        else if (byte >= 0xc2 && byte <= 0xdf) width = 2;
        else if (byte >= 0xe0 && byte <= 0xef) width = 3;
        else if (byte >= 0xf0 && byte <= 0xf4) width = 4;
        else return false;
        if (i + width > value.size()) return false;
        for (std::size_t j = 1; j < width; ++j) {
            if ((static_cast<unsigned char>(value[i + j]) & 0xc0) != 0x80) return false;
        }
        if (width == 3) {
            const auto second = static_cast<unsigned char>(value[i + 1]);
            if (byte == 0xe0 && second < 0xa0) return false;
            if (byte == 0xed && second >= 0xa0) return false;
        }
        if (width == 4) {
            const auto second = static_cast<unsigned char>(value[i + 1]);
            if (byte == 0xf0 && second < 0x90) return false;
            if (byte == 0xf4 && second >= 0x90) return false;
        }
        i += width;
    }
    return true;
}

bool option_name_and_value(const std::vector<std::string> &args, std::size_t *index,
                           std::string *name, std::string *value, bool *has_value) {
    if (index == nullptr || name == nullptr || value == nullptr || has_value == nullptr ||
        *index >= args.size()) return false;
    const auto &arg = args[*index];
    const auto equal = arg.find('=');
    *name = equal == std::string::npos ? arg : arg.substr(0, equal);
    if (equal != std::string::npos) {
        *value = arg.substr(equal + 1);
        *has_value = true;
        return true;
    }
    if (*index + 1 < args.size() && args[*index + 1].rfind("--", 0) != 0) {
        *value = args[++*index];
        *has_value = true;
    } else {
        value->clear();
        *has_value = false;
    }
    return true;
}

bool parse_i32_value(const std::string &value, std::int32_t *result,
                     const char *option, std::string *error) {
    if (value.empty()) {
        if (error != nullptr) *error = std::string(option) + " requires a value";
        return false;
    }
    char *end = nullptr;
    errno = 0;
    const auto parsed = std::strtol(value.c_str(), &end, 10);
    if (errno != 0 || end == value.c_str() || *end != '\0' ||
        parsed < std::numeric_limits<std::int32_t>::min() ||
        parsed > std::numeric_limits<std::int32_t>::max()) {
        if (error != nullptr) *error = std::string(option) + " has an invalid value";
        return false;
    }
    *result = static_cast<std::int32_t>(parsed);
    return true;
}

bool parse_memory_topology(
        const std::string & value,
        staged::llama_runtime::MemoryTopology * result,
        std::string * error) {
    using staged::llama_runtime::MemoryTopologyKind;
    if (result->kind != MemoryTopologyKind::Unspecified) {
        if (error != nullptr) *error = "--memory-topology cannot be repeated";
        return false;
    }
    if (value == "discrete") {
        result->kind = MemoryTopologyKind::Discrete;
        return true;
    }
    constexpr const char * prefix = "host-shared:";
    if (value.rfind(prefix, 0) != 0 || value.size() == std::char_traits<char>::length(prefix)) {
        if (error != nullptr) {
            *error = "--memory-topology requires discrete or host-shared:<device-indices>";
        }
        return false;
    }
    std::size_t begin = std::char_traits<char>::length(prefix);
    while (begin < value.size()) {
        const auto end = value.find(',', begin);
        std::int32_t index = -1;
        if (!parse_i32_value(value.substr(begin, end - begin), &index,
                             "--memory-topology", error) || index < 0 ||
            std::find(result->host_shared_devices.begin(),
                      result->host_shared_devices.end(), index) !=
                result->host_shared_devices.end()) {
            if (error != nullptr && (error->empty() || index < 0)) {
                *error = "--memory-topology device indices must be unique and non-negative";
            }
            return false;
        }
        result->host_shared_devices.push_back(index);
        if (end == std::string::npos) break;
        begin = end + 1;
    }
    result->kind = MemoryTopologyKind::HostShared;
    return true;
}

bool stage_only(const std::string &name) {
    return name == "--port" || name == "--bind" ||
           name == "--layer-begin" || name == "--layer-end" ||
           name == "--kv-layer-begin" || name == "--kv-layer-end" ||
           name == "--kv-root" || name == "--model-identity" ||
           name == "--memory-topology" || name == "--expect-layer-device" ||
           name == "--stage-only" || name == "--validate-plan" ||
           name == "--inspect-memory-plan";
}

bool parse_stage_option(const std::string &name, const std::string &value,
                        bool has_value, ParsedLlamaOptions *parsed,
                        std::string *error) {
    if (name == "--port" || name == "--bind") {
        if (!has_value || value.empty()) {
            if (error != nullptr) *error = name + " requires a value";
            return false;
        }
        return true;
    }
    if (name == "--stage-only" || name == "--validate-plan" ||
        name == "--inspect-memory-plan") {
        if (has_value) {
            if (error != nullptr) *error = name + " does not take a value";
            return false;
        }
        if (name == "--validate-plan") parsed->validate_plan = true;
        if (name == "--inspect-memory-plan") parsed->inspect_memory_plan = true;
        return true;
    }
    if (name == "--model") {
        if (!has_value || value.empty()) {
            if (error != nullptr) *error = "--model requires a value";
            return false;
        }
        parsed->model_path = value;
        return true;
    }
    if (name == "--layer-begin") return parse_i32_value(value, &parsed->layer_begin, name.c_str(), error);
    if (name == "--layer-end") return parse_i32_value(value, &parsed->layer_end, name.c_str(), error);
    if (name == "--kv-layer-begin") return parse_i32_value(value, &parsed->kv_layer_begin, name.c_str(), error);
    if (name == "--kv-layer-end") return parse_i32_value(value, &parsed->kv_layer_end, name.c_str(), error);
    if (name == "--kv-root") {
        if (!has_value) { if (error != nullptr) *error = "--kv-root requires a value"; return false; }
        parsed->kv_root = value;
        return true;
    }
    if (name == "--model-identity") {
        if (!has_value) { if (error != nullptr) *error = "--model-identity requires a value"; return false; }
        parsed->model_identity = value;
        return true;
    }
    if (name == "--expect-layer-device") {
        const auto first = value.find(':');
        const auto second = first == std::string::npos ? first : value.find(':', first + 1);
        staged::llama_runtime::LayerDeviceExpectation expectation;
        if (!has_value || first == std::string::npos || second == std::string::npos ||
            !parse_i32_value(value.substr(0, first), &expectation.begin, name.c_str(), error) ||
            !parse_i32_value(value.substr(first + 1, second - first - 1), &expectation.end, name.c_str(), error)) {
            if (error) *error = "--expect-layer-device requires begin:end:backend-device-name";
            return false;
        }
        expectation.device = value.substr(second + 1);
        parsed->layer_device_expectations.push_back(std::move(expectation));
        return true;
    }
    if (name == "--memory-topology") {
        if (!has_value) {
            if (error != nullptr) *error = "--memory-topology requires a value";
            return false;
        }
        return parse_memory_topology(value, &parsed->memory_topology, error);
    }
    return false;
}

bool add_common(std::vector<std::string> *args, const std::string &name,
                const std::string &value, bool has_value) {
    if (name == "--port" || name == "--bind" || stage_only(name)) return true;
    if (name == "--n-seq-max") {
        args->push_back("--parallel");
        if (has_value) args->push_back(value);
        return true;
    }
    args->push_back(name);
    if (has_value) args->push_back(value);
    return true;
}

} // namespace

bool parse_plan_tokens(const std::string &plan, std::vector<std::string> *tokens,
                       std::string *error) {
    if (tokens == nullptr || !valid_utf8(plan)) {
        if (error != nullptr) *error = "startup plan is not valid UTF-8";
        return false;
    }
    tokens->clear();
    for (std::size_t i = 0; i < plan.size();) {
        while (i < plan.size() && static_cast<unsigned char>(plan[i]) <= ' ') ++i;
        if (i == plan.size()) break;
        std::string token;
        bool quoted = false;
        bool had_input = false;
        while (i < plan.size()) {
            const char c = plan[i];
            if (!quoted && static_cast<unsigned char>(c) <= ' ') break;
            if (c == '"') { quoted = !quoted; had_input = true; ++i; continue; }
            if (c == '\\' && i + 1 < plan.size() && plan[i + 1] == '"') {
                token.push_back('"'); had_input = true; i += 2; continue;
            }
            token.push_back(c); had_input = true; ++i;
        }
        if (quoted) {
            if (error != nullptr) *error = "startup plan has an unterminated quote";
            return false;
        }
        if (had_input) tokens->push_back(std::move(token));
    }
    return true;
}

bool parse_llama_options(int argc, char **argv,
                         const std::vector<std::string> &plan_tokens,
                         ParsedLlamaOptions *parsed, std::string *error) {
    if (parsed == nullptr) return false;
    std::vector<std::string> process_args;
    for (int i = 1; i < argc; ++i) process_args.emplace_back(argv[i]);
    std::vector<std::string> common_args{
        "p4_staged_server",
        // On Windows common_params_parse replaces argv with the process
        // command line when argc happens to match. The stage-only process
        // options are intentionally absent from common_args, so keep the
        // count distinct with an inert server option.
        "--log-disable"};
    auto consume = [&](const std::vector<std::string> &args) {
        for (std::size_t i = 0; i < args.size(); ++i) {
            std::string name, value;
            bool has_value = false;
            if (!option_name_and_value(args, &i, &name, &value, &has_value)) return false;
            if (stage_only(name) || name == "--model") {
                if (!parse_stage_option(name, value, has_value, parsed, error)) return false;
            }
            if (!add_common(&common_args, name, value, has_value)) return false;
        }
        return true;
    };
    if (!consume(process_args) || !consume(plan_tokens)) return false;
    if (!staged::llama_runtime::validate_layer_device_expectations(
            parsed->layer_device_expectations, parsed->layer_begin, parsed->layer_end, error)) return false;
    if (parsed->memory_topology.kind ==
        staged::llama_runtime::MemoryTopologyKind::Unspecified) {
        if (error != nullptr) {
            *error = "startup plan requires explicit --memory-topology";
        }
        return false;
    }
    // On Windows common_params_parse reconstructs UTF-8 argv when the
    // supplied argc equals the process command-line argc.  The plan is
    // intentionally supplied through stdin, so that reconstruction would
    // reintroduce the server-only --bind/--port arguments and can reject a
    // valid forwarded option (notably --kv-unified).  Keep the synthetic
    // argv count distinct even when a plan happens to have the same width as
    // the process command line.
    while (static_cast<int>(common_args.size()) == argc) {
        common_args.emplace_back("--log-disable");
    }
    // The argument grammar is llama.cpp's and the call to it lives in the
    // compat unit; this file's business is which arguments to hand over.
    if (!parsed->params.parse_arguments(common_args)) {
        if (error != nullptr) {
            *error = "common_params_parse rejected startup plan; forwarded args=";
            for (const auto & arg : common_args) {
                *error += '[' + arg + ']';
            }
        }
        return false;
    }
    // llama_new_context_with_model rejects this combination after the model
    // and every selected tensor have already been loaded.  It is a plan
    // invariant, not a model-dependent capability: quantized V is consumed by
    // the flash-attention kernels.  Reject it here so an OUTER-supplied opaque
    // plan cannot turn a deterministic argument error into an expensive load
    // failure on every node.
    if (parsed->params.quantized_v_without_flash_attention()) {
        if (error != nullptr) {
            *error = "quantized V cache requires flash attention; use --flash-attn on or an unquantized V cache";
        }
        return false;
    }
    parsed->mtp_requested = parsed->params.requests_draft_mtp();
    parsed->speculative_requested = parsed->params.requests_any_speculative();
    // n_batch is the logical admission width and n_ubatch is llama.cpp's
    // physical graph width. They must remain distinct: the first stage sends
    // one mixed logical batch to llama_decode(), and the compatibility
    // callback captures every exact physical ubatch produced by llama.cpp.
    // Downstream nodes replay those capsules without reconstructing them.
    // Collapsing n_batch to n_ubatch here silently disabled that mechanism.
    //
    // Preserve llama.cpp's requested KV topology.  The physical-v2 boundary
    // captures the exact ubatch positions and sequence memberships produced by
    // llama.cpp and replays that capsule at every downstream stage, so it does
    // not require a unified cache.  Forcing one here also changes model
    // semantics: MiniMax M3 requires per-sequence streams when n_seq_max > 1
    // and otherwise falls back from MSA to dense attention.

    if (parsed->model_path.empty()) parsed->model_path = parsed->params.model_path();
    return true;
}

CapabilityReport capability_report(const ParsedLlamaOptions &parsed) {
    const bool unsupported_speculative = parsed.params.requests_unsupported_speculative();
    const bool mtp_execution = parsed.mtp_requested && !unsupported_speculative;
    std::string blocker = "none";
    if (unsupported_speculative && parsed.params.requests_draft_family()) {
        blocker = "draft_context_and_proposal_state_not_in_hop";
    } else if (unsupported_speculative) {
        blocker = "proposal_accept_rollback_state_not_in_hop";
    }
    return CapabilityReport{
        true, true, mtp_execution, true, mtp_execution, true,
        parsed.mtp_requested, parsed.speculative_requested, std::move(blocker)};
}

std::string CapabilityReport::serialize() const {
    return "mtp_parser=" + std::string(mtp_parser ? "1" : "0") +
           ";mtp_auxiliary_ownership=" +
           std::string(mtp_auxiliary_ownership ? "1" : "0") +
           ";mtp_execution=" + std::string(mtp_execution ? "1" : "0") +
           ";speculative_parser=" + std::string(speculative_parser ? "1" : "0") +
           ";speculative_execution=" + std::string(speculative_execution ? "1" : "0") +
           ";normal_decode_execution=" +
           std::string(normal_decode_execution ? "1" : "0") +
           ";mtp_requested=" + std::string(mtp_requested ? "1" : "0") +
           ";speculative_requested=" +
           std::string(speculative_requested ? "1" : "0") +
           ";execution_blocker=" + execution_blocker;
}

} // namespace staged::server
#endif
