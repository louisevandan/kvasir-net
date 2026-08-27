cmake_minimum_required(VERSION 3.20)

project(linker_tokenizer_probe LANGUAGES CXX)

set(TOKENIZER_SOURCE_DIR "" CACHE PATH
    "Prepared patched llama.cpp source tree used for the artifact")
set(TOKENIZER_ARTIFACT_DIR "" CACHE PATH
    "Runtime directory containing llama.dll and llama-common.dll")
set(TOKENIZER_BUILD_DIR "" CACHE PATH
    "CMake build tree that produced TOKENIZER_ARTIFACT_DIR")
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

if(TOKENIZER_ARTIFACT_DIR AND TOKENIZER_BUILD_DIR)
    foreach(required IN ITEMS
            "${TOKENIZER_ARTIFACT_DIR}/llama.dll"
            "${TOKENIZER_ARTIFACT_DIR}/llama-common.dll"
            "${TOKENIZER_BUILD_DIR}/llama-pipeline/src/Release/llama.lib"
            "${TOKENIZER_BUILD_DIR}/llama-pipeline/common/Release/llama-common.lib")
        if(NOT EXISTS "${required}")
            message(FATAL_ERROR "chat template probe dependency not found: ${required}")
        endif()
    endforeach()
    add_library(tokenizer_llama SHARED IMPORTED)
    set_target_properties(tokenizer_llama PROPERTIES
        IMPORTED_LOCATION "${TOKENIZER_ARTIFACT_DIR}/llama.dll"
        IMPORTED_IMPLIB "${TOKENIZER_BUILD_DIR}/llama-pipeline/src/Release/llama.lib")
    add_library(tokenizer_common SHARED IMPORTED)
    set_target_properties(tokenizer_common PROPERTIES
        IMPORTED_LOCATION "${TOKENIZER_ARTIFACT_DIR}/llama-common.dll"
        IMPORTED_IMPLIB "${TOKENIZER_BUILD_DIR}/llama-pipeline/common/Release/llama-common.lib")
    add_executable(linker-chat-template-probe llama-chat-template.cpp)
    target_compile_features(linker-chat-template-probe PRIVATE cxx_std_17)
    target_include_directories(linker-chat-template-probe PRIVATE
        "${TOKENIZER_SOURCE_DIR}/common"
        "${TOKENIZER_SOURCE_DIR}/vendor"
        "${TOKENIZER_SOURCE_DIR}/include"
        "${TOKENIZER_SOURCE_DIR}/ggml/include"
    )
    target_link_libraries(linker-chat-template-probe PRIVATE tokenizer_common tokenizer_llama)
    if(MSVC)
        target_compile_options(linker-chat-template-probe PRIVATE /utf-8)
    endif()
endif()
