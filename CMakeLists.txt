cmake_minimum_required(VERSION 3.18)
project(linkcpp C CXX)

# The desktop app ships only the expert worker, which needs ggml and nothing
# else from llama.cpp. Building just those two keeps a Windows/CUDA packaging
# build to ggml + one executable instead of the full server toolchain.
option(LINKCPP_EXPERT_WORKER_ONLY "Build only ggml and linkcpp-expert-worker" OFF)
if(LINKCPP_EXPERT_WORKER_ONLY)
    add_subdirectory(external/llama.cpp/ggml)
    add_subdirectory(apps/linkcpp-expert-worker)
    return()
endif()

# Build the pinned llama.cpp submodule in-tree. All backends (CUDA/Vulkan/CPU),
# model support, and the ggml-rpc transport come from here unmodified.
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON ON CACHE BOOL "" FORCE)   # needed for tools + our node
if(CMAKE_SYSTEM_NAME STREQUAL "iOS")
    # Mobile builds only produce the linkcpp-stage library; llama.cpp's server
    # tool cannot configure for iOS (bundle install rules) and no bundled
    # binary can be exec'd on the device anyway.
    set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
else()
    set(LLAMA_BUILD_TOOLS ON CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_SERVER ON CACHE BOOL "" FORCE)   # llama-cli lives behind this gate
endif()
set(LLAMA_BUILD_UI OFF CACHE BOOL "" FORCE)      # linkcpp uses its own hub UI
set(LLAMA_USE_PREBUILT_UI OFF CACHE BOOL "" FORCE)
add_subdirectory(external/llama.cpp)

# llama.cpp's server targets add the project root to their private include
# paths. On the default case-insensitive macOS filesystem that makes this
# project's required `VERSION` file shadow libc++'s standard `<version>`
# header. The server sources already have their own source and generated
# include paths, so the root path is unnecessary here.
if(APPLE)
    foreach(_linkcpp_server_target IN ITEMS server-context llama-server-impl)
        if(TARGET ${_linkcpp_server_target})
            get_target_property(_linkcpp_server_includes ${_linkcpp_server_target} INCLUDE_DIRECTORIES)
            if(_linkcpp_server_includes)
                list(REMOVE_ITEM _linkcpp_server_includes "${CMAKE_SOURCE_DIR}")
                set_property(TARGET ${_linkcpp_server_target}
                    PROPERTY INCLUDE_DIRECTORIES "${_linkcpp_server_includes}")
            endif()
        endif()
    endforeach()
endif()

# The stable RPC runtime must build against stock upstream without compiling the
# private-API ring adapter.  Enable this only with a reviewed compatible adapter
# revision (the Docker product image does so explicitly).
option(LINKCPP_BUILD_RING_ADAPTER "Build the private-API linkcpp ring stage adapter" OFF)
if(LINKCPP_BUILD_RING_ADAPTER)
    include(cmake/LinkcppProxy.cmake)
endif()
