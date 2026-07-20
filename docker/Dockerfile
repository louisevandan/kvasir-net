# linkcpp hub: portable control-plane image.
# Docker Desktop for macOS runs Linux containers and cannot expose Apple Metal.
# Use the native Metal node agent for a Mac GPU; Linux NVIDIA uses Dockerfile.cuda.
#   docker build -f docker/Dockerfile -t linkcpp:latest .

# ---- builder: compile llama.cpp (CPU + RPC + server) -------------------------
ARG LINKCPP_BUILDER_IMAGE=ubuntu:22.04
ARG LINKCPP_RUNTIME_IMAGE=ubuntu:22.04
FROM ${LINKCPP_BUILDER_IMAGE} AS builder
ARG LINKCPP_BUILD_JOBS=4
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake git ninja-build libcurl4-openssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY external/llama.cpp ./external/llama.cpp
COPY VERSION ./VERSION
COPY CMakeLists.txt ./CMakeLists.txt
COPY apps ./apps
COPY src ./src
COPY cmake ./cmake
# Keep builds reliable on Docker Desktop installations with a conservative VM
# memory limit while still allowing callers to raise this explicitly.
RUN cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DGGML_RPC=ON -DLLAMA_CURL=ON -DLLAMA_BUILD_UI=OFF -DLLAMA_USE_PREBUILT_UI=OFF \
        -DLINKCPP_BUILD=OFF -DLINKCPP_BUILD_RING_ADAPTER=ON \
        -DLINKCPP_RING_BUILD_ID="linkcpp-ring-$(cat VERSION)" \
    && cmake --build build -j"${LINKCPP_BUILD_JOBS}" --target ggml-rpc-server llama-server linkcpp-node linkcpp-server

# ---- runtime ----------------------------------------------------------------
FROM ${LINKCPP_RUNTIME_IMAGE} AS hub
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 python3-pip libgomp1 libcurl4 ca-certificates curl procps \
    && pip3 install --no-cache-dir fastapi "uvicorn[standard]" httpx gguf numpy python-multipart \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/build/bin/*.so* /usr/local/lib/
RUN ldconfig
COPY --from=builder /src/build/bin/ggml-rpc-server /app/bin/ggml-rpc-server
COPY --from=builder /src/build/bin/llama-server    /app/bin/llama-server
COPY --from=builder /src/build/apps/linkcpp-node/linkcpp-node /app/bin/linkcpp-node
COPY --from=builder /src/build/apps/linkcpp-server/linkcpp-server /app/bin/linkcpp-server
COPY VERSION /app/VERSION
COPY controller /app/controller
ENV PYTHONPATH=/app LINKCPP_LLAMA_CPP_BACKEND=cpu \
    RPC_BIN=/app/bin/ggml-rpc-server LLAMA_SERVER_BIN=/app/bin/llama-server \
    LINKCPP_STAGE_BIN=/app/bin/linkcpp-node LINKCPP_SERVER_BIN=/app/bin/linkcpp-server \
    LINKCPP_MODEL_DIR=/models LINKCPP_DATA_DIR=/models/linkcpp LINKCPP_STAGE_DIR=/models/linkcpp/stage
WORKDIR /app
EXPOSE 9000
VOLUME ["/models"]
ENTRYPOINT ["python3", "-m", "uvicorn", "controller.hub:app", "--host", "0.0.0.0", "--port", "9000"]
