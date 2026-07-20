# linkcpp-expert-worker: the MoE expert-parallel worker (M1).
# Pure ggml/gguf — no libllama — it loads an expert-sliced mini-GGUF and runs
# the per-expert FFN graph (mul_mat_id x3 + swiglu). Backend-agnostic: CUDA,
# ROCm, Metal, Vulkan, CPU.

add_executable(linkcpp-expert-worker main.cpp)

target_link_libraries(linkcpp-expert-worker PRIVATE ggml)
target_include_directories(linkcpp-expert-worker PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
set_target_properties(linkcpp-expert-worker PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)

# linkcpp-expert: the same worker as a static library (expert_api.h). Mobile
# apps (iOS cannot exec bundled binaries) link this and call linkcpp_expert_run()
# from a worker thread instead of spawning the binary — mirrors linkcpp-stage.
add_library(linkcpp-expert STATIC main.cpp)
target_compile_definitions(linkcpp-expert PRIVATE LINKCPP_EXPERT_NO_MAIN)
target_link_libraries(linkcpp-expert PRIVATE ggml)
target_include_directories(linkcpp-expert PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
target_include_directories(linkcpp-expert INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})
set_target_properties(linkcpp-expert PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
