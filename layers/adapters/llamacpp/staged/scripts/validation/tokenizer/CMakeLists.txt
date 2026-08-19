cmake_minimum_required(VERSION 3.20)

project(linker_tokenizer_probe LANGUAGES CXX)

set(TOKENIZER_SOURCE_DIR "" CACHE PATH
    "Prepared patched llama.cpp source tree used for the artifact")
if(NOT EXISTS "${TOKENIZER_SOURCE_DIR}/include/llama.h")
    message(FATAL_ERROR
        "TOKENIZER_SOURCE_DIR must point to a prepared llama.cpp source tree")
endif()

add_executable(linker-tokenizer-probe llama-token-count.cpp)
target_compile_features(linker-tokenizer-probe PRIVATE cxx_std_17)
target_include_directories(linker-tokenizer-probe PRIVATE
    "${TOKENIZER_SOURCE_DIR}/include"
    "${TOKENIZER_SOURCE_DIR}/ggml/include"
)
if(MSVC)
    target_compile_options(linker-tokenizer-probe PRIVATE /utf-8)
endif()
