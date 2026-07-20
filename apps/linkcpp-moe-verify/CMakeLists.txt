# linkcpp-moe-verify: functional check of the build_moe_ffn expert-dispatch hook.
add_executable(linkcpp-moe-verify main.cpp)
target_link_libraries(linkcpp-moe-verify PRIVATE llama ggml)
target_include_directories(linkcpp-moe-verify PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/include
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
set_target_properties(linkcpp-moe-verify PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
