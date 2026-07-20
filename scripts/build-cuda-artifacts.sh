#!/usr/bin/env bash
set -euo pipefail

mode="${1:-all}"
cuda_version="${CUDA_VERSION:-13.0.0}"
cuda_archs="${CUDA_ARCHS:-75;80;86;89;90;120;121}"
jobs="${LINKCPP_BUILD_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"
if [[ "$jobs" -gt 32 ]]; then jobs=32; fi
case "$mode" in all|rpc|proxy) ;; *) echo "usage: $0 [all|rpc|proxy]" >&2; exit 2 ;; esac

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
# Docker contexts copy only tracked runtime inputs; unrelated user scratch
# files and the ignored prebuilt cache must not block artifact reuse.
dirty="$(git status --porcelain --untracked-files=no)"
if [[ -n "$dirty" ]]; then
    echo "artifact builds require a clean worktree" >&2
    exit 2
fi
llama_revision="$(git -C external/llama.cpp rev-parse HEAD)"
adapter_revision="$(git rev-parse HEAD:CMakeLists.txt HEAD:apps HEAD:src HEAD:cmake HEAD:scripts/package-ring-runtime.py | sha256sum | awk '{print $1}')"
cuda_tag="${cuda_version//./-}"
arch_tag="${cuda_archs//;/-}"
toolchain="linkcpp-cuda-toolchain:${cuda_tag}"
llama_key="${llama_revision:0:12}-cuda${cuda_tag}-sm${arch_tag}"
proxy_key="${llama_revision:0:12}-${adapter_revision:0:12}-cuda${cuda_tag}-sm${arch_tag}"
llama_build="linkcpp-llama-build:${llama_key}"
platform="$(docker version --format '{{.Server.Os}}-{{.Server.Arch}}')"
prebuilt_dir="${LINKCPP_PREBUILT_DIR:-$repo/artifacts/prebuilt}"
cache_dir="$prebuilt_dir/${platform}-cuda${cuda_tag}-sm${arch_tag}/${llama_revision:0:12}/${adapter_revision:0:12}"
rpc_artifact="linkcpp-rpc-artifacts:${llama_key}"
proxy_artifact="linkcpp-proxy-artifacts:${proxy_key}"

restore_prebuilt() {
    local image="$1" archive="$2"
    docker image inspect "$image" >/dev/null 2>&1 && return 0
    [[ -f "$archive" ]] || return 1
    echo "= restore $image from $archive"
    docker load --input "$archive"
    docker image inspect "$image" >/dev/null 2>&1
}

save_prebuilt() {
    local image="$1" archive="$2" kind="$3"
    mkdir -p "$(dirname "$archive")"
    echo "= cache $image at $archive"
    docker save --output "$archive" "$image"
    printf '{"platform":"%s","cuda_version":"%s","cuda_archs":"%s","llama_revision":"%s","adapter_revision":"%s","image":"%s","kind":"%s"}\n' \
        "$platform" "$cuda_version" "$cuda_archs" "$llama_revision" "$adapter_revision" "$image" "$kind" > "$archive.json"
}

if ! docker image inspect "$toolchain" >/dev/null 2>&1; then
    docker build -f docker/cuda/Dockerfile.toolchain \
        --build-arg "CUDA_DEVEL_IMAGE=nvidia/cuda:${cuda_version}-devel-ubuntu22.04" \
        -t "$toolchain" -t linkcpp-cuda-toolchain:local docker/cuda
else
    docker tag "$toolchain" linkcpp-cuda-toolchain:local
fi
if ! restore_prebuilt "$rpc_artifact" "$cache_dir/rpc-artifacts.tar"; then
    docker build -f docker/cuda/Dockerfile.llama --target llama-build \
        --build-arg "CUDA_TOOLCHAIN_IMAGE=$toolchain" --build-arg "CUDA_ARCHS=$cuda_archs" \
        --build-arg "LINKCPP_BUILD_JOBS=$jobs" -t "$llama_build" -t linkcpp-llama-build:local .
    docker build -f docker/cuda/Dockerfile.llama --target llama-artifacts \
        --build-arg "CUDA_TOOLCHAIN_IMAGE=$toolchain" --build-arg "CUDA_ARCHS=$cuda_archs" \
        --build-arg "LINKCPP_BUILD_JOBS=$jobs" -t "$rpc_artifact" -t linkcpp-rpc-artifacts:local .
    save_prebuilt "$rpc_artifact" "$cache_dir/rpc-artifacts.tar" rpc
fi

if [[ "$mode" == "all" || "$mode" == "proxy" ]]; then
    if ! restore_prebuilt "$proxy_artifact" "$cache_dir/proxy-artifacts.tar"; then
        if ! docker image inspect "$llama_build" >/dev/null 2>&1; then
            docker build -f docker/cuda/Dockerfile.llama --target llama-build \
                --build-arg "CUDA_TOOLCHAIN_IMAGE=$toolchain" --build-arg "CUDA_ARCHS=$cuda_archs" \
                --build-arg "LINKCPP_BUILD_JOBS=$jobs" -t "$llama_build" -t linkcpp-llama-build:local .
        fi
        docker build -f docker/cuda/Dockerfile.proxy --target proxy-artifacts \
            --build-arg "LLAMA_BUILD_IMAGE=$llama_build" --build-arg "LINKCPP_BUILD_JOBS=$jobs" \
            --build-arg "LINKCPP_RING_BUILD_ID=${llama_revision}.${adapter_revision}" \
            -t "$proxy_artifact" -t linkcpp-proxy-artifacts:local .
        save_prebuilt "$proxy_artifact" "$cache_dir/proxy-artifacts.tar" proxy
    fi
    docker tag "$proxy_artifact" linkcpp-proxy-artifacts:local
fi
