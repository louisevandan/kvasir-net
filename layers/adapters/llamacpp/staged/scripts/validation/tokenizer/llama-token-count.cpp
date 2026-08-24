#include "llama.h"

#if defined(_WIN32)
#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

struct Options {
    std::filesystem::path artifact_directory;
    std::filesystem::path model;
    std::filesystem::path output;
    std::filesystem::path seed_file;
    std::filesystem::path required_suffix_file;
    int target_tokens = 5000;
};

#if defined(_WIN32)
template <typename Function>
Function load_symbol(HMODULE library, const char * name) {
    auto address = GetProcAddress(library, name);
    if (address == nullptr) {
        throw std::runtime_error(std::string("missing llama.dll symbol: ") + name);
    }
    return reinterpret_cast<Function>(address);
}
#endif

Options parse_options(int argc, wchar_t ** argv) {
    Options options;
    for (int index = 1; index < argc; ++index) {
        const std::wstring argument = argv[index];
        auto value = [&](const wchar_t * name) {
            if (argument != name || index + 1 >= argc) {
                return std::filesystem::path();
            }
            return std::filesystem::path(argv[++index]);
        };
        if (auto value_path = value(L"--artifact-directory"); !value_path.empty()) {
            options.artifact_directory = value_path;
        } else if (auto value_path = value(L"--model"); !value_path.empty()) {
            options.model = value_path;
        } else if (auto value_path = value(L"--output"); !value_path.empty()) {
            options.output = value_path;
        } else if (auto value_path = value(L"--seed-file"); !value_path.empty()) {
            options.seed_file = value_path;
        } else if (auto value_path = value(L"--required-suffix-file"); !value_path.empty()) {
            options.required_suffix_file = value_path;
        } else if (argument == L"--target" && index + 1 < argc) {
            options.target_tokens = std::stoi(argv[++index]);
        } else {
            throw std::runtime_error("usage: --artifact-directory <dir> --model <gguf> --output <file> --target <n> [--seed-file <txt>] [--required-suffix-file <txt>]");
        }
    }
    if (options.artifact_directory.empty() || options.model.empty() ||
        options.output.empty() || options.target_tokens <= 0) {
        throw std::runtime_error("artifact directory, model, output, and positive target are required");
    }
    return options;
}

std::string read_utf8(const std::filesystem::path & file) {
    std::ifstream input(file, std::ios::binary);
    if (!input) throw std::runtime_error("could not open prompt: " + file.string());
    std::ostringstream contents;
    contents << input.rdbuf();
    return contents.str();
}

std::string repeat(const std::string & value, int count) {
    std::string result;
    result.reserve(value.size() * static_cast<size_t>(count));
    for (int index = 0; index < count; ++index) result += value;
    return result;
}

std::vector<std::string> suffixes() {
    std::vector<std::string> result{""};
    std::string repeated_space_a;
    for (int count = 1; count <= 32; ++count) {
        repeated_space_a += " a";
        result.push_back(repeated_space_a);
    }
    const std::string alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
    for (char first : alphabet) result.push_back(std::string(" ") + first);
    for (char first : alphabet) {
        for (char second : alphabet) {
            result.push_back(std::string(" ") + first + second);
        }
    }
    return result;
}

struct LlamaApi {
#if defined(_WIN32)
    HMODULE library = nullptr;
    decltype(&llama_model_default_params) model_default_params = nullptr;
    decltype(&llama_backend_init) backend_init = nullptr;
    decltype(&llama_model_load_from_file) model_load_from_file = nullptr;
    decltype(&llama_model_get_vocab) model_get_vocab = nullptr;
    decltype(&llama_vocab_get_add_bos) vocab_get_add_bos = nullptr;
    decltype(&llama_tokenize) tokenize = nullptr;
#endif
};

LlamaApi load_api(const std::filesystem::path & artifact_directory) {
#if defined(_WIN32)
    SetDllDirectoryW(artifact_directory.c_str());
    const auto library_path = artifact_directory / L"llama.dll";
    auto library = LoadLibraryW(library_path.c_str());
    if (library == nullptr) {
        throw std::runtime_error("could not load artifact llama.dll: " + library_path.string());
    }
    LlamaApi api;
    api.library = library;
    api.model_default_params = load_symbol<decltype(api.model_default_params)>(library, "llama_model_default_params");
    api.backend_init = load_symbol<decltype(api.backend_init)>(library, "llama_backend_init");
    api.model_load_from_file = load_symbol<decltype(api.model_load_from_file)>(library, "llama_model_load_from_file");
    api.model_get_vocab = load_symbol<decltype(api.model_get_vocab)>(library, "llama_model_get_vocab");
    api.vocab_get_add_bos = load_symbol<decltype(api.vocab_get_add_bos)>(library, "llama_vocab_get_add_bos");
    api.tokenize = load_symbol<decltype(api.tokenize)>(library, "llama_tokenize");
    return api;
#else
    (void) artifact_directory;
    throw std::runtime_error("this validation probe currently requires Windows llama.dll");
#endif
}

int token_count(const LlamaApi & api, const llama_vocab * vocab, const std::string & text, bool add_bos) {
    if (text.size() > static_cast<size_t>(std::numeric_limits<int32_t>::max())) {
        throw std::runtime_error("prompt is too large for llama_tokenize");
    }
    const auto text_length = static_cast<int32_t>(text.size());
    const auto required = api.tokenize(vocab, text.data(), text_length, nullptr, 0, add_bos, true);
    if (required == std::numeric_limits<int32_t>::min()) throw std::runtime_error("tokenization overflow");
    const auto capacity = required < 0 ? -required : required;
    if (capacity == 0) return 0;
    std::vector<llama_token> tokens(static_cast<size_t>(capacity));
    const auto actual = api.tokenize(vocab, text.data(), text_length, tokens.data(), capacity, add_bos, true);
    if (actual < 0) throw std::runtime_error("tokenization failed after sizing pass");
    return actual;
}

std::string make_fixture(const LlamaApi & api, const llama_vocab * vocab, bool add_bos,
                         int target, int & repetitions, std::string & suffix) {
    const std::string prefix = "Deterministic tokenizer calibration for the staged CUDA artifact.\n\n";
    const std::string unit = "The patched pipeline keeps this sentence stable while validating the prompt boundary. ";
    int low = 0;
    int high = 1;
    auto candidate = [&](int count, const std::string & tail) {
        return prefix + repeat(unit, count) + tail;
    };
    while (token_count(api, vocab, candidate(high, ""), add_bos) < target) {
        low = high;
        high *= 2;
        if (high > 1'000'000) throw std::runtime_error("could not reach target token count");
    }
    while (low + 1 < high) {
        const int middle = low + (high - low) / 2;
        if (token_count(api, vocab, candidate(middle, ""), add_bos) < target) low = middle;
        else high = middle;
    }
    const auto tails = suffixes();
    for (int count = low; count >= std::max(0, low - 4); --count) {
        for (const auto & tail : tails) {
            auto text = candidate(count, tail);
            if (token_count(api, vocab, text, add_bos) == target) {
                repetitions = count;
                suffix = tail;
                return text;
            }
        }
    }
    throw std::runtime_error("deterministic suffix search did not find an exact token count");
}

std::string utf8_prefix(const std::string & text, size_t length) {
    length = std::min(length, text.size());
    while (length > 0 && (static_cast<unsigned char>(text[length - 1]) & 0xc0) == 0x80) {
        --length;
    }
    return text.substr(0, length);
}

std::string make_seed_fixture(const LlamaApi & api, const llama_vocab * vocab, bool add_bos,
                              const std::string & seed, const std::string & required_suffix,
                              int target, int & repetitions, std::string & suffix) {
    if (seed.empty()) throw std::runtime_error("seed file is empty");
    size_t low = 0;
    size_t high = seed.size();
    auto candidate = [&](size_t length, const std::string & adjustment) {
        return utf8_prefix(seed, length) + adjustment + required_suffix;
    };
    if (token_count(api, vocab, required_suffix, add_bos) > target) {
        throw std::runtime_error("required suffix has more tokens than target");
    }
    if (token_count(api, vocab, candidate(high, ""), add_bos) < target) {
        throw std::runtime_error("seed plus required suffix has fewer tokens than target");
    }
    while (low + 1 < high) {
        const auto middle = low + (high - low) / 2;
        if (token_count(api, vocab, candidate(middle, ""), add_bos) < target) {
            low = middle;
        } else {
            high = middle;
        }
    }
    const auto tails = suffixes();
    const auto lower = low > 8 ? low - 8 : 0;
    for (size_t length = high;; --length) {
        for (const auto & tail : tails) {
            auto text = candidate(length, tail);
            if (token_count(api, vocab, text, add_bos) == target) {
                repetitions = 1;
                suffix = tail;
                return text;
            }
        }
        if (length == lower) break;
    }
    throw std::runtime_error("seed prefix/adjustment/required-suffix search did not find an exact token count");
}

} // namespace

#if defined(_WIN32)
int wmain(int argc, wchar_t ** argv) {
#else
int main(int argc, char ** argv) {
#endif
    try {
#if defined(_WIN32)
        const auto options = parse_options(argc, argv);
#else
        (void) argc;
        (void) argv;
        throw std::runtime_error("Windows artifact probe required");
#endif
        auto api = load_api(options.artifact_directory);
        api.backend_init();
        auto model_params = api.model_default_params();
        model_params.vocab_only = true;
        auto model = api.model_load_from_file(options.model.string().c_str(), model_params);
        if (model == nullptr) throw std::runtime_error("vocab-only GGUF load failed");
        const auto * vocab = api.model_get_vocab(model);
        const bool add_bos = api.vocab_get_add_bos(vocab);
        int repetitions = 0;
        std::string suffix;
        const auto required_suffix = options.required_suffix_file.empty()
            ? std::string()
            : read_utf8(options.required_suffix_file);
        const auto fixture = options.seed_file.empty()
            ? make_fixture(api, vocab, add_bos, options.target_tokens, repetitions, suffix)
            : make_seed_fixture(api, vocab, add_bos, read_utf8(options.seed_file),
                                required_suffix, options.target_tokens, repetitions, suffix);
        std::filesystem::create_directories(options.output.parent_path());
        std::ofstream output(options.output, std::ios::binary);
        if (!output) throw std::runtime_error("could not create fixture: " + options.output.string());
        output.write(fixture.data(), static_cast<std::streamsize>(fixture.size()));
        output.close();
        const auto verified = token_count(api, vocab, read_utf8(options.output), add_bos);
        if (verified != options.target_tokens) throw std::runtime_error("fixture verification mismatch");
        std::cout << "TOKEN_COUNT=" << verified << "\n"
                  << "PROMPT_BYTES=" << fixture.size() << "\n"
                  << "ADD_BOS=" << (add_bos ? 1 : 0) << "\n"
                  << "PARSE_SPECIAL=1\n"
                  << "UNIT_REPETITIONS=" << repetitions << "\n"
                  << "SUFFIX_BYTES=" << suffix.size() << "\n"
                  << "SEED_FILE=" << options.seed_file.string() << "\n"
                  << "REQUIRED_SUFFIX_FILE=" << options.required_suffix_file.string() << "\n"
                  << "VOCAB_ONLY=1\n"
                  << "INFERENCE=not-run\n"
                  << "EXPLICIT_UNLOAD=not-called\n";
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "tokenizer-probe: " << error.what() << "\n";
        return 1;
    }
}
