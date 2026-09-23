#!/usr/bin/env bash
# Build the iOS static libraries the wallet links against.
#
# wallet/ios/project.yml puts `build-ios-ring/...` on LIBRARY_SEARCH_PATHS and
# links -llinkcpp-stage -llinkcpp-expert from it, but nothing in the tree built
# that directory: it was configured by hand once and the flags survived only in
# its own CMakeCache. This reproduces it, so the Xcode build has a source.
#
# The build id is the fingerprint two ring peers compare before they will talk
# to each other, so it must be an immutable artifact id. CMake derives it from
# the two git HEADs and refuses a dirty tree; pass RING_BUILD_ID to build from
# a working tree anyway, and understand that peers built from a different tree
# under the same id will disagree about the wire and fail in confusing ways.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=${BUILD_DIR:-$here/build-ios-ring}

[ -d "$here/external/llama.cpp" ] || {
  echo "external/llama.cpp is missing; init the submodule first" >&2; exit 1; }

args=(
  -S "$here" -B "$build"
  -DCMAKE_SYSTEM_NAME=iOS
  -DCMAKE_OSX_SYSROOT=iphoneos
  -DCMAKE_OSX_ARCHITECTURES=arm64
  -DCMAKE_OSX_DEPLOYMENT_TARGET=16.0     # matches project.yml deploymentTarget
  -DCMAKE_BUILD_TYPE=Release
  -DBUILD_SHARED_LIBS=OFF                # the app links static archives
  -DLINKCPP_BUILD_RING_ADAPTER=ON        # without this, apps/* is not in the graph
  -DGGML_METAL=ON
  -DGGML_METAL_EMBED_LIBRARY=ON          # no .metallib to ship beside the app
  -DGGML_ACCELERATE=ON
  -DGGML_OPENMP=OFF                      # no libomp on iOS
  -DGGML_BLAS=OFF
  -DGGML_CUDA=OFF
)
[ -n "${RING_BUILD_ID:-}" ] && args+=(-DLINKCPP_RING_BUILD_ID="$RING_BUILD_ID")

cmake "${args[@]}"
cmake --build "$build" --target linkcpp-stage linkcpp-expert -j "$(sysctl -n hw.ncpu)"

echo
echo "archives:"
find "$build" -name '*.a' -newer "$build/CMakeCache.txt" -o -name 'liblinkcpp-*.a' | sort -u
