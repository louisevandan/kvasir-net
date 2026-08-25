#include "plan.hpp"

#ifdef P4_STAGED_WITH_LLAMA

#include <cerrno>
#include <cstdlib>
#include <limits>
#include <algorithm>
#include <utility>

#include "arg.h"

namespace staged::server {
namespace {

bool has_speculative_type(const common_params &params,
                          common_speculative_type type) {
    return std::find(params.speculative.types.begin(),
                     params.speculative.types.end(), type) !=
           params.speculative.types.end();
}

// Which family a requested (non-MTP) speculative type belongs to, for the
// capability report's blocker classification below. This has to key off the
// requested *type*, not common_params_speculative::has_dft(): that flag is
// only true once a --model-draft path has actually been resolved, so at
// parse time (or in a test that requests draft-simple without ever
// supplying a draft model) it reads false and silently misclassifies every
// draft-family request as an ngram-family one.
bool has_draft_family_type(const common_params &params) {
    return has_speculative_type(params, COMMON_SPECULATIVE_TYPE_DRAFT_SIMPLE) ||
           has_speculative_type(params, COMMON_SPECULATIVE_TYPE_DRAFT_EAGLE3) ||
           has_speculative_type(params, COMMON_SPECULATIVE_TYPE_DRAFT_DFLASH) ||
           has_speculative_type(params, COMMON_SPECULATIVE_TYPE_DRAFT_DSPARK);
}

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

bool stage_only(const std::string &name) {
    return name == "--port" || name == "--bind" ||
           name == "--layer-begin" || name == "--layer-end" ||
           name == "--kv-layer-begin" || name == "--kv-layer-end" ||
           name == "--kv-root" || name == "--model-identity" ||
           name == "--stage-only" || name == "--validate-plan";
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
    if (name == "--stage-only" || name == "--validate-plan") {
        if (has_value) {
            if (error != nullptr) *error = name + " does not take a value";
            return false;
        }
        if (name == "--validate-plan") parsed->validate_plan = true;
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
    std::vector<char *> pointers;
    pointers.reserve(common_args.size() + 1);
    for (auto &arg : common_args) pointers.push_back(arg.data());
    pointers.push_back(nullptr);
    if (!common_params_parse(static_cast<int>(common_args.size()), pointers.data(),
                             parsed->params, LLAMA_EXAMPLE_SERVER, nullptr)) {
        if (error != nullptr) {
            *error = "common_params_parse rejected startup plan; forwarded args=";
            for (const auto & arg : common_args) {
                *error += '[' + arg + ']';
            }
        }
        return false;
    }
    parsed->mtp_requested = has_speculative_type(
        parsed->params, COMMON_SPECULATIVE_TYPE_DRAFT_MTP);
    parsed->speculative_requested = parsed->mtp_requested ||
        parsed->params.speculative.has_dft();
    for (const auto type : parsed->params.speculative.types) {
        if (type != COMMON_SPECULATIVE_TYPE_NONE) {
            parsed->speculative_requested = true;
        }
    }
    // n_batch is the logical admission width and n_ubatch is llama.cpp's
    // physical graph width. They must remain distinct: the first stage sends
    // one mixed logical batch to llama_decode(), and the compatibility
    // callback captures every exact physical ubatch produced by llama.cpp.
    // Downstream nodes replay those capsules without reconstructing them.
    // Collapsing n_batch to n_ubatch here silently disabled that mechanism.
    //
    // Keep the unified cache because a physical capsule may contain rows from
    // several active sequences and must retain the same sequence membership
    // at every stage.
    parsed->params.kv_unified = true;

    if (parsed->model_path.empty()) parsed->model_path = parsed->params.model.path;
    return true;
}

CapabilityReport capability_report(const ParsedLlamaOptions &parsed) {
    const bool unsupported_speculative = parsed.params.speculative.has_dft()
        || std::any_of(
            parsed.params.speculative.types.begin(),
            parsed.params.speculative.types.end(),
            [](const common_speculative_type type) {
                return type != COMMON_SPECULATIVE_TYPE_NONE
                    && type != COMMON_SPECULATIVE_TYPE_DRAFT_MTP;
            });
    const bool mtp_execution = parsed.mtp_requested && !unsupported_speculative;
    std::string blocker = "none";
    if (unsupported_speculative && has_draft_family_type(parsed.params)) {
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
