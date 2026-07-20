#!/usr/bin/env bash
# Build a native managed-node runtime for this host — the RPC worker plus the
# ring stage/coordinator binaries a hub operator needs to serve accelerated
# layer windows.
#
# Usage:
#   scripts/build-node-runtime.sh [auto|cuda|rocm|metal|vulkan|cpu]
#
# ROCm/HIP note: the HIP compiler check fails ("broken") unless the ROCm clang++
# is pointed at the host gcc toolchain. This script auto-detects both the ROCm
# path and the GPU arch (via rocminfo) so an operator does not have to.
set -euo pipefail
cd "$(dirname "$0")/.."

backend="${1:-${LINKCPP_LLAMA_CPP_BACKEND:-auto}}"
system="$(uname -s | tr '[:upper:]' '[:lower:]')"

if [[ "$backend" == "auto" ]]; then
  if [[ "$system" == "darwin" ]]; then
    backend="metal"
  elif command -v nvidia-smi >/dev/null 2>&1; then
    backend="cuda"
  elif command -v rocminfo >/dev/null 2>&1 || [[ -d /opt/rocm ]]; then
    backend="rocm"
  else
    backend="cpu"
  fi
fi

# The ring stage/coordinator (linkcpp-node/linkcpp-server) is the primary data
# plane; build it alongside the RPC worker unless explicitly disabled.
build_ring="${LINKCPP_BUILD_RING:-1}"

flags=(
  -DCMAKE_BUILD_TYPE=Release
  -DGGML_RPC=ON
  -DLLAMA_CURL=ON
  -DLINKCPP_BUILD=OFF
  -DLINKCPP_BUILD_RING_ADAPTER=ON
)
[[ "$build_ring" == "1" ]] && flags+=(-DLINKCPP_BUILD_RING_ADAPTER=ON)

# A ring stage and its coordinator must report one immutable identity across
# every platform.  The release version is bumped for runtime-affecting changes,
# so it is the portable identity for the paired native artifacts.
release_version="$(tr -d '[:space:]' < VERSION)"
if [[ -z "$release_version" ]]; then
  echo "VERSION is empty; cannot create a ring runtime identity" >&2
  exit 2
fi
flags+=("-DLINKCPP_RING_BUILD_ID=${LINKCPP_RING_BUILD_ID:-linkcpp-ring-${release_version}}")

case "$backend" in
  cuda)
    flags+=(-DGGML_CUDA=ON)
    ;;
  rocm)
    rocm_path="${ROCM_PATH:-$(ls -d /opt/rocm /opt/rocm-* 2>/dev/null | head -1)}"
    if [[ -z "$rocm_path" || ! -x "$rocm_path/llvm/bin/clang++" ]]; then
      echo "ROCm not found (set ROCM_PATH to a ROCm install with llvm/bin/clang++)" >&2
      exit 2
    fi
    # GPU arch: prefer an explicit override, else the first gfx target rocminfo reports.
    arch="${AMDGPU_TARGETS:-$(rocminfo 2>/dev/null | grep -o 'gfx[0-9a-f]*' | head -1)}"
    if [[ -z "$arch" ]]; then
      echo "could not detect an AMD GPU arch (set AMDGPU_TARGETS, e.g. gfx90a)" >&2
      exit 2
    fi
    # Host gcc toolchain the ROCm clang++ must use, else the HIP compiler check breaks.
    gcc_dir="$(dirname "$("${CC:-gcc}" --print-libgcc-file-name 2>/dev/null)")"
    echo "rocm: path=$rocm_path arch=$arch gcc-install-dir=$gcc_dir"
    flags+=(
      -DGGML_HIP=ON
      "-DCMAKE_HIP_COMPILER=$rocm_path/llvm/bin/clang++"
      "-DCMAKE_HIP_FLAGS=--gcc-install-dir=$gcc_dir -Wno-c++11-narrowing"
      "-DAMDGPU_TARGETS=$arch" "-DGPU_TARGETS=$arch"
    )
    ;;
  metal)
    if [[ "$system" != "darwin" ]]; then
      echo "Metal builds require macOS; got ${system}" >&2
      exit 2
    fi
    flags+=(-DGGML_METAL=ON)
    ;;
  vulkan)
    flags+=(-DGGML_VULKAN=ON)
    ;;
  cpu)
    if [[ "$system" == "darwin" ]]; then
      flags+=(-DGGML_METAL=OFF)
    fi
    ;;
  *)
    echo "unknown backend: ${backend}" >&2
    exit 2
    ;;
esac

# The ring adapter lives in external/llama.cpp (a submodule); make sure it's present.
if [[ "$build_ring" == "1" && ! -f external/llama.cpp/CMakeLists.txt ]]; then
  echo "fetching external/llama.cpp submodule..."
  git submodule update --init external/llama.cpp
fi

build_dir="${LINKCPP_NODE_BUILD_DIR:-build-node-${system}-${backend}}"
# Native managed runtimes may be copied to a release directory after their
# initial RPC build.  CMake caches the absolute source directory, so reusing
# that cache would reject the proxy rebuild.  Discard only that generated cache
# when it belongs to another source checkout.
cache_file="${build_dir}/CMakeCache.txt"
if [[ -f "$cache_file" ]] && ! grep -Fqx "CMAKE_HOME_DIRECTORY:INTERNAL=${PWD}" "$cache_file"; then
  echo "resetting stale native build cache: ${build_dir}" >&2
  rm -rf "$build_dir"
fi
cmake -S . -B "$build_dir" "${flags[@]}"

targets=(ggml-rpc-server)
[[ "$build_ring" == "1" ]] && targets+=(linkcpp-node linkcpp-server)
build_cmd=(cmake --build "$build_dir" --config Release --target "${targets[@]}")
[[ -n "${LINKCPP_BUILD_JOBS:-}" ]] && build_cmd+=(--parallel "$LINKCPP_BUILD_JOBS")
"${build_cmd[@]}"

echo "built ${backend} node runtime in ${build_dir}/:"
echo "  RPC worker : ${build_dir}/bin/ggml-rpc-server"
if [[ "$build_ring" == "1" ]]; then
  echo "  ring stage : ${build_dir}/apps/linkcpp-node/linkcpp-node"
  echo "  ring coord : ${build_dir}/apps/linkcpp-server/linkcpp-server"
fi
