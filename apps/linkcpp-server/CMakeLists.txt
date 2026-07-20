# Proxy-mode coordinator.  It deliberately reuses llama.cpp's complete server
# implementation so new model/output workflows remain owned by the pinned
# llama.cpp version rather than being duplicated in linkcpp.
add_executable(linkcpp-server main.cpp ring-executor.cpp)
target_link_libraries(linkcpp-server PRIVATE llama-server-impl)
target_compile_definitions(linkcpp-server PRIVATE LINKCPP_RING_BUILD_ID="${LINKCPP_RING_BUILD_ID}")
target_include_directories(linkcpp-server PRIVATE ${CMAKE_SOURCE_DIR}/external/llama.cpp/include)

# The proxy coordinator is compatible with the generic linkcpp stage ABI in
# upstream llama.cpp.  Expert dispatch is an optional extension of that ABI;
# only compile its integration when the pinned header actually exposes the
# hook.  This keeps the stable RPC and ring runtimes buildable while a newer
# upstream snapshot omits the experimental MoE callback.
file(READ "${CMAKE_SOURCE_DIR}/external/llama.cpp/include/llama.h" LINKCPP_LLAMA_HEADER)
if (LINKCPP_LLAMA_HEADER MATCHES "llama_linkcpp_set_moe_dispatch")
    target_compile_definitions(linkcpp-server PRIVATE LINKCPP_MOE_DISPATCH_AVAILABLE=1)
endif()
set_target_properties(linkcpp-server PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
