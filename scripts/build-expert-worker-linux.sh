#!/bin/bash
# Build the Linux CUDA expert worker, portably, on a machine without an NVIDIA GPU.
#
#   scripts/build-expert-worker-linux.sh [<source dir>] [<output dir>]
#
# Needs Docker and the repository's source. It does NOT need a GPU: nvcc
# compiles against a toolkit, and a card is only required to run what comes out.
# That is what lets a host with AMD accelerators build this at all — which
# mattered, because for a while no NVIDIA Linux machine existed in the fleet.
#
# ## Three things here are load-bearing, and each was learned the hard way
#
# **ubi8, not the obvious base.** A binary's glibc floor is whatever it was
# BUILT against. Built on nvidia/cuda:13.0.1-devel-ubuntu24.04, the worker
# needed GLIBC_2.38 and refused to start on Ubuntu 22.04, Debian 12 and RHEL —
# most of the server fleet. ubi8 is RHEL 8, glibc 2.28, and with the static
# libstdc++ below the result runs on glibc 2.27 and newer: Ubuntu 18.04,
# Debian 10, RHEL 8, anything since. RHEL 8's own gcc is 8.5 and too old for
# ggml, so the image adds gcc-toolset-13; nvcc takes it as the host compiler.
#
# **GGML_CUDA_NCCL=OFF.** It defaults ON, and NVIDIA's devel images happen to
# ship NCCL, so the first build linked libnccl.so.2 — a library the worker
# never calls, on a machine that would then refuse to start without it. The
# Windows pack escaped this only because its build host had no NCCL to find.
#
# **Static everything except the driver.** cuBLAS, cuBLASLt and cudart are
# linked in, so an operator needs no CUDA toolkit and no LD_LIBRARY_PATH; the
# only thing resolved from the system is libcuda.so.1, which comes with the
# driver. It costs about 500 MB, and it buys an install that is one file.
#
# The checks at the end are the point of the script as much as the build is.
# Every one of them corresponds to a binary that worked on the machine that
# made it and nowhere else.
set -euo pipefail

SRC="${1:-$PWD}"
OUT="${2:-$PWD/build/linux-worker}"
IMAGE=kvasir-worker-build:ubi8

# Real SASS for the cards a volunteer plausibly owns, plus PTX so a newer one
# can compile at first run. 10.0/12.1 are Grace-class parts — aarch64, built
# separately. Each real architecture adds roughly 40 MiB.
ARCHS='75-real;80-real;86-real;89-real;90-real;120-real;120-virtual'

# The oldest glibc this build is allowed to require. Raise it only with a
# reason; every bump drops distributions that were working.
GLIBC_FLOOR=2.28

command -v docker >/dev/null || { echo "docker is required" >&2; exit 1; }
[ -f "$SRC/CMakeLists.txt" ] || { echo "$SRC does not look like the repository" >&2; exit 1; }

ctx="$(mktemp -d)"
trap 'rm -rf "$ctx"' EXIT
cat > "$ctx/Dockerfile" <<'DOCKER'
FROM nvidia/cuda:13.0.1-devel-ubi8
RUN dnf install -y gcc-toolset-13 cmake ninja-build && dnf clean all
ENV PATH=/opt/rh/gcc-toolset-13/root/usr/bin:$PATH
ENV LD_LIBRARY_PATH=/opt/rh/gcc-toolset-13/root/usr/lib64:$LD_LIBRARY_PATH
DOCKER

rm -rf "$OUT" && mkdir -p "$OUT"
docker build -q -t "$IMAGE" "$ctx"

docker run --rm -v "$SRC:/src:ro" -v "$OUT:/out" "$IMAGE" bash -c "
set -euo pipefail
gcc --version | head -1; ldd --version | head -1
cp -r /src /build && cd /build
cmake -S . -B out -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DLINKCPP_EXPERT_WORKER_ONLY=ON \
  -DBUILD_SHARED_LIBS=OFF -DGGML_STATIC=ON \
  -DGGML_CUDA=ON -DGGML_CUDA_FORCE_CUBLAS=ON \
  -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_CUDA_NCCL=OFF \
  -DCMAKE_CUDA_ARCHITECTURES='$ARCHS' \
  -DCMAKE_EXE_LINKER_FLAGS='-static-libstdc++ -static-libgcc'
cmake --build out --target linkcpp-expert-worker -j \$(nproc)
cp out/apps/linkcpp-expert-worker/linkcpp-expert-worker /out/
chmod 0755 /out/linkcpp-expert-worker
"

BIN="$OUT/linkcpp-expert-worker"
[ -f "$BIN" ] || { echo "the build reported success but produced nothing" >&2; exit 1; }

fail() { echo "build-expert-worker-linux: $*" >&2; exit 1; }

# 1. Nothing but the driver may be unresolved. A missing cuBLAS or NCCL here is
#    a worker that will not start on a machine that has no toolkit.
unresolved="$(ldd "$BIN" 2>/dev/null | awk '/not found/ {print $1}' | grep -v '^libcuda\.so' || true)"
[ -z "$unresolved" ] || fail "links libraries that are not the driver: $unresolved"

# 2. No RUNPATH. One with a build-tree path in it runs only where it was built.
! readelf -d "$BIN" | grep -qiE 'runpath|rpath' || fail "carries an RPATH/RUNPATH"

# 3. No GLIBCXX floor at all — that is what -static-libstdc++ is for.
if objdump -T "$BIN" | grep -q GLIBCXX_; then
  fail "still needs libstdc++ from the system: $(objdump -T "$BIN" | grep -o 'GLIBCXX_[0-9.]*' | sort -Vu | tail -1)"
fi

# 4. The glibc floor, which is the one that silently excludes whole distributions.
need="$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)"
if [ "$(printf '%s\n%s\n' "$GLIBC_FLOOR" "$need" | sort -V | tail -1)" != "$GLIBC_FLOOR" ]; then
  fail "needs glibc $need, above the $GLIBC_FLOOR floor — it will not start on older distributions"
fi

echo
echo "$BIN"
echo "  bytes        $(stat -c%s "$BIN" 2>/dev/null || stat -f%z "$BIN")"
echo "  glibc floor  $need  (limit $GLIBC_FLOOR)"
echo "  libstdc++    static"
echo "  runpath      none"
echo "  needs        the NVIDIA driver (CUDA 13 / R580+) and nothing else"
