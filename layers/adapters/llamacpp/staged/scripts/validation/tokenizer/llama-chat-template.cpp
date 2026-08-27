#include "chat.h"
#include "llama.h"
#include "nlohmann/json.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>

namespace {

std::string read_utf8(const std::filesystem::path & path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("could not open messages file");
    std::ostringstream contents;
    contents << input.rdbuf();
    return contents.str();
}

void write_utf8(const std::filesystem::path & path, const std::string & value) {
    if (!path.parent_path().empty()) std::filesystem::create_directories(path.parent_path());
    std::ofstream output(path, std::ios::binary);
    if (!output) throw std::runtime_error("could not create rendered prompt");
    output.write(value.data(), static_cast<std::streamsize>(value.size()));
}

} // namespace

#if defined(_WIN32)
int wmain(int argc, wchar_t ** argv) {
#else
int main(int argc, char ** argv) {
#endif
    try {
        if (argc != 4) throw std::runtime_error("usage: MODEL.gguf MESSAGES.json OUTPUT.txt");
        const std::filesystem::path model_path(argv[1]);
        const std::filesystem::path messages_path(argv[2]);
        const std::filesystem::path output_path(argv[3]);
        const auto messages_json = nlohmann::ordered_json::parse(read_utf8(messages_path));
        if (!messages_json.is_array() || messages_json.empty()) {
            throw std::runtime_error("messages must be a non-empty JSON array");
        }

        llama_log_set([](ggml_log_level, const char *, void *) {}, nullptr);
        llama_backend_init();
        auto params = llama_model_default_params();
        params.vocab_only = true;
        auto * model = llama_model_load_from_file(model_path.string().c_str(), params);
        if (model == nullptr) throw std::runtime_error("vocab-only GGUF load failed");
        try {
            auto templates = common_chat_templates_init(model, "");
            common_chat_templates_inputs inputs;
            inputs.add_generation_prompt = true;
            inputs.use_jinja = true;
            for (const auto & value : messages_json) {
                if (!value.is_object() || !value.contains("role") || !value.contains("content")
                    || !value["role"].is_string() || !value["content"].is_string()) {
                    throw std::runtime_error("every message needs string role and content");
                }
                common_chat_msg message;
                message.role = value["role"].get<std::string>();
                message.content = value["content"].get<std::string>();
                inputs.messages.push_back(std::move(message));
            }
            const auto rendered = common_chat_templates_apply(templates.get(), inputs).prompt;
            if (rendered.empty()) throw std::runtime_error("chat template returned an empty prompt");
            write_utf8(output_path, rendered);
            std::cout << "CHAT_TEMPLATE_CHARS=" << rendered.size() << "\n"
                      << "CHAT_TEMPLATE_SOURCE=model-metadata-jinja\n";
        } catch (...) {
            llama_model_free(model);
            throw;
        }
        llama_model_free(model);
        llama_backend_free();
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "chat-template-probe: " << error.what() << "\n";
        return 1;
    }
}
