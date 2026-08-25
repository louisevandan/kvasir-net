cmake_minimum_required(VERSION 3.20)

project(p4_llamacpp_staged_server LANGUAGES CXX)

include(CTest)
enable_testing()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Keep the protocol/state-machine tests buildable without a llama.cpp checkout.
# The real runtime is enabled by passing the prepared compat worktree produced
# by prepare-pipeline-upstream.mjs. The source tree is never modified here.
set(P4_STAGED_LLAMA_SOURCE_DIR "" CACHE PATH
    "Prepared, patched llama.cpp source tree for the real stage runtime")
set(P4_STAGED_LLAMA_BUILD_DIR "" CACHE PATH
    "Existing llama.cpp build root (llama-pipeline) for a staged-only relink")
option(P4_STAGED_BUILD_LLAMA "Build the C++ stage runtime against llama.cpp" ON)
option(P4_STAGED_CUDA "Enable the CUDA backend for the staged llama runtime" OFF)

# The outer build config is Release (/O2 /DNDEBUG per CMAKE_CXX_FLAGS_RELEASE),
# which turns every <cassert> assert() into a no-op. A prior verification pass
# found 192 asserts across 7 of the 9 test files silently disarmed by this:
# ctest reporting all-green proved only that nothing crashed, not that the
# assertions held. Force assertions live on every test binary from this one
# place so a newly added test file cannot silently lose them again the way
# per-file #undef NDEBUG (present in only 2 of the 9 files) already had.
function(p4_stage_enable_asserts target)
    target_compile_options(${target} PRIVATE
        $<$<CXX_COMPILER_ID:MSVC>:/UNDEBUG>
        $<$<NOT:$<CXX_COMPILER_ID:MSVC>>:-UNDEBUG>)
endfunction()

add_library(p4_staged_protocol STATIC
    src/protocol/protocol.cpp
)
target_include_directories(p4_staged_protocol PUBLIC src/protocol)

add_library(p4_staged_runtime STATIC
    src/runtime/runtime.cpp
    src/runtime/state_store.cpp
    src/runtime/state_store_checksum.cpp
    src/runtime/transaction_store.cpp
)
target_include_directories(p4_staged_runtime PUBLIC
    src/runtime
    src/protocol
)

add_library(p4_staged_server_core STATIC
    src/server/server.cpp
    src/server/server_session.cpp
    src/server/server_hop.cpp
    src/server/server_physical.cpp
    src/server/plan.cpp
)
target_include_directories(p4_staged_server_core PUBLIC
    src/server
    src/protocol
    src/runtime
)
target_link_libraries(p4_staged_server_core PUBLIC
    p4_staged_protocol
    p4_staged_runtime
)

add_executable(p4_staged_server
    src/main.cpp
)
target_link_libraries(p4_staged_server PRIVATE p4_staged_server_core)
if(WIN32)
    target_link_libraries(p4_staged_server PRIVATE ws2_32)
endif()

add_executable(p4_staged_server_test
    src/server/server_test.cpp
)
target_link_libraries(p4_staged_server_test PRIVATE p4_staged_server_core)
p4_stage_enable_asserts(p4_staged_server_test)
add_test(NAME p4_staged_server_test COMMAND p4_staged_server_test)

add_executable(p4_staged_protocol_test
    src/protocol/protocol_test.cpp
)
target_link_libraries(p4_staged_protocol_test PRIVATE p4_staged_protocol)
p4_stage_enable_asserts(p4_staged_protocol_test)
add_test(NAME p4_staged_protocol_test COMMAND p4_staged_protocol_test)

add_executable(p4_staged_runtime_test
    src/runtime/runtime_test.cpp
)
target_link_libraries(p4_staged_runtime_test PRIVATE p4_staged_runtime)
p4_stage_enable_asserts(p4_staged_runtime_test)
add_test(NAME p4_staged_runtime_test COMMAND p4_staged_runtime_test)

add_executable(p4_staged_state_store_test
    src/runtime/state_store_test.cpp
)
target_link_libraries(p4_staged_state_store_test PRIVATE p4_staged_runtime)
p4_stage_enable_asserts(p4_staged_state_store_test)
add_test(NAME p4_staged_state_store_test COMMAND p4_staged_state_store_test)

if(P4_STAGED_BUILD_LLAMA AND P4_STAGED_LLAMA_SOURCE_DIR)
    if(NOT EXISTS "${P4_STAGED_LLAMA_SOURCE_DIR}/CMakeLists.txt")
        message(FATAL_ERROR "P4_STAGED_LLAMA_SOURCE_DIR is not a llama.cpp source tree: ${P4_STAGED_LLAMA_SOURCE_DIR}")
    endif()

    execute_process(
        COMMAND git -C "${P4_STAGED_LLAMA_SOURCE_DIR}" rev-parse HEAD
        OUTPUT_VARIABLE P4_STAGED_LLAMA_UPSTREAM_COMMIT
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET
        RESULT_VARIABLE P4_STAGED_LLAMA_GIT_RESULT
    )
    if(NOT P4_STAGED_LLAMA_GIT_RESULT EQUAL 0
       OR P4_STAGED_LLAMA_UPSTREAM_COMMIT STREQUAL "")
        set(P4_STAGED_LLAMA_UPSTREAM_COMMIT "unknown")
    endif()

    if(P4_STAGED_LLAMA_BUILD_DIR)
        # A relink deliberately reuses only an already-built llama.cpp ABI.
        # The source tree is still required for headers and provenance, but
        # no upstream target or CUDA template is regenerated in this build.
        set(_p4_llama_root "${P4_STAGED_LLAMA_BUILD_DIR}")
        foreach(_p4_required
            "${_p4_llama_root}/src/Release/llama.lib"
            "${_p4_llama_root}/common/Release/llama-common.lib"
            "${_p4_llama_root}/ggml/src/Release/ggml-base.lib"
            "${_p4_llama_root}/ggml/src/Release/ggml-cpu.lib"
            "${_p4_llama_root}/ggml/src/Release/ggml.lib")
            if(NOT EXISTS "${_p4_required}")
                message(FATAL_ERROR "P4_STAGED_LLAMA_BUILD_DIR is missing: ${_p4_required}")
            endif()
        endforeach()

        get_filename_component(_p4_llama_cache_root "${_p4_llama_root}" DIRECTORY)
        set(_p4_runtime_dir "${_p4_llama_cache_root}/Release")
        # The runtime directory is normally supplied beside the imported
        # libraries. It may be overridden when the cache came from elsewhere.
        set(P4_STAGED_LLAMA_RUNTIME_DIR "${_p4_runtime_dir}" CACHE PATH
            "DLL directory matching P4_STAGED_LLAMA_BUILD_DIR")
        foreach(_p4_runtime llama.dll llama-common.dll ggml.dll ggml-base.dll ggml-cpu.dll)
            if(NOT EXISTS "${P4_STAGED_LLAMA_RUNTIME_DIR}/${_p4_runtime}")
                message(FATAL_ERROR "P4_STAGED_LLAMA_RUNTIME_DIR is missing: ${P4_STAGED_LLAMA_RUNTIME_DIR}/${_p4_runtime}")
            endif()
        endforeach()

        set(_p4_inc
            "${P4_STAGED_LLAMA_SOURCE_DIR}/include"
            "${P4_STAGED_LLAMA_SOURCE_DIR}/common"
            "${P4_STAGED_LLAMA_SOURCE_DIR}/vendor"
            "${P4_STAGED_LLAMA_SOURCE_DIR}/ggml/include"
            "${P4_STAGED_LLAMA_SOURCE_DIR}/ggml/src")
        foreach(_p4_shared llama llama-common ggml ggml-base ggml-cpu)
            add_library(${_p4_shared} SHARED IMPORTED GLOBAL)
        endforeach()
        set_target_properties(llama PROPERTIES
            IMPORTED_IMPLIB "${_p4_llama_root}/src/Release/llama.lib"
            IMPORTED_LOCATION "${P4_STAGED_LLAMA_RUNTIME_DIR}/llama.dll"
            INTERFACE_INCLUDE_DIRECTORIES "${_p4_inc}")
        set_target_properties(llama-common PROPERTIES
            IMPORTED_IMPLIB "${_p4_llama_root}/common/Release/llama-common.lib"
            IMPORTED_LOCATION "${P4_STAGED_LLAMA_RUNTIME_DIR}/llama-common.dll"
            INTERFACE_INCLUDE_DIRECTORIES "${_p4_inc}")
        set_target_properties(ggml PROPERTIES
            IMPORTED_IMPLIB "${_p4_llama_root}/ggml/src/Release/ggml.lib"
            IMPORTED_LOCATION "${P4_STAGED_LLAMA_RUNTIME_DIR}/ggml.dll"
            INTERFACE_INCLUDE_DIRECTORIES "${_p4_inc}")
        set_target_properties(ggml-base PROPERTIES
            IMPORTED_IMPLIB "${_p4_llama_root}/ggml/src/Release/ggml-base.lib"
            IMPORTED_LOCATION "${P4_STAGED_LLAMA_RUNTIME_DIR}/ggml-base.dll"
            INTERFACE_INCLUDE_DIRECTORIES "${_p4_inc}")
        set_target_properties(ggml-cpu PROPERTIES
            IMPORTED_IMPLIB "${_p4_llama_root}/ggml/src/Release/ggml-cpu.lib"
            IMPORTED_LOCATION "${P4_STAGED_LLAMA_RUNTIME_DIR}/ggml-cpu.dll"
            INTERFACE_INCLUDE_DIRECTORIES "${_p4_inc}")
        set_property(TARGET ggml PROPERTY INTERFACE_LINK_LIBRARIES "ggml-base;ggml-cpu")
        set_property(TARGET llama-common PROPERTY INTERFACE_LINK_LIBRARIES "ggml")
        set_property(TARGET llama PROPERTY INTERFACE_LINK_LIBRARIES "llama-common;ggml")
        # Accelerator plugins are runtime-discovered by ggml_backend_load_all.
        # An imported relink therefore copies every matching plugin instead of
        # hard-linking the P4 core to one CUDA target.
        file(GLOB _p4_backend_runtime_files
            "${P4_STAGED_LLAMA_RUNTIME_DIR}/ggml-*.dll"
            "${P4_STAGED_LLAMA_RUNTIME_DIR}/libggml-*.so*"
            "${P4_STAGED_LLAMA_RUNTIME_DIR}/libggml-*.dylib")
        set(P4_STAGED_LLAMA_IMPORTED ON)
    else()
        # These cache values are scoped to this configure and only affect the
        # external prepared worktree. They keep the embedded build from producing
        # unrelated examples/tools while preserving all common option parsing.
        set(LLAMA_BUILD_COMMON ON CACHE BOOL "" FORCE)
        set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
        set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
        set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
        set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
        set(LLAMA_BUILD_HTTP OFF CACHE BOOL "" FORCE)
        if(P4_STAGED_CUDA)
            set(GGML_CUDA ON CACHE BOOL "" FORCE)
            enable_language(CUDA)
        endif()
        add_subdirectory("${P4_STAGED_LLAMA_SOURCE_DIR}"
                         "${CMAKE_BINARY_DIR}/llama-pipeline" EXCLUDE_FROM_ALL)
    endif()


    if(P4_STAGED_CUDA AND NOT TARGET ggml-cuda)
        message(FATAL_ERROR "P4_STAGED_CUDA=ON but the prepared llama.cpp tree did not create the ggml-cuda target")
    endif()

    add_library(p4_staged_llama_runtime STATIC
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_physical.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_physical_sample.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_physical_mtp.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_physical_mtp_process.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_physical_mtp_settle.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/physical_wire_decode.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/physical_wire_encode.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop_batch.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop_decode.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop_decode_split.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop_sample.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_hop_sample_outcome.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_kv.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/llama_stage_runtime_mtp.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/request_options.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/request_stops.cpp"
        "${CMAKE_CURRENT_LIST_DIR}/src/runtime/request_options_grammar.cpp"
    )
    target_include_directories(p4_staged_llama_runtime PUBLIC
        src/runtime
        src/protocol
        "${P4_STAGED_LLAMA_SOURCE_DIR}/include"
        "${P4_STAGED_LLAMA_SOURCE_DIR}/common"
    )
    target_link_libraries(p4_staged_llama_runtime PUBLIC
        llama
        llama-common
        p4_staged_runtime
        p4_staged_protocol
    )
    target_compile_definitions(p4_staged_llama_runtime PRIVATE
        P4_STAGED_LLAMA_UPSTREAM_COMMIT=\"${P4_STAGED_LLAMA_UPSTREAM_COMMIT}\"
    )

    target_link_libraries(p4_staged_server_core PUBLIC p4_staged_llama_runtime)
    # server.cpp is compiled into the core library, so the llama bridge
    # definition must be attached here rather than only to the final binary.
    target_compile_definitions(p4_staged_server_core PUBLIC P4_STAGED_WITH_LLAMA=1)

    add_executable(p4_staged_llama_runtime_compile_test
        src/runtime/llama_stage_runtime_compile_test.cpp
    )
    target_link_libraries(p4_staged_llama_runtime_compile_test
        PRIVATE p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_llama_runtime_compile_test)
    add_test(NAME p4_staged_llama_runtime_compile_test
             COMMAND p4_staged_llama_runtime_compile_test)

    add_executable(p4_staged_physical_wire_test
        src/runtime/physical_wire_test.cpp
    )
    target_link_libraries(p4_staged_physical_wire_test
        PRIVATE p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_physical_wire_test)
    add_test(NAME p4_staged_physical_wire_test
             COMMAND p4_staged_physical_wire_test)

    add_executable(p4_staged_request_options_test
        src/runtime/request_options_test.cpp
    )
    target_link_libraries(p4_staged_request_options_test
        PRIVATE p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_request_options_test)
    add_test(NAME p4_staged_request_options_test
             COMMAND p4_staged_request_options_test)

    add_executable(p4_staged_request_stops_test
        src/runtime/request_stops_test.cpp
    )
    target_link_libraries(p4_staged_request_stops_test
        PRIVATE p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_request_stops_test)
    add_test(NAME p4_staged_request_stops_test
             COMMAND p4_staged_request_stops_test)

    add_executable(p4_staged_capability_test
        src/server/capability_test.cpp
    )
    target_link_libraries(p4_staged_capability_test
        PRIVATE p4_staged_server_core p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_capability_test)
    add_test(NAME p4_staged_capability_test COMMAND p4_staged_capability_test)

    add_executable(p4_staged_plan_invariants_test
        src/server/plan_invariants_test.cpp
    )
    target_link_libraries(p4_staged_plan_invariants_test
        PRIVATE p4_staged_server_core p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_plan_invariants_test)
    add_test(NAME p4_staged_plan_invariants_test
             COMMAND p4_staged_plan_invariants_test)

    add_executable(p4_staged_mtp_ownership_test
        src/runtime/llama_stage_mtp_ownership_test.cpp
    )
    target_link_libraries(p4_staged_mtp_ownership_test
        PRIVATE p4_staged_llama_runtime)
    p4_stage_enable_asserts(p4_staged_mtp_ownership_test)
    add_test(NAME p4_staged_mtp_ownership_test
             COMMAND p4_staged_mtp_ownership_test)

    # Keep native DLLs beside each executable. This makes both direct launch
    # and CTest independent of generator-specific PATH handling on Windows.
    function(p4_stage_copy_runtime target)
        foreach(runtime_target llama llama-common ggml ggml-base ggml-cpu
                ggml-cuda ggml-vulkan ggml-hip ggml-metal ggml-opencl)
            if(TARGET ${runtime_target})
                add_custom_command(TARGET ${target} POST_BUILD
                    COMMAND ${CMAKE_COMMAND} -E copy_if_different
                        "$<TARGET_FILE:${runtime_target}>"
                        "$<TARGET_FILE_DIR:${target}>")
            endif()
        endforeach()
        foreach(runtime_file IN LISTS _p4_backend_runtime_files)
            add_custom_command(TARGET ${target} POST_BUILD
                COMMAND ${CMAKE_COMMAND} -E copy_if_different
                    "${runtime_file}" "$<TARGET_FILE_DIR:${target}>")
        endforeach()
    endfunction()

    p4_stage_copy_runtime(p4_staged_server)
    p4_stage_copy_runtime(p4_staged_server_test)
    p4_stage_copy_runtime(p4_staged_llama_runtime_compile_test)
    p4_stage_copy_runtime(p4_staged_physical_wire_test)
    p4_stage_copy_runtime(p4_staged_request_stops_test)
    p4_stage_copy_runtime(p4_staged_mtp_ownership_test)
endif()
