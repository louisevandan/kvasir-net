# Proxy-only build graph.  The stock RPC build does not include this file.
option(LINKCPP_BUILD_RING_ARCH_MATRIX "Build llama.cpp synthetic architecture fixtures" OFF)

if(NOT LINKCPP_RING_BUILD_ID)
    execute_process(
        COMMAND git rev-parse HEAD
        WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/external/llama.cpp
        OUTPUT_VARIABLE LINKCPP_RING_LLAMA_REV
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET
    )
    execute_process(
        COMMAND git rev-parse HEAD
        WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
        OUTPUT_VARIABLE LINKCPP_RING_ADAPTER_REV
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET
    )
    if(NOT LINKCPP_RING_LLAMA_REV OR NOT LINKCPP_RING_ADAPTER_REV)
        message(FATAL_ERROR
            "Proxy build identity is unavailable; pass "
            "-DLINKCPP_RING_BUILD_ID=<immutable artifact id>")
    endif()
    execute_process(
        COMMAND git diff --quiet --ignore-submodules --
        WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/external/llama.cpp
        RESULT_VARIABLE LINKCPP_RING_LLAMA_DIRTY
        ERROR_QUIET
    )
    execute_process(
        COMMAND git diff --quiet --ignore-submodules --
        WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
        RESULT_VARIABLE LINKCPP_RING_ADAPTER_DIRTY
        ERROR_QUIET
    )
    if(NOT LINKCPP_RING_LLAMA_DIRTY EQUAL 0 OR NOT LINKCPP_RING_ADAPTER_DIRTY EQUAL 0)
        message(FATAL_ERROR
            "Refusing an ambiguous proxy build from dirty sources; commit the "
            "sources or pass -DLINKCPP_RING_BUILD_ID=<immutable artifact id>")
    endif()
    set(LINKCPP_RING_BUILD_ID "${LINKCPP_RING_LLAMA_REV}.${LINKCPP_RING_ADAPTER_REV}")
endif()

add_subdirectory(apps/linkcpp-node)
add_subdirectory(apps/linkcpp-expert-worker)
if(NOT CMAKE_SYSTEM_NAME STREQUAL "iOS")
    add_subdirectory(apps/linkcpp-moe-verify)
endif()
if(NOT CMAKE_SYSTEM_NAME STREQUAL "iOS")
    # The ring coordinator reuses llama.cpp's full server implementation, which
    # does not build for iOS; phones only run stages (linkcpp-stage).
    add_subdirectory(apps/linkcpp-server)
endif()

if(LINKCPP_BUILD_RING_ARCH_MATRIX)
    add_executable(linkcpp-arch-fixtures
        apps/linkcpp-arch-fixtures/main.cpp
        external/llama.cpp/tests/get-model.cpp
    )
    target_link_libraries(linkcpp-arch-fixtures PRIVATE llama-common)
    target_include_directories(linkcpp-arch-fixtures PRIVATE
        ${CMAKE_SOURCE_DIR}/external/llama.cpp/src
        ${CMAKE_SOURCE_DIR}/external/llama.cpp/tests
    )
    set_target_properties(linkcpp-arch-fixtures PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
endif()
