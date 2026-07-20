# linkcpp-node: the per-node orchestrator binary.
# At M0 it is a build-wiring smoke test that links libllama and reports the
# runtime/backends it sees. It grows into the pipeline driver in later milestones.

add_executable(linkcpp-node main.cpp)

target_link_libraries(linkcpp-node PRIVATE llama-common llama ggml)
target_compile_definitions(linkcpp-node PRIVATE LINKCPP_RING_BUILD_ID="${LINKCPP_RING_BUILD_ID}")
target_include_directories(linkcpp-node PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/common
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/include
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
set_target_properties(linkcpp-node PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)

# linkcpp-stage: the same stage runner as a static library (stage_api.h). Mobile
# apps link this — iOS cannot exec a bundled binary — and run the stage on a
# worker thread via linkcpp_stage_run().
add_library(linkcpp-stage STATIC main.cpp)
target_link_libraries(linkcpp-stage PRIVATE llama-common llama ggml)
target_compile_definitions(linkcpp-stage
    PRIVATE LINKCPP_RING_BUILD_ID="${LINKCPP_RING_BUILD_ID}" LINKCPP_STAGE_NO_MAIN)
target_include_directories(linkcpp-stage PRIVATE
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/common
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/include
    ${CMAKE_SOURCE_DIR}/external/llama.cpp/ggml/include
)
target_include_directories(linkcpp-stage INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})
set_target_properties(linkcpp-stage PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
