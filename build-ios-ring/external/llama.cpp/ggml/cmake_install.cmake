# Install script for directory: /Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml

# Set the install prefix
if(NOT DEFINED CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX "/usr/local")
endif()
string(REGEX REPLACE "/$" "" CMAKE_INSTALL_PREFIX "${CMAKE_INSTALL_PREFIX}")

# Set the install configuration name.
if(NOT DEFINED CMAKE_INSTALL_CONFIG_NAME)
  if(BUILD_TYPE)
    string(REGEX REPLACE "^[^A-Za-z0-9_]+" ""
           CMAKE_INSTALL_CONFIG_NAME "${BUILD_TYPE}")
  else()
    set(CMAKE_INSTALL_CONFIG_NAME "Release")
  endif()
  message(STATUS "Install configuration: \"${CMAKE_INSTALL_CONFIG_NAME}\"")
endif()

# Set the component getting installed.
if(NOT CMAKE_INSTALL_COMPONENT)
  if(COMPONENT)
    message(STATUS "Install component: \"${COMPONENT}\"")
    set(CMAKE_INSTALL_COMPONENT "${COMPONENT}")
  else()
    set(CMAKE_INSTALL_COMPONENT)
  endif()
endif()

# Is this installation the result of a crosscompile?
if(NOT DEFINED CMAKE_CROSSCOMPILING)
  set(CMAKE_CROSSCOMPILING "TRUE")
endif()

# Set path to fallback-tool for dependency-resolution.
if(NOT DEFINED CMAKE_OBJDUMP)
  set(CMAKE_OBJDUMP "/usr/bin/objdump")
endif()

if(NOT CMAKE_INSTALL_LOCAL_ONLY)
  # Include the install script for the subdirectory.
  include("/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/src/cmake_install.cmake")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib" TYPE STATIC_LIBRARY FILES "/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/src/libggml.a")
  if(EXISTS "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml.a" AND
     NOT IS_SYMLINK "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml.a")
    execute_process(COMMAND "/usr/bin/ranlib" "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml.a")
  endif()
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/include" TYPE FILE FILES
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-cpu.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-alloc.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-backend.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-blas.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-cann.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-cpp.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-cuda.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-opt.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-metal.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-rpc.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-virtgpu.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-sycl.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-vulkan.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-webgpu.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-zendnn.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/ggml-openvino.h"
    "/Volumes/Transcend/Projects/kvasir-net/external/llama.cpp/ggml/include/gguf.h"
    )
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib" TYPE STATIC_LIBRARY FILES "/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/src/libggml-base.a")
  if(EXISTS "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml-base.a" AND
     NOT IS_SYMLINK "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml-base.a")
    execute_process(COMMAND "/usr/bin/ranlib" "$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/lib/libggml-base.a")
  endif()
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/cmake/ggml" TYPE FILE FILES
    "/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/ggml-config.cmake"
    "/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/ggml-version.cmake"
    )
endif()

string(REPLACE ";" "\n" CMAKE_INSTALL_MANIFEST_CONTENT
       "${CMAKE_INSTALL_MANIFEST_FILES}")
if(CMAKE_INSTALL_LOCAL_ONLY)
  file(WRITE "/Volumes/Transcend/Projects/kvasir-net/build-ios-ring/external/llama.cpp/ggml/install_local_manifest.txt"
     "${CMAKE_INSTALL_MANIFEST_CONTENT}")
endif()
