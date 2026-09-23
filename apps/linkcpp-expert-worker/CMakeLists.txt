# linkcpp-expert-worker: the MoE expert-parallel worker (M1).
# Pure ggml/gguf — no libllama — it loads an expert-sliced mini-GGUF and runs
# the per-expert FFN graph (mul_mat_id x3 + swiglu). Backend-agnostic: CUDA,
# ROCm, Metal, Vulkan, CPU.

add_executable(linkcpp-expert-worker main.cpp)

# Whether this build computes through cuBLAS rather than ggml's own quantized
# kernels. The worker cannot ask ggml at runtime — GGML_CUDA_FORCE_CUBLAS is a
# ggml-internal define and does not reach here — but the answer changes what the
# worker must do: an MMQ build has to keep every graph under the batch-width
# cliff, and a cuBLAS build must NOT, because chunking would throw away exactly
# the speed the larger binary was chosen for. See MMVQ_MAX_BATCH_SIZE.
# (Applied to linkcpp-expert further down, once that target exists: CMake
# refuses definitions for a target not yet declared.)
if (GGML_CUDA_FORCE_CUBLAS)
    target_compile_definitions(linkcpp-expert-worker PRIVATE LINKCPP_CUDA_CUBLAS)
endif()

target_link_libraries(linkcpp-expert-worker PRIVATE ggml)
if (WIN32)
    target_link_libraries(linkcpp-expert-worker PRIVATE ws2_32)
endif()
if (MSVC)
    # Sources are UTF-8; without this MSVC reads them in the system code page.
    target_compile_options(linkcpp-expert-worker PRIVATE /utf-8)
endif()
target_include_directories(linkcpp-expert-worker PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
set_target_properties(linkcpp-expert-worker PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)

# linkcpp-expert: the same worker as a static library (expert_api.h). Mobile
# apps (iOS cannot exec bundled binaries) link this and call linkcpp_expert_run()
# from a worker thread instead of spawning the binary — mirrors linkcpp-stage.
add_library(linkcpp-expert STATIC main.cpp)
target_compile_definitions(linkcpp-expert PRIVATE LINKCPP_EXPERT_NO_MAIN)
if (GGML_CUDA_FORCE_CUBLAS)
    target_compile_definitions(linkcpp-expert PRIVATE LINKCPP_CUDA_CUBLAS)
endif()
target_link_libraries(linkcpp-expert PRIVATE ggml)
if (WIN32)
    target_link_libraries(linkcpp-expert INTERFACE ws2_32)
endif()
if (MSVC)
    target_compile_options(linkcpp-expert PRIVATE /utf-8)
endif()
target_include_directories(linkcpp-expert PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
target_include_directories(linkcpp-expert INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})
set_target_properties(linkcpp-expert PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
