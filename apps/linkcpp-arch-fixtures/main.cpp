// Reuse llama.cpp's maintained synthetic architecture fixtures without
// copying its per-model metadata/tensor rules into linkcpp.
#define main llama_cpp_arch_test_main
#include "../../external/llama.cpp/tests/test-llama-archs.cpp"
#undef main

#include <filesystem>

namespace fs = std::filesystem;

static bool fixture_uses_encoder(llm_arch arch) {
    return arch == LLM_ARCH_T5 || arch == LLM_ARCH_DREAM || arch == LLM_ARCH_LLADA
        || arch == LLM_ARCH_LLADA_MOE || arch == LLM_ARCH_RND1;
}

int main(int argc, char ** argv) {
    std::string arch_name;
    std::string output_dir;
    size_t seed = 1;
    for (int i = 1; i < argc; ++i) {
        if ((!strcmp(argv[i], "-a") || !strcmp(argv[i], "--arch")) && i + 1 < argc) {
            arch_name = argv[++i];
        } else if ((!strcmp(argv[i], "-o") || !strcmp(argv[i], "--out")) && i + 1 < argc) {
            output_dir = argv[++i];
        } else if ((!strcmp(argv[i], "-s") || !strcmp(argv[i], "--seed")) && i + 1 < argc) {
            seed = std::stoull(argv[++i]);
        } else {
            fprintf(stderr, "usage: %s --arch NAME --out DIR [--seed N]\n", argv[0]);
            return 2;
        }
    }
    const llm_arch arch = llm_arch_from_string(arch_name);
    if (arch == LLM_ARCH_UNKNOWN || output_dir.empty()) return 2;
    fs::create_directories(output_dir);
    common_init();
    llama_backend_init();
    int generated = 0;
    try {
        if (!arch_supported(arch)) {
            fprintf(stderr, "llama.cpp synthetic evaluator does not support %s\n", arch_name.c_str());
            llama_backend_free();
            return 3;
        }
        for (bool moe : {false, true}) {
            if ((moe && !moe_implemented(arch)) || (!moe && moe_mandatory(arch))) continue;
            if (!llama_model_saver_supports_arch(arch)) continue;
            gguf_context_ptr gguf_ctx = get_gguf_ctx(arch, moe);
            auto model_and_ctx = get_model_and_ctx(
                gguf_ctx.get(), nullptr, seed, {}, LLAMA_SPLIT_MODE_LAYER,
                fixture_uses_encoder(arch));
            const std::vector<llama_token> tokens = get_tokens(4, 128, seed);
            const std::vector<float> logits = get_logits(
                model_and_ctx.first.get(), model_and_ctx.second.get(), tokens,
                fixture_uses_encoder(arch));
            const uint32_t n_vocab = llama_vocab_n_tokens(
                llama_model_get_vocab(model_and_ctx.first.get()));
            if (logits.size() < n_vocab || n_vocab == 0) {
                throw std::runtime_error("synthetic model did not produce a terminal output");
            }
            const auto begin = logits.end() - n_vocab;
            const int32_t argmax = (int32_t) std::distance(
                begin, std::max_element(begin, logits.end()));
            const std::string stem = arch_name + (moe ? "-moe" : "-dense");
            const fs::path model_path = fs::path(output_dir) / (stem + ".gguf");
            const fs::path golden_path = fs::path(output_dir) / (stem + ".golden.json");
            llama_model_save_to_file(model_and_ctx.first.get(), model_path.string().c_str());
            FILE * golden = fopen(golden_path.string().c_str(), "wb");
            if (!golden) throw std::runtime_error("failed to create golden sidecar");
            fprintf(golden,
                    "{\"schema\":1,\"architecture\":\"%s\",\"moe\":%s,"
                    "\"tokens\":[%d,%d,%d,%d],\"argmax\":%d}\n",
                    arch_name.c_str(), moe ? "true" : "false",
                    tokens[0], tokens[1], tokens[2], tokens[3], argmax);
            fclose(golden);
            ++generated;
        }
    } catch (const std::exception & error) {
        fprintf(stderr, "fixture generation failed for %s: %s\n", arch_name.c_str(), error.what());
        llama_backend_free();
        return 1;
    }
    llama_backend_free();
    return generated > 0 ? 0 : 3;
}
