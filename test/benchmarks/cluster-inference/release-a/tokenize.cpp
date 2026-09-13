// Public llama vocabulary probe. Link headers and library from the same sealed runtime.
#include "llama.h"
#include <cstdint>
#include <fstream>
#include <iostream>
#include <iterator>
#include <limits>
#include <string>
#include <vector>

int main(int argc, char ** argv) {
    if (argc != 2) return 2;
    auto params = llama_model_default_params();
    params.vocab_only = true;
    params.n_gpu_layers = 0;
    auto * model = llama_model_load_from_file(argv[1], params);
    if (!model) return 3;
    const auto * vocab = llama_model_get_vocab(model);
    std::string line;
    while (std::getline(std::cin, line)) {
        const auto separator = line.find('\t');
        const auto input = line.substr(0, separator);
        const auto output = separator == std::string::npos ? std::string() : line.substr(separator + 1);
        std::ifstream file(input, std::ios::binary);
        if (!file) return 4;
        const std::string text((std::istreambuf_iterator<char>(file)), {});
        if (text.size() > static_cast<std::size_t>(std::numeric_limits<std::int32_t>::max())) return 5;
        const auto required = llama_tokenize(vocab, text.data(), static_cast<int32_t>(text.size()), nullptr, 0, true, true);
        if (required >= 0 || required == std::numeric_limits<int32_t>::min()) return 6;
        std::vector<llama_token> tokens(static_cast<std::size_t>(-required));
        const auto actual = llama_tokenize(vocab, text.data(), static_cast<int32_t>(text.size()), tokens.data(),
            static_cast<int32_t>(tokens.size()), true, true);
        if (actual != -required) return 7;
        if (!output.empty()) {
            std::ofstream out(output, std::ios::binary);
            if (!out) return 8;
            for (const auto token : tokens) {
                const auto value = static_cast<std::uint32_t>(token);
                for (unsigned shift = 0; shift < 32; shift += 8) out.put(static_cast<char>(value >> shift));
            }
            if (!out) return 9;
        }
        std::cout << actual << std::endl;
    }
    llama_model_free(model);
    return 0;
}
